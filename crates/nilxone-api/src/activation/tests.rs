// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::*;
use crate::error::ErrorEnvelope;

const ID: &str = "0123456789abcdef0123456789abcdef";
const EMPTY_DIGEST: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn pinned<T: Serialize + DeserializeOwned>(fixture: &str) -> T {
    let expected: Value = serde_json::from_str(fixture).expect("fixture is JSON");
    let parsed: T = serde_json::from_str(fixture).expect("fixture parses");
    assert_eq!(serde_json::to_value(&parsed).expect("serializes"), expected);
    parsed
}

#[test]
fn state_fixtures_round_trip() {
    let elsewhere: ActivationState = pinned(include_str!(
        "../../fixtures/activation/state_active_elsewhere.json"
    ));
    assert_eq!(elsewhere.state, ClientState::ActiveElsewhere);
    assert_eq!(
        elsewhere.active_client_label.as_deref(),
        Some("iPhone · Telegram")
    );
    assert_eq!(elsewhere.request, None);

    let syncing: ActivationState = pinned(include_str!(
        "../../fixtures/activation/state_activation_syncing.json"
    ));
    let request = syncing.request.expect("request");
    assert_eq!(request.id, ID);
    assert_eq!(request.role, ActivationRole::Requester);
    assert_eq!(request.status, ActivationStatus::Syncing);
    assert_eq!(request.from_label, "iPhone · Telegram");
    assert_eq!(request.to_label, "Mac · Safari");
    assert_eq!(request.deadline, Some(1_800_000_700));
    assert_eq!(request.progress.expect("progress").percent, 78);

    let asked: ActivationState = pinned(include_str!(
        "../../fixtures/activation/state_approval_requested.json"
    ));
    assert_eq!(asked.state, ClientState::ApprovalRequested);
    assert!(asked.active && asked.may_act);

    let uploading: ActivationState = pinned(include_str!(
        "../../fixtures/activation/state_upload_required.json"
    ));
    assert_eq!(uploading.state, ClientState::UploadRequired);
    assert!(uploading.active && !uploading.may_act);

    let switching: ActivationState = pinned(include_str!(
        "../../fixtures/activation/state_handoff_switching.json"
    ));
    assert_eq!(switching.state, ClientState::HandoffSyncing);

    let completed: ActivationState = pinned(include_str!(
        "../../fixtures/activation/state_completed.json"
    ));
    let request = completed.request.expect("request");
    assert_eq!(request.status, ActivationStatus::Completed);
    assert_eq!(request.deadline, None);
    assert_eq!(request.progress.expect("progress").percent, 100);

    let rejected: ActivationState = pinned(include_str!(
        "../../fixtures/activation/state_activation_rejected.json"
    ));
    assert_eq!(rejected.state, ClientState::ActivationRejected);
    assert_eq!(rejected.request.expect("request").progress, None);
}

#[test]
fn every_state_fixture_is_one_a_client_may_draw() {
    for fixture in [
        include_str!("../../fixtures/activation/state_active_elsewhere.json"),
        include_str!("../../fixtures/activation/state_approval_requested.json"),
        include_str!("../../fixtures/activation/state_upload_required.json"),
        include_str!("../../fixtures/activation/state_activation_syncing.json"),
        include_str!("../../fixtures/activation/state_handoff_switching.json"),
        include_str!("../../fixtures/activation/state_completed.json"),
        include_str!("../../fixtures/activation/state_activation_rejected.json"),
    ] {
        assert!(
            matches!(read_activation(true, fixture), ActivationReading::State(_)),
            "{fixture}"
        );
    }
}

fn step_states(progress: &HandoffProgress) -> [StepState; 3] {
    assert_eq!(
        progress.steps.map(|row| row.step),
        [HandoffStep::Confirm, HandoffStep::Sync, HandoffStep::Switch]
    );
    progress.steps.map(|row| row.state)
}

