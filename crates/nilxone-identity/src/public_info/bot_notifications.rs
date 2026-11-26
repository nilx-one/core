// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! The queue of what the Telegram bot says in an owner's chat
//! (`docs/bot-notifications.md`). The back-end enqueues what happened; the
//! bot pulls the rows of the chats it signed in, delivers them and
//! acknowledges them, and the acknowledgement deletes them.

use nilxone_api::bot_notifications::{
    BotNotification, LABEL_MAX_BYTES, MAX_BATCH, NotificationKind, NotificationRarity,
};
use sqlx::{Row, SqliteConnection};

use super::{PublicInfoError, PublicInfoRepository};

/// How long a notification waits for the bot before it is dropped.
pub const NOTIFICATION_TTL_SECONDS: i64 = 24 * 60 * 60;

/// Something that happened to an owner, for the bot to say.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewBotNotification {
    pub kind: NotificationKind,
    pub rarity: Option<NotificationRarity>,
    /// At most [`LABEL_MAX_BYTES`]; a longer one is cut at a char boundary.
    pub label: Option<String>,
    pub request_id: Option<String>,
    /// One row per event: the request, attestation, orb or landmark id.
    pub dedupe: String,
}

fn bounded(label: Option<&str>) -> Option<String> {
    let label = label?.trim();
    if label.is_empty() {
        return None;
    }
    let mut end = label.len().min(LABEL_MAX_BYTES);
    while !label.is_char_boundary(end) {
        end -= 1;
    }
    Some(label[..end].chars().filter(|c| !c.is_control()).collect())
}

/// Queues `notification` for the owner inside the caller's transaction, if
/// the owner has a live signed-in chat; otherwise nobody would hear it and
/// nothing is stored. The same event again changes nothing. Answers whether
/// it is queued.
pub(super) async fn enqueue_in(
    connection: &mut SqliteConnection,
    issuer: &str,
    subject: &str,
    notification: &NewBotNotification,
    now: i64,
) -> Result<bool, sqlx::Error> {
    if notification.dedupe.is_empty() || notification.dedupe.len() > 128 {
        return Err(sqlx::Error::Protocol(
            "a notification needs a dedupe key".into(),
        ));
    }
    let heard: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM telegram_chat_sessions c \
         JOIN product_sessions s ON s.token_hash = c.token_hash \
         WHERE s.issuer = ? AND s.subject = ? AND s.expires_at > ?)",
    )
    .bind(issuer)
    .bind(subject)
    .bind(now)
    .fetch_one(&mut *connection)
    .await?;
    if !heard {
        return Ok(false);
    }
    sqlx::query(
        "INSERT INTO bot_notifications \
         (issuer, subject, kind, rarity, label, request_id, dedupe, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (issuer, subject, kind, dedupe) DO NOTHING",
    )
    .bind(issuer)
    .bind(subject)
    .bind(notification.kind.as_str())
    .bind(notification.rarity.map(NotificationRarity::as_str))
    .bind(bounded(notification.label.as_deref()))
    .bind(notification.request_id.as_deref())
    .bind(&notification.dedupe)
    .bind(now)
    .execute(&mut *connection)
    .await?;
    Ok(true)
}

impl PublicInfoRepository {
    /// Queues `notification` for the owner of `issuer` and `subject`, if the
    /// owner has a live signed-in chat. Answers whether it is queued.
    pub async fn enqueue_bot_notification(
        &self,
        issuer: &str,
        subject: &str,
        notification: &NewBotNotification,
        now: u64,
    ) -> Result<bool, PublicInfoError> {
        let now = i64::try_from(now).map_err(|_| PublicInfoError::InvalidSession)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let queued = enqueue_in(&mut transaction, issuer, subject, notification, now).await?;
        transaction.commit().await?;
        Ok(queued)
    }

    /// The oldest undelivered notifications of the chats `workload_subject`
    /// signed in, at most `limit` (bounded by [`MAX_BATCH`]). Nothing is
    /// marked: a notification stays until acknowledged or a day old.
    pub async fn pull_bot_notifications(
        &self,
        workload_subject: &str,
        limit: usize,
        now: u64,
    ) -> Result<Vec<BotNotification>, PublicInfoError> {
        let now = i64::try_from(now).map_err(|_| PublicInfoError::InvalidSession)?;
        let limit = i64::try_from(limit.clamp(1, MAX_BATCH)).unwrap_or(1);
        sqlx::query("DELETE FROM bot_notifications WHERE created_at <= ?")
            .bind(now.saturating_sub(NOTIFICATION_TTL_SECONDS))
            .execute(&self.pool)
            .await?;
        let rows = sqlx::query(
            "SELECT n.id, c.actor_id, n.kind, n.rarity, n.label, n.request_id, n.created_at \
             FROM bot_notifications n \
             JOIN product_sessions s ON s.issuer = n.issuer AND s.subject = n.subject \
             JOIN telegram_chat_sessions c ON c.token_hash = s.token_hash \
             WHERE c.workload_subject = ? AND s.expires_at > ? \
             ORDER BY n.id LIMIT ?",
        )
        .bind(workload_subject)
        .bind(now)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let unsigned =
                    |value: i64| u64::try_from(value).map_err(|_| PublicInfoError::InvalidSession);
                let kind: String = row.try_get("kind")?;
                let rarity: Option<String> = row.try_get("rarity")?;
                Ok(BotNotification {
                    id: unsigned(row.try_get("id")?)?,
                    provider_actor_id: unsigned(row.try_get("actor_id")?)?,
                    kind: NotificationKind::parse(&kind).ok_or(PublicInfoError::InvalidSession)?,
                    rarity: rarity
                        .map(|rarity| {
                            NotificationRarity::parse(&rarity)
                                .ok_or(PublicInfoError::InvalidSession)
                        })
                        .transpose()?,
                    label: row.try_get("label")?,
                    request_id: row.try_get("request_id")?,
                    created_at: unsigned(row.try_get("created_at")?)?,
                })
            })
            .collect()
    }

    /// The bot delivered these; only rows of the chats `workload_subject`
    /// signed in are deleted. Answers how many.
    pub async fn acknowledge_bot_notifications(
        &self,
        workload_subject: &str,
        ids: &[u64],
    ) -> Result<u64, PublicInfoError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut deleted = 0;
        for id in ids.iter().take(MAX_BATCH) {
            let Ok(id) = i64::try_from(*id) else {
                continue;
            };
            deleted += sqlx::query(
                "DELETE FROM bot_notifications WHERE id = ? AND EXISTS (\
                 SELECT 1 FROM product_sessions s \
                 JOIN telegram_chat_sessions c ON c.token_hash = s.token_hash \
                 WHERE s.issuer = bot_notifications.issuer \
                   AND s.subject = bot_notifications.subject \
                   AND c.workload_subject = ?)",
            )
            .bind(id)
            .bind(workload_subject)
            .execute(&mut *transaction)
            .await?
            .rows_affected();
        }
        transaction.commit().await?;
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests;
