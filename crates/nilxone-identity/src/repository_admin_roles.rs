// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdminRoleRequest {
    pub pub_dress: String,
    pub requested_at: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdminRoleDecision {
    Approved,
    Denied,
    AlreadyDecided,
    Forbidden,
}

impl IdentityRepository {
    /// A successful sign-in is the trigger, never a typed address or a failed
    /// password. At most one durable request exists per reserved human Bond.
    pub async fn request_admin_review_on_login(
        &self,
        pub_dress: &str,
        now: u64,
    ) -> Result<(), RepositoryError> {
        if !pub_dress.starts_with("0x0") {
            return Ok(());
        }
        sqlx::query(
            "INSERT OR IGNORE INTO admin_role_requests (pub_dress, status, requested_at) \
             SELECT i.pub_dress, 'pending', ? FROM identities i \
             LEFT JOIN bond_roles r ON r.pub_dress = i.pub_dress \
             WHERE i.pub_dress = ? AND i.identity_kind = 'human' \
             AND COALESCE(r.role, 'user') <> 'admin'",
        )
        .bind(now as i64)
        .bind(pub_dress)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Shared, server-owned inbox: every admin sees all outstanding requests.
    pub async fn pending_admin_role_requests(
        &self,
    ) -> Result<Vec<AdminRoleRequest>, RepositoryError> {
        let rows = sqlx::query(
            "SELECT pub_dress, requested_at FROM admin_role_requests \
             WHERE status = 'pending' ORDER BY requested_at, pub_dress LIMIT 100",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|row| AdminRoleRequest {
            pub_dress: row.get("pub_dress"),
            requested_at: row.get("requested_at"),
        }).collect())
    }

    /// An authenticated admin decides atomically with the role mutation.
    /// Exactly one decision wins even when different admins act concurrently.
    pub async fn decide_admin_role_request(
        &self,
        actor: &str,
        candidate: &PubDress,
        approve: bool,
        now: u64,
    ) -> Result<AdminRoleDecision, RepositoryError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let is_admin: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM bond_roles r \
             JOIN identities i ON i.pub_dress = r.pub_dress \
             WHERE r.pub_dress = ? AND r.role = 'admin' AND i.identity_kind = 'human')",
        ).bind(actor).fetch_one(&mut *tx).await?;
        if is_admin == 0 {
            return Ok(AdminRoleDecision::Forbidden);
        }
        let result = sqlx::query(
            "UPDATE admin_role_requests SET status = ?, decided_by = ?, decided_at = ? \
             WHERE pub_dress = ? AND status = 'pending'",
        )
        .bind(if approve { "approved" } else { "denied" })
        .bind(actor)
        .bind(now as i64)
        .bind(candidate.as_str())
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Ok(AdminRoleDecision::AlreadyDecided);
        }
        if approve {
            sqlx::query(
                "INSERT INTO bond_roles (pub_dress, role) VALUES (?, 'admin') \
                 ON CONFLICT(pub_dress) DO UPDATE SET role = 'admin'",
            )
            .bind(candidate.as_str())
            .execute(&mut *tx)
            .await?;

            // Snapshot the Telegram-linked admins in the very same commit as
            // the role grant. This is never run for a denied or stale request.
            // Exclude the newly promoted Bond: the announcement goes to the
            // admins who were eligible to review the request.
            sqlx::query(
                "INSERT OR IGNORE INTO admin_role_telegram_confirmations \
                 (pub_dress, telegram_user_id) \
                 SELECT ?, CAST(p.provider_subject AS INTEGER) \
                 FROM bond_roles r \
                 JOIN identities i ON i.pub_dress = r.pub_dress AND i.identity_kind = 'human' \
                 JOIN identity_providers p ON p.pub_dress = r.pub_dress AND p.provider = 'telegram' \
                 WHERE r.role = 'admin' AND r.pub_dress <> ? \
                   AND p.provider_subject <> '' \
                   AND p.provider_subject NOT GLOB '*[^0-9]*' \
                   AND CAST(p.provider_subject AS INTEGER) > 0",
            )
            .bind(candidate.as_str())
            .bind(candidate.as_str())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(if approve {
            AdminRoleDecision::Approved
        } else {
            AdminRoleDecision::Denied
        })
    }
}

#[cfg(test)]
mod admin_role_tests {
    use super::*;
    use crate::BondAccessRole;

    #[tokio::test]
    async fn a_reserved_bond_needs_a_real_admin_decision() {
        let repository = IdentityRepository::connect("sqlite::memory:").await.expect("repository");
        for address in ["0x0admin", "0x0candidate", "0x1ordinary"] {
            sqlx::query("INSERT INTO identities (pub_dress, identity_kind, created_at) VALUES (?, 'human', 1)")
                .bind(address).execute(&repository.pool).await.expect("fixture identity");
        }
        let admin: PubDress = "0x0admin".parse().expect("admin");
        let candidate: PubDress = "0x0candidate".parse().expect("candidate");
        repository.set_role(&admin, BondAccessRole::Admin).await.expect("bootstrap");
        repository.request_admin_review_on_login("0x1ordinary", 2).await.expect("ordinary");
        repository.request_admin_review_on_login(admin.as_str(), 3).await.expect("existing admin");
        repository.request_admin_review_on_login(candidate.as_str(), 10).await.expect("request");
        repository.request_admin_review_on_login(candidate.as_str(), 20).await.expect("dedupe");
        let items = repository.pending_admin_role_requests().await.expect("inbox");
        assert_eq!(items, vec![AdminRoleRequest { pub_dress: candidate.to_string(), requested_at: 10 }]);
        assert_eq!(repository.role_for(candidate.as_str()).await.expect("role"), BondAccessRole::User);
        assert_eq!(
            repository.decide_admin_role_request(candidate.as_str(), &candidate, true, 30).await.expect("nonadmin"),
            AdminRoleDecision::Forbidden
        );
        assert_eq!(
            repository.decide_admin_role_request(admin.as_str(), &candidate, true, 30).await.expect("approve"),
            AdminRoleDecision::Approved
        );
        assert_eq!(
            repository.decide_admin_role_request(admin.as_str(), &candidate, false, 31).await.expect("second decision"),
            AdminRoleDecision::AlreadyDecided
        );
        assert_eq!(repository.role_for(candidate.as_str()).await.expect("role"), BondAccessRole::Admin);
        assert!(repository.pending_admin_role_requests().await.expect("inbox").is_empty());
    }

    #[tokio::test]
    async fn denial_does_not_grant_privileges_or_reopen_on_login() {
        let repository = IdentityRepository::connect("sqlite::memory:").await.expect("repository");
        for address in ["0x0admin", "0x0pending"] {
            sqlx::query("INSERT INTO identities (pub_dress, identity_kind, created_at) VALUES (?, 'human', 1)")
                .bind(address).execute(&repository.pool).await.expect("fixture identity");
        }
        let admin: PubDress = "0x0admin".parse().expect("admin");
        let pending: PubDress = "0x0pending".parse().expect("pending");
        repository.set_role(&admin, BondAccessRole::Admin).await.expect("bootstrap");
        repository.request_admin_review_on_login(pending.as_str(), 10).await.expect("request");
        assert_eq!(repository.decide_admin_role_request(admin.as_str(), &pending, false, 12).await.expect("deny"), AdminRoleDecision::Denied);
        repository.request_admin_review_on_login(pending.as_str(), 13).await.expect("no reopen");
        assert!(repository.pending_admin_role_requests().await.expect("empty").is_empty());
        assert_eq!(repository.role_for(pending.as_str()).await.expect("role"), BondAccessRole::User);
    }
}
