// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use std::{
    io::{Read, Write},
    net::TcpListener,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signer as _, SigningKey};
use serde_json::json;

use super::*;
use crate::{
    NativeAuthConfig,
    aiaiaiai_sign_in::{SignInClient, SignInClientConfig},
};

const ISSUER: &str = "https://auth.aiaiaiai.org";
const NOW: u64 = 1_800_000_000;

fn identity(subject: &str, address: Option<&str>, auth_time: u64) -> VerifiedSignIn {
    VerifiedSignIn {
        issuer: ISSUER.to_owned(),
        subject: subject.to_owned(),
        pub_dress: address.map(str::to_owned),
        auth_time,
    }
}

async fn repository() -> PublicInfoRepository {
    PublicInfoRepository::connect("sqlite::memory:")
        .await
        .unwrap()
}

fn flow(backchannel: Option<&str>, client: &str) -> SignInFlow {
    let client = SignInClient::new(
        SignInClientConfig::new(
            ISSUER,
            backchannel,
            client,
            "a client secret with at least 32 bytes",
            "https://nilx.one/api/v1/auth/aiaiaiai/callback",
        )
        .unwrap(),
    );
    SignInFlow::new(
        client,
        NativeAuthConfig::new([1; 32], [2; 32])
            .unwrap()
            .secret_digester(),
    )
}

#[tokio::test]
async fn separate_database_and_subject_attachment_preserve_experience_across_rename() {
    let repository = repository().await;
    let first = repository
        .attach(&identity("subject-1", Some("0x0sky"), NOW))
        .await
        .unwrap();
    let published = repository
        .publish_experience_carry(&first, 120, 45)
        .await
        .unwrap();
    let renamed = repository
        .attach(&identity("subject-1", Some("0x0sea"), NOW + 1))
        .await
        .unwrap();
    assert_eq!(renamed.profile.profile_id, first.profile.profile_id);
    assert_eq!(renamed.profile.pub_dress.as_deref(), Some("0x0sea"));
    assert_eq!(renamed.profile.experience, published.experience);
    let stale = repository
        .attach(&identity("subject-1", Some("0x0sky"), NOW))
        .await
        .unwrap();
    assert_eq!(stale.profile, renamed.profile);
    let missing_claim = repository
        .attach(&identity("subject-1", None, NOW + 2))
        .await
        .unwrap();
    assert_eq!(missing_claim.profile, renamed.profile);

    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table'")
            .fetch_all(&repository.pool)
            .await
            .unwrap();
    assert_eq!(tables, ["public_profiles", "product_sessions"]);
}

#[tokio::test]
async fn matching_public_name_never_acquires_another_profile_or_its_data() {
    let repository = repository().await;
    let owner = repository
        .attach(&identity("subject-1", Some("0x0sky"), NOW))
        .await
        .unwrap();
    repository
        .publish_experience_carry(&owner, 120, 45)
        .await
        .unwrap();
    let other = repository
        .attach(&identity("subject-2", Some("0x0sky"), NOW))
        .await
        .unwrap();
    assert_ne!(owner.profile.profile_id, other.profile.profile_id);
    assert_eq!(other.profile.experience.bond_xp, 0);
    let mut other_issuer = identity("subject-1", Some("0x0sky"), NOW);
    other_issuer.issuer = "https://other.example".to_owned();
    let other_issuer = repository.attach(&other_issuer).await.unwrap();
    assert_ne!(other_issuer.profile.profile_id, owner.profile.profile_id);
    assert_ne!(other_issuer.profile.profile_id, other.profile.profile_id);
}

#[tokio::test]
async fn public_projection_excludes_authentication_material_and_carries_are_idempotent() {
    let repository = repository().await;
    let owner = repository
        .attach(&identity("private-pairwise-subject", Some("0x0sky"), NOW))
        .await
        .unwrap();
    let first = repository
        .publish_experience_carry(&owner, 120, 45)
        .await
        .unwrap();
    assert_eq!(
        repository
            .publish_experience_carry(&owner, 120, 45)
            .await
            .unwrap(),
        first
    );
    assert_eq!(
        repository
            .publish_experience_carry(&owner, 10, 5)
            .await
            .unwrap(),
        first
    );
    assert!(matches!(
        repository
            .publish_experience_carry(&owner, MAX_CARRY_XP + 1, 0)
            .await,
        Err(PublicInfoError::InvalidExperience)
    ));
    let body = serde_json::to_value(
        repository
            .read_public(&first.profile_id)
            .await
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        body,
        json!({
            "profile_id": first.profile_id, "pub_dress": "0x0sky",
            "experience": { "authority": "client", "bond_xp": 120, "avaia_xp": 45 }
        })
    );
    assert!(!body.to_string().contains("private-pairwise-subject"));
    assert!(!body.to_string().contains(ISSUER));
    assert!(repository.read_public("missing").await.unwrap().is_none());
    sqlx::query("DELETE FROM public_profiles")
        .execute(&repository.pool)
        .await
        .unwrap();
    assert!(matches!(
        repository.publish_experience_carry(&owner, 0, 0).await,
        Err(PublicInfoError::ProfileGone)
    ));
}

