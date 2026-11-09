// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

/// What became of a pick-up the service was asked to attest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttestOutcome {
    /// Attested now. `pickup_credit` seeds were credited (money), or 0.
    Attested { attestation_id: String, item_id: String, pickup_credit: u64 },
    /// This Bond had it attested before, on this or another device. Nothing
    /// changed; the same id answers.
    AlreadyYours { attestation_id: String, item_id: String },
    /// Another Bond picked this find of a claimed tier up first.
    Taken,
}

/// What became of a sale of an attested find.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SaleOutcome {
    /// Sold now; `credit` seeds were credited.
    Sold { credit: u64 },
    /// Sold before. Nothing changed; the first credit answers.
    AlreadySold { credit: u64 },
    /// No attestation of that id is this Bond's.
    Unknown,
    /// Money, a flyer: nothing a buyer pays for.
    NotForSale,
    /// Used up in a craft or repair.
    UsedUp,
}

fn new_attestation_id() -> Result<String, RepositoryError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| RepositoryError::CorruptSeedsBalance)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

async fn credit_seeds_in(
    transaction: &mut Transaction<'_, Sqlite>,
    owner: &PubDress,
    amount: u64,
) -> Result<(), RepositoryError> {
    let amount = i64::try_from(amount).map_err(|_| RepositoryError::CorruptSeedsBalance)?;
    let updated = sqlx::query(
        "INSERT INTO bond_seeds(owner_pub_dress, balance) VALUES (?, ?)
         ON CONFLICT(owner_pub_dress) DO UPDATE
         SET balance = bond_seeds.balance + excluded.balance
         WHERE bond_seeds.balance <= ?",
    )
    .bind(owner.as_str())
    .bind(amount)
    .bind(i64::MAX - amount)
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(RepositoryError::CorruptSeedsBalance);
    }
    Ok(())
}

impl IdentityRepository {
    /// Attests a verified pick-up for `owner`, once (docs/find-attestation.md).
    /// Money is credited in the same transaction. A rare find already claimed
    /// by another Bond through a committed award is `Taken` here too.
    pub async fn attest_pickup(
        &self,
        owner: &PubDress,
        pickup: &crate::find_attestation::VerifiedPickup,
        now_ms: u64,
    ) -> Result<AttestOutcome, RepositoryError> {
        let at = i64::try_from(now_ms).map_err(|_| RepositoryError::CorruptSeedsBalance)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let existing = sqlx::query_as::<_, (String, String, String)>(
            "SELECT attestation_id, owner_pub_dress, item_id FROM find_attestations
             WHERE artifact_sha = ? AND (owner_pub_dress = ? OR tier >= 4)",
        )
        .bind(&pickup.artifact_sha)
        .bind(owner.as_str())
        .fetch_all(&mut *transaction)
        .await?;
        if let Some((attestation_id, _, item_id)) =
            existing.iter().find(|(_, holder, _)| holder == owner.as_str())
        {
            return Ok(AttestOutcome::AlreadyYours {
                attestation_id: attestation_id.clone(),
                item_id: item_id.clone(),
            });
        }
        if !existing.is_empty() {
            return Ok(AttestOutcome::Taken);
        }
        if pickup.tier >= crate::finds::CLAIMED_MIN_TIER {
            let claimed_by_other = sqlx::query_scalar::<_, i64>(
                "SELECT EXISTS(SELECT 1 FROM find_claims
                 WHERE artifact_sha = ? AND owner_pub_dress != ?)",
            )
            .bind(&pickup.artifact_sha)
            .bind(owner.as_str())
            .fetch_one(&mut *transaction)
            .await?
                != 0;
            if claimed_by_other {
                return Ok(AttestOutcome::Taken);
            }
        }

