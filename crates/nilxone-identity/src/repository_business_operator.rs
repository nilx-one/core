// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

/// A deliberately small operator projection. Never includes provider subjects,
/// sessions, password hashes, private subjects or recovery material.
///
/// The last four fields exist so an operator can snapshot who held what before
/// a reset (to honour a bonus, or to see who ever came back). They are the
/// least that serves that: when the Bond was made, whether it ever got a
/// password, its seeds balance, and whether a subscription clock exists.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct OperatorBond {
    pub pub_dress: String,
    pub global_role: String,
    pub created_at: String,
    pub has_password: bool,
    pub seeds: u64,
    pub has_subscription: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperatorBondKind {
    Human,
    Business,
}

/// Only these two roles may be requested by the initial operator surface.
/// Other stored roles remain valid and are never silently overwritten.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperatorMemberRole {
    Member,
    Owner,
}

impl OperatorMemberRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Member => "member",
            Self::Owner => "owner",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperatorMembershipOutcome {
    Added,
    Promoted,
    AlreadyMember,
    MemberNotHuman,
    BusinessNotFound,
    RoleConflict,
}

impl IdentityRepository {
    /// Opens an existing operator-selected database WITHOUT initialization,
    /// backfills, schema migration, reserved-prefix grants or WAL-mode changes.
    /// Authorization and selection of the production volume belong to infra.
    /// Read-only callers additionally get SQLite's own read-only enforcement.
    pub async fn connect_operator(
        database_url: &str,
        read_only: bool,
    ) -> Result<Self, RepositoryError> {
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(false)
            .foreign_keys(true)
            .read_only(read_only);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        Ok(Self { pool })
    }

    /// Lists people or Business Bonds, sorted by their exact binary address.
    /// Avaia and business stations are not human members. In the existing
    /// schema a Business Bond has identity_kind=human AND global role business.
    pub async fn operator_bonds(
        &self,
        kind: OperatorBondKind,
    ) -> Result<Vec<OperatorBond>, RepositoryError> {
        let rows = sqlx::query(
            "SELECT i.pub_dress, COALESCE(r.role, 'user') AS global_role,
                    CAST(i.created_at AS TEXT) AS created_at,
                    EXISTS (SELECT 1 FROM native_credentials c
                            WHERE c.pub_dress = i.pub_dress) AS has_password,
                    COALESCE((SELECT balance FROM bond_seeds s
                              WHERE s.owner_pub_dress = i.pub_dress), 0) AS seeds,
                    EXISTS (SELECT 1 FROM bond_subscription_clocks k
                            WHERE k.owner_pub_dress = i.pub_dress) AS has_subscription
             FROM identities i LEFT JOIN bond_roles r ON r.pub_dress = i.pub_dress
             WHERE i.identity_kind = 'human'
               AND ((COALESCE(r.role, 'user') = 'business') = ?)
             ORDER BY i.pub_dress COLLATE BINARY",
        )
        .bind(kind == OperatorBondKind::Business)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(OperatorBond {
                    pub_dress: row.try_get("pub_dress")?,
                    global_role: row.try_get("global_role")?,
                    created_at: row.try_get("created_at")?,
                    has_password: row.try_get("has_password")?,
                    seeds: u64::try_from(row.try_get::<i64, _>("seeds")?)
                        .map_err(|_| RepositoryError::CorruptSeedsBalance)?,
                    has_subscription: row.try_get("has_subscription")?,
                })
            })
            .collect()
    }

    /// Operator-only creation with a REQUIRED, existing human owner. Uses the
    /// same atomic allocation as authenticated creation, without impersonating
    /// the selected owner or exposing an arbitrary-owner public HTTP route.
    pub async fn operator_create_business(
        &self,
        business: &PubDress,
        owner: &PubDress,
        now: u64,
    ) -> Result<CreateBusinessOrganizationOutcome, RepositoryError> {
        self.create_business_organization(owner, business, "owner", now)
            .await
    }

    /// Add is strict: a duplicate is an error outcome. A member becomes owner
    /// only when owner was explicitly requested. No ordinal role comparison,
    /// demotion, ownership transfer or change to a legacy role is implicit.
    pub async fn operator_add_member(
        &self,
        business: &PubDress,
        member: &PubDress,
        role: OperatorMemberRole,
        now: u64,
    ) -> Result<OperatorMembershipOutcome, RepositoryError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let human: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM identities i
             LEFT JOIN bond_roles r ON r.pub_dress = i.pub_dress
             WHERE i.pub_dress = ? AND i.identity_kind = 'human'
               AND COALESCE(r.role, 'user') != 'business')",
        )
        .bind(member.as_str())
        .fetch_one(&mut *tx)
        .await?;
        if !human {
            tx.rollback().await?;
            return Ok(OperatorMembershipOutcome::MemberNotHuman);
        }
        let is_business: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM identities i
             JOIN bond_roles r ON r.pub_dress = i.pub_dress
             WHERE i.pub_dress = ? AND i.identity_kind = 'human'
               AND r.role = 'business')",
        )
        .bind(business.as_str())
        .fetch_one(&mut *tx)
        .await?;
        if !is_business {
            tx.rollback().await?;
            return Ok(OperatorMembershipOutcome::BusinessNotFound);
        }
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT role FROM business_memberships
             WHERE business_pub_dress = ? AND member_pub_dress = ?",
        )
        .bind(business.as_str())
        .bind(member.as_str())
        .fetch_optional(&mut *tx)
        .await?;
        let result = match existing.as_deref() {
            None => {
                sqlx::query(
                    "INSERT INTO business_memberships
                     (business_pub_dress, member_pub_dress, role, created_at)
                     VALUES (?, ?, ?, ?)",
                )
                .bind(business.as_str())
                .bind(member.as_str())
                .bind(role.as_str())
                .bind(i64::try_from(now).map_err(|_| sqlx::Error::Protocol("invalid time".into()))?)
                .execute(&mut *tx)
                .await?;
                OperatorMembershipOutcome::Added
            }
            Some("member") if role == OperatorMemberRole::Owner => {
                sqlx::query(
                    "UPDATE business_memberships SET role = 'owner'
                     WHERE business_pub_dress = ? AND member_pub_dress = ?",
                )
                .bind(business.as_str())
                .bind(member.as_str())
                .execute(&mut *tx)
                .await?;
                OperatorMembershipOutcome::Promoted
            }
            Some(value) if value == role.as_str() => {
                tx.rollback().await?;
                return Ok(OperatorMembershipOutcome::AlreadyMember);
            }
            Some(_) => {
                tx.rollback().await?;
                return Ok(OperatorMembershipOutcome::RoleConflict);
            }
        };
        tx.commit().await?;
        Ok(result)
    }
}

