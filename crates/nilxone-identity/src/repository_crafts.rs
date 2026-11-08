// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

/// What became of a craft or repair the device asked to pay for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CraftOutcome {
    /// Paid now: `seeds` debited and the inputs used up.
    Started { seeds: u64 },
    /// This craft id was paid before with the same recipe and inputs.
    AlreadyStarted { seeds: u64 },
    /// The same craft id with another recipe or other inputs.
    ConflictingReplay,
    SessionChanged,
    UnknownRecipe,
    /// The recipe uses up a crafted thing, which has no attestation yet.
    UnverifiableInputs,
    /// The inputs are not the recipe's things, attested to this Bond, unsold
    /// and unused.
    WrongInputs,
    InsufficientSeeds,
}

impl CraftOutcome {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Started { .. } => "started",
            Self::AlreadyStarted { .. } => "already_started",
            Self::ConflictingReplay => "conflicting_replay",
            Self::SessionChanged => "session_changed",
            Self::UnknownRecipe => "unknown_recipe",
            Self::UnverifiableInputs => "unverifiable_inputs",
            Self::WrongInputs => "wrong_inputs",
            Self::InsufficientSeeds => "not_enough_seeds",
        }
    }
}

/// The recipe's used-up things as one sorted list of item ids, or `None` when
/// one of them is not a found thing (and so has no attestation).
fn recipe_inputs(recipe: &nilxone_contracts::Recipe) -> Option<Vec<&'static str>> {
    let mut inputs = Vec::new();
    for &(item, count) in recipe.consumes {
        let found = nilxone_contracts::find_item(item)?;
        if found.seeds > 0 {
            return None;
        }
        for _ in 0..count {
            inputs.push(found.id);
        }
    }
    inputs.sort_unstable();
    Some(inputs)
}

