// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

// Which Bond a verified aiaiaiai Identity sign-in belongs to. 0x1 keeps this
// mapping itself; it never asks the issuer's storage (`aiaiaiai_sign_in`).

/// Where a verified sign-in lands in 0x1.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AiaiaiaiAccountOutcome {
    /// The subject belongs to this Bond. `newly_linked` is true on the first
    /// sign-in, when the issuer-asserted address named it.
    Linked { pub_dress: PubDress, newly_linked: bool },
    /// No Bond is linked to the subject and the token names none that exists.
    Unknown,
    /// The token names a Bond that is already linked to a different subject
    /// of this issuer. Linking it again would let a second Identity account
    /// take the Bond over, so this needs the owner, not a retry.
    BondLinkedElsewhere,
}

impl IdentityRepository {
    /// Resolves, and on first sight links, the Bond of a verified sign-in in
    /// one immediate transaction, so two concurrent first sign-ins cannot
    /// link one Bond twice.
    pub async fn resolve_aiaiaiai_account(
        &self,
        sign_in: &crate::aiaiaiai_sign_in::VerifiedSignIn,
        now: u64,
    ) -> Result<AiaiaiaiAccountOutcome, RepositoryError> {
        let linked_at = i64::try_from(now).map_err(|_| sqlx::Error::Protocol("time overflow".into()))?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let linked: Option<String> = sqlx::query_scalar(
            "SELECT pub_dress FROM aiaiaiai_accounts WHERE issuer = ? AND subject = ?",
        )
        .bind(&sign_in.issuer)
        .bind(&sign_in.subject)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(stored) = linked {
            transaction.commit().await?;
            let pub_dress =
                PubDress::from_str(&stored).map_err(|_| RepositoryError::CorruptHumanPubDress)?;
            return Ok(AiaiaiaiAccountOutcome::Linked { pub_dress, newly_linked: false });
        }

        let Some(pub_dress) = sign_in
            .pub_dress
            .as_deref()
            .and_then(|value| PubDress::from_str(value).ok())
        else {
            transaction.rollback().await?;
            return Ok(AiaiaiaiAccountOutcome::Unknown);
        };
        let human_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM identities \
             WHERE pub_dress = ? AND identity_kind = 'human')",
        )
        .bind(pub_dress.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        if !human_exists {
            transaction.rollback().await?;
            return Ok(AiaiaiaiAccountOutcome::Unknown);
        }
        let taken = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM aiaiaiai_accounts WHERE issuer = ? AND pub_dress = ?)",
        )
        .bind(&sign_in.issuer)
        .bind(pub_dress.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        if taken {
            transaction.rollback().await?;
            return Ok(AiaiaiaiAccountOutcome::BondLinkedElsewhere);
        }
        sqlx::query(
            "INSERT INTO aiaiaiai_accounts (issuer, subject, pub_dress, linked_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(&sign_in.issuer)
        .bind(&sign_in.subject)
        .bind(pub_dress.as_str())
        .bind(linked_at)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(AiaiaiaiAccountOutcome::Linked { pub_dress, newly_linked: true })
    }
}

#[cfg(test)]
mod aiaiaiai_account_tests {
    use std::str::FromStr;

    use super::{AiaiaiaiAccountOutcome, IdentityRepository};
    use crate::{NativeAuthConfig, PubDress, aiaiaiai_sign_in::VerifiedSignIn};

    const ISSUER: &str = "https://auth.aiaiaiai.org";

    async fn repository_with(humans: &[&str]) -> IdentityRepository {
        let repository = IdentityRepository::connect("sqlite::memory:").await.expect("repository");
        for address in humans {
            sqlx::query("INSERT INTO identities (pub_dress, identity_kind, created_at) VALUES (?, 'human', 1)")
                .bind(address)
                .execute(&repository.pool)
                .await
                .expect("human");
        }
        repository
    }

    fn sign_in(subject: &str, pub_dress: Option<&str>) -> VerifiedSignIn {
        VerifiedSignIn {
            issuer: ISSUER.to_owned(),
            subject: subject.to_owned(),
            pub_dress: pub_dress.map(str::to_owned),
            auth_time: 100,
        }
    }

