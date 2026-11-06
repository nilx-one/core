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
    assert_eq!(tables, ["public_profiles"]);
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