#[test]
fn progress_names_one_current_step_and_reaches_100_only_at_the_switch() {
    use ActivationStatus::{Approved, Completed, Pending, Syncing};
    use StepState::{Current, Done, Waiting};

    const TOTAL: u64 = 4 * SYNC_PART_BYTES;
    let project = |status, total, uploaded, delivered| {
        HandoffProgress::project(status, total, uploaded, delivered).expect("progress")
    };

    let pending = project(Pending, None, 0, 0);
    assert_eq!(step_states(&pending), [Current, Waiting, Waiting]);
    assert_eq!((pending.percent, pending.total_bytes), (0, None));

    // Agreed, nothing uploaded yet: the size is still unknown.
    let agreed = project(Approved, None, 0, 0);
    assert_eq!(step_states(&agreed), [Done, Current, Waiting]);
    assert_eq!(agreed.percent, 0);

    let mut last = 0;
    for parts in 1..4 {
        let uploading = project(Approved, Some(TOTAL), parts * SYNC_PART_BYTES, 0);
        assert_eq!(step_states(&uploading), [Done, Current, Waiting]);
        assert!(uploading.percent > last && uploading.percent < 50);
        last = uploading.percent;
    }
    let uploaded = project(Syncing, Some(TOTAL), TOTAL, 0);
    assert_eq!(step_states(&uploaded), [Done, Current, Waiting]);
    assert_eq!(uploaded.percent, 50);
    for parts in 1..4 {
        let delivering = project(Syncing, Some(TOTAL), TOTAL, parts * SYNC_PART_BYTES);
        assert_eq!(step_states(&delivering), [Done, Current, Waiting]);
        assert!(delivering.percent > last && delivering.percent < 99);
        last = delivering.percent;
    }
    let delivered = project(Syncing, Some(TOTAL), TOTAL, TOTAL);
    assert_eq!(step_states(&delivered), [Done, Done, Current]);
    assert_eq!(delivered.percent, 99);
    // A restarted fetch is shown as what it is.
    assert_eq!(
        project(Syncing, Some(TOTAL), TOTAL, SYNC_PART_BYTES).percent,
        62
    );

    let switched = project(Completed, Some(TOTAL), TOTAL, TOTAL);
    assert_eq!(step_states(&switched), [Done, Done, Done]);
    assert_eq!(switched.percent, 100);

    // Nothing to carry is still a handoff: only the switch is left.
    let empty = project(Syncing, Some(0), 0, 0);
    assert_eq!(step_states(&empty), [Done, Done, Current]);
    assert_eq!(empty.percent, 99);

    // Counts that cannot be are clamped, never shown past the truth.
    let clamped = project(Approved, Some(TOTAL), u64::MAX, u64::MAX);
    assert_eq!(
        (clamped.uploaded_bytes, clamped.delivered_bytes),
        (TOTAL, 0)
    );
    assert_eq!(clamped.percent, 50);

    for status in ActivationStatus::ALL {
        let progress = HandoffProgress::project(*status, Some(TOTAL), TOTAL, TOTAL);
        assert_eq!(
            progress.is_some(),
            status.is_open() || *status == Completed,
            "{status:?}"
        );
        if let Some(progress) = progress.filter(|_| status.is_open()) {
            let current = step_states(&progress)
                .iter()
                .filter(|state| **state == Current)
                .count();
            assert_eq!(current, 1, "{status:?}");
            assert!(progress.percent < 100);
        }
    }
}

