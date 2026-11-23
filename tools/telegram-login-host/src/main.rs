// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use std::{env, error::Error, io};

use nilxone_identity::{
    aiaiaiai_sign_in::{SignInClient, SignInClientConfig},
    public_info::{LegacyBondMigrationRepository, PublicInfoRepository},
};
use nilxone_telegram_login_host::{Authority, ChatSignIn, ISSUER, authenticated_router};

fn configured(name: &str) -> io::Result<String> {
    env::var(name).map_err(|_| io::Error::other(format!("{name} is required")))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::init();

    let callback = "https://nilx.one/api/v1/telegram/login/callback";
    let config = SignInClientConfig::public(ISSUER, None, "0x1", callback)?;
    // The bot signs its requests with the @nilxone_bot token both processes
    // already hold; the host holds no other credential.
    let authority = Authority::from_bot_token(&configured("TELEGRAM_BOT_TOKEN")?)
        .ok_or_else(|| io::Error::other("TELEGRAM_BOT_TOKEN is not a Telegram Bot API token"))?;
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
            provisioning: Some(provisioning),
            session_ttl_seconds,
        }),
    );
    let listener = tokio::net::TcpListener::bind(configured("TELEGRAM_LOGIN_LISTEN_ADDR")?).await?;
    axum::serve(listener, router).await?;
    Ok(())
}
