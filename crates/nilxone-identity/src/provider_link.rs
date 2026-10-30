// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use std::str::FromStr;

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

use nilxone_contracts::{
    DisconnectDecision, DisconnectSnapshot, LinkDecision, LinkSnapshot, ProviderAccount,
    ProviderSubject, ProviderType, decide_link, decide_self_disconnect,
};

use crate::{IdentityProvider, ProviderIdentity, PubDress};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderLinkOutcome {
    Linked,
    AlreadyLinked,
    ProviderAlreadyLinked,
    ProviderTypeAlreadyLinked,
    IdentityMissing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelfDisconnectOutcome {
    Disconnected,
    NotLinked,
    /// Refused: removing this provider would leave the Bond with no other
    /// linked provider and no active native password, so its owner could
    /// never reach it again.
    SoleAccessPath,
}

#[derive(Clone, Debug)]
pub struct ProviderLinkRepository {
    pool: SqlitePool,
}

impl ProviderLinkRepository {
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let max_connections = if database_url.contains(":memory:") {
            1
        } else {
            5
        };
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(false)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await?;
        sqlx::raw_sql(include_str!(
            "../migrations/0011_provider_type_cardinality.sql"
        ))
        .execute(&pool)
        .await?;
        Ok(Self { pool })
    }

    /// Connects `provider` to `pub_dress`. The binding rules are Core's
    /// (`nilxone_contracts::decide_link`); this reads the snapshot Core decides
    /// on and stores what Core says, in one transaction.
    pub async fn link(
        &self,
        pub_dress: &PubDress,
        provider: &ProviderIdentity,
    ) -> Result<ProviderLinkOutcome, sqlx::Error> {
        let account = core_account(provider)?;
        let mut transaction = self.pool.begin().await?;
        let snapshot = link_snapshot(&mut transaction, pub_dress, provider).await?;
        let decision = decide_link(pub_dress, &account, &snapshot);
        if decision != LinkDecision::Insert {
            transaction.commit().await?;
            return Ok(link_outcome(decision));
        }

        let insert = sqlx::query(
            "INSERT INTO identity_providers (provider, provider_subject, pub_dress) VALUES (?, ?, ?) ON CONFLICT DO NOTHING",
        )
        .bind(provider.provider.as_str())
        .bind(&provider.subject)
        .bind(pub_dress.as_str())
        .execute(&mut *transaction)
        .await?;
        if insert.rows_affected() == 1 {
            transaction.commit().await?;
            return Ok(ProviderLinkOutcome::Linked);
        }

        // A concurrent link won the insert: decide again on what it stored.
        let snapshot = link_snapshot(&mut transaction, pub_dress, provider).await?;
        transaction.commit().await?;
        Ok(match decide_link(pub_dress, &account, &snapshot) {
            LinkDecision::Insert => ProviderLinkOutcome::IdentityMissing,
            decision => link_outcome(decision),
        })
    }

    /// The Bond `account` is bound to, if any.
    pub async fn owner(&self, account: &ProviderAccount) -> Result<Option<PubDress>, sqlx::Error> {
        sqlx::query_scalar::<_, String>(
            "SELECT pub_dress FROM identity_providers WHERE provider = ? AND provider_subject = ?",
        )
        .bind(account.provider.as_str())
        .bind(account.subject.as_str())
        .fetch_optional(&self.pool)
        .await?
        .as_deref()
        .map(stored_pub_dress)
        .transpose()
    }

    pub async fn list(&self, pub_dress: &PubDress) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar::<_, String>(
            "SELECT provider FROM identity_providers WHERE pub_dress = ? ORDER BY CASE provider WHEN 'telegram' THEN 0 WHEN 'discord' THEN 1 WHEN 'github' THEN 2 ELSE 99 END",
        )
        .bind(pub_dress.as_str())
        .fetch_all(&self.pool)
        .await
    }

    pub async fn unlink(
        &self,
        pub_dress: &PubDress,
        provider: IdentityProvider,
    ) -> Result<bool, sqlx::Error> {
        let deleted =
            sqlx::query("DELETE FROM identity_providers WHERE pub_dress = ? AND provider = ?")
                .bind(pub_dress.as_str())
                .bind(provider.as_str())
                .execute(&self.pool)
                .await?;
        Ok(deleted.rows_affected() > 0)
    }

    /// Detaches `provider` from `pub_dress` at that provider's own request,
    /// refusing when it is the only way back into the Bond. The remaining-access
    /// check and the delete run in one transaction, so a concurrent link,
    /// unlink, or password change cannot race this into an orphaned Bond.
    pub async fn unlink_self_service(
        &self,
        pub_dress: &PubDress,
        provider: IdentityProvider,
    ) -> Result<SelfDisconnectOutcome, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;

        let linked = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM identity_providers WHERE pub_dress = ? AND provider = ?)",
        )
        .bind(pub_dress.as_str())
        .bind(provider.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        let other_providers = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM identity_providers WHERE pub_dress = ? AND provider <> ?",
        )
        .bind(pub_dress.as_str())
        .bind(provider.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        let password_active = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM native_credentials WHERE pub_dress = ? AND active = 1)",
        )
        .bind(pub_dress.as_str())
        .fetch_one(&mut *transaction)
        .await?;

        let decision = decide_self_disconnect(DisconnectSnapshot {
            linked,
            other_providers: u32::try_from(other_providers).unwrap_or(u32::MAX),
            password_active,
        });
        match decision {
            DisconnectDecision::Delete => {
                sqlx::query("DELETE FROM identity_providers WHERE pub_dress = ? AND provider = ?")
                    .bind(pub_dress.as_str())
                    .bind(provider.as_str())
                    .execute(&mut *transaction)
                    .await?;
                transaction.commit().await?;
                Ok(SelfDisconnectOutcome::Disconnected)
            }
            DisconnectDecision::NotLinked => {
                transaction.rollback().await?;
                Ok(SelfDisconnectOutcome::NotLinked)
            }
            DisconnectDecision::SoleAccessPath => {
                transaction.rollback().await?;
                Ok(SelfDisconnectOutcome::SoleAccessPath)
            }
        }
    }
}