#[tokio::test]
async fn profile_survives_reopening_and_concurrent_first_sign_ins() {
    let directory = tempfile::tempdir().unwrap();
    let url = format!(
        "sqlite://{}",
        directory.path().join("product.sqlite").display()
    );
    let first = PublicInfoRepository::connect(&url).await.unwrap();
    let second = PublicInfoRepository::connect(&url).await.unwrap();
    let sign_in = identity("same-subject", Some("0x0sky"), NOW);
    let (a, b) = tokio::join!(first.attach(&sign_in), second.attach(&sign_in));
    let a = a.unwrap();
    assert_eq!(a.profile, b.unwrap().profile);
    first.publish_experience_carry(&a, 120, 45).await.unwrap();
    first.pool.close().await;
    second.pool.close().await;
    let reopened = PublicInfoRepository::connect(&url).await.unwrap();
    let again = reopened.attach(&sign_in).await.unwrap();
    assert_eq!(again.profile.profile_id, a.profile.profile_id);
    assert_eq!(again.profile.experience.bond_xp, 120);
}

#[tokio::test]
async fn file_store_allows_writes_while_another_pool_holds_a_read_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let url = format!(
        "sqlite://{}",
        directory.path().join("product.sqlite").display()
    );
    let reader = PublicInfoRepository::connect(&url).await.unwrap();
    let writer = PublicInfoRepository::connect(&url).await.unwrap();
    assert_eq!(reader.pool.options().get_max_connections(), 5);
    assert_eq!(repository().await.pool.options().get_max_connections(), 1);
    // Check a second simultaneous connection too: options must apply to every
    // pooled connection, not just the one that initialized the schema.
    let mut first = reader.pool.acquire().await.unwrap();
    let mut second = reader.pool.acquire().await.unwrap();
    for connection in [&mut first, &mut second] {
        let journal: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut **connection)
            .await
            .unwrap();
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&mut **connection)
            .await
            .unwrap();
        let busy_timeout: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
            .fetch_one(&mut **connection)
            .await
            .unwrap();
        assert_eq!(journal, "wal");
        assert_eq!(foreign_keys, 1);
        assert_eq!(busy_timeout, 5_000);
    }
    drop(first);
    drop(second);
    let owner = reader
        .attach(&identity("subject", Some("0x0sky"), NOW))
        .await
        .unwrap();
    let mut snapshot = reader.pool.begin().await.unwrap();
    let before: i64 =
        sqlx::query_scalar("SELECT bond_xp FROM public_profiles WHERE profile_id = ?")
            .bind(&owner.profile.profile_id)
            .fetch_one(&mut *snapshot)
            .await
            .unwrap();
    assert_eq!(before, 0);
    let committed = tokio::time::timeout(
        Duration::from_secs(2),
        writer.publish_experience_carry(&owner, 120, 45),
    )
    .await
    .expect("WAL writer must not wait for the read snapshot")
    .unwrap();
    assert_eq!(committed.experience.bond_xp, 120);
    let still_before: i64 =
        sqlx::query_scalar("SELECT bond_xp FROM public_profiles WHERE profile_id = ?")
            .bind(&owner.profile.profile_id)
            .fetch_one(&mut *snapshot)
            .await
            .unwrap();
    assert_eq!(still_before, 0);
    snapshot.rollback().await.unwrap();
    assert_eq!(
        reader
            .read_public(&owner.profile.profile_id)
            .await
            .unwrap()
            .unwrap(),
        committed
    );
}

#[tokio::test]
async fn malformed_display_claim_on_a_known_subject_leaves_the_entire_row_unchanged() {
    let repository = repository().await;
    let owner = repository
        .attach(&identity("subject", Some("0x0sky"), NOW))
        .await
        .unwrap();
    let published = repository
        .publish_experience_carry(&owner, 120, 45)
        .await
        .unwrap();
    for auth_time in [NOW - 1, NOW, NOW + 1] {
        assert!(matches!(
            repository
                .attach(&identity("subject", Some("not an address"), auth_time))
                .await,
            Err(PublicInfoError::InvalidPublicAddress)
        ));
        assert_eq!(
            repository
                .read_public(&published.profile_id)
                .await
                .unwrap()
                .unwrap(),
            published
        );
        let stored_auth_time: i64 =
            sqlx::query_scalar("SELECT last_auth_time FROM public_profiles WHERE profile_id = ?")
                .bind(&published.profile_id)
                .fetch_one(&repository.pool)
                .await
                .unwrap();
        assert_eq!(stored_auth_time, i64::try_from(NOW).unwrap());
    }
    let missing = repository
        .attach(&identity("subject", None, NOW + 2))
        .await
        .unwrap();
    assert_eq!(missing.profile, published);
}