#[cfg(test)]
mod business_operator_tests {
    use super::*;

    #[tokio::test]
    async fn lists_people_separately_and_never_claims_existing_business() {
        let repo = IdentityRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        let alice: PubDress = "0x1alice".parse().unwrap();
        let bob: PubDress = "0x1bob".parse().unwrap();
        let business: PubDress = "0xdstudio".parse().unwrap();
        let missing: PubDress = "0x1missing".parse().unwrap();
        repo.register(&alice, &ProviderIdentity::telegram(401), 1)
            .await
            .unwrap();
        repo.register(&bob, &ProviderIdentity::telegram(402), 1)
            .await
            .unwrap();
        assert_eq!(
            repo.operator_create_business(&business, &missing, 2)
                .await
                .unwrap(),
            CreateBusinessOrganizationOutcome::CreatorNotHuman
        );
        assert!(
            repo.operator_bonds(OperatorBondKind::Business)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            repo.operator_create_business(&business, &alice, 3)
                .await
                .unwrap(),
            CreateBusinessOrganizationOutcome::Created
        );
        assert_eq!(
            repo.operator_create_business(&business, &bob, 4)
                .await
                .unwrap(),
            CreateBusinessOrganizationOutcome::AddressUnavailable
        );
        assert_eq!(
            repo.operator_create_business(&business, &alice, 4)
                .await
                .unwrap(),
            CreateBusinessOrganizationOutcome::IdempotentReplay
        );
        let people = repo.operator_bonds(OperatorBondKind::Human).await.unwrap();
        assert_eq!(
            people
                .iter()
                .map(|b| b.pub_dress.as_str())
                .collect::<Vec<_>>(),
            ["0x1alice", "0x1bob"]
        );
        // Telegram-registered and never given a password or a clock; seeds
        // are whatever the fixed starter rule credits, read straight from the wallet.
        assert!(people.iter().all(|b| !b.has_password && !b.has_subscription));
        assert!(people.iter().all(|b| !b.created_at.is_empty()));
        repo.grant_starter_seeds_once(&alice, 5_000).await.unwrap();
        let after = repo.operator_bonds(OperatorBondKind::Human).await.unwrap();
        assert_eq!(after[0].seeds, repo.read_seeds_balance(&alice).await.unwrap());
        assert!(after[0].seeds > 0);
        assert_eq!(after[1].seeds, 0);
        let businesses = repo
            .operator_bonds(OperatorBondKind::Business)
            .await
            .unwrap();
        assert_eq!(businesses.len(), 1);
        assert_eq!(businesses[0].pub_dress, business.to_string());
        assert_eq!(businesses[0].global_role, "business");
        assert!(!businesses[0].has_password);
        assert_eq!(businesses[0].seeds, 0);
        assert!(!businesses[0].has_subscription);
    }

