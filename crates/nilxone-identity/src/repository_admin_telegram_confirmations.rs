// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

/// A queued notification derived from a committed, approved role request.
/// The role comes from the persisted Bond role, never from the 0x0 prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdminRoleTelegramConfirmation {
    pub pub_dress: String,
    pub role: String,
    pub telegram_user_id: i64,
}

impl IdentityRepository {
    /// Returns approved, undelivered results. A revoked role or a denied
    /// request cannot be presented as a successful promotion.
    pub async fn due_admin_role_telegram_confirmations(
        &self,
        now: u64,
    ) -> Result<Vec<AdminRoleTelegramConfirmation>, RepositoryError> {
        let cutoff = (now as i64).saturating_sub(300);
        let rows = sqlx::query(
            "SELECT c.pub_dress, r.role, c.telegram_user_id \
             FROM admin_role_telegram_confirmations c \
             JOIN admin_role_requests q ON q.pub_dress = c.pub_dress \
             JOIN bond_roles r ON r.pub_dress = c.pub_dress \
             WHERE q.status = 'approved' AND r.role = 'admin' \
               AND c.delivered_at IS NULL \
               AND (c.last_attempt_at IS NULL OR c.last_attempt_at <= ?) \
             ORDER BY q.decided_at, c.pub_dress, c.telegram_user_id LIMIT 50",
        )
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| AdminRoleTelegramConfirmation {
                pub_dress: row.get("pub_dress"),
                role: row.get("role"),
                telegram_user_id: row.get("telegram_user_id"),
            })
            .collect())
    }

    /// A claim is an atomic lease: competing bot workers cannot immediately
    /// fan out the same announcement. Failed sends are retried after 5 min.
    pub async fn claim_admin_role_telegram_confirmation(
        &self,
        confirmation: &AdminRoleTelegramConfirmation,
        now: u64,
    ) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            "UPDATE admin_role_telegram_confirmations \
             SET attempts = attempts + 1, last_attempt_at = ? \
             WHERE pub_dress = ? AND telegram_user_id = ? \
               AND delivered_at IS NULL \
               AND (last_attempt_at IS NULL OR last_attempt_at <= ?) \
               AND EXISTS ( \
                 SELECT 1 FROM admin_role_requests q \
                 JOIN bond_roles r ON r.pub_dress = q.pub_dress \
                 WHERE q.pub_dress = admin_role_telegram_confirmations.pub_dress \
                   AND q.status = 'approved' AND r.role = 'admin' \
               )",
        )
        .bind(now as i64)
        .bind(&confirmation.pub_dress)
        .bind(confirmation.telegram_user_id)
        .bind((now as i64).saturating_sub(300))
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn confirm_admin_role_telegram_confirmation(
        &self,
        confirmation: &AdminRoleTelegramConfirmation,
        now: u64,
    ) -> Result<(), RepositoryError> {
        sqlx::query(
            "UPDATE admin_role_telegram_confirmations SET delivered_at = ? \
             WHERE pub_dress = ? AND telegram_user_id = ? \
               AND delivered_at IS NULL",
        )
        .bind(now as i64)
        .bind(&confirmation.pub_dress)
        .bind(confirmation.telegram_user_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod admin_telegram_confirmation_tests {
    use super::*;
    use crate::{BondAccessRole, ProviderIdentity, RegistrationOutcome};

    #[tokio::test]
    async fn only_committed_approvals_announce_to_existing_telegram_admins() {
        let repo = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("database");
        for (address, user_id, is_admin) in [
            ("0x1first", 7, true),
            ("0x1second", 8, true),
            ("0x1ordinary", 9, false),
            ("0x0new", 10, false),
            ("0x0denied", 11, false),
        ] {
            let bond: PubDress = address.parse().expect("address");
            assert!(matches!(
                repo.register(&bond, &ProviderIdentity::telegram(user_id), 1)
                    .await
                    .expect("register"),
                RegistrationOutcome::Registered(_)
            ));
            if is_admin {
                repo.set_role(&bond, BondAccessRole::Admin)
                    .await
                    .expect("admin grant");
            }
        }

        repo.request_admin_review_on_login("0x0denied", 10)
            .await
            .expect("denied request");
        let denied: PubDress = "0x0denied".parse().expect("denied");
        assert_eq!(
            repo.decide_admin_role_request("0x1first", &denied, false, 12)
                .await
                .expect("reject"),
            crate::AdminRoleDecision::Denied
        );
        assert!(repo.due_admin_role_telegram_confirmations(13)
            .await
            .expect("denial does not notify")
            .is_empty());

        repo.request_admin_review_on_login("0x0new", 14)
            .await
            .expect("pending");
        assert!(repo.due_admin_role_telegram_confirmations(14)
            .await
            .expect("pending does not notify")
            .is_empty());
        let candidate: PubDress = "0x0new".parse().expect("candidate");
        assert_eq!(
            repo.decide_admin_role_request("0x1first", &candidate, true, 15)
                .await
                .expect("approve"),
            crate::AdminRoleDecision::Approved
        );
        assert_eq!(
            repo.role_for(candidate.as_str()).await.expect("stored role"),
            BondAccessRole::Admin
        );

        let due = repo.due_admin_role_telegram_confirmations(16)
            .await
            .expect("announcements");
        assert_eq!(due.len(), 2);
        assert_eq!(
            due.iter().map(|v| v.telegram_user_id).collect::<Vec<_>>(),
            vec![7, 8]
        );
        for item in &due {
            assert_eq!(item.pub_dress, candidate.as_str());
            assert_eq!(item.role, "admin");
        }
        assert!(repo.claim_admin_role_telegram_confirmation(&due[0], 20)
            .await
            .expect("first claim"));
        assert!(!repo.claim_admin_role_telegram_confirmation(&due[0], 21)
            .await
            .expect("not claimed twice"));
        repo.confirm_admin_role_telegram_confirmation(&due[0], 22)
            .await
            .expect("receipt");
        assert_eq!(repo.due_admin_role_telegram_confirmations(23)
            .await
            .expect("one unsent")
            .len(), 1);
        assert_eq!(
            repo.decide_admin_role_request("0x1second", &candidate, true, 30)
                .await
                .expect("repeated"),
            crate::AdminRoleDecision::AlreadyDecided
        );
        assert_eq!(repo.due_admin_role_telegram_confirmations(24)
            .await
            .expect("no duplicate")
            .len(), 1);
        let remaining = &due[1];
        assert!(repo.claim_admin_role_telegram_confirmation(remaining, 31)
            .await
            .expect("claim second"));
        let retries = repo.due_admin_role_telegram_confirmations(332)
            .await
            .expect("retry after lease");
        assert_eq!(retries.len(), 1);
        assert_eq!(retries[0].telegram_user_id, 8);
        repo.confirm_admin_role_telegram_confirmation(remaining, 333)
            .await
            .expect("receipt second");
        assert!(repo.due_admin_role_telegram_confirmations(400)
            .await
            .expect("all delivered")
            .is_empty());
    }
}
