// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use nilxone_api::bot_notifications::{NotificationKind, NotificationRarity};

use super::super::{NewBotNotification, PublicInfoRepository};
use crate::aiaiaiai_sign_in::VerifiedSignIn;

const NOW: u64 = 1_800_000_000;
const DAY: u64 = 24 * 60 * 60;
const ISSUER: &str = "https://auth.aiaiaiai.org";

fn token(name: &str) -> String {
    format!("{name:=<64}")
}

async fn repository() -> PublicInfoRepository {
    PublicInfoRepository::connect("sqlite::memory:")
        .await
        .unwrap()
}

async fn sign_in(
    repository: &PublicInfoRepository,
    subject: &str,
    name: &str,
    label: &str,
) -> bool {
    let owner = repository
        .attach(&VerifiedSignIn {
            issuer: ISSUER.to_owned(),
            subject: subject.to_owned(),
            pub_dress: None,
            auth_time: NOW,
        })
        .await
        .unwrap();
    repository
        .open_session(&owner, &token(name), label, None, NOW, NOW + DAY)
        .await
        .unwrap()
}

async fn chat(repository: &PublicInfoRepository, subject: &str, name: &str, actor: i64) {
    sign_in(repository, subject, name, "Telegram · Chat").await;
    sqlx::query(
        "INSERT INTO telegram_chat_sessions (workload_subject, actor_id, token_hash) \
         VALUES ('bot', ?, ?)",
    )
    .bind(actor)
    .bind(token(name))
    .execute(&repository.pool)
    .await
    .unwrap();
}

fn find(dedupe: &str, rarity: NotificationRarity) -> NewBotNotification {
    NewBotNotification {
        kind: NotificationKind::Find,
        rarity: Some(rarity),
        label: Some("Old key".into()),
        request_id: None,
        dedupe: dedupe.into(),
    }
}

#[tokio::test]
async fn only_an_owner_with_a_chat_is_told_and_only_once() {
    let repository = repository().await;
    assert!(sign_in(&repository, "lonely", "web", "Mac · Safari").await);
    assert!(
        !repository
            .enqueue_bot_notification(ISSUER, "lonely", &find("a1", NotificationRarity::Rare), NOW)
            .await
            .unwrap(),
        "nobody would hear it"
    );
    chat(&repository, "bond", "chat", 42).await;
    for _ in 0..2 {
        assert!(
            repository
                .enqueue_bot_notification(
                    ISSUER,
                    "bond",
                    &find("a1", NotificationRarity::Legendary),
                    NOW
                )
                .await
                .unwrap()
        );
    }
    let pulled = repository
        .pull_bot_notifications("bot", 50, NOW)
        .await
        .unwrap();
    assert_eq!(pulled.len(), 1, "the same event is queued once");
    let notification = &pulled[0];
    assert_eq!(notification.provider_actor_id, 42);
    assert_eq!(notification.kind, NotificationKind::Find);
    assert_eq!(notification.rarity, Some(NotificationRarity::Legendary));
    assert_eq!(notification.label.as_deref(), Some("Old key"));
    // Another bot sees nothing; pulling again without an acknowledgement
    // answers the same row.
    assert!(
        repository
            .pull_bot_notifications("other", 50, NOW)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        repository
            .pull_bot_notifications("bot", 50, NOW)
            .await
            .unwrap(),
        pulled
    );
    assert_eq!(
        repository
            .acknowledge_bot_notifications("other", &[notification.id])
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        repository
            .acknowledge_bot_notifications("bot", &[notification.id])
            .await
            .unwrap(),
        1
    );
    assert!(
        repository
            .pull_bot_notifications("bot", 50, NOW)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn an_undelivered_notification_lapses_after_a_day() {
    let repository = repository().await;
    chat(&repository, "bond", "chat", 42).await;
    repository
        .enqueue_bot_notification(ISSUER, "bond", &find("a1", NotificationRarity::Rare), NOW)
        .await
        .unwrap();
    assert_eq!(
        repository
            .pull_bot_notifications("bot", 50, NOW + 10)
            .await
            .unwrap()
            .len(),
        1
    );
    // The chat session lives a day too; a longer one keeps the binding.
    sqlx::query("UPDATE product_sessions SET expires_at = expires_at + 864000")
        .execute(&repository.pool)
        .await
        .unwrap();
    assert!(
        repository
            .pull_bot_notifications("bot", 50, NOW + DAY)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_device_asking_the_chat_is_queued_for_the_bot() {
    let repository = repository().await;
    chat(&repository, "bond", "chat", 42).await;
    assert!(!sign_in(&repository, "bond", "web", "Mac · Safari").await);
    let asked = repository
        .request_activation(&token("web"), NOW + 1)
        .await
        .unwrap();
    let id = asked.request.unwrap().id;
    let pulled = repository
        .pull_bot_notifications("bot", 50, NOW + 1)
        .await
        .unwrap();
    assert_eq!(pulled.len(), 1);
    assert_eq!(pulled[0].kind, NotificationKind::ActivationRequested);
    assert_eq!(pulled[0].label.as_deref(), Some("Mac · Safari"));
    assert_eq!(pulled[0].request_id.as_deref(), Some(id.as_str()));
    // Asking again while it is open is the same request: nothing new.
    repository
        .request_activation(&token("web"), NOW + 2)
        .await
        .unwrap();
    assert_eq!(
        repository
            .pull_bot_notifications("bot", 50, NOW + 2)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn a_request_between_devices_is_not_the_bots() {
    let repository = repository().await;
    assert!(sign_in(&repository, "bond", "phone", "iPhone · Safari").await);
    chat(&repository, "bond", "chat", 42).await;
    assert!(!sign_in(&repository, "bond", "web", "Mac · Safari").await);
    repository
        .request_activation(&token("web"), NOW + 1)
        .await
        .unwrap();
    assert!(
        repository
            .pull_bot_notifications("bot", 50, NOW + 1)
            .await
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_label_is_bounded_and_clean() {
    assert_eq!(super::bounded(Some("  ")), None);
    assert_eq!(super::bounded(Some("a\u{7}b")).as_deref(), Some("ab"));
    let long = "ї".repeat(60);
    let cut = super::bounded(Some(&long)).unwrap();
    assert!(cut.len() <= 80 && long.starts_with(&cut));
}