/// Reads what Core decides a link on. The GitHub evidence table, where it
/// exists, is consulted first, exactly as before the rules moved to Core: its
/// owner and its subject for this Bond take precedence over the provider map.
async fn link_snapshot(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    pub_dress: &PubDress,
    provider: &ProviderIdentity,
) -> Result<LinkSnapshot, sqlx::Error> {
    let bond_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM identities WHERE pub_dress = ? AND identity_kind = 'human')",
    )
    .bind(pub_dress.as_str())
    .fetch_one(&mut **transaction)
    .await?;
    if !bond_exists {
        return Ok(LinkSnapshot {
            bond_exists,
            account_owner: None,
            bond_subject: None,
        });
    }

    if provider.provider == IdentityProvider::Github {
        let evidence_table_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'github_evidence_connections')",
        )
        .fetch_one(&mut **transaction)
        .await?;
        if evidence_table_exists {
            let evidence_owner = sqlx::query_scalar::<_, String>(
                "SELECT pub_dress FROM github_evidence_connections WHERE github_user_id = ?",
            )
            .bind(&provider.subject)
            .fetch_optional(&mut **transaction)
            .await?;
            if let Some(owner) = evidence_owner.filter(|owner| owner != pub_dress.as_str()) {
                return Ok(LinkSnapshot {
                    bond_exists,
                    account_owner: Some(stored_pub_dress(&owner)?),
                    bond_subject: None,
                });
            }
            let evidence_subject = sqlx::query_scalar::<_, String>(
                "SELECT github_user_id FROM github_evidence_connections WHERE pub_dress = ?",
            )
            .bind(pub_dress.as_str())
            .fetch_optional(&mut **transaction)
            .await?;
            if let Some(subject) = evidence_subject.filter(|subject| *subject != provider.subject) {
                return Ok(LinkSnapshot {
                    bond_exists,
                    account_owner: None,
                    bond_subject: Some(stored_subject(&subject)?),
                });
            }
        }
    }

    let account_owner = sqlx::query_scalar::<_, String>(
        "SELECT pub_dress FROM identity_providers WHERE provider = ? AND provider_subject = ?",
    )
    .bind(provider.provider.as_str())
    .bind(&provider.subject)
    .fetch_optional(&mut **transaction)
    .await?;
    let bond_subject = sqlx::query_scalar::<_, String>(
        "SELECT provider_subject FROM identity_providers WHERE pub_dress = ? AND provider = ? LIMIT 1",
    )
    .bind(pub_dress.as_str())
    .bind(provider.provider.as_str())
    .fetch_optional(&mut **transaction)
    .await?;
    Ok(LinkSnapshot {
        bond_exists,
        account_owner: account_owner.as_deref().map(stored_pub_dress).transpose()?,
        bond_subject: bond_subject.as_deref().map(stored_subject).transpose()?,
    })
}