        let attestation_id = new_attestation_id()?;
        let credit =
            i64::try_from(pickup.pickup_credit).map_err(|_| RepositoryError::CorruptSeedsBalance)?;
        sqlx::query(
            "INSERT INTO find_attestations
             (attestation_id, owner_pub_dress, artifact_id, artifact_sha, epoch, tier, item_id,
              finder, found_at_ms, longitude_e7, latitude_e7, attested_at_ms, pickup_credit)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&attestation_id)
        .bind(owner.as_str())
        .bind(&pickup.reported.artifact_id)
        .bind(&pickup.artifact_sha)
        .bind(pickup.epoch)
        .bind(i64::from(pickup.tier))
        .bind(pickup.item_id)
        .bind(pickup.reported.finder.as_str())
        .bind(pickup.reported.found_at_ms)
        .bind(pickup.reported.longitude_e7)
        .bind(pickup.reported.latitude_e7)
        .bind(at)
        .bind(credit)
        .execute(&mut *transaction)
        .await?;
        if pickup.pickup_credit > 0 {
            credit_seeds_in(&mut transaction, owner, pickup.pickup_credit).await?;
        }
        transaction.commit().await?;
        Ok(AttestOutcome::Attested {
            attestation_id,
            item_id: pickup.item_id.to_owned(),
            pickup_credit: pickup.pickup_credit,
        })
    }

    /// Sells the thing an attestation of `owner`'s names, once, for Core's
    /// price of that item. A retry answers the first credit and changes
    /// nothing.
    pub async fn sell_attested(
        &self,
        owner: &PubDress,
        attestation_id: &str,
        now_ms: u64,
    ) -> Result<SaleOutcome, RepositoryError> {
        let at = i64::try_from(now_ms).map_err(|_| RepositoryError::CorruptSeedsBalance)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query_as::<_, (String, Option<i64>)>(
            "SELECT item_id, sale_credit FROM find_attestations
             WHERE attestation_id = ? AND owner_pub_dress = ?",
        )
        .bind(attestation_id)
        .bind(owner.as_str())
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((item_id, sold)) = row else {
            return Ok(SaleOutcome::Unknown);
        };
        if let Some(credit) = sold {
            let credit = u64::try_from(credit).map_err(|_| RepositoryError::CorruptSeedsBalance)?;
            return Ok(SaleOutcome::AlreadySold { credit });
        }
        let used_up = sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(SELECT 1 FROM craft_inputs WHERE attestation_id = ?)",
        )
        .bind(attestation_id)
        .fetch_one(&mut *transaction)
        .await?
            != 0;
        if used_up {
            return Ok(SaleOutcome::UsedUp);
        }
        let Some(price) = crate::find_attestation::sale_price(&item_id) else {
            return Ok(SaleOutcome::NotForSale);
        };
        let stored_price = i64::try_from(price).map_err(|_| RepositoryError::CorruptSeedsBalance)?;
        let marked = sqlx::query(
            "UPDATE find_attestations SET sold_at_ms = ?, sale_credit = ?
             WHERE attestation_id = ? AND owner_pub_dress = ? AND sold_at_ms IS NULL",
        )
        .bind(at)
        .bind(stored_price)
        .bind(attestation_id)
        .bind(owner.as_str())
        .execute(&mut *transaction)
        .await?;
        if marked.rows_affected() != 1 {
            return Err(RepositoryError::CorruptSeedsBalance);
        }
        credit_seeds_in(&mut transaction, owner, price).await?;
        transaction.commit().await?;
        Ok(SaleOutcome::Sold { credit: price })
    }
}

