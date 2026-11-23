// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use std::{env, error::Error, io};

use nilxone_identity::{
    aiaiaiai_sign_in::{SignInClient, SignInClientConfig},
    public_info::{LegacyBondMigrationRepository, PublicInfoRepository},
};
use nilxone_telegram_login_host::{Authority, ChatSignIn, HostAisToken, ISSUER, authenticated_router};

fn configured(name: &str) -> io::Result<String> {
    env::var(name).map_err(|_| io::Error::other(format!("{name} is required")))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::init();

    let callback = "https://nilx.one/api/v1/telegram/login/callback";
    let config = SignInClientConfig::public(ISSUER, None, "0x1", callback)?;
    let authority = Authority::from_public_jwk(&configured("TELEGRAM_LOGIN_AIS_PUBLIC_JWK")?)
        .ok_or_else(|| io::Error::other("invalid AIS signing JWK"))?;
    let ais = HostAisToken::new(
        ISSUER,
        &configured("TELEGRAM_LOGIN_HOST_AIS_KEY_ID")?,
        &configured("TELEGRAM_LOGIN_HOST_AIS_SIGNING_SEED")?,
    )
    .ok_or_else(|| io::Error::other("invalid AIS product workload signing configuration"))?;
    let session_ttl_seconds = configured("TELEGRAM_LOGIN_SESSION_TTL_SECONDS")?.parse()?;
    if session_ttl_seconds == 0 {
        return Err(io::Error::other("Telegram session TTL must be positive").into());
    }
    let repository = PublicInfoRepository::connect(&configured("DATABASE_URL")?).await?;
    let provisioning = LegacyBondMigrationRepository::prepare(repository.clone()).await?;
    let router = authenticated_router(
        repository,
        SignInClient::new(config),
        authority,
        Some(ChatSignIn {
            ais,
            provisioning: Some(provisioning),
            session_ttl_seconds,
        }),
    );
    let listener = tokio::net::TcpListener::bind(configured("TELEGRAM_LOGIN_LISTEN_ADDR")?).await?;
    axum::serve(listener, router).await?;
    Ok(())
}