    fn linked(pub_dress: &str, newly_linked: bool) -> AiaiaiaiAccountOutcome {
        AiaiaiaiAccountOutcome::Linked { pub_dress: PubDress::from_str(pub_dress).unwrap(), newly_linked }
    }

    #[tokio::test]
    async fn links_on_first_sign_in_and_then_keys_by_subject() {
        let repository = repository_with(&["0x0sky"]).await;
        let first = repository.resolve_aiaiaiai_account(&sign_in("s1", Some("0x0sky")), 100).await.unwrap();
        assert_eq!(first, linked("0x0sky", true));
        // Later sign-ins resolve by subject, whatever address the token names.
        let again = repository.resolve_aiaiaiai_account(&sign_in("s1", None), 200).await.unwrap();
        assert_eq!(again, linked("0x0sky", false));
    }

    #[tokio::test]
    async fn a_linked_subject_follows_a_rename() {
        let repository = repository_with(&["0x0sky"]).await;
        repository.resolve_aiaiaiai_account(&sign_in("s1", Some("0x0sky")), 100).await.unwrap();
        sqlx::query("UPDATE identities SET pub_dress = '0x0sea' WHERE pub_dress = '0x0sky'")
            .execute(&repository.pool)
            .await
            .unwrap();
        let after = repository.resolve_aiaiaiai_account(&sign_in("s1", Some("0x0sky")), 200).await.unwrap();
        assert_eq!(after, linked("0x0sea", false));
    }

    #[tokio::test]
    async fn a_second_subject_cannot_take_a_linked_bond() {
        let repository = repository_with(&["0x0sky"]).await;
        repository.resolve_aiaiaiai_account(&sign_in("s1", Some("0x0sky")), 100).await.unwrap();
        let other = repository.resolve_aiaiaiai_account(&sign_in("s2", Some("0x0sky")), 200).await.unwrap();
        assert_eq!(other, AiaiaiaiAccountOutcome::BondLinkedElsewhere);
    }

    #[tokio::test]
    async fn an_unknown_subject_without_a_bond_is_not_linked() {
        let repository = repository_with(&["0x0sky"]).await;
        for claim in [None, Some("0x0nobody"), Some("not an address")] {
            let outcome = repository.resolve_aiaiaiai_account(&sign_in("s1", claim), 100).await.unwrap();
            assert_eq!(outcome, AiaiaiaiAccountOutcome::Unknown, "{claim:?}");
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM aiaiaiai_accounts")
            .fetch_one(&repository.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn pairwise_subjects_are_stable_per_client_and_differ_across_clients() {
        let repository = repository_with(&["0x0sky", "0x0sea"]).await;
        let digester = NativeAuthConfig::new([1; 32], [2; 32]).unwrap().secret_digester();
        let sky = PubDress::from_str("0x0sky").unwrap();
        let sea = PubDress::from_str("0x0sea").unwrap();
        let first = repository.client_pairwise_subject(&sky, "nilx-one", &digester).await.unwrap().unwrap();
        let again = repository.client_pairwise_subject(&sky, "nilx-one", &digester).await.unwrap().unwrap();
        let market = repository.client_pairwise_subject(&sky, "0xda-market", &digester).await.unwrap().unwrap();
        let other = repository.client_pairwise_subject(&sea, "nilx-one", &digester).await.unwrap().unwrap();
        assert_eq!(first, again);
        assert_ne!(first, market);
        assert_ne!(first, other);
        let private: Vec<u8> = sqlx::query_scalar("SELECT subject FROM identity_subjects WHERE pub_dress = '0x0sky'")
            .fetch_one(&repository.pool)
            .await
            .unwrap();
        let hex: String = private.iter().map(|byte| format!("{byte:02x}")).collect();
        assert!(!first.contains(&hex));
        let missing = PubDress::from_str("0x0nobody").unwrap();
        assert_eq!(repository.client_pairwise_subject(&missing, "nilx-one", &digester).await.unwrap(), None);
    }
}
