// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

/// Presentation-only Telegram slots. A slot records a Telegram-issued message
/// ID, not a protocol interaction or an inferred Bond relationship.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelegramMessageSlot {
    Open,
    Status,
    Admin,
}

impl IdentityRepository {
    pub async fn telegram_message_id(
        &self,
        chat_id: i64,
        slot: TelegramMessageSlot,
    ) -> Result<Option<i32>, RepositoryError> {
        let column = match slot {
            TelegramMessageSlot::Open => "open_message_id",
            TelegramMessageSlot::Status => "status_message_id",
            TelegramMessageSlot::Admin => "admin_message_id",
        };
        let sql = format!("SELECT {column} FROM telegram_message_slots WHERE chat_id = ?");
        Ok(sqlx::query_scalar::<_, Option<i32>>(&sql)
            .bind(chat_id)
            .fetch_optional(&self.pool)
            .await?
            .flatten())
    }

    pub async fn save_telegram_message_id(
        &self,
        chat_id: i64,
        slot: TelegramMessageSlot,
        message_id: Option<i32>,
    ) -> Result<(), RepositoryError> {
        // The identifier is selected from this closed enum, never from user
        // content. Preserve the other two slots across every upsert.
        let column = match slot {
            TelegramMessageSlot::Open => "open_message_id",
            TelegramMessageSlot::Status => "status_message_id",
            TelegramMessageSlot::Admin => "admin_message_id",
        };
        let sql = format!(
            "INSERT INTO telegram_message_slots (chat_id, {column}) VALUES (?, ?)              ON CONFLICT(chat_id) DO UPDATE SET {column} = excluded.{column}"
        );
        sqlx::query(&sql)
            .bind(chat_id)
            .bind(message_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The actionable admin inbox, not a one-message-per-request outbox.
    /// Authority still belongs to decide_admin_role_request's atomic check.
    pub async fn telegram_admin_inbox(
        &self,
    ) -> Result<Vec<(String, String)>, RepositoryError> {
        let rows = sqlx::query(
            "SELECT request_token, pub_dress FROM admin_role_requests              WHERE status = 'pending' ORDER BY requested_at, pub_dress LIMIT 10",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter()
            .map(|row| (row.get("request_token"), row.get("pub_dress")))
            .collect())
    }
}

#[cfg(test)]
mod telegram_message_slot_tests {
    use super::*;

    #[tokio::test]
    async fn slots_are_independent_and_reused_after_reopening_storage() {
        let dir = tempfile::tempdir().expect("temporary database directory");
        let url = format!("sqlite://{}", dir.path().join("telegram-slots.db").display());
        let repo = IdentityRepository::connect(&url).await.expect("database");
        assert_eq!(repo.telegram_message_id(123, TelegramMessageSlot::Open).await.unwrap(), None);
        repo.save_telegram_message_id(123, TelegramMessageSlot::Open, Some(5)).await.unwrap();
        repo.save_telegram_message_id(123, TelegramMessageSlot::Status, Some(6)).await.unwrap();
        repo.save_telegram_message_id(123, TelegramMessageSlot::Admin, Some(7)).await.unwrap();
        repo.save_telegram_message_id(123, TelegramMessageSlot::Status, Some(8)).await.unwrap();
        drop(repo);
        let repo = IdentityRepository::connect(&url).await.expect("reopened database");
        assert_eq!(repo.telegram_message_id(123, TelegramMessageSlot::Open).await.unwrap(), Some(5));
        assert_eq!(repo.telegram_message_id(123, TelegramMessageSlot::Status).await.unwrap(), Some(8));
        assert_eq!(repo.telegram_message_id(123, TelegramMessageSlot::Admin).await.unwrap(), Some(7));
        repo.save_telegram_message_id(123, TelegramMessageSlot::Admin, None).await.unwrap();
        assert_eq!(repo.telegram_message_id(123, TelegramMessageSlot::Admin).await.unwrap(), None);
        assert_eq!(repo.telegram_message_id(124, TelegramMessageSlot::Status).await.unwrap(), None);
    }
}
