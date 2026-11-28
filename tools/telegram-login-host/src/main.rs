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

/// The bot's public numeric ID in its canonical form: digits only, no sign or
/// leading zero, and it fits `u64`. Web and Infra enforce the same rule, so the
/// three places that read `TELEGRAM_BOT_ID` agree on what is accepted.
fn canonical_bot_id(value: &str) -> io::Result<u64> {
    if value.is_empty() || value.starts_with('0') || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(io::Error::other(
            "TELEGRAM_BOT_ID must be a canonical positive number",
        ));
    }
    value
        .parse()
        .map_err(|_| io::Error::other("TELEGRAM_BOT_ID must fit an unsigned 64-bit number"))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::init();

    let check_only = match env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => false,
        [flag] if flag == "--check-public-authority" => true,
        _ => {
            return Err(io::Error::other(
                "Usage: nilxone-telegram-login-host [--check-public-authority]",
            )
            .into());
        }
    };
    if env::var_os("TELEGRAM_BOT_TOKEN").is_some() {
        return Err(io::Error::other("Bot API token must not be present in Core").into());
    }
    let callback = "https://nilx.one/api/v1/telegram/login/callback";
    let config = SignInClientConfig::public(ISSUER, None, "0x1", callback)?;
    // Only public configuration crosses the Bot boundary. No token fallback.
    let bot_id = canonical_bot_id(&configured("TELEGRAM_BOT_ID")?)?;
    let authority = Authority::from_public_key(bot_id, &configured("TELEGRAM_BOT_PUBLIC_KEY")?)
        .ok_or_else(|| io::Error::other("Invalid Telegram Bot public authority"))?;
    if check_only {
        println!("Telegram Gateway public authority v2 validated");
        return Ok(());
    }
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

#[cfg(test)]
mod tests {
    use super::canonical_bot_id;

    #[test]
    fn the_bot_id_is_canonical_and_fits_u64() {
        assert_eq!(canonical_bot_id("123456").unwrap(), 123_456);
        assert_eq!(canonical_bot_id("1").unwrap(), 1);
        assert_eq!(canonical_bot_id("18446744073709551615").unwrap(), u64::MAX);
        for bad in [
            "",
            "0",
            "0123",
            "+123",
            "-1",
            " 123",
            "12 3",
            "123:token",
            "18446744073709551616",
            "99999999999999999999",
        ] {
            assert!(canonical_bot_id(bad).is_err(), "{bad:?}");
        }
    }
}