fn core_account(provider: &ProviderIdentity) -> Result<ProviderAccount, sqlx::Error> {
    let provider_type = match provider.provider {
        IdentityProvider::Telegram => ProviderType::Telegram,
        IdentityProvider::Discord => ProviderType::Discord,
        IdentityProvider::Github => ProviderType::Github,
    };
    Ok(ProviderAccount {
        provider: provider_type,
        subject: stored_subject(&provider.subject)?,
    })
}

fn stored_subject(value: &str) -> Result<ProviderSubject, sqlx::Error> {
    value
        .parse()
        .map_err(|error| sqlx::Error::Decode(Box::new(error)))
}

fn stored_pub_dress(value: &str) -> Result<PubDress, sqlx::Error> {
    value
        .parse()
        .map_err(|error| sqlx::Error::Decode(Box::new(error)))
}

const fn link_outcome(decision: LinkDecision) -> ProviderLinkOutcome {
    match decision {
        LinkDecision::Insert => ProviderLinkOutcome::Linked,
        LinkDecision::AlreadyLinked => ProviderLinkOutcome::AlreadyLinked,
        LinkDecision::ProviderAlreadyLinked => ProviderLinkOutcome::ProviderAlreadyLinked,
        LinkDecision::ProviderTypeAlreadyLinked => ProviderLinkOutcome::ProviderTypeAlreadyLinked,
        LinkDecision::IdentityMissing => ProviderLinkOutcome::IdentityMissing,
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{ProviderLinkOutcome, ProviderLinkRepository, SelfDisconnectOutcome};
    use crate::{
        GithubEvidenceRepository, IdentityProvider, IdentityRepository, ProviderIdentity, PubDress,
    };

    async fn register_bond(
        identities: &IdentityRepository,
        pub_dress: &str,
        seed: &str,
    ) -> PubDress {
        let bond = PubDress::from_str(pub_dress).expect("valid Bond");
        identities
            .register_native(
                &bond,
                "hash",
                1,
                format!("recovery-{seed}").as_bytes(),
                format!("challenge-{seed}").as_bytes(),
                format!("idempotency-{seed}").as_bytes(),
                100,
                200,
            )
            .await
            .expect("native Bond registration");
        bond
    }

    #[tokio::test]
    async fn verified_provider_can_only_link_to_an_existing_human_bond() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("identity.sqlite");
        let database_url = format!("sqlite://{}", database.display());
        let identities = IdentityRepository::connect(&database_url)
            .await
            .expect("identity repository");
        let links = ProviderLinkRepository::connect(&database_url)
            .await
            .expect("provider link repository");
        let bond = register_bond(&identities, "0x1sky", "first").await;

        assert_eq!(
            links
                .link(&bond, &ProviderIdentity::telegram(42))
                .await
                .expect("link"),
            ProviderLinkOutcome::Linked
        );
        assert_eq!(
            links
                .link(&bond, &ProviderIdentity::telegram(42))
                .await
                .expect("idempotent link"),
            ProviderLinkOutcome::AlreadyLinked
        );
        assert_eq!(
            identities
                .find_by_provider(&ProviderIdentity::telegram(42))
                .await
                .expect("provider lookup")
                .expect("bound identity")
                .pub_dress,
            "0x1sky"
        );
        assert_eq!(
            links
                .link(&bond, &ProviderIdentity::github(75973992))
                .await
                .expect("GitHub link"),
            ProviderLinkOutcome::Linked
        );
        assert_eq!(
            links.list(&bond).await.expect("provider list"),
            vec!["telegram".to_owned(), "github".to_owned()]
        );
    }

    #[tokio::test]
    async fn one_bond_has_one_account_per_provider_type_and_can_disconnect_it() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("identity.sqlite");
        let database_url = format!("sqlite://{}", database.display());
        let identities = IdentityRepository::connect(&database_url)
            .await
            .expect("identity repository");
        let links = ProviderLinkRepository::connect(&database_url)
            .await
            .expect("provider link repository");
        let bond = register_bond(&identities, "0x1sky", "one-provider").await;

        assert_eq!(
            links
                .link(&bond, &ProviderIdentity::github(1))
                .await
                .expect("first GitHub link"),
            ProviderLinkOutcome::Linked
        );
        assert_eq!(
            links
                .link(&bond, &ProviderIdentity::github(2))
                .await
                .expect("second GitHub link"),
            ProviderLinkOutcome::ProviderTypeAlreadyLinked
        );
        assert_eq!(
            links.list(&bond).await.expect("provider list"),
            vec!["github".to_owned()]
        );
        assert!(
            links
                .unlink(&bond, IdentityProvider::Github)
                .await
                .expect("unlink")
        );
        assert!(links.list(&bond).await.expect("provider list").is_empty());
        assert!(
            !links
                .unlink(&bond, IdentityProvider::Github)
                .await
                .expect("idempotent absence")
        );
    }

    #[tokio::test]
    async fn github_provider_binding_cannot_drift_from_existing_evidence_account() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("identity.sqlite");
        let database_url = format!("sqlite://{}", database.display());
        let identities = IdentityRepository::connect(&database_url)
            .await
            .expect("identity repository");
        let links = ProviderLinkRepository::connect(&database_url)
            .await
            .expect("provider link repository");
        let _evidence = GithubEvidenceRepository::connect(&database_url)
            .await
            .expect("GitHub evidence repository");
        let first = register_bond(&identities, "0x1sky", "evidence-first").await;
        let second = register_bond(&identities, "0x2sky", "evidence-second").await;

        sqlx::query(
            "INSERT INTO github_evidence_connections \
             (pub_dress, github_user_id, login, profile_url, avatar_url, encrypted_access_token, connection_state, connected_at, refreshed_at) \
             VALUES (?, '42', 'evidence-user', 'https://github.com/evidence-user', 'https://avatars.example/42', X'01', 'connected', 1, 1)",
        )
        .bind(first.as_str())
        .execute(&links.pool)
        .await
        .expect("evidence fixture");

        assert_eq!(
            links
                .link(&first, &ProviderIdentity::github(43))
                .await
                .expect("different account for evidence owner"),
            ProviderLinkOutcome::ProviderTypeAlreadyLinked
        );
        assert_eq!(
            links
                .link(&second, &ProviderIdentity::github(42))
                .await
                .expect("evidence account on another Bond"),
            ProviderLinkOutcome::ProviderAlreadyLinked
        );
        assert_eq!(
            links
                .link(&first, &ProviderIdentity::github(42))
                .await
                .expect("matching account"),
            ProviderLinkOutcome::Linked
        );
    }

    #[tokio::test]
    async fn initialized_provider_link_storage_enforces_provider_type_cardinality_directly() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("identity.sqlite");
        let database_url = format!("sqlite://{}", database.display());
        let identities = IdentityRepository::connect(&database_url)
            .await
            .expect("identity repository");
        let links = ProviderLinkRepository::connect(&database_url)
            .await
            .expect("provider link repository");
        let bond = register_bond(&identities, "0x1sky", "db-boundary").await;

        links
            .link(&bond, &ProviderIdentity::github(1))
            .await
            .expect("first GitHub link");
        let duplicate = sqlx::query(
            "INSERT INTO identity_providers (provider, provider_subject, pub_dress) VALUES ('github', '2', ?)",
        )
        .bind(bond.as_str())
        .execute(&links.pool)
        .await;
        assert!(
            duplicate.is_err(),
            "database must reject a second GitHub account for one Bond"
        );
    }

    #[tokio::test]
    async fn provider_binding_cannot_be_moved_between_bonds() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("identity.sqlite");
        let database_url = format!("sqlite://{}", database.display());
        let identities = IdentityRepository::connect(&database_url)
            .await
            .expect("identity repository");
        let links = ProviderLinkRepository::connect(&database_url)
            .await
            .expect("provider link repository");
        let first = register_bond(&identities, "0x1sky", "first").await;
        let second = register_bond(&identities, "0x2sky", "second").await;
        let provider = ProviderIdentity::discord("42");

        assert_eq!(
            links.link(&first, &provider).await.expect("first link"),
            ProviderLinkOutcome::Linked
        );
        assert_eq!(
            links.link(&second, &provider).await.expect("second link"),
            ProviderLinkOutcome::ProviderAlreadyLinked
        );
    }

    #[tokio::test]
    async fn self_disconnect_removes_a_provider_when_another_access_path_remains() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("identity.sqlite");
        let database_url = format!("sqlite://{}", database.display());
        let identities = IdentityRepository::connect(&database_url)
            .await
            .expect("identity repository");
        let links = ProviderLinkRepository::connect(&database_url)
            .await
            .expect("provider link repository");
        let bond = register_bond(&identities, "0x1sky", "self-disconnect-other-provider").await;
        links
            .link(&bond, &ProviderIdentity::telegram(42))
            .await
            .expect("telegram link");
        links
            .link(&bond, &ProviderIdentity::github(1))
            .await
            .expect("github link");

        assert_eq!(
            links
                .unlink_self_service(&bond, IdentityProvider::Telegram)
                .await
                .expect("self disconnect"),
            SelfDisconnectOutcome::Disconnected
        );
        assert_eq!(
            links.list(&bond).await.expect("provider list"),
            vec!["github".to_owned()]
        );
    }

    #[tokio::test]
    async fn self_disconnect_refuses_to_orphan_a_bond_with_no_other_access_path() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("identity.sqlite");
        let database_url = format!("sqlite://{}", database.display());
        let identities = IdentityRepository::connect(&database_url)
            .await
            .expect("identity repository");
        let links = ProviderLinkRepository::connect(&database_url)
            .await
            .expect("provider link repository");
        // register_native leaves the native credential inactive until a
        // recovery-key acknowledgement, so this Bond starts with no active
        // password — exactly the state a Telegram-only registration is in.
        let bond = register_bond(&identities, "0x1sky", "self-disconnect-sole-path").await;
        links
            .link(&bond, &ProviderIdentity::telegram(42))
            .await
            .expect("telegram link");

        assert_eq!(
            links
                .unlink_self_service(&bond, IdentityProvider::Telegram)
                .await
                .expect("self disconnect"),
            SelfDisconnectOutcome::SoleAccessPath
        );
        assert_eq!(
            links.list(&bond).await.expect("provider list"),
            vec!["telegram".to_owned()]
        );
    }

    #[tokio::test]
    async fn self_disconnect_succeeds_when_an_active_native_password_remains() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("identity.sqlite");
        let database_url = format!("sqlite://{}", database.display());
        let identities = IdentityRepository::connect(&database_url)
            .await
            .expect("identity repository");
        let links = ProviderLinkRepository::connect(&database_url)
            .await
            .expect("provider link repository");
        let bond = register_bond(&identities, "0x1sky", "self-disconnect-active-password").await;
        identities
            .activate_native_registration(b"challenge-self-disconnect-active-password", 150)
            .await
            .expect("activation must succeed")
            .expect("registration challenge must still be valid");
        links
            .link(&bond, &ProviderIdentity::telegram(42))
            .await
            .expect("telegram link");

        assert_eq!(
            links
                .unlink_self_service(&bond, IdentityProvider::Telegram)
                .await
                .expect("self disconnect"),
            SelfDisconnectOutcome::Disconnected
        );
        assert!(links.list(&bond).await.expect("provider list").is_empty());
    }

    #[tokio::test]
    async fn self_disconnect_reports_not_linked_for_an_unlinked_provider() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("identity.sqlite");
        let database_url = format!("sqlite://{}", database.display());
        let identities = IdentityRepository::connect(&database_url)
            .await
            .expect("identity repository");
        let links = ProviderLinkRepository::connect(&database_url)
            .await
            .expect("provider link repository");
        let bond = register_bond(&identities, "0x1sky", "self-disconnect-not-linked").await;

        assert_eq!(
            links
                .unlink_self_service(&bond, IdentityProvider::Telegram)
                .await
                .expect("self disconnect"),
            SelfDisconnectOutcome::NotLinked
        );
    }
}