#[test]
fn parts_are_fixed_size_and_only_the_last_is_shorter() {
    assert_eq!(sync_part_len(0, 0), Some(0));
    assert_eq!(sync_part_len(0, SYNC_PART_BYTES), None);
    assert_eq!(sync_part_len(1, 0), Some(1));
    assert_eq!(sync_part_len(SYNC_PART_BYTES, 0), Some(SYNC_PART_BYTES));
    assert_eq!(sync_part_len(SYNC_PART_BYTES, SYNC_PART_BYTES), None);
    assert_eq!(sync_part_len(SYNC_PART_BYTES + 7, SYNC_PART_BYTES), Some(7));
    assert_eq!(sync_part_len(SYNC_PART_BYTES + 7, 1), None);
    assert_eq!(
        sync_part_len(SYNC_MAX_BYTES, SYNC_MAX_BYTES - SYNC_PART_BYTES),
        Some(SYNC_PART_BYTES)
    );
    assert_eq!(sync_part_len(SYNC_MAX_BYTES + 1, 0), None);
    assert_eq!(sync_part_len(SYNC_MAX_BYTES, u64::MAX), None);

    // Walking the parts covers the data exactly once.
    let total = 3 * SYNC_PART_BYTES + 5;
    let mut offset = 0;
    let mut parts = 0;
    while let Some(len) = sync_part_len(total, offset) {
        offset += len;
        parts += 1;
    }
    assert_eq!((offset, parts), (total, 4));

    let part = |offset, total_bytes| UploadPartQuery {
        offset,
        total_bytes,
    };
    assert_eq!(part(0, 7).validate(7), Ok(()));
    assert_eq!(part(0, 0).validate(0), Ok(()));
    assert_eq!(
        part(0, 7).validate(6),
        Err(ActivationRefusal::SyncOutOfOrder)
    );
    assert_eq!(
        part(3, 7).validate(4),
        Err(ActivationRefusal::SyncOutOfOrder)
    );
    assert_eq!(
        part(0, SYNC_MAX_BYTES + 1).validate(1),
        Err(ActivationRefusal::SyncTooLarge)
    );
    assert_eq!(
        sync_part_wire("600000", "262144"),
        r#"{"end":524288,"kind":"part","last":false,"offset":262144}"#
    );
    assert_eq!(
        sync_part_wire("600000", "524288"),
        r#"{"end":600000,"kind":"part","last":true,"offset":524288}"#
    );
    assert_eq!(
        sync_part_wire("0", "0"),
        r#"{"end":0,"kind":"part","last":true,"offset":0}"#
    );
    for (total, offset) in [
        ("600000", "600000"),
        ("600000", "1"),
        ("16777217", "0"),
        ("600000", ""),
        ("0600000", "0"),
        ("600000", "+0"),
        ("-1", "0"),
        ("18446744073709551616", "0"),
    ] {
        assert_eq!(sync_part_wire(total, offset), r#"{"kind":"none"}"#);
    }
    assert!(serde_json::from_str::<UploadPartQuery>(r#"{"offset":0,"total_bytes":7}"#).is_ok());
    assert!(
        serde_json::from_str::<UploadPartQuery>(r#"{"offset":0,"total_bytes":7,"digest":"x"}"#)
            .is_err()
    );
    assert!(serde_json::from_str::<DownloadPartQuery>(r#"{"offset":262144}"#).is_ok());
    assert!(serde_json::from_str::<DownloadPartQuery>(r#"{"offset":-1}"#).is_err());
}

#[test]
fn a_completion_names_a_digest_and_nothing_else() {
    let request: CompleteRequest = pinned(include_str!(
        "../../fixtures/activation/complete_request.json"
    ));
    assert_eq!(request.validate(), Ok(EMPTY_DIGEST));
    assert!(
        serde_json::from_str::<CompleteRequest>(include_str!(
            "../../fixtures/activation/rejected/complete_request_unknown_field.json"
        ))
        .is_err()
    );
    for digest in ["", "abc", &EMPTY_DIGEST.to_uppercase(), &"g".repeat(64)] {
        let request = CompleteRequest {
            digest: digest.to_owned(),
        };
        assert_eq!(
            request.validate(),
            Err(ActivationRefusal::InvalidSyncDigest)
        );
    }
}

#[test]
fn the_digest_is_sha_256_of_the_exact_bytes() {
    assert_eq!(sync_digest(b""), EMPTY_DIGEST);
    assert_eq!(
        sync_digest(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert!(is_sync_digest(&sync_digest(b"0x1")));
    assert_ne!(sync_digest(b"0x1"), sync_digest(b"0x1 "));

    // Parts in order give the digest of the whole.
    let mut parts = SyncDigest::new();
    parts.update(b"a");
    parts.update(b"");
    parts.update(b"bc");
    assert_eq!(parts.finish(), sync_digest(b"abc"));
    assert_eq!(SyncDigest::new().finish(), EMPTY_DIGEST);
}

#[test]
fn request_paths_take_only_request_ids() {
    assert_eq!(
        request_path(APPROVE_PATH, ID).as_deref(),
        Some("/api/v1/session/activation/requests/0123456789abcdef0123456789abcdef/approve")
    );
    for template in [
        CANCEL_PATH,
        APPROVE_PATH,
        DECLINE_PATH,
        SYNC_PATH,
        COMPLETE_PATH,
    ] {
        assert!(template.starts_with(REQUESTS_PATH));
        assert!(template.contains("{id}"));
        assert_eq!(request_path(template, "../state"), None);
        assert_eq!(request_path(template, &ID.to_uppercase()), None);
    }
    assert!(is_client_label("iPhone · Safari"));
    assert!(is_client_label(""));
    assert!(!is_client_label("iPhone\nSafari"));
    assert!(!is_client_label(&"x".repeat(CLIENT_LABEL_MAX_BYTES + 1)));
}

#[test]
fn statuses_have_one_spelling_and_three_are_open() {
    for status in ActivationStatus::ALL {
        assert_eq!(ActivationStatus::parse(status.as_str()), Some(*status));
        assert_eq!(
            serde_json::to_value(status).expect("serializes"),
            Value::String(status.as_str().to_owned())
        );
    }
    assert_eq!(ActivationStatus::parse("accepted"), None);
    let open: Vec<_> = ActivationStatus::ALL
        .iter()
        .filter(|status| status.is_open())
        .collect();
    assert_eq!(
        open,
        [
            &ActivationStatus::Pending,
            &ActivationStatus::Approved,
            &ActivationStatus::Syncing
        ]
    );
}

#[test]
fn the_active_client_is_asked_then_stops_acting_once_it_agrees() {
    use ActivationRole::{Approver, Requester};
    use ActivationStatus::{Approved, Completed, Declined, Pending, Syncing};

    let active = |request| ClientState::project(true, false, request);
    assert_eq!(active(None), ClientState::AuthenticatedActive);
    assert_eq!(
        active(Some((Approver, Pending))),
        ClientState::ApprovalRequested
    );
    assert_eq!(
        active(Some((Approver, Approved))),
        ClientState::UploadRequired
    );
    assert_eq!(
        active(Some((Approver, Syncing))),
        ClientState::HandoffSyncing
    );
    // A request that is over asks nothing more of the active client, and the
    // client that just completed its own request is simply active.
    assert_eq!(
        active(Some((Approver, Declined))),
        ClientState::AuthenticatedActive
    );
    assert_eq!(
        active(Some((Requester, Completed))),
        ClientState::AuthenticatedActive
    );

    assert!(ClientState::AuthenticatedActive.may_act());
    assert!(ClientState::ApprovalRequested.may_act());
    assert!(!ClientState::UploadRequired.may_act());
    assert!(!ClientState::HandoffSyncing.may_act());
}

#[test]
fn an_inactive_client_never_acts_and_sees_how_its_last_request_ended() {
    use ActivationRole::{Approver, Requester};
    use ActivationStatus::{
        Approved, Cancelled, Completed, Declined, Expired, Interrupted, Objected, Pending,
        Superseded, Syncing,
    };

    let inactive = |request| ClientState::project(false, true, request);
    let expected = [
        (None, ClientState::ActiveElsewhere),
        (Some((Requester, Pending)), ClientState::ActivationPending),
        (Some((Requester, Approved)), ClientState::ActivationApproved),
        (Some((Requester, Syncing)), ClientState::ActivationSyncing),
        (Some((Requester, Declined)), ClientState::ActivationRejected),
        (Some((Requester, Objected)), ClientState::ActivationObjected),
        (
            Some((Requester, Cancelled)),
            ClientState::ActivationCancelled,
        ),
        (
            Some((Requester, Superseded)),
            ClientState::ActivationSuperseded,
        ),
        (Some((Requester, Expired)), ClientState::ActivationExpired),
        (
            Some((Requester, Interrupted)),
            ClientState::ActivationInterrupted,
        ),
        // It was active once and handed over since: back to the plain state.
        (Some((Requester, Completed)), ClientState::ActiveElsewhere),
        // The client that approved is inactive after the switch.
        (Some((Approver, Completed)), ClientState::ActiveElsewhere),
    ];
    for (request, state) in expected {
        assert_eq!(inactive(request), state, "{request:?}");
        assert!(!state.may_act());
    }

    // With no active client there is nobody to ask: an old outcome gives way
    // to signing in again, but a request still open is shown as it is.
    let alone = |request| ClientState::project(false, false, request);
    assert_eq!(alone(None), ClientState::SignInRequired);
    assert_eq!(
        alone(Some((Requester, Interrupted))),
        ClientState::SignInRequired
    );
    assert_eq!(
        alone(Some((Requester, Syncing))),
        ClientState::ActivationSyncing
    );
    assert!(!ClientState::SignInRequired.may_act());
}

#[test]
fn refusals_carry_stable_codes() {
    let error: ErrorEnvelope = pinned(include_str!(
        "../../fixtures/activation/error_step_refused.json"
    ));
    assert_eq!(
        ActivationRefusal::from_code(&error.error.code),
        Some(ActivationRefusal::StepRefused)
    );
    assert_eq!(
        ActivationRefusal::AuthenticationRequired.code(),
        crate::error::PROVIDER_AUTHENTICATION_REQUIRED
    );
    let statuses: Vec<_> = ActivationRefusal::ALL
        .iter()
        .map(|refusal| (refusal.code(), refusal.http_status()))
        .collect();
    assert_eq!(
        statuses,
        [
            ("provider_authentication_required", 401),
            ("already_active", 409),
            ("no_active_client", 409),
            ("activation_request_limit", 429),
            ("activation_request_not_found", 404),
            ("activation_step_refused", 409),
            ("sync_payload_too_large", 413),
            ("sync_out_of_order", 409),
            ("invalid_sync_digest", 422),
            ("sync_incomplete", 409),
        ]
    );
}

#[test]
fn a_client_reads_a_state_or_a_reason_and_never_guesses_a_switch() {
    let syncing = include_str!("../../fixtures/activation/state_activation_syncing.json");
    let ActivationReading::State(state) = read_activation(true, syncing) else {
        panic!("a state");
    };
    assert_eq!(state.state, ClientState::ActivationSyncing);
    let wire: Value = serde_json::from_str(&read_activation_wire(true, syncing)).expect("json");
    assert_eq!(wire["kind"], "state");
    assert_eq!(wire["state"], "activation_syncing");
    assert_eq!(wire["request"]["status"], "syncing");
    assert_eq!(wire["request"]["progress"]["percent"], 78);
    assert_eq!(wire["request"]["progress"]["steps"][1]["step"], "sync");
    assert_eq!(wire["request"]["progress"]["steps"][1]["state"], "current");

    let refused = include_str!("../../fixtures/activation/error_step_refused.json");
    assert_eq!(
        read_activation_wire(false, refused),
        r#"{"kind":"refused","reason":"activation_step_refused"}"#
    );
    assert_eq!(
        read_activation_wire(
            false,
            r#"{"error":{"code":"provider_authentication_required","message":"x"}}"#
        ),
        r#"{"kind":"authentication_required"}"#
    );
    // A failed answer is never a state, whatever its body says, and neither
    // is a body that contradicts itself or names a state Core does not know.
    let unavailable = r#"{"kind":"unavailable"}"#;
    assert_eq!(read_activation_wire(false, syncing), unavailable);
    assert_eq!(
        read_activation_wire(false, r#"{"error":{"code":"teapot","message":"x"}}"#),
        unavailable
    );
    assert_eq!(
        read_activation_wire(
            true,
            include_str!("../../fixtures/activation/rejected/state_may_act_disagrees.json")
        ),
        unavailable
    );
    assert_eq!(
        read_activation_wire(
            true,
            include_str!("../../fixtures/activation/rejected/state_progress_disagrees.json")
        ),
        unavailable
    );
    // An open request always says how far it is.
    let ActivationReading::State(mut silent) = read_activation(true, syncing) else {
        panic!("a state");
    };
    silent.request.as_mut().expect("request").progress = None;
    assert_eq!(
        read_activation_wire(true, &serde_json::to_string(&silent).expect("serializes")),
        unavailable
    );
    assert_eq!(
        read_activation_wire(true, &syncing.replace("activation_syncing", "world_ready")),
        unavailable
    );
    assert_eq!(read_activation_wire(true, "{}"), unavailable);
}