#[tokio::test]
async fn malformed_display_claim_is_not_persisted() {
    let repository = repository().await;
    assert!(matches!(
        repository
            .attach(&identity("subject", Some("not an address"), NOW))
            .await,
        Err(PublicInfoError::InvalidPublicAddress)
    ));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM public_profiles")
        .fetch_one(&repository.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let unnamed = repository
        .attach(&identity("subject", None, NOW))
        .await
        .unwrap();
    assert_eq!(unnamed.profile.pub_dress, None);
}

#[tokio::test]
async fn other_product_client_and_invalid_callback_cannot_create_profiles() {
    let repository = repository().await;
    assert!(matches!(
        PublicInfoService::new(flow(None, "prism-hub"), repository.clone()),
        Err(PublicInfoError::WrongClient)
    ));
    let service = PublicInfoService::new(flow(None, "nilx-one"), repository.clone()).unwrap();
    let result = service.finish(None, Callback::default(), NOW).await;
    assert!(matches!(result.outcome, Err(SignInFailure::Expired)));
    assert!(result.clear_cookie.contains("Max-Age=0"));
    let started = service.start(Some("//other.example"), NOW).unwrap();
    let cookie = started.set_cookie.split(';').next().unwrap();
    let result = service
        .finish(
            Some(cookie),
            Callback {
                state: Some("foreign-state"),
                code: Some("code"),
                error: None,
            },
            NOW,
        )
        .await;
    assert!(matches!(result.outcome, Err(SignInFailure::Invalid)));
    assert!(result.clear_cookie.contains("Max-Age=0"));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM public_profiles")
        .fetch_one(&repository.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

// A local issuer substitute exercises real HTTP code exchange, JWKS and
// Ed25519 verification, followed by product profile creation. No live AIS.
#[tokio::test]
async fn oidc_callback_creates_product_profile_only_for_a_valid_nilx_one_token() {
    for audience in ["nilx-one", "prism-hub"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let backchannel = format!("http://{}", listener.local_addr().unwrap());
        let repository = repository().await;
        let service =
            PublicInfoService::new(flow(Some(&backchannel), "nilx-one"), repository.clone())
                .unwrap();
        let started = service.start(Some("/map"), NOW).unwrap();
        let pairs = started
            .location
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>();
        let state = pairs.get("state").unwrap().to_string();
        let key = SigningKey::from_bytes(&[7; 32]);
        let claims = json!({ "iss": ISSUER, "sub": "ais-pairwise-subject", "aud": audience,
            "exp": NOW + 300, "iat": NOW, "auth_time": NOW,
            "nonce": pairs.get("nonce").unwrap(), "pub_dress": "0x0sky" });
        let signed = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(json!({"alg":"EdDSA", "kid":"test"}).to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let token = format!(
            "{signed}.{}",
            URL_SAFE_NO_PAD.encode(key.sign(signed.as_bytes()).to_bytes())
        );
        let keys = json!({"keys":[{"kty":"OKP", "crv":"Ed25519", "alg":"EdDSA", "use":"sig", "kid":"test", "x":URL_SAFE_NO_PAD.encode(key.verifying_key().as_bytes())}]}).to_string();
        let server = std::thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            for _ in 0..2 {
                let (mut stream, _) = loop {
                    match listener.accept() {
                        Ok(connection) => break connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                std::time::Instant::now() < deadline,
                                "issuer request timed out"
                            );
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Err(error) => panic!("issuer accept failed: {error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 4096];
                loop {
                    let read = stream.read(&mut buffer).unwrap();
                    assert_ne!(read, 0);
                    request.extend_from_slice(&buffer[..read]);
                    if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]);
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|value| value.trim().parse().unwrap())
                            })
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let body = if request.starts_with(b"POST /token ") {
                    json!({"id_token": token}).to_string()
                } else {
                    assert!(request.starts_with(b"GET /.well-known/jwks.json "));
                    keys.clone()
                };
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let cookie = started.set_cookie.split(';').next().unwrap();
        let result = service
            .finish(
                Some(cookie),
                Callback {
                    state: Some(&state),
                    code: Some("test-code"),
                    error: None,
                },
                NOW,
            )
            .await;
        server.join().unwrap();
        assert!(result.clear_cookie.contains("Max-Age=0"));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM public_profiles")
            .fetch_one(&repository.pool)
            .await
            .unwrap();
        if audience == "nilx-one" {
            let signed_in = result.outcome.unwrap();
            assert_eq!(signed_in.return_path, "/map");
            assert_eq!(
                signed_in.owner.public_profile().pub_dress.as_deref(),
                Some("0x0sky")
            );
            assert_eq!(count, 1);
        } else {
            assert!(matches!(result.outcome, Err(SignInFailure::Invalid)));
            assert_eq!(count, 0);
        }
    }
}

const TOKEN_HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_HASH: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

#[tokio::test]
async fn a_session_restores_its_owner_until_it_expires_or_is_closed() {
    let repository = repository().await;
    let owner = repository
        .attach(&identity("subject-1", Some("0x0sky"), NOW))
        .await
        .unwrap();
    repository
        .open_session(&owner, TOKEN_HASH, NOW, NOW + 60)
        .await
        .unwrap();

    // The restored owner is a full authorization result: it can mutate its
    // own row, and it reflects what the profile is now, not at sign-in.
    let restored = repository
        .restore_session(TOKEN_HASH, NOW + 1)
        .await
        .unwrap()
        .expect("live session");
    assert_eq!(restored.profile, owner.profile);
    repository
        .publish_experience_carry(&restored, 7, 3)
        .await
        .unwrap();
    let again = repository
        .restore_session(TOKEN_HASH, NOW + 2)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(again.profile.experience.bond_xp, 7);

    assert!(
        repository
            .restore_session(OTHER_HASH, NOW + 1)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        repository
            .restore_session(TOKEN_HASH, NOW + 60)
            .await
            .unwrap()
            .is_none(),
        "expired exactly at expires_at"
    );
    repository.close_session(TOKEN_HASH).await.unwrap();
    repository.close_session(TOKEN_HASH).await.unwrap();
    assert!(
        repository
            .restore_session(TOKEN_HASH, NOW + 1)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_session_is_refused_or_dies_when_its_binding_does_not_hold() {
    let repository = repository().await;
    let owner = repository
        .attach(&identity("subject-1", Some("0x0sky"), NOW))
        .await
        .unwrap();
    let other = repository
        .attach(&identity("subject-2", Some("0x0sea"), NOW))
        .await
        .unwrap();

    // No lifetime, and no second session under one token digest.
    assert!(matches!(
        repository.open_session(&owner, TOKEN_HASH, NOW, NOW).await,
        Err(PublicInfoError::InvalidSession)
    ));
    repository
        .open_session(&owner, TOKEN_HASH, NOW, NOW + 60)
        .await
        .unwrap();
    assert!(matches!(
        repository
            .open_session(&other, TOKEN_HASH, NOW, NOW + 60)
            .await,
        Err(PublicInfoError::InvalidSession)
    ));
    let still_owner = repository
        .restore_session(TOKEN_HASH, NOW + 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(still_owner.profile.profile_id, owner.profile.profile_id);

    // A row whose profile id no longer matches its subject restores nobody,
    // even though the token digest and the subject are both real.
    sqlx::query("UPDATE product_sessions SET profile_id = ? WHERE token_hash = ?")
        .bind(&other.profile.profile_id)
        .bind(TOKEN_HASH)
        .execute(&repository.pool)
        .await
        .unwrap();
    assert!(
        repository
            .restore_session(TOKEN_HASH, NOW + 1)
            .await
            .unwrap()
            .is_none()
    );

    // A deleted profile takes its sessions with it and cannot open new ones.
    repository
        .open_session(&other, OTHER_HASH, NOW, NOW + 60)
        .await
        .unwrap();
    sqlx::query("DELETE FROM public_profiles WHERE subject = 'subject-2'")
        .execute(&repository.pool)
        .await
        .unwrap();
    assert!(
        repository
            .restore_session(OTHER_HASH, NOW + 1)
            .await
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        repository
            .open_session(&other, OTHER_HASH, NOW, NOW + 60)
            .await,
        Err(PublicInfoError::ProfileGone)
    ));
}

#[tokio::test]
async fn opening_a_session_drops_the_owners_expired_ones_only() {
    let repository = repository().await;
    let owner = repository
        .attach(&identity("subject-1", Some("0x0sky"), NOW))
        .await
        .unwrap();
    let other = repository
        .attach(&identity("subject-2", Some("0x0sea"), NOW))
        .await
        .unwrap();
    repository
        .open_session(&owner, TOKEN_HASH, NOW, NOW + 10)
        .await
        .unwrap();
    repository
        .open_session(&other, OTHER_HASH, NOW, NOW + 10)
        .await
        .unwrap();
    let third = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    repository
        .open_session(&owner, third, NOW + 20, NOW + 80)
        .await
        .unwrap();
    let left: Vec<String> =
        sqlx::query_scalar("SELECT token_hash FROM product_sessions ORDER BY token_hash")
            .fetch_all(&repository.pool)
            .await
            .unwrap();
    assert_eq!(left, [third, OTHER_HASH]);
}