    #[tokio::test]
    async fn duplicates_fail_and_only_explicit_owner_promotes() {
        let repo = IdentityRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        let person: PubDress = "0x1alice".parse().unwrap();
        let business: PubDress = "0xdstudio".parse().unwrap();
        repo.register(&person, &ProviderIdentity::telegram(411), 1)
            .await
            .unwrap();
        assert_eq!(
            repo.operator_add_member(&business, &person, OperatorMemberRole::Member, 2)
                .await
                .unwrap(),
            OperatorMembershipOutcome::BusinessNotFound
        );
        repo.register_business(&business, 2).await.unwrap();
        assert_eq!(
            repo.operator_add_member(&business, &business, OperatorMemberRole::Owner, 3)
                .await
                .unwrap(),
            OperatorMembershipOutcome::MemberNotHuman
        );
        assert_eq!(
            repo.operator_add_member(&business, &person, OperatorMemberRole::Member, 3)
                .await
                .unwrap(),
            OperatorMembershipOutcome::Added
        );
        assert_eq!(
            repo.operator_add_member(&business, &person, OperatorMemberRole::Member, 4)
                .await
                .unwrap(),
            OperatorMembershipOutcome::AlreadyMember
        );
        assert_eq!(
            repo.operator_add_member(&business, &person, OperatorMemberRole::Owner, 5)
                .await
                .unwrap(),
            OperatorMembershipOutcome::Promoted
        );
        assert_eq!(
            repo.operator_add_member(&business, &person, OperatorMemberRole::Owner, 6)
                .await
                .unwrap(),
            OperatorMembershipOutcome::AlreadyMember
        );
        assert_eq!(
            repo.operator_add_member(&business, &person, OperatorMemberRole::Member, 7)
                .await
                .unwrap(),
            OperatorMembershipOutcome::RoleConflict
        );
        let created: i64 = sqlx::query_scalar("SELECT created_at FROM business_memberships")
            .fetch_one(&repo.pool)
            .await
            .unwrap();
        assert_eq!(created, 3);
        sqlx::query("UPDATE business_memberships SET role = 'maintainer'")
            .execute(&repo.pool)
            .await
            .unwrap();
        assert_eq!(
            repo.operator_add_member(&business, &person, OperatorMemberRole::Owner, 8)
                .await
                .unwrap(),
            OperatorMembershipOutcome::RoleConflict
        );
    }

    #[tokio::test]
    async fn operator_read_does_not_initialize_or_create_a_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.db");
        let url = format!("sqlite://{}", path.display());
        assert!(
            IdentityRepository::connect_operator(&url, true)
                .await
                .is_err()
        );
        assert!(!path.exists());
        let repo = IdentityRepository::connect(&url).await.unwrap();
        let person: PubDress = "0x1alice".parse().unwrap();
        repo.register(&person, &ProviderIdentity::telegram(421), 1)
            .await
            .unwrap();
        repo.pool.close().await;
        let ro = IdentityRepository::connect_operator(&url, true)
            .await
            .unwrap();
        assert_eq!(
            ro.operator_bonds(OperatorBondKind::Human)
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(
            sqlx::query("DELETE FROM identities")
                .execute(&ro.pool)
                .await
                .is_err()
        );
        ro.pool.close().await;
        // A database with no service tables must remain unmigrated.
        let blank = dir.path().join("blank.db");
        std::fs::File::create(&blank).unwrap();
        let ro =
            IdentityRepository::connect_operator(&format!("sqlite://{}", blank.display()), true)
                .await
                .unwrap();
        assert!(ro.operator_bonds(OperatorBondKind::Human).await.is_err());
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type='table'")
                .fetch_one(&ro.pool)
                .await
                .unwrap();
        assert_eq!(count, 0);
    }
}
