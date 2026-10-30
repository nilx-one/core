// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminRoleTelegramDelivery {
    pub request_token: String,
    pub pub_dress: String,
    pub telegram_user_id: i64,
}

impl IdentityRepository {
    /// All admins with a Telegram account linked to their *stored* admin Bond.
    /// Failed sends are retried; new admin bindings pick up still-pending items.
    pub async fn due_admin_role_telegram_deliveries(
        &self,
        now: u64,
    ) -> Result<Vec<AdminRoleTelegramDelivery>, RepositoryError> {
        let cutoff = (now as i64).saturating_sub(300);
        let rows = sqlx::query(
            "SELECT q.request_token, q.pub_dress, \
             CAST(p.provider_subject AS INTEGER) AS telegram_user_id \
             FROM admin_role_requests q \
             JOIN bond_roles roles ON roles.role = 'admin' \
             JOIN identities admin ON admin.pub_dress = roles.pub_dress AND admin.identity_kind = 'human' \
             JOIN identity_providers p ON p.pub_dress = roles.pub_dress AND p.provider = 'telegram' \
             LEFT JOIN admin_role_telegram_deliveries d \
               ON d.pub_dress = q.pub_dress AND d.telegram_user_id = CAST(p.provider_subject AS INTEGER) \
             WHERE q.status = 'pending' AND d.delivered_at IS NULL \
               AND (d.last_attempt_at IS NULL OR d.last_attempt_at <= ?) \
             ORDER BY q.requested_at, q.pub_dress, p.provider_subject LIMIT 50",
        )
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().filter_map(|row| {
            let telegram_user_id: i64 = row.get("telegram_user_id");
            (telegram_user_id > 0).then(|| AdminRoleTelegramDelivery {
                request_token: row.get("request_token"),
                pub_dress: row.get("pub_dress"),
                telegram_user_id,
            })
        }).collect())
    }

    /// Claim a retry window *before* calling Telegram: two service replicas
    /// must not fan out the same pending request at the same moment.
    pub async fn claim_admin_role_telegram_delivery(
        &self,
        delivery: &AdminRoleTelegramDelivery,
        now: u64,
    ) -> Result<bool, RepositoryError> {
        let updated = sqlx::query(
            "INSERT INTO admin_role_telegram_deliveries \
             (pub_dress, telegram_user_id, last_attempt_at, attempts) \
             SELECT q.pub_dress, ?, ?, 1 FROM admin_role_requests q \
             WHERE q.request_token = ? AND q.pub_dress = ? AND q.status = 'pending' \
             ON CONFLICT(pub_dress, telegram_user_id) DO UPDATE SET \
               last_attempt_at = excluded.last_attempt_at, attempts = attempts + 1 \
             WHERE delivered_at IS NULL AND last_attempt_at <= ?",
        )
        .bind(delivery.telegram_user_id)
        .bind(now as i64)
        .bind(&delivery.request_token)
        .bind(&delivery.pub_dress)
        .bind((now as i64).saturating_sub(300))
        .execute(&self.pool)
        .await?;
        Ok(updated.rows_affected() == 1)
    }

    pub async fn confirm_admin_role_telegram_delivery(
        &self,
        delivery: &AdminRoleTelegramDelivery,
        now: u64,
    ) -> Result<(), RepositoryError> {
        sqlx::query(
            "UPDATE admin_role_telegram_deliveries SET delivered_at = ? \
             WHERE pub_dress = ? AND telegram_user_id = ? AND delivered_at IS NULL",
        )
        .bind(now as i64)
        .bind(&delivery.pub_dress)
        .bind(delivery.telegram_user_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Callback payload contains only a numeric request ID, not authority.
    pub async fn pending_admin_role_request_by_token(
        &self,
        request_token: &str,
    ) -> Result<Option<String>, RepositoryError> {
        Ok(sqlx::query_scalar(
            "SELECT pub_dress FROM admin_role_requests WHERE request_token = ? AND status = 'pending'",
        )
        .bind(request_token)
        .fetch_optional(&self.pool)
        .await?)
    }
}

#[cfg(test)]
mod admin_telegram_delivery_tests {
    use super::*;
    use crate::{BondAccessRole, ProviderIdentity, RegistrationOutcome};

    #[tokio::test]
    async fn every_telegram_linked_admin_receives_one_retryable_notification() {
        let repo = IdentityRepository::connect("sqlite::memory:").await.expect("database");
        for (address, user_id, admin) in [("0x1first", 7, true), ("0x1second", 8, true),
                                          ("0x1ordinary", 9, false), ("0x0new", 10, false)] {
            let pub_dress: PubDress = address.parse().expect("address");
            assert!(matches!(repo.register(&pub_dress, &ProviderIdentity::telegram(user_id), 1).await.expect("register"), RegistrationOutcome::Registered(_)));
            if admin { repo.set_role(&pub_dress, BondAccessRole::Admin).await.expect("role"); }
        }
        repo.request_admin_review_on_login("0x0new", 10).await.expect("request");
        let due = repo.due_admin_role_telegram_deliveries(20).await.expect("due");
        assert_eq!(due.len(), 2);
        assert_eq!(due.iter().map(|v| v.telegram_user_id).collect::<Vec<_>>(), vec![7, 8]);
        assert_eq!(repo.pending_admin_role_request_by_token(&due[0].request_token).await.expect("lookup").as_deref(), Some("0x0new"));
        assert!(repo.claim_admin_role_telegram_delivery(&due[0], 20).await.expect("claim"));
        assert!(!repo.claim_admin_role_telegram_delivery(&due[0], 21).await.expect("duplicate"));
        assert_eq!(repo.due_admin_role_telegram_deliveries(21).await.expect("due").len(), 1);
        repo.confirm_admin_role_telegram_delivery(&due[0], 22).await.expect("delivered");
        assert!(repo.claim_admin_role_telegram_delivery(&due[1], 23).await.expect("claim second"));
        let retries = repo.due_admin_role_telegram_deliveries(324).await.expect("retry");
        assert_eq!(retries.len(), 1);
        assert_eq!(retries[0].telegram_user_id, 8);
        let applicant: PubDress = "0x0new".parse().expect("applicant");
        assert_eq!(repo.decide_admin_role_request("0x1first", &applicant, true, 330).await.expect("decide"), crate::AdminRoleDecision::Approved);
        assert!(repo.due_admin_role_telegram_deliveries(630).await.expect("terminal").is_empty());
    }
}
