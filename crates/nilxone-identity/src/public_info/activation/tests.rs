// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use std::{fmt::Debug, str::FromStr};

use nilxone_api::activation::{
    HandoffStep, SYNC_MAX_BYTES, SYNC_PART_BYTES, StepState, sync_digest,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use super::{super::PublicInfoError, *};
use crate::aiaiaiai_sign_in::VerifiedSignIn;

const ISSUER: &str = "https://auth.aiaiaiai.org";
const NOW: u64 = 1_800_000_000;
const DAY: u64 = 24 * 60 * 60;
const TELEGRAM: &str = "iPhone · Telegram";
const BROWSER: &str = "Mac · Safari";

fn token(name: &str) -> String {
    format!("{name:=<64}")
}

async fn repository() -> PublicInfoRepository {
    PublicInfoRepository::connect("sqlite::memory:")
        .await
        .unwrap()
}

/// Signs `subject` in on a client and returns whether it became the active
/// one.
async fn sign_in(
    repository: &PublicInfoRepository,
    subject: &str,
    name: &str,
    label: &str,
    now: u64,
    ttl: u64,
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
        .open_session(&owner, &token(name), label, None, now, now + ttl)
        .await
        .unwrap()
}

/// One Bond with Telegram active and a browser signed in after it.
async fn pair() -> (PublicInfoRepository, String, String) {
    let repository = repository().await;
    assert!(sign_in(&repository, "bond", "telegram", TELEGRAM, NOW, DAY).await);
    assert!(!sign_in(&repository, "bond", "browser", BROWSER, NOW, DAY).await);
    (repository, token("telegram"), token("browser"))
}

async fn state(repository: &PublicInfoRepository, token: &str, now: u64) -> ActivationState {
    repository.activation_state(token, now).await.unwrap()
}

async fn ask(repository: &PublicInfoRepository, token: &str, now: u64) -> String {
    let asked = repository.request_activation(token, now).await.unwrap();
    assert_eq!(asked.state, ClientState::ActivationPending);
    asked.request.unwrap().id
}

fn refused<T: Debug>(result: Result<T, ActivationError>) -> ActivationRefusal {
    match result {
        Err(ActivationError::Refused(refusal)) => refusal,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn payload(len: u64) -> Vec<u8> {
    (0..len).map(|index| (index % 251) as u8).collect()
}

async fn upload(
    repository: &PublicInfoRepository,
    token: &str,
    id: &str,
    data: &[u8],
    now: u64,
) -> ActivationState {
    let total_bytes = data.len() as u64;
    let mut offset = 0;
    loop {
        let len = sync_part_len(total_bytes, offset).unwrap();
        let end = offset + len;
        let answer = repository
            .upload_activation_sync_part(
                token,
                id,
                UploadPartQuery {
                    offset,
                    total_bytes,
                },
                &data[offset as usize..end as usize],
                now,
            )
            .await
            .unwrap();
        offset = end;
        if offset == total_bytes {
            return answer;
        }
    }
}

async fn download(
    repository: &PublicInfoRepository,
    token: &str,
    id: &str,
    total_bytes: u64,
    now: u64,
) -> Vec<u8> {
    let mut data = Vec::new();
    loop {
        let part = repository
            .download_activation_sync_part(token, id, data.len() as u64, now)
            .await
            .unwrap();
        data.extend_from_slice(&part);
        if data.len() as u64 == total_bytes {
            return data;
        }
    }
}

/// Brings a request to `syncing` with `data` uploaded, and returns its id.
async fn uploaded(
    repository: &PublicInfoRepository,
    telegram: &str,
    browser: &str,
    data: &[u8],
    now: u64,
) -> String {
    let id = ask(repository, browser, now).await;
    repository
        .approve_activation(telegram, &id, now)
        .await
        .unwrap();
    let answer = upload(repository, telegram, &id, data, now).await;
    assert_eq!(answer.state, ClientState::HandoffSyncing);
    id
}

async fn count(repository: &PublicInfoRepository, sql: &str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(&repository.pool)
        .await
        .unwrap()
}

async fn stored_parts(repository: &PublicInfoRepository) -> i64 {
    count(repository, "SELECT COUNT(*) FROM product_activation_sync").await
}

async fn active_clients(repository: &PublicInfoRepository) -> Vec<String> {
    sqlx::query_scalar("SELECT token_hash FROM product_sessions WHERE active = 1")
        .fetch_all(&repository.pool)
        .await
        .unwrap()
}

async fn epoch(repository: &PublicInfoRepository) -> i64 {
    count(repository, "SELECT session_epoch FROM public_profiles").await
}

async fn status_of(repository: &PublicInfoRepository, id: &str) -> String {
    sqlx::query_scalar("SELECT status FROM product_activation_requests WHERE id = ?")
        .bind(id)
        .fetch_one(&repository.pool)
        .await
        .unwrap()
}

/// Whether the client behind `token` is allowed a Bond write.
async fn can_write(repository: &PublicInfoRepository, token: &str, now: u64) -> bool {
    let owner = repository
        .restore_session(token, now)
        .await
        .unwrap()
        .unwrap();
    match repository.publish_experience_carry(&owner, 1, 1).await {
        Ok(_) => true,
        Err(PublicInfoError::SessionInactive) => false,
        Err(other) => panic!("{other:?}"),
    }
}

fn steps(state: &ActivationState) -> [StepState; 3] {
    let progress = state.request.as_ref().unwrap().progress.as_ref().unwrap();
    assert_eq!(
        progress.steps.map(|row| row.step),
        [HandoffStep::Confirm, HandoffStep::Sync, HandoffStep::Switch]
    );
    progress.steps.map(|row| row.state)
}

fn percent(state: &ActivationState) -> u8 {
    let progress = state.request.as_ref().unwrap().progress.as_ref().unwrap();
    progress.percent
}

#[tokio::test]
async fn the_first_sign_in_is_active_and_every_later_one_is_not() {
    let (repository, telegram, browser) = pair().await;
    assert!(!sign_in(&repository, "bond", "tablet", "iPad · Discord", NOW, DAY).await);
    assert_eq!(active_clients(&repository).await, [telegram.as_str()]);
    assert_eq!(epoch(&repository).await, 1);

    let active = state(&repository, &telegram, NOW).await;
    assert_eq!(active.state, ClientState::AuthenticatedActive);
    assert!(active.active && active.may_act);
    assert_eq!(active.client_label, TELEGRAM);
    assert_eq!(active.active_client_label, None);
    assert_eq!(active.request, None);

    let inactive = state(&repository, &browser, NOW).await;
    assert_eq!(inactive.state, ClientState::ActiveElsewhere);
    assert!(!inactive.active && !inactive.may_act);
    assert_eq!(inactive.client_label, BROWSER);
    assert_eq!(inactive.active_client_label.as_deref(), Some(TELEGRAM));
    assert_eq!(inactive.request, None);

    assert!(can_write(&repository, &telegram, NOW).await);
    assert!(!can_write(&repository, &browser, NOW).await);
    // Another Bond's clients are its own.
    assert!(sign_in(&repository, "other", "other", "Pixel · Chrome", NOW, DAY).await);
    assert_eq!(active_clients(&repository).await.len(), 2);
}

#[tokio::test]
async fn a_handoff_switches_only_when_the_asking_client_holds_the_data() {
    use StepState::{Current, Done, Waiting};

    let (repository, telegram, browser) = pair().await;
    let data = payload(2 * SYNC_PART_BYTES + 1_000);
    let total = data.len() as u64;

    let asked = repository.request_activation(&browser, NOW).await.unwrap();
    assert_eq!(asked.state, ClientState::ActivationPending);
    let request = asked.request.clone().unwrap();
    assert_eq!(request.role, ActivationRole::Requester);
    assert_eq!(request.status, ActivationStatus::Pending);
    assert_eq!(
        (request.from_label.as_str(), request.to_label.as_str()),
        (TELEGRAM, BROWSER)
    );
    assert_eq!(request.deadline, Some(NOW + RESPOND_SECONDS));
    assert_eq!(steps(&asked), [Current, Waiting, Waiting]);
    assert_eq!(percent(&asked), 0);
    let id = request.id;

    let shown = state(&repository, &telegram, NOW + 1).await;
    assert_eq!(shown.state, ClientState::ApprovalRequested);
    assert!(shown.may_act);
    let request = shown.request.unwrap();
    assert_eq!(request.id, id);
    assert_eq!(request.role, ActivationRole::Approver);
    assert_eq!(
        (request.from_label.as_str(), request.to_label.as_str()),
        (TELEGRAM, BROWSER)
    );

    let agreed = repository
        .approve_activation(&telegram, &id, NOW + 2)
        .await
        .unwrap();
    assert_eq!(agreed.state, ClientState::UploadRequired);
    assert!(agreed.active && !agreed.may_act);
    assert_eq!(steps(&agreed), [Done, Current, Waiting]);
    assert_eq!(percent(&agreed), 0);
    let waiting = state(&repository, &browser, NOW + 2).await;
    assert_eq!(waiting.state, ClientState::ActivationApproved);
    // Agreeing switches nothing.
    assert_eq!(active_clients(&repository).await, [telegram.as_str()]);
    assert_eq!(
        refused(
            repository
                .download_activation_sync_part(&browser, &id, 0, NOW + 2)
                .await
        ),
        ActivationRefusal::StepRefused
    );

    // The upload fills the first half, part by part, and both clients are
    // shown the same progress.
    let mut last_percent = 0;
    let mut offset = 0;
    while let Some(len) = sync_part_len(total, offset) {
        let end = offset + len;
        let answer = repository
            .upload_activation_sync_part(
                &telegram,
                &id,
                UploadPartQuery {
                    offset,
                    total_bytes: total,
                },
                &data[offset as usize..end as usize],
                NOW + 3,
            )
            .await
            .unwrap();
        let progress = answer.request.clone().unwrap().progress.unwrap();
        assert_eq!(progress.total_bytes, Some(total));
        assert_eq!(progress.uploaded_bytes, end);
        assert!(progress.percent > last_percent && progress.percent <= 50);
        last_percent = progress.percent;
        let seen = state(&repository, &browser, NOW + 3).await;
        assert_eq!(seen.request.unwrap().progress.unwrap(), progress);
        if end == total {
            assert_eq!(answer.state, ClientState::HandoffSyncing);
            assert_eq!(seen.state, ClientState::ActivationSyncing);
        } else {
            assert_eq!(answer.state, ClientState::UploadRequired);
            assert_eq!(seen.state, ClientState::ActivationApproved);
        }
        assert_eq!(steps(&answer), [Done, Current, Waiting]);
        offset = end;
    }
    assert_eq!(last_percent, 50);
    assert_eq!(stored_parts(&repository).await, 3);
    let syncing = state(&repository, &browser, NOW + 4).await;
    assert_eq!(
        syncing.request.unwrap().deadline,
        Some(NOW + 3 + SYNC_SECONDS)
    );

    // The delivery fills the second half.
    let mut fetched = Vec::new();
    while (fetched.len() as u64) < total {
        let part = repository
            .download_activation_sync_part(&browser, &id, fetched.len() as u64, NOW + 5)
            .await
            .unwrap();
        fetched.extend_from_slice(&part);
        let seen = state(&repository, &telegram, NOW + 5).await;
        assert_eq!(seen.state, ClientState::HandoffSyncing);
        let progress = seen.request.clone().unwrap().progress.unwrap();
        assert_eq!(progress.delivered_bytes, fetched.len() as u64);
        assert!(progress.percent > last_percent && progress.percent < 100);
        last_percent = progress.percent;
        if fetched.len() as u64 == total {
            assert_eq!(steps(&seen), [Done, Done, Current]);
        } else {
            assert_eq!(steps(&seen), [Done, Current, Waiting]);
        }
    }
    assert_eq!(fetched, data);
    assert_eq!(last_percent, 99);

    // Everything is delivered and still nothing has switched.
    assert_eq!(active_clients(&repository).await, [telegram.as_str()]);
    assert_eq!(epoch(&repository).await, 1);
    assert!(!can_write(&repository, &browser, NOW + 6).await);

    let switched = repository
        .complete_activation(&browser, &id, &sync_digest(&fetched), NOW + 6)
        .await
        .unwrap();
    assert_eq!(switched.state, ClientState::AuthenticatedActive);
    assert!(switched.active && switched.may_act);
    assert_eq!(steps(&switched), [Done, Done, Done]);
    assert_eq!(percent(&switched), 100);
    let request = switched.request.unwrap();
    assert_eq!(request.status, ActivationStatus::Completed);
    assert_eq!(request.deadline, None);

    assert_eq!(active_clients(&repository).await, [browser.as_str()]);
    assert_eq!(epoch(&repository).await, 2);
    assert_eq!(stored_parts(&repository).await, 0);
    assert_eq!(status_of(&repository, &id).await, "completed");
    assert!(can_write(&repository, &browser, NOW + 7).await);
    assert!(!can_write(&repository, &telegram, NOW + 7).await);

    // The former active client stays signed in, inactive, and names the new one.
    let left = state(&repository, &telegram, NOW + 7).await;
    assert_eq!(left.state, ClientState::ActiveElsewhere);
    assert_eq!(left.active_client_label.as_deref(), Some(BROWSER));
    assert_eq!(left.request, None);
    let arrived = state(&repository, &browser, NOW + 7).await;
    assert_eq!(arrived.state, ClientState::AuthenticatedActive);
    assert_eq!(arrived.request, None);
}

#[tokio::test]
async fn the_former_active_client_can_ask_to_return() {
    let (repository, telegram, browser) = pair().await;
    let data = payload(10);
    let id = uploaded(&repository, &telegram, &browser, &data, NOW).await;
    repository
        .complete_activation(&browser, &id, &sync_digest(&data), NOW)
        .await
        .unwrap();

    let back = ask(&repository, &telegram, NOW + 10).await;
    assert_ne!(back, id);
    let shown = state(&repository, &browser, NOW + 10).await;
    assert_eq!(shown.state, ClientState::ApprovalRequested);
    let request = shown.request.unwrap();
    assert_eq!(
        (request.from_label.as_str(), request.to_label.as_str()),
        (BROWSER, TELEGRAM)
    );
    repository
        .approve_activation(&browser, &back, NOW + 11)
        .await
        .unwrap();
    upload(&repository, &browser, &back, b"", NOW + 12).await;
    assert_eq!(
        download(&repository, &telegram, &back, 0, NOW + 13).await,
        b""
    );
    let returned = repository
        .complete_activation(&telegram, &back, &sync_digest(b""), NOW + 14)
        .await
        .unwrap();
    assert_eq!(returned.state, ClientState::AuthenticatedActive);
    assert_eq!(active_clients(&repository).await, [telegram]);
    assert_eq!(epoch(&repository).await, 3);
}

#[tokio::test]
async fn a_reload_restarts_the_fetch_without_a_new_request_or_approval() {
    let (repository, telegram, browser) = pair().await;
    let data = payload(3 * SYNC_PART_BYTES);
    let total = data.len() as u64;
    let id = uploaded(&repository, &telegram, &browser, &data, NOW).await;
    let fetch = |offset| repository.download_activation_sync_part(&browser, &id, offset, NOW + 1);

    assert_eq!(fetch(0).await.unwrap().len() as u64, SYNC_PART_BYTES);
    assert_eq!(
        fetch(SYNC_PART_BYTES).await.unwrap().len() as u64,
        SYNC_PART_BYTES
    );
    let before = state(&repository, &browser, NOW + 1).await;
    assert_eq!(
        before.request.unwrap().progress.unwrap().delivered_bytes,
        2 * SYNC_PART_BYTES
    );
    // The part just handed out may be fetched again; a skipped part or an
    // offset that is no part's start is not in order.
    assert!(fetch(SYNC_PART_BYTES).await.is_ok());
    let skipped = sync_part_len(total, 0).unwrap() * 3;
    for offset in [1, skipped, total, total + SYNC_PART_BYTES] {
        assert_eq!(
            refused(fetch(offset).await),
            ActivationRefusal::SyncOutOfOrder,
            "{offset}"
        );
    }

    // The page reloads: asking again resumes the same request, and the fetch
    // starts over from the first part. The active client is asked nothing.
    let resumed = repository
        .request_activation(&browser, NOW + 2)
        .await
        .unwrap();
    assert_eq!(resumed.state, ClientState::ActivationSyncing);
    assert_eq!(resumed.request.unwrap().id, id);
    fetch(0).await.unwrap();
    let restarted = state(&repository, &telegram, NOW + 2).await;
    assert_eq!(restarted.state, ClientState::HandoffSyncing);
    let progress = restarted.request.unwrap().progress.unwrap();
    assert_eq!(progress.delivered_bytes, SYNC_PART_BYTES);
    assert_eq!(progress.uploaded_bytes, total);
    assert_eq!(
        count(
            &repository,
            "SELECT COUNT(*) FROM product_activation_requests"
        )
        .await,
        1
    );

    // Holding other bytes, or only some of them, does not complete.
    for wrong in [sync_digest(&data[1..]), sync_digest(b"")] {
        assert_eq!(
            refused(
                repository
                    .complete_activation(&browser, &id, &wrong, NOW + 3)
                    .await
            ),
            ActivationRefusal::SyncIncomplete
        );
    }
    assert_eq!(
        refused(
            repository
                .complete_activation(&browser, &id, "not a digest", NOW + 3)
                .await
        ),
        ActivationRefusal::InvalidSyncDigest
    );
    assert_eq!(status_of(&repository, &id).await, "syncing");
    assert_eq!(active_clients(&repository).await, [telegram.as_str()]);

    let fetched = download(&repository, &browser, &id, total, NOW + 4).await;
    assert_eq!(fetched, data);
    let digest = sync_digest(&fetched);
    let switched = repository
        .complete_activation(&browser, &id, &digest, NOW + 5)
        .await
        .unwrap();
    assert_eq!(switched.state, ClientState::AuthenticatedActive);
    // A second completion is the same answer, not a second switch.
    let again = repository
        .complete_activation(&browser, &id, &digest, NOW + 6)
        .await
        .unwrap();
    assert_eq!(again, switched);
    assert_eq!(epoch(&repository).await, 2);
}

#[tokio::test]
async fn a_decline_ends_the_request_before_or_after_agreeing() {
    let (repository, telegram, browser) = pair().await;

    let id = ask(&repository, &browser, NOW).await;
    let declined = repository
        .decline_activation(&telegram, &id, NOW + 1)
        .await
        .unwrap();
    assert_eq!(declined.state, ClientState::AuthenticatedActive);
    assert!(declined.may_act);
    let request = declined.request.clone().unwrap();
    assert_eq!(request.status, ActivationStatus::Declined);
    assert_eq!((request.deadline, request.progress), (None, None));
    // A second tap is the same answer.
    assert_eq!(
        repository
            .decline_activation(&telegram, &id, NOW + 2)
            .await
            .unwrap(),
        declined
    );
    assert_eq!(
        refused(repository.approve_activation(&telegram, &id, NOW + 2).await),
        ActivationRefusal::StepRefused
    );
    let told = state(&repository, &browser, NOW + 2).await;
    assert_eq!(told.state, ClientState::ActivationRejected);
    assert_eq!(told.request.unwrap().status, ActivationStatus::Declined);
    assert_eq!(state(&repository, &telegram, NOW + 2).await.request, None);

    // After it agreed and uploaded, taking it back still ends the request,
    // and the uploaded data goes with it.
    let data = payload(SYNC_PART_BYTES + 1);
    let second = uploaded(&repository, &telegram, &browser, &data, NOW + 10).await;
    assert_ne!(second, id);
    assert_eq!(stored_parts(&repository).await, 2);
    let objected = repository
        .decline_activation(&telegram, &second, NOW + 11)
        .await
        .unwrap();
    assert_eq!(objected.state, ClientState::AuthenticatedActive);
    assert_eq!(objected.request.unwrap().status, ActivationStatus::Objected);
    assert_eq!(stored_parts(&repository).await, 0);
    assert_eq!(
        state(&repository, &browser, NOW + 11).await.state,
        ClientState::ActivationObjected
    );
    assert_eq!(
        refused(
            repository
                .download_activation_sync_part(&browser, &second, 0, NOW + 12)
                .await
        ),
        ActivationRefusal::StepRefused
    );
    assert_eq!(
        refused(
            repository
                .complete_activation(&browser, &second, &sync_digest(&data), NOW + 12)
                .await
        ),
        ActivationRefusal::StepRefused
    );
    assert_eq!(active_clients(&repository).await, [telegram.as_str()]);
    assert_eq!(epoch(&repository).await, 1);

    // What was uploaded does not carry over: a fresh request starts again
    // with the active client's answer.
    let third = ask(&repository, &browser, NOW + 20).await;
    assert_ne!(third, second);
    assert_eq!(
        state(&repository, &telegram, NOW + 20).await.state,
        ClientState::ApprovalRequested
    );
}

#[tokio::test]
async fn the_asking_client_cancels_at_any_open_step() {
    let (repository, telegram, browser) = pair().await;

    let id = ask(&repository, &browser, NOW).await;
    let cancelled = repository
        .cancel_activation(&browser, &id, NOW + 1)
        .await
        .unwrap();
    assert_eq!(cancelled.state, ClientState::ActivationCancelled);
    assert_eq!(
        cancelled.request.clone().unwrap().status,
        ActivationStatus::Cancelled
    );
    assert_eq!(
        repository
            .cancel_activation(&browser, &id, NOW + 2)
            .await
            .unwrap(),
        cancelled
    );
    assert_eq!(
        refused(repository.approve_activation(&telegram, &id, NOW + 2).await),
        ActivationRefusal::StepRefused
    );
    assert_eq!(
        state(&repository, &telegram, NOW + 2).await.state,
        ClientState::AuthenticatedActive
    );

    // While the active client uploads.
    let id = ask(&repository, &browser, NOW + 10).await;
    repository
        .approve_activation(&telegram, &id, NOW + 10)
        .await
        .unwrap();
    let data = payload(2 * SYNC_PART_BYTES);
    let part = UploadPartQuery {
        offset: 0,
        total_bytes: data.len() as u64,
    };
    repository
        .upload_activation_sync_part(
            &telegram,
            &id,
            part,
            &data[..SYNC_PART_BYTES as usize],
            NOW + 11,
        )
        .await
        .unwrap();
    repository
        .cancel_activation(&browser, &id, NOW + 12)
        .await
        .unwrap();
    assert_eq!(stored_parts(&repository).await, 0);
    assert_eq!(
        refused(
            repository
                .upload_activation_sync_part(
                    &telegram,
                    &id,
                    UploadPartQuery {
                        offset: SYNC_PART_BYTES,
                        ..part
                    },
                    &data[SYNC_PART_BYTES as usize..],
                    NOW + 13,
                )
                .await
        ),
        ActivationRefusal::StepRefused
    );
    let released = state(&repository, &telegram, NOW + 13).await;
    assert_eq!(released.state, ClientState::AuthenticatedActive);
    assert!(released.may_act);

    // After the upload, and by signing out instead of cancelling.
    let id = uploaded(&repository, &telegram, &browser, &data, NOW + 20).await;
    repository.close_session(&browser, NOW + 21).await.unwrap();
    assert_eq!(status_of(&repository, &id).await, "cancelled");
    assert_eq!(stored_parts(&repository).await, 0);
    assert_eq!(active_clients(&repository).await, [telegram.as_str()]);
    assert_eq!(epoch(&repository).await, 1);
    assert_eq!(
        refused(repository.activation_state(&browser, NOW + 22).await),
        ActivationRefusal::AuthenticationRequired
    );
}

#[tokio::test]
async fn a_newer_request_by_another_client_supersedes_the_open_one() {
    let (repository, telegram, browser) = pair().await;
    assert!(!sign_in(&repository, "bond", "tablet", "iPad · Discord", NOW, DAY).await);
    let tablet = token("tablet");
    let data = payload(100);

    let first = uploaded(&repository, &telegram, &browser, &data, NOW).await;
    let second = ask(&repository, &tablet, NOW + 1).await;
    assert_ne!(first, second);
    assert_eq!(status_of(&repository, &first).await, "superseded");
    assert_eq!(stored_parts(&repository).await, 0);
    assert_eq!(
        count(
            &repository,
            "SELECT COUNT(*) FROM product_activation_requests \
             WHERE status IN ('pending', 'approved', 'syncing')"
        )
        .await,
        1
    );

    let replaced = state(&repository, &browser, NOW + 2).await;
    assert_eq!(replaced.state, ClientState::ActivationSuperseded);
    assert_eq!(replaced.request.unwrap().id, first);
    for step in [
        refused(
            repository
                .cancel_activation(&browser, &first, NOW + 2)
                .await,
        ),
        refused(
            repository
                .complete_activation(&browser, &first, &sync_digest(&data), NOW + 2)
                .await,
        ),
    ] {
        assert_eq!(step, ActivationRefusal::StepRefused);
    }

    // The active client is asked about the newer request only.
    let shown = state(&repository, &telegram, NOW + 2).await;
    assert_eq!(shown.state, ClientState::ApprovalRequested);
    let request = shown.request.unwrap();
    assert_eq!(request.id, second);
    assert_eq!(request.to_label, "iPad · Discord");
    assert_eq!(active_clients(&repository).await, [telegram]);
}

#[tokio::test]
async fn every_open_step_runs_out_and_never_into_a_switch() {
    let (repository, telegram, browser) = pair().await;

    // Nobody answers.
    let id = ask(&repository, &browser, NOW).await;
    assert_eq!(
        state(&repository, &browser, NOW + RESPOND_SECONDS - 1)
            .await
            .state,
        ClientState::ActivationPending
    );
    let late = state(&repository, &browser, NOW + RESPOND_SECONDS).await;
    assert_eq!(late.state, ClientState::ActivationExpired);
    assert_eq!(late.request.unwrap().status, ActivationStatus::Expired);
    assert_eq!(
        refused(
            repository
                .approve_activation(&telegram, &id, NOW + RESPOND_SECONDS)
                .await
        ),
        ActivationRefusal::StepRefused
    );
    assert_eq!(
        state(&repository, &telegram, NOW + RESPOND_SECONDS)
            .await
            .state,
        ClientState::AuthenticatedActive
    );

    // The active client agrees and never uploads: its side broke off.
    let start = NOW + 1_000;
    let id = ask(&repository, &browser, start).await;
    let agreed = repository
        .approve_activation(&telegram, &id, start + 10)
        .await
        .unwrap();
    assert_eq!(
        agreed.request.unwrap().deadline,
        Some(start + 10 + UPLOAD_SECONDS)
    );
    let broken = state(&repository, &browser, start + 10 + UPLOAD_SECONDS).await;
    assert_eq!(broken.state, ClientState::ActivationInterrupted);
    assert_eq!(status_of(&repository, &id).await, "interrupted");
    let released = state(&repository, &telegram, start + 10 + UPLOAD_SECONDS).await;
    assert_eq!(released.state, ClientState::AuthenticatedActive);
    assert!(released.may_act);

    // Every part gives the upload its time again; a stalled one breaks off
    // and takes the parts with it.
    let start = NOW + 2_000;
    let id = ask(&repository, &browser, start).await;
    repository
        .approve_activation(&telegram, &id, start)
        .await
        .unwrap();
    let data = payload(3 * SYNC_PART_BYTES);
    let part = |index: u64| {
        let from = (index * SYNC_PART_BYTES) as usize;
        (
            UploadPartQuery {
                offset: index * SYNC_PART_BYTES,
                total_bytes: data.len() as u64,
            },
            &data[from..from + SYNC_PART_BYTES as usize],
        )
    };
    let (query, bytes) = part(0);
    repository
        .upload_activation_sync_part(&telegram, &id, query, bytes, start + 100)
        .await
        .unwrap();
    let (query, bytes) = part(1);
    let second = repository
        .upload_activation_sync_part(&telegram, &id, query, bytes, start + 100 + 110)
        .await
        .unwrap();
    assert_eq!(
        second.request.unwrap().deadline,
        Some(start + 210 + UPLOAD_SECONDS)
    );
    let (query, bytes) = part(2);
    assert_eq!(
        refused(
            repository
                .upload_activation_sync_part(
                    &telegram,
                    &id,
                    query,
                    bytes,
                    start + 210 + UPLOAD_SECONDS
                )
                .await
        ),
        ActivationRefusal::StepRefused
    );
    assert_eq!(status_of(&repository, &id).await, "interrupted");
    assert_eq!(stored_parts(&repository).await, 0);

    // The asking client does not finish fetching in time.
    let start = NOW + 3_000;
    let data = payload(SYNC_PART_BYTES + 5);
    let id = uploaded(&repository, &telegram, &browser, &data, start).await;
    repository
        .download_activation_sync_part(&browser, &id, 0, start + SYNC_SECONDS - 1)
        .await
        .unwrap();
    assert_eq!(
        refused(
            repository
                .complete_activation(&browser, &id, &sync_digest(&data), start + SYNC_SECONDS)
                .await
        ),
        ActivationRefusal::StepRefused
    );
    assert_eq!(status_of(&repository, &id).await, "expired");
    assert_eq!(stored_parts(&repository).await, 0);
    assert_eq!(
        state(&repository, &browser, start + SYNC_SECONDS)
            .await
            .state,
        ClientState::ActivationExpired
    );

    assert_eq!(active_clients(&repository).await, [telegram]);
    assert_eq!(epoch(&repository).await, 1);
}

#[tokio::test]
async fn the_active_client_leaving_interrupts_and_leaves_nobody_active() {
    let (repository, telegram, browser) = pair().await;
    let data = payload(50);
    let id = uploaded(&repository, &telegram, &browser, &data, NOW).await;

    repository.close_session(&telegram, NOW + 1).await.unwrap();
    assert_eq!(status_of(&repository, &id).await, "interrupted");
    assert_eq!(stored_parts(&repository).await, 0);
    assert!(active_clients(&repository).await.is_empty());
    assert_eq!(epoch(&repository).await, 2);

    // The uploaded data does not make the asking client active, and with
    // nobody active there is nobody to ask: it has to sign in again.
    assert_eq!(
        refused(
            repository
                .complete_activation(&browser, &id, &sync_digest(&data), NOW + 2)
                .await
        ),
        ActivationRefusal::StepRefused
    );
    let alone = state(&repository, &browser, NOW + 2).await;
    assert_eq!(alone.state, ClientState::SignInRequired);
    assert_eq!(alone.active_client_label, None);
    assert!(!alone.active && !alone.may_act);
    assert_eq!(
        refused(repository.request_activation(&browser, NOW + 2).await),
        ActivationRefusal::NoActiveClient
    );
    assert!(!can_write(&repository, &browser, NOW + 2).await);

    // The next sign-in is the active client, without a request.
    assert!(sign_in(&repository, "bond", "phone", "Pixel · Chrome", NOW + 3, DAY).await);
    assert_eq!(active_clients(&repository).await, [token("phone")]);
    assert_eq!(epoch(&repository).await, 3);
    let told = state(&repository, &browser, NOW + 3).await;
    assert_eq!(told.state, ClientState::ActivationInterrupted);
    assert_eq!(told.active_client_label.as_deref(), Some("Pixel · Chrome"));
    // And it is the one a fresh request goes to.
    let fresh = ask(&repository, &browser, NOW + 4).await;
    assert_ne!(fresh, id);
    assert_eq!(
        state(&repository, &token("phone"), NOW + 4).await.state,
        ClientState::ApprovalRequested
    );
}

#[tokio::test]
async fn a_session_running_out_ends_its_part_in_a_request() {
    // The active client's session runs out while it is asked.
    let repository = repository().await;
    assert!(sign_in(&repository, "bond", "telegram", TELEGRAM, NOW, 1_000).await);
    assert!(!sign_in(&repository, "bond", "browser", BROWSER, NOW, DAY).await);
    let (telegram, browser) = (token("telegram"), token("browser"));
    let id = ask(&repository, &browser, NOW + 950).await;
    repository
        .approve_activation(&telegram, &id, NOW + 960)
        .await
        .unwrap();
    let alone = state(&repository, &browser, NOW + 1_000).await;
    assert_eq!(alone.state, ClientState::SignInRequired);
    assert_eq!(status_of(&repository, &id).await, "interrupted");
    assert_eq!(
        refused(repository.activation_state(&telegram, NOW + 1_000).await),
        ActivationRefusal::AuthenticationRequired
    );
    // An expired session is not an active client: the next sign-in is.
    assert!(
        sign_in(
            &repository,
            "bond",
            "phone",
            "Pixel · Chrome",
            NOW + 1_001,
            DAY
        )
        .await
    );
    assert_eq!(active_clients(&repository).await, [token("phone")]);

    // The asking client's session runs out while its data waits.
    let repository = self::repository().await;
    assert!(sign_in(&repository, "bond", "telegram", TELEGRAM, NOW, DAY).await);
    assert!(!sign_in(&repository, "bond", "browser", BROWSER, NOW, 1_000).await);
    let id = uploaded(&repository, &telegram, &browser, &payload(9), NOW + 950).await;
    let released = state(&repository, &telegram, NOW + 1_000).await;
    assert_eq!(released.state, ClientState::AuthenticatedActive);
    assert_eq!(released.request, None);
    assert_eq!(status_of(&repository, &id).await, "expired");
    assert_eq!(stored_parts(&repository).await, 0);
    assert_eq!(active_clients(&repository).await, [telegram]);
}

#[tokio::test]
async fn a_request_exists_only_for_its_two_clients() {
    let (repository, telegram, browser) = pair().await;
    assert!(!sign_in(&repository, "bond", "tablet", "iPad · Discord", NOW, DAY).await);
    assert!(sign_in(&repository, "other", "stranger", "Pixel · Chrome", NOW, DAY).await);
    let (tablet, stranger) = (token("tablet"), token("stranger"));
    let data = payload(20);
    let digest = sync_digest(&data);
    let id = uploaded(&repository, &telegram, &browser, &data, NOW).await;
    let part = UploadPartQuery {
        offset: 0,
        total_bytes: data.len() as u64,
    };
    let now = NOW + 1;

    // A client that is neither side, of this Bond or another, finds nothing.
    for outsider in [&tablet, &stranger] {
        let attempts = [
            refused(repository.cancel_activation(outsider, &id, now).await),
            refused(repository.approve_activation(outsider, &id, now).await),
            refused(repository.decline_activation(outsider, &id, now).await),
            refused(
                repository
                    .upload_activation_sync_part(outsider, &id, part, &data, now)
                    .await,
            ),
            refused(
                repository
                    .download_activation_sync_part(outsider, &id, 0, now)
                    .await,
            ),
            refused(
                repository
                    .complete_activation(outsider, &id, &digest, now)
                    .await,
            ),
        ];
        for attempt in attempts {
            assert_eq!(attempt, ActivationRefusal::RequestNotFound);
        }
    }
    // Each side has its own steps and none of the other's.
    let crossed = [
        refused(repository.approve_activation(&browser, &id, now).await),
        refused(repository.decline_activation(&browser, &id, now).await),
        refused(
            repository
                .upload_activation_sync_part(&browser, &id, part, &data, now)
                .await,
        ),
        refused(repository.cancel_activation(&telegram, &id, now).await),
        refused(
            repository
                .download_activation_sync_part(&telegram, &id, 0, now)
                .await,
        ),
        refused(
            repository
                .complete_activation(&telegram, &id, &digest, now)
                .await,
        ),
    ];
    for attempt in crossed {
        assert_eq!(attempt, ActivationRefusal::RequestNotFound);
    }
    // A guessed or malformed id is no request.
    for guess in [
        "ffffffffffffffffffffffffffffffff",
        "",
        "../sync",
        &id.to_uppercase(),
    ] {
        assert_eq!(
            refused(repository.cancel_activation(&browser, guess, now).await),
            ActivationRefusal::RequestNotFound,
            "{guess}"
        );
    }
    // No session, no step.
    assert_eq!(
        refused(repository.activation_state(&token("nobody"), now).await),
        ActivationRefusal::AuthenticationRequired
    );
    assert_eq!(
        refused(
            repository
                .complete_activation(&token("nobody"), &id, &digest, now)
                .await
        ),
        ActivationRefusal::AuthenticationRequired
    );
    // The active client has nothing to ask for.
    assert_eq!(
        refused(repository.request_activation(&telegram, now).await),
        ActivationRefusal::AlreadyActive
    );

    // None of it moved the request.
    assert_eq!(status_of(&repository, &id).await, "syncing");
    assert_eq!(stored_parts(&repository).await, 1);
    assert_eq!(active_clients(&repository).await.len(), 2);
    let other = state(&repository, &tablet, now).await;
    assert_eq!(other.state, ClientState::ActiveElsewhere);
    assert_eq!(other.request, None);
}

#[tokio::test]
async fn parts_arrive_in_order_and_a_retry_changes_nothing() {
    let (repository, telegram, browser) = pair().await;
    let data = payload(2 * SYNC_PART_BYTES + 3);
    let total_bytes = data.len() as u64;
    let part_bytes = SYNC_PART_BYTES as usize;
    let id = ask(&repository, &browser, NOW).await;
    let send = |offset: u64, total_bytes: u64, bytes: &[u8]| {
        let bytes = bytes.to_vec();
        let (repository, telegram, id) = (&repository, &telegram, &id);
        async move {
            repository
                .upload_activation_sync_part(
                    telegram,
                    id,
                    UploadPartQuery {
                        offset,
                        total_bytes,
                    },
                    &bytes,
                    NOW + 1,
                )
                .await
        }
    };

    // Nothing is uploaded before the active client agreed.
    assert_eq!(
        refused(send(0, total_bytes, &data[..part_bytes]).await),
        ActivationRefusal::StepRefused
    );
    let agreed = repository
        .approve_activation(&telegram, &id, NOW + 1)
        .await
        .unwrap();
    // Agreeing twice is the same answer.
    assert_eq!(
        repository
            .approve_activation(&telegram, &id, NOW + 1)
            .await
            .unwrap(),
        agreed
    );

    assert_eq!(
        refused(send(0, SYNC_MAX_BYTES + 1, &data[..part_bytes]).await),
        ActivationRefusal::SyncTooLarge
    );
    let misplaced = [
        // Not the first part, a part of the wrong size, one past the end.
        send(
            SYNC_PART_BYTES,
            total_bytes,
            &data[part_bytes..2 * part_bytes],
        )
        .await,
        send(0, total_bytes, &data[..part_bytes - 1]).await,
        send(0, total_bytes, &data[..=part_bytes]).await,
        send(7, total_bytes, &data[7..10]).await,
        send(total_bytes, total_bytes, b"").await,
    ];
    for attempt in misplaced {
        assert_eq!(refused(attempt), ActivationRefusal::SyncOutOfOrder);
    }
    assert_eq!(stored_parts(&repository).await, 0);

    let first = send(0, total_bytes, &data[..part_bytes]).await.unwrap();
    // The same part again is a retry; other bytes in its place are not.
    assert_eq!(
        send(0, total_bytes, &data[..part_bytes]).await.unwrap(),
        first
    );
    assert_eq!(
        refused(send(0, total_bytes, &vec![0xff; part_bytes]).await),
        ActivationRefusal::SyncOutOfOrder
    );
    // The size is fixed by the first part.
    assert_eq!(
        refused(
            send(
                SYNC_PART_BYTES,
                total_bytes + 1,
                &data[part_bytes..2 * part_bytes]
            )
            .await
        ),
        ActivationRefusal::SyncOutOfOrder
    );
    // A gap is not the next part.
    assert_eq!(
        refused(send(2 * SYNC_PART_BYTES, total_bytes, &data[2 * part_bytes..]).await),
        ActivationRefusal::SyncOutOfOrder
    );
    send(
        SYNC_PART_BYTES,
        total_bytes,
        &data[part_bytes..2 * part_bytes],
    )
    .await
    .unwrap();
    let whole = send(2 * SYNC_PART_BYTES, total_bytes, &data[2 * part_bytes..])
        .await
        .unwrap();
    assert_eq!(whole.state, ClientState::HandoffSyncing);
    // A retry of the last part after the upload is whole is still a retry.
    assert_eq!(
        send(2 * SYNC_PART_BYTES, total_bytes, &data[2 * part_bytes..])
            .await
            .unwrap(),
        whole
    );
    assert_eq!(
        refused(send(2 * SYNC_PART_BYTES, total_bytes, b"abc").await),
        ActivationRefusal::SyncOutOfOrder
    );
    assert_eq!(stored_parts(&repository).await, 3);

    let fetched = download(&repository, &browser, &id, total_bytes, NOW + 2).await;
    assert_eq!(fetched, data);
    repository
        .complete_activation(&browser, &id, &sync_digest(&fetched), NOW + 3)
        .await
        .unwrap();
    assert_eq!(active_clients(&repository).await, [browser]);
}

#[tokio::test]
async fn a_bond_opens_a_bounded_number_of_requests_an_hour() {
    let (repository, _telegram, browser) = pair().await;
    for attempt in 0..u64::from(REQUESTS_PER_HOUR) {
        let id = ask(&repository, &browser, NOW + attempt).await;
        // Asking again while it is open is the same request, not another.
        assert_eq!(ask(&repository, &browser, NOW + attempt).await, id);
        repository
            .cancel_activation(&browser, &id, NOW + attempt)
            .await
            .unwrap();
    }
    assert_eq!(
        refused(repository.request_activation(&browser, NOW + 100).await),
        ActivationRefusal::RequestLimit
    );
    // The refusal leaves the last outcome in place.
    assert_eq!(
        state(&repository, &browser, NOW + 100).await.state,
        ClientState::ActivationCancelled
    );
    ask(&repository, &browser, NOW + 60 * 60).await;
}

#[tokio::test]
async fn a_request_made_under_an_earlier_epoch_cannot_commit() {
    let (repository, telegram, browser) = pair().await;
    let data = payload(64);
    let id = uploaded(&repository, &telegram, &browser, &data, NOW).await;
    download(&repository, &browser, &id, 64, NOW).await;

    // The active client changed by some other way since the request was made.
    sqlx::query("UPDATE public_profiles SET session_epoch = session_epoch + 1")
        .execute(&repository.pool)
        .await
        .unwrap();
    assert_eq!(
        refused(
            repository
                .complete_activation(&browser, &id, &sync_digest(&data), NOW + 1)
                .await
        ),
        ActivationRefusal::StepRefused
    );
    assert_eq!(status_of(&repository, &id).await, "interrupted");
    assert_eq!(stored_parts(&repository).await, 0);
    assert_eq!(active_clients(&repository).await, [telegram]);
    assert_eq!(
        state(&repository, &browser, NOW + 1).await.state,
        ClientState::ActivationInterrupted
    );
}

#[tokio::test]
async fn only_a_restored_active_session_writes() {
    let (repository, telegram, browser) = pair().await;
    // A verified callback names the owner but is no session.
    let callback = repository
        .attach(&VerifiedSignIn {
            issuer: ISSUER.to_owned(),
            subject: "bond".to_owned(),
            pub_dress: None,
            auth_time: NOW,
        })
        .await
        .unwrap();
    assert!(matches!(
        repository.publish_experience_carry(&callback, 5, 5).await,
        Err(PublicInfoError::SessionInactive)
    ));
    assert!(can_write(&repository, &telegram, NOW).await);
    assert!(!can_write(&repository, &browser, NOW).await);

    // An authorization restored before the switch does not outlive it.
    let before = repository
        .restore_session(&telegram, NOW)
        .await
        .unwrap()
        .unwrap();
    let id = uploaded(&repository, &telegram, &browser, b"", NOW).await;
    repository
        .complete_activation(&browser, &id, &sync_digest(b""), NOW)
        .await
        .unwrap();
    assert!(matches!(
        repository.publish_experience_carry(&before, 9, 9).await,
        Err(PublicInfoError::SessionInactive)
    ));
    assert!(can_write(&repository, &browser, NOW).await);
    let profile = repository
        .restore_session(&browser, NOW)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(profile.profile.experience.bond_xp, 1);
}

#[tokio::test]
async fn signing_in_again_on_the_same_client_keeps_its_place() {
    let (repository, telegram, browser) = pair().await;
    let owner = repository
        .restore_session(&telegram, NOW)
        .await
        .unwrap()
        .unwrap();
    let open = |name: &'static str, label: &'static str, previous: Option<String>| {
        let (repository, owner) = (&repository, &owner);
        async move {
            repository
                .open_session(
                    owner,
                    &token(name),
                    label,
                    previous.as_deref(),
                    NOW + 1,
                    NOW + DAY,
                )
                .await
                .unwrap()
        }
    };

    // The active client signs in again: its old session is closed first, so
    // it is still the active one.
    assert!(open("telegram-2", TELEGRAM, Some(telegram.clone())).await);
    assert_eq!(active_clients(&repository).await, [token("telegram-2")]);
    assert_eq!(
        refused(repository.activation_state(&telegram, NOW + 1).await),
        ActivationRefusal::AuthenticationRequired
    );
    // An inactive client signing in again stays inactive.
    assert!(!open("browser-2", BROWSER, Some(browser.clone())).await);
    assert_eq!(active_clients(&repository).await, [token("telegram-2")]);
    assert_eq!(
        count(&repository, "SELECT COUNT(*) FROM product_sessions").await,
        2
    );

    // Presenting another Bond's session closes nothing of theirs.
    assert!(sign_in(&repository, "other", "stranger", "Pixel · Chrome", NOW, DAY).await);
    assert!(!open("browser-3", BROWSER, Some(token("stranger"))).await);
    assert_eq!(
        state(&repository, &token("stranger"), NOW + 1).await.state,
        ClientState::AuthenticatedActive
    );

    // A label a person cannot read as one line is not a label.
    assert!(matches!(
        repository
            .open_session(
                &owner,
                &token("bad"),
                "iPhone\nSafari",
                None,
                NOW,
                NOW + DAY
            )
            .await,
        Err(PublicInfoError::InvalidSession)
    ));
}

const STORE_BEFORE_ACTIVATION: &str = "
CREATE TABLE public_profiles (
    issuer TEXT NOT NULL COLLATE BINARY CHECK (length(issuer) > 0),
    subject TEXT NOT NULL COLLATE BINARY CHECK (length(subject) BETWEEN 1 AND 255),
    profile_id TEXT NOT NULL UNIQUE DEFAULT (lower(hex(randomblob(16)))),
    pub_dress TEXT COLLATE BINARY,
    last_auth_time INTEGER NOT NULL CHECK (last_auth_time >= 0),
    bond_xp INTEGER NOT NULL DEFAULT 0 CHECK (bond_xp BETWEEN 0 AND 1000000000),
    avaia_xp INTEGER NOT NULL DEFAULT 0 CHECK (avaia_xp BETWEEN 0 AND 1000000000),
    PRIMARY KEY (issuer, subject)
) STRICT;
CREATE TABLE product_sessions (
    token_hash TEXT PRIMARY KEY NOT NULL COLLATE BINARY CHECK (length(token_hash) BETWEEN 32 AND 128),
    issuer TEXT NOT NULL COLLATE BINARY,
    subject TEXT NOT NULL COLLATE BINARY,
    profile_id TEXT NOT NULL COLLATE BINARY,
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    expires_at INTEGER NOT NULL CHECK (expires_at > created_at),
    FOREIGN KEY (issuer, subject) REFERENCES public_profiles (issuer, subject) ON DELETE CASCADE
) STRICT;
CREATE INDEX product_sessions_by_owner ON product_sessions (issuer, subject);
";

#[tokio::test]
async fn a_store_from_before_activation_keeps_every_session_and_one_active() {
    let directory = tempfile::tempdir().unwrap();
    let url = format!(
        "sqlite://{}",
        directory.path().join("product.sqlite").display()
    );
    let old = SqlitePoolOptions::new()
        .connect_with(
            SqliteConnectOptions::from_str(&url)
                .unwrap()
                .create_if_missing(true),
        )
        .await
        .unwrap();
    sqlx::raw_sql(STORE_BEFORE_ACTIVATION)
        .execute(&old)
        .await
        .unwrap();
    for (subject, profile) in [("bond", "profile-1"), ("other", "profile-2")] {
        sqlx::query(
            "INSERT INTO public_profiles (issuer, subject, profile_id, last_auth_time) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(ISSUER)
        .bind(subject)
        .bind(profile)
        .bind(NOW as i64)
        .execute(&old)
        .await
        .unwrap();
    }
    for (name, subject, profile, created_at) in [
        ("older", "bond", "profile-1", NOW),
        ("newer", "bond", "profile-1", NOW + 50),
        ("only", "other", "profile-2", NOW + 10),
    ] {
        sqlx::query(
            "INSERT INTO product_sessions \
             (token_hash, issuer, subject, profile_id, created_at, expires_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(token(name))
        .bind(ISSUER)
        .bind(subject)
        .bind(profile)
        .bind(created_at as i64)
        .bind((created_at + DAY) as i64)
        .execute(&old)
        .await
        .unwrap();
    }
    old.close().await;

    let repository = PublicInfoRepository::connect(&url).await.unwrap();
    let mut active = active_clients(&repository).await;
    active.sort();
    assert_eq!(active, [token("newer"), token("only")]);
    assert_eq!(
        count(&repository, "SELECT COUNT(*) FROM product_sessions").await,
        3
    );
    assert_eq!(
        state(&repository, &token("older"), NOW + 60).await.state,
        ClientState::ActiveElsewhere
    );
    // The upgraded store runs a handoff like a new one.
    let (newer, older) = (token("newer"), token("older"));
    let id = uploaded(&repository, &newer, &older, b"world", NOW + 60).await;
    repository
        .complete_activation(&older, &id, &sync_digest(b"world"), NOW + 61)
        .await
        .unwrap();
    assert!(can_write(&repository, &older, NOW + 61).await);
    repository.pool.close().await;

    // Opening it again changes nothing.
    let reopened = PublicInfoRepository::connect(&url).await.unwrap();
    let mut active = active_clients(&reopened).await;
    active.sort();
    assert_eq!(active, [token("older"), token("only")]);
}