#[cfg(test)]
mod find_attestation_tests {
    use super::{
        AttestOutcome, AwardOutcome, CommittedAward, ExperienceEarner, FindClaim,
        IdentityRepository, SaleOutcome,
    };
    use crate::find_attestation::{Finder, ReportedPickup, VerifiedPickup};
    use crate::{ProviderIdentity, PubDress};

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
        }
        (repository, sky, other)
    }

    fn verified(sha: char, tier: u8, item_id: &'static str, pickup_credit: u64) -> VerifiedPickup {
        VerifiedPickup {
            reported: ReportedPickup {
                artifact_id: format!("art:seg:1:2:e2961:1:0#{sha}"),
                finder: Finder::Bond,
                found_at_ms: 1_000,
                longitude_e7: 305_000_000,
                latitude_e7: 504_000_000,
            },
            artifact_sha: sha.to_string().repeat(64),
            epoch: 2_961,
            tier,
            item_id,
            pickup_credit,
        }
    }

    #[tokio::test]
    async fn attests_once_and_sells_once_at_cores_price() {
        let (repository, sky, _) = repository().await;
        let before = repository.read_seeds_balance(&sky).await.unwrap();
        let AttestOutcome::Attested { attestation_id, item_id, pickup_credit } = repository
            .attest_pickup(&sky, &verified('a', 1, "bottle", 0), 2_000)
            .await
            .unwrap()
        else {
            panic!("attested")
        };
        assert_eq!((item_id.as_str(), pickup_credit), ("bottle", 0));
        assert_eq!(attestation_id.len(), 36);
        assert_eq!(
            repository
                .attest_pickup(&sky, &verified('a', 1, "bottle", 0), 3_000)
                .await
                .unwrap(),
            AttestOutcome::AlreadyYours {
                attestation_id: attestation_id.clone(),
                item_id: "bottle".to_owned()
            }
        );
        assert_eq!(
            repository.sell_attested(&sky, &attestation_id, 4_000).await.unwrap(),
            SaleOutcome::Sold { credit: 5 }
        );
        assert_eq!(
            repository.sell_attested(&sky, &attestation_id, 5_000).await.unwrap(),
            SaleOutcome::AlreadySold { credit: 5 }
        );
        assert_eq!(repository.read_seeds_balance(&sky).await.unwrap(), before + 5);
    }

    #[tokio::test]
    async fn money_credits_at_pick_up_and_is_not_sold() {
        let (repository, sky, _) = repository().await;
        let AttestOutcome::Attested { attestation_id, pickup_credit, .. } = repository
            .attest_pickup(&sky, &verified('b', 1, "small_change", 15), 2_000)
            .await
            .unwrap()
        else {
            panic!("attested")
        };
        assert_eq!(pickup_credit, 15);
        assert_eq!(repository.read_seeds_balance(&sky).await.unwrap(), 15);
        // A retry does not credit again.
        repository
            .attest_pickup(&sky, &verified('b', 1, "small_change", 15), 3_000)
            .await
            .unwrap();
        assert_eq!(repository.read_seeds_balance(&sky).await.unwrap(), 15);
        assert_eq!(
            repository.sell_attested(&sky, &attestation_id, 4_000).await.unwrap(),
            SaleOutcome::NotForSale
        );
    }

    #[tokio::test]
    async fn a_claimed_find_is_one_bonds_and_attestations_are_private() {
        let (repository, sky, other) = repository().await;
        let AttestOutcome::Attested { attestation_id, .. } = repository
            .attest_pickup(&sky, &verified('c', 5, "cd_player", 0), 2_000)
            .await
            .unwrap()
        else {
            panic!("attested")
        };
        assert_eq!(
            repository
                .attest_pickup(&other, &verified('c', 5, "cd_player", 0), 2_500)
                .await
                .unwrap(),
            AttestOutcome::Taken
        );
        // Another Bond cannot sell this Bond's attestation.
        assert_eq!(
            repository.sell_attested(&other, &attestation_id, 3_000).await.unwrap(),
            SaleOutcome::Unknown
        );
        assert_eq!(
            repository.sell_attested(&sky, &attestation_id, 3_000).await.unwrap(),
            SaleOutcome::Sold { credit: 600 }
        );
        // A common find is each Bond's own copy.
        assert!(matches!(
            repository
                .attest_pickup(&sky, &verified('d', 2, "scratched_cd", 0), 2_000)
                .await
                .unwrap(),
            AttestOutcome::Attested { .. }
        ));
        assert!(matches!(
            repository
                .attest_pickup(&other, &verified('d', 2, "scratched_cd", 0), 2_000)
                .await
                .unwrap(),
            AttestOutcome::Attested { .. }
        ));
    }

    #[tokio::test]
    async fn a_find_claimed_by_another_bonds_award_is_taken() {
        let (repository, sky, other) = repository().await;
        sqlx::query(
            "INSERT INTO find_claims (artifact_sha, epoch, tier, owner_pub_dress) VALUES (?, 2961, 5, ?)",
        )
        .bind("e".repeat(64))
        .bind(other.as_str())
        .execute(&repository.pool)
        .await
        .unwrap();
        assert_eq!(
            repository
                .attest_pickup(&sky, &verified('e', 5, "cd_player", 0), 2_000)
                .await
                .unwrap(),
            AttestOutcome::Taken
        );
    }

    #[tokio::test]
    async fn a_committed_award_cannot_claim_a_find_another_bond_had_attested() {
        let (repository, sky, other) = repository().await;
        repository
            .attest_pickup(&sky, &verified('f', 5, "cd_player", 0), 2_000)
            .await
            .unwrap();
        let award = CommittedAward {
            id: format!("xp:{}", "f".repeat(43)),
            parent: None,
            chain: "ch:phone".to_owned(),
            kind: "find_picked_up",
            tier: 5,
            earner: ExperienceEarner::Bond,
            amount: 400,
            weekly_cap: 10,
            claim: Some(FindClaim {
                artifact_sha: "f".repeat(64),
                epoch: 2_961,
                tier: 5,
            }),
            orb: None,
            cell_orb: None,
        };
        let (_, outcomes) = repository
            .commit_awards(&other, &[award], 2_961, 3)
            .await
            .unwrap();
        assert_eq!(outcomes, vec![AwardOutcome::Taken]);
    }
}