impl IdentityRepository {
    /// Pays for a craft or repair from the Identity wallet and uses up the
    /// attested finds it consumes, in one transaction, once per `craft_id`.
    /// Core's recipe says the price and the things; the device names only
    /// which of its attested finds are used up.
    pub async fn start_craft_with_seeds(
        &self,
        owner: &PubDress,
        active_session_hash: &[u8],
        craft_id: &str,
        recipe_id: &str,
        input_attestations: &[String],
        now_ms: u64,
    ) -> Result<CraftOutcome, RepositoryError> {
        let Some(recipe) = nilxone_contracts::recipe(recipe_id) else {
            return Ok(CraftOutcome::UnknownRecipe);
        };
        let Some(expected) = recipe_inputs(recipe) else {
            return Ok(CraftOutcome::UnverifiableInputs);
        };
        let mut asked: Vec<&str> = input_attestations.iter().map(String::as_str).collect();
        asked.sort_unstable();
        let at = i64::try_from(now_ms).map_err(|_| RepositoryError::CorruptSeedsBalance)?;
        let price = i64::try_from(recipe.seeds).map_err(|_| RepositoryError::CorruptSeedsBalance)?;

        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let earlier = sqlx::query_as::<_, (String, String, i64)>(
            "SELECT owner_pub_dress, recipe_id, seeds_debited FROM craft_receipts WHERE craft_id = ?",
        )
        .bind(craft_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((holder, earlier_recipe, seeds)) = earlier {
            let mut used = sqlx::query_scalar::<_, String>(
                "SELECT attestation_id FROM craft_inputs WHERE craft_id = ?",
            )
            .bind(craft_id)
            .fetch_all(&mut *tx)
            .await?;
            used.sort_unstable();
            let same = holder == owner.as_str()
                && earlier_recipe == recipe_id
                && used.iter().map(String::as_str).eq(asked.iter().copied());
            return Ok(if same {
                CraftOutcome::AlreadyStarted {
                    seeds: u64::try_from(seeds).map_err(|_| RepositoryError::CorruptSeedsBalance)?,
                }
            } else {
                CraftOutcome::ConflictingReplay
            });
        }
        if !spend_session_active_in(&mut tx, owner, active_session_hash, now_ms / 1000).await? {
            return Ok(CraftOutcome::SessionChanged);
        }
        credit_starter_seeds_in(&mut tx, owner, now_ms).await?;

        if asked.len() != expected.len() || asked.windows(2).any(|pair| pair[0] == pair[1]) {
            return Ok(CraftOutcome::WrongInputs);
        }
        let mut items = Vec::with_capacity(asked.len());
        for attestation in &asked {
            let item = sqlx::query_scalar::<_, String>(
                "SELECT item_id FROM find_attestations
                 WHERE attestation_id = ? AND owner_pub_dress = ? AND sold_at_ms IS NULL
                 AND NOT EXISTS (SELECT 1 FROM craft_inputs
                                 WHERE craft_inputs.attestation_id = find_attestations.attestation_id)",
            )
            .bind(attestation)
            .bind(owner.as_str())
            .fetch_optional(&mut *tx)
            .await?;
            let Some(item) = item else {
                return Ok(CraftOutcome::WrongInputs);
            };
            items.push(item);
        }
        items.sort_unstable();
        if !items.iter().map(String::as_str).eq(expected.iter().copied()) {
            return Ok(CraftOutcome::WrongInputs);
        }

        if price > 0 {
            let spent = sqlx::query(
                "UPDATE bond_seeds SET balance = balance - ?
                 WHERE owner_pub_dress = ? AND balance >= ?",
            )
            .bind(price)
            .bind(owner.as_str())
            .bind(price)
            .execute(&mut *tx)
            .await?;
            if spent.rows_affected() != 1 {
                return Ok(CraftOutcome::InsufficientSeeds);
            }
        }
        sqlx::query(
            "INSERT INTO craft_receipts
             (craft_id, owner_pub_dress, recipe_id, seeds_debited, started_at_ms)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(craft_id)
        .bind(owner.as_str())
        .bind(recipe.id)
        .bind(price)
        .bind(at)
        .execute(&mut *tx)
        .await?;
        for attestation in &asked {
            sqlx::query("INSERT INTO craft_inputs (attestation_id, craft_id) VALUES (?, ?)")
                .bind(attestation)
                .bind(craft_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(CraftOutcome::Started { seeds: recipe.seeds })
    }
}

#[cfg(test)]
mod craft_tests {
    use super::{AttestOutcome, CraftOutcome, IdentityRepository, SaleOutcome};
    use crate::find_attestation::{Finder, ReportedPickup, VerifiedPickup};
    use crate::{ProviderIdentity, PubDress};

    const SESSION: [u8; 32] = [7; 32];

    async fn repository() -> (IdentityRepository, PubDress, PubDress) {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository");
        let sky: PubDress = "0x1sky".parse().expect("owner");
        let other: PubDress = "0xfrSb".parse().expect("other");
        for (owner, telegram) in [(&sky, 7), (&other, 8)] {
            repository
                .register(owner, &ProviderIdentity::telegram(telegram), 100)
                .await
                .expect("registration");
            sqlx::query(
                "INSERT INTO native_sessions (token_hash, pub_dress, created_at, expires_at, active)
                 VALUES (?, ?, 0, 4000000000, 1)",
            )
            .bind(if telegram == 7 { SESSION.to_vec() } else { vec![8; 32] })
            .bind(owner.as_str())
            .execute(&repository.pool)
            .await
            .expect("session");
        }
        (repository, sky, other)
    }

    async fn attest(repository: &IdentityRepository, owner: &PubDress, sha: char, item: &'static str) -> String {
        let pickup = VerifiedPickup {
            reported: ReportedPickup {
                artifact_id: format!("art:seg:1:2:e2961:1:0#{sha}"),
                finder: Finder::Bond,
                found_at_ms: 1_000,
                longitude_e7: 305_000_000,
                latitude_e7: 504_000_000,
            },
            artifact_sha: sha.to_string().repeat(64),
            epoch: 2_961,
            tier: 3,
            item_id: item,
            pickup_credit: 0,
        };
        match repository.attest_pickup(owner, &pickup, 2_000).await.unwrap() {
            AttestOutcome::Attested { attestation_id, .. } => attestation_id,
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_repair_debits_cores_price_once_and_uses_up_its_find() {
        let (repository, sky, _) = repository().await;
        let broken = attest(&repository, &sky, 'a', "broken_cd_player").await;
        let start = || {
            repository.start_craft_with_seeds(
                &sky,
                &SESSION,
                "craft-0001",
                "repair_cd_player",
                std::slice::from_ref(&broken),
                3_000,
            )
        };
        assert_eq!(start().await.unwrap(), CraftOutcome::Started { seeds: 200 });
        assert_eq!(repository.read_seeds_balance(&sky).await.unwrap(), 10_000 - 200);
        assert_eq!(start().await.unwrap(), CraftOutcome::AlreadyStarted { seeds: 200 });
        assert_eq!(repository.read_seeds_balance(&sky).await.unwrap(), 10_000 - 200);
        // Used up, the broken player is neither sold nor used again.
        assert_eq!(
            repository.sell_attested(&sky, &broken, 4_000).await.unwrap(),
            SaleOutcome::UsedUp
        );
        assert_eq!(
            repository
                .start_craft_with_seeds(&sky, &SESSION, "craft-0002", "repair_cd_player", &[broken], 5_000)
                .await
                .unwrap(),
            CraftOutcome::WrongInputs
        );
    }

    #[tokio::test]
    async fn refuses_wrong_sold_foreign_or_unverifiable_inputs() {
        let (repository, sky, other) = repository().await;
        let cd = attest(&repository, &sky, 'b', "scratched_cd").await;
        let theirs = attest(&repository, &other, 'c', "broken_cd_player").await;
        let sold = attest(&repository, &sky, 'd', "broken_dictaphone").await;
        assert!(matches!(
            repository.sell_attested(&sky, &sold, 2_500).await.unwrap(),
            SaleOutcome::Sold { .. }
        ));
        for (recipe, inputs) in [
            ("repair_cd_player", vec![cd.clone()]),
            ("repair_cd_player", vec![theirs]),
            ("repair_dictaphone", vec![sold]),
            ("craft_album", vec![cd.clone(), cd.clone(), cd.clone()]),
            ("repair_cd_player", vec![]),
        ] {
            assert_eq!(
                repository
                    .start_craft_with_seeds(&sky, &SESSION, "craft-0003", recipe, &inputs, 3_000)
                    .await
                    .unwrap(),
                CraftOutcome::WrongInputs,
                "{recipe}"
            );
        }
        assert_eq!(
            repository
                .start_craft_with_seeds(&sky, &SESSION, "craft-0004", "craft_kyiv_anthology", &[], 3_000)
                .await
                .unwrap(),
            CraftOutcome::UnverifiableInputs
        );
        assert_eq!(
            repository
                .start_craft_with_seeds(&sky, &SESSION, "craft-0005", "nonsense", &[], 3_000)
                .await
                .unwrap(),
            CraftOutcome::UnknownRecipe
        );
        // A refusal commits nothing, not even the starter credit: only the
        // dictaphone's sale was credited.
        assert_eq!(repository.read_seeds_balance(&sky).await.unwrap(), 50);
    }

    #[tokio::test]
    async fn needs_the_active_session_and_enough_seeds_and_refuses_a_changed_replay() {
        let (repository, sky, _) = repository().await;
        let broken = attest(&repository, &sky, 'e', "broken_cd_player").await;
        assert_eq!(
            repository
                .start_craft_with_seeds(&sky, &[9; 32], "craft-0006", "repair_cd_player", std::slice::from_ref(&broken), 3_000)
                .await
                .unwrap(),
            CraftOutcome::SessionChanged
        );
        repository
            .mark_starter_seeds_issued_for_test(sky.as_str())
            .await
            .unwrap();
        let fund = |balance: i64| {
            sqlx::query(
                "INSERT INTO bond_seeds (owner_pub_dress, balance) VALUES (?, ?)
                 ON CONFLICT(owner_pub_dress) DO UPDATE SET balance = excluded.balance",
            )
            .bind(sky.as_str())
            .bind(balance)
            .execute(&repository.pool)
        };
        fund(199).await.unwrap();
        assert_eq!(
            repository
                .start_craft_with_seeds(&sky, &SESSION, "craft-0007", "repair_cd_player", std::slice::from_ref(&broken), 3_000)
                .await
                .unwrap(),
            CraftOutcome::InsufficientSeeds
        );
        fund(500).await.unwrap();
        assert_eq!(
            repository
                .start_craft_with_seeds(&sky, &SESSION, "craft-0008", "repair_cd_player", std::slice::from_ref(&broken), 3_000)
                .await
                .unwrap(),
            CraftOutcome::Started { seeds: 200 }
        );
        assert_eq!(
            repository
                .start_craft_with_seeds(&sky, &SESSION, "craft-0008", "repair_dictaphone", std::slice::from_ref(&broken), 3_000)
                .await
                .unwrap(),
            CraftOutcome::ConflictingReplay
        );
    }
}
