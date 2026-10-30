// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! The 0x1 issuer side of 0xda-market's Special orders paid in seeds.
//!
//! Market owns offers and orders; Identity owns the wallet and the grant. This
//! worker polls Market's issuer API, claims a pending order, debits seeds and
//! grants Special in one SQLite transaction keyed by the order id, then reports
//! the outcome. A committed receipt is always confirmed, never rejected, so a
//! crash between the debit and the report can only lead to a repeated
//! confirmation, never to a second debit or an order lost after payment.
//!
//! It is off unless `MARKET_ISSUER_WORKER=1`, and it has no HTTP route: no
//! browser or user can reach the debit.

use std::{future::Future, sync::Arc, time::Duration};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tracing::{error, info, warn};

use nilxone_contracts::{SpecialSeedsOrder, special_seeds_term_months};

use crate::{
    IdentityRepository,
    repository::{IssuerSpecialOrderSnapshot, SpecialSeedsOutcome},
};

pub const ISSUER: &str = "0x1";
const VISIT_PROVIDER: &str = "nilx_visit";
/// Market's only subscription term policy. Core's `grant_special_months`
/// applies it; this service never computes a calendar month itself.
pub const TERM_POLICY: &str = "calendar_months_utc";
const PAGE_LIMIT: u32 = 50;
/// A tick reads at most this many pages per status, so one huge backlog
/// cannot starve the next tick; the following tick continues where it is.
const MAX_PAGES_PER_TICK: usize = 20;
const DEFAULT_MARKET_ISSUER_URL: &str = "https://0xda-market.nilx.one/issuer";

/// Rejection codes the issuer reports to Market. None of them follows a debit.
pub mod rejection {
    pub const INVALID_TERMS: &str = "invalid_terms";
    pub const VISIT_UNKNOWN: &str = "visit_unknown";
    pub const VISIT_EXPIRED: &str = "visit_expired";
    pub const RECIPIENT_MISMATCH: &str = "recipient_mismatch";
    pub const INSUFFICIENT_SEEDS: &str = "insufficient_seeds";
    pub const SESSION_CHANGED: &str = "session_changed";
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct IssuerOrder {
    pub id: String,
    pub attributes: IssuerOrderAttributes,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct IssuerOrderAttributes {
    pub sku: String,
    pub denomination: String,
    pub issuer: String,
    pub amount_minor: u64,
    pub price_id: u64,
    pub recipient_subject: String,
    pub link_provider: String,
    pub link_provider_user_id: String,
    pub status: String,
    pub created_at: String,
    #[serde(default)]
    pub term: Option<IssuerOrderTerm>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct IssuerOrderTerm {
    pub months: u32,
    pub policy: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IssuerOrderPage {
    pub orders: Vec<IssuerOrder>,
    pub next_cursor: Option<String>,
}

/// What Market checks against its frozen order before it records fulfilment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IssuerConfirmation {
    pub issuer_reference: String,
    pub amount_minor: u64,
    pub denomination: String,
    pub recipient_subject: String,
    pub term_months: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum MarketIssuerError {
    #[error("market issuer API is unavailable")]
    Unavailable,
    #[error("market issuer API answered {0}")]
    Status(u16),
    #[error("market issuer API answered an unreadable body")]
    Malformed,
}

/// Market's issuer API, as the worker needs it.
pub trait MarketIssuerApi: Send + Sync {
    fn orders(
        &self,
        status: &str,
        cursor: Option<&str>,
    ) -> impl Future<Output = Result<IssuerOrderPage, MarketIssuerError>> + Send;
    fn claim(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<IssuerOrder, MarketIssuerError>> + Send;
    fn fulfil(
        &self,
        id: &str,
        confirmation: &IssuerConfirmation,
    ) -> impl Future<Output = Result<IssuerOrder, MarketIssuerError>> + Send;
    fn reject(
        &self,
        id: &str,
        code: &str,
    ) -> impl Future<Output = Result<IssuerOrder, MarketIssuerError>> + Send;
}

/// How one order ended in this pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Settlement {
    Fulfilled,
    Rejected(&'static str),
    /// Another issuer decision exists already (expired, cancelled, done).
    Skipped,
    /// Left processing for an operator: a committed receipt disagrees with it.
    Held,
}

pub struct MarketIssuerWorker<M> {
    repository: IdentityRepository,
    market: M,
    now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl<M: MarketIssuerApi> MarketIssuerWorker<M> {
    pub fn new(
        repository: IdentityRepository,
        market: M,
        now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        Self {
            repository,
            market,
            now_ms,
        }
    }

    /// Recovers claimed orders first, then takes new ones. One failing order
    /// does not stop the others; it is retried on the next tick.
    pub async fn tick(&self) -> Result<(), MarketIssuerError> {
        for status in ["processing", "pending"] {
            let mut cursor: Option<String> = None;
            for _ in 0..MAX_PAGES_PER_TICK {
                let page = self.market.orders(status, cursor.as_deref()).await?;
                for order in &page.orders {
                    match self.handle(order).await {
                        Ok(outcome) => {
                            info!(order = %order.id, ?outcome, "market issuer order handled")
                        }
                        Err(failure) => {
                            warn!(order = %order.id, %failure, "market issuer order deferred")
                        }
                    }
                }
                cursor = page.next_cursor;
                if cursor.is_none() {
                    break;
                }
            }
        }
        Ok(())
    }

    pub async fn handle(&self, order: &IssuerOrder) -> Result<Settlement, WorkerFailure> {
        let order = match order.attributes.status.as_str() {
            "processing" => order.clone(),
            "pending" => self.market.claim(&order.id).await?,
            _ => return Ok(Settlement::Skipped),
        };
        // An expired authorization comes back rejected from the claim.
        if order.attributes.status != "processing" {
            return Ok(Settlement::Skipped);
        }
        self.settle(&order).await
    }

    async fn settle(&self, order: &IssuerOrder) -> Result<Settlement, WorkerFailure> {
        let attributes = &order.attributes;
        let months = match valid_terms(attributes) {
            Some(months) => months,
            None => return self.reject(order, rejection::INVALID_TERMS).await,
        };
        // A receipt means the debit and grant are committed: only confirm it.
        // The SKU fixes the term; the receipt's duration is what that term came
        // to on the clock at the time, so it is never recomputed here.
        if let Some(receipt) = self
            .repository
            .find_special_seeds_receipt(&order.id)
            .await?
        {
            let same = receipt.sku == attributes.sku
                && receipt.price_id == attributes.price_id
                && receipt.amount_minor == attributes.amount_minor;
            if !same {
                error!(order = %order.id, "committed receipt disagrees with the market order");
                return Ok(Settlement::Held);
            }
            return self.fulfil(order, months).await;
        }

        let Some(visit) = self
            .repository
            .find_market_visit(&attributes.link_provider_user_id)
            .await?
        else {
            return self.reject(order, rejection::VISIT_UNKNOWN).await;
        };
        if visit.recipient_pub_dress != attributes.recipient_subject {
            return self.reject(order, rejection::RECIPIENT_MISMATCH).await;
        }
        let now_ms = (self.now_ms)();
        if now_ms >= visit.spend_until_ms {
            return self.reject(order, rejection::VISIT_EXPIRED).await;
        }

        let snapshot = IssuerSpecialOrderSnapshot {
            order_id: order.id.clone(),
            active_session_hash: visit.session_token_hash.clone(),
            issuer: attributes.issuer.clone(),
            denomination: attributes.denomination.clone(),
            status: attributes.status.clone(),
            // The Bond's current handle; a rename after the visit still pays
            // from, and grants to, the same Bond.
            recipient_pub_dress: visit.owner.as_str().to_owned(),
            sku: attributes.sku.clone(),
            price_id: attributes.price_id,
            amount_minor: attributes.amount_minor,
            term_months: months,
        };
        match self
            .repository
            .settle_special_seeds(&visit.owner, &snapshot, now_ms)
            .await?
        {
            SpecialSeedsOutcome::Granted | SpecialSeedsOutcome::AlreadyGranted => {
                self.fulfil(order, months).await
            }
            SpecialSeedsOutcome::InsufficientSeeds => {
                self.reject(order, rejection::INSUFFICIENT_SEEDS).await
            }
            SpecialSeedsOutcome::InvalidTerms => self.reject(order, rejection::INVALID_TERMS).await,
            SpecialSeedsOutcome::SessionChanged => {
                self.reject(order, rejection::SESSION_CHANGED).await
            }
            SpecialSeedsOutcome::ConflictingReplay => {
                error!(order = %order.id, "order id was committed under different terms");
                Ok(Settlement::Held)
            }
        }
    }

    async fn fulfil(&self, order: &IssuerOrder, months: u32) -> Result<Settlement, WorkerFailure> {
        let attributes = &order.attributes;
        let confirmation = IssuerConfirmation {
            issuer_reference: issuer_reference(&order.id),
            amount_minor: attributes.amount_minor,
            denomination: attributes.denomination.clone(),
            recipient_subject: attributes.recipient_subject.clone(),
            term_months: months,
        };
        self.market.fulfil(&order.id, &confirmation).await?;
        Ok(Settlement::Fulfilled)
    }

    async fn reject(
        &self,
        order: &IssuerOrder,
        code: &'static str,
    ) -> Result<Settlement, WorkerFailure> {
        self.market.reject(&order.id, code).await?;
        Ok(Settlement::Rejected(code))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WorkerFailure {
    #[error(transparent)]
    Market(#[from] MarketIssuerError),
    #[error(transparent)]
    Repository(#[from] crate::RepositoryError),
}

/// One issuer operation per Market order: the reference is the order itself.
pub fn issuer_reference(order_id: &str) -> String {
    format!("identity:{order_id}")
}

/// Months of a well-formed Special order, or `None` for anything else. Core
/// decides the SKU's term and the order's structure; the worker adds what only
/// Market's wire carries: the visit binding and the frozen term policy.
fn valid_terms(attributes: &IssuerOrderAttributes) -> Option<u32> {
    let months = special_seeds_term_months(&SpecialSeedsOrder {
        sku: &attributes.sku,
        issuer: &attributes.issuer,
        denomination: &attributes.denomination,
        status: &attributes.status,
        price_id: attributes.price_id,
        amount_minor: attributes.amount_minor,
    })?;
    let term = attributes.term.as_ref()?;
    let ok = attributes.link_provider == VISIT_PROVIDER
        && term.policy == TERM_POLICY
        && term.months == months;
    ok.then_some(months)
}

/// Configuration read once at start-up.
#[derive(Clone)]
pub struct MarketIssuerConfig {
    pub base_url: url::Url,
    token: String,
    pub poll_interval: Duration,
}

impl std::fmt::Debug for MarketIssuerConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MarketIssuerConfig")
            .field("base_url", &self.base_url.as_str())
            .field("poll_interval", &self.poll_interval)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub enum MarketIssuerConfigError {
    #[error("MARKET_ISSUER_WORKER needs NILX_BROWSER_HANDOFF_TOKEN of at least 32 bytes")]
    WeakHandoffSecret,
    #[error("MARKET_ISSUER_URL must be an https URL without credentials, query or fragment")]
    InvalidUrl,
}

impl MarketIssuerConfig {
    /// Market derives the same issuer credential from the shared handoff
    /// secret (`ENABLE_NILX_SEEDS_CHECKOUT` in 0xda-market/core): lowercase
    /// hex HMAC-SHA256 of `nilx-market-issuer-v1`. No second secret travels.
    pub fn new(
        handoff_secret: &str,
        base_url: Option<&str>,
    ) -> Result<Self, MarketIssuerConfigError> {
        if handoff_secret.len() < 32 {
            return Err(MarketIssuerConfigError::WeakHandoffSecret);
        }
        let mut mac = Hmac::<Sha256>::new_from_slice(handoff_secret.as_bytes())
            .map_err(|_| MarketIssuerConfigError::WeakHandoffSecret)?;
        mac.update(b"nilx-market-issuer-v1");
        let token = mac
            .finalize()
            .into_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let mut base_url = url::Url::parse(base_url.unwrap_or(DEFAULT_MARKET_ISSUER_URL))
            .map_err(|_| MarketIssuerConfigError::InvalidUrl)?;
        if base_url.scheme() != "https"
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err(MarketIssuerConfigError::InvalidUrl);
        }
        if !base_url.path().ends_with('/') {
            let path = format!("{}/", base_url.path());
            base_url.set_path(&path);
        }
        Ok(Self {
            base_url,
            token,
            poll_interval: Duration::from_secs(5),
        })
    }

    /// `None` unless `MARKET_ISSUER_WORKER=1`.
    pub fn from_environment() -> Option<Result<Self, MarketIssuerConfigError>> {
        if std::env::var("MARKET_ISSUER_WORKER").ok().as_deref() != Some("1") {
            return None;
        }
        let secret = std::env::var("NILX_BROWSER_HANDOFF_TOKEN").unwrap_or_default();
        let url = std::env::var("MARKET_ISSUER_URL")
            .ok()
            .filter(|value| !value.is_empty());
        Some(Self::new(&secret, url.as_deref()))
    }
}

/// Market's issuer API over HTTPS.
pub struct HttpMarketIssuer {
    client: reqwest::Client,
    config: MarketIssuerConfig,
}

#[derive(Deserialize)]
struct Resource {
    data: IssuerOrder,
}

#[derive(Deserialize)]
struct Collection {
    data: Vec<IssuerOrder>,
    #[serde(default)]
    meta: CollectionMeta,
}

#[derive(Default, Deserialize)]
struct CollectionMeta {
    #[serde(default)]
    next_cursor: Option<String>,
}

impl HttpMarketIssuer {
    pub fn new(config: MarketIssuerConfig) -> Result<Self, MarketIssuerError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| MarketIssuerError::Unavailable)?;
        Ok(Self { client, config })
    }

    fn url(&self, path: &str) -> Result<url::Url, MarketIssuerError> {
        self.config
            .base_url
            .join(path)
            .map_err(|_| MarketIssuerError::Malformed)
    }

    async fn send<T: for<'de> Deserialize<'de>>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, MarketIssuerError> {
        let response = request
            .bearer_auth(&self.config.token)
            .send()
            .await
            .map_err(|_| MarketIssuerError::Unavailable)?;
        let status = response.status();
        if !status.is_success() {
            return Err(MarketIssuerError::Status(status.as_u16()));
        }
        response
            .json()
            .await
            .map_err(|_| MarketIssuerError::Malformed)
    }

    /// A read-only order lookup for authenticated checkout completion.
    pub async fn read_order(&self, id: &str) -> Result<IssuerOrder, MarketIssuerError> {
        let path = Self::order_path(id, "")?;
        let url = self.url(path.trim_end_matches('/'))?;
        let resource: Resource = self.send(self.client.get(url)).await?;
        Ok(resource.data)
    }

    fn order_path(id: &str, action: &str) -> Result<String, MarketIssuerError> {
        if id.is_empty()
            || id.len() > 128
            || matches!(id, "." | "..")
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b':'))
        {
            return Err(MarketIssuerError::Malformed);
        }
        Ok(format!("v1/orders/{id}/{action}"))
    }
}

impl MarketIssuerApi for HttpMarketIssuer {
    async fn orders(
        &self,
        status: &str,
        cursor: Option<&str>,
    ) -> Result<IssuerOrderPage, MarketIssuerError> {
        let mut url = self.url("v1/orders")?;
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("status", status)
                .append_pair("limit", &PAGE_LIMIT.to_string());
            if let Some(cursor) = cursor {
                query.append_pair("cursor", cursor);
            }
        }
        let collection: Collection = self.send(self.client.get(url)).await?;
        Ok(IssuerOrderPage {
            orders: collection.data,
            next_cursor: collection.meta.next_cursor,
        })
    }

    async fn claim(&self, id: &str) -> Result<IssuerOrder, MarketIssuerError> {
        let url = self.url(&Self::order_path(id, "claim")?)?;
        let resource: Resource = self.send(self.client.post(url)).await?;
        Ok(resource.data)
    }

    async fn fulfil(
        &self,
        id: &str,
        confirmation: &IssuerConfirmation,
    ) -> Result<IssuerOrder, MarketIssuerError> {
        let url = self.url(&Self::order_path(id, "fulfil")?)?;
        let resource: Resource = self.send(self.client.post(url).json(confirmation)).await?;
        Ok(resource.data)
    }

    async fn reject(&self, id: &str, code: &str) -> Result<IssuerOrder, MarketIssuerError> {
        let url = self.url(&Self::order_path(id, "reject")?)?;
        let resource: Resource = self
            .send(
                self.client
                    .post(url)
                    .json(&serde_json::json!({ "code": code })),
            )
            .await?;
        Ok(resource.data)
    }
}

/// Polls until the process ends. A failing tick backs off up to a minute.
pub async fn run_market_issuer_worker<M: MarketIssuerApi>(
    worker: MarketIssuerWorker<M>,
    interval: Duration,
) {
    let mut delay = interval;
    loop {
        match worker.tick().await {
            Ok(()) => delay = interval,
            Err(failure) => {
                warn!(%failure, "market issuer tick failed");
                delay = (delay * 2).min(Duration::from_secs(60));
            }
        }
        tokio::time::sleep(delay).await;
    }
}

/// Wall clock in milliseconds; a clock before 1970 reads as 0.
pub fn system_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::PubDress;

    const DAY: u64 = 86_400_000;
    // 2026-10-29T14:05:00.123Z
    const CREATED_MS: u64 = 1_793_282_700_123;
    const NOW_MS: u64 = CREATED_MS + 60_000;
    const VISIT: &str = "vvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvv";

    #[test]
    fn credential_matches_the_market_derivation() {
        let config = MarketIssuerConfig::new(&"h".repeat(32), None).expect("config");
        // OpenSSL::HMAC.hexdigest("SHA256", "h" * 32, "nilx-market-issuer-v1")
        assert_eq!(
            config.token,
            "f4e75f5ae9bdca0bb7774cc05d520ebb036d2b1981dacda17ae68de0d988216c"
        );
        assert_eq!(
            config.base_url.as_str(),
            "https://0xda-market.nilx.one/issuer/"
        );
        assert!(!format!("{config:?}").contains(&config.token));
        assert_eq!(
            MarketIssuerConfig::new("short", None).unwrap_err(),
            MarketIssuerConfigError::WeakHandoffSecret
        );
        for url in [
            "http://market.example/issuer",
            "https://user@market.example/",
            "https://market.example/?a=1",
        ] {
            assert_eq!(
                MarketIssuerConfig::new(&"h".repeat(32), Some(url)).unwrap_err(),
                MarketIssuerConfigError::InvalidUrl
            );
        }
    }

    #[derive(Default)]
    struct FakeMarket {
        orders: Mutex<HashMap<String, IssuerOrder>>,
        confirmations: Mutex<Vec<IssuerConfirmation>>,
        rejections: Mutex<Vec<String>>,
        expire_on_claim: Mutex<bool>,
        lose_next_fulfil: Mutex<bool>,
    }

    impl FakeMarket {
        fn status(&self, id: &str) -> String {
            self.orders.lock().unwrap()[id].attributes.status.clone()
        }
        fn set_status(&self, id: &str, status: &str) -> IssuerOrder {
            let mut orders = self.orders.lock().unwrap();
            let order = orders.get_mut(id).expect("order");
            order.attributes.status = status.to_owned();
            order.clone()
        }
    }

    impl MarketIssuerApi for &FakeMarket {
        async fn orders(
            &self,
            status: &str,
            _: Option<&str>,
        ) -> Result<IssuerOrderPage, MarketIssuerError> {
            let orders = self.orders.lock().unwrap();
            let mut page: Vec<_> = orders
                .values()
                .filter(|o| o.attributes.status == status)
                .cloned()
                .collect();
            page.sort_by(|a, b| a.id.cmp(&b.id));
            Ok(IssuerOrderPage {
                orders: page,
                next_cursor: None,
            })
        }
        async fn claim(&self, id: &str) -> Result<IssuerOrder, MarketIssuerError> {
            if *self.expire_on_claim.lock().unwrap() {
                return Ok(self.set_status(id, "rejected"));
            }
            Ok(self.set_status(id, "processing"))
        }
        async fn fulfil(
            &self,
            id: &str,
            confirmation: &IssuerConfirmation,
        ) -> Result<IssuerOrder, MarketIssuerError> {
            self.confirmations
                .lock()
                .unwrap()
                .push(confirmation.clone());
            if std::mem::take(&mut *self.lose_next_fulfil.lock().unwrap()) {
                return Err(MarketIssuerError::Unavailable);
            }
            Ok(self.set_status(id, "fulfilled"))
        }
        async fn reject(&self, id: &str, code: &str) -> Result<IssuerOrder, MarketIssuerError> {
            self.rejections.lock().unwrap().push(code.to_owned());
            Ok(self.set_status(id, "rejected"))
        }
    }

    struct Fixture {
        _directory: tempfile::TempDir,
        repository: IdentityRepository,
        pool: sqlx::SqlitePool,
        owner: PubDress,
        market: FakeMarket,
    }

    async fn fixture(balance: i64) -> Fixture {
        let directory = tempfile::tempdir().expect("directory");
        let url = format!(
            "sqlite://{}",
            directory.path().join("identity.db").display()
        );
        let repository = IdentityRepository::connect(&url).await.expect("repository");
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("fixture pool");
        let owner: PubDress = "0x0sky".parse().expect("owner");
        sqlx::query(
            "INSERT INTO identities(pub_dress, identity_kind, created_at) VALUES (?, 'human', 1)",
        )
        .bind(owner.as_str())
        .execute(&pool)
        .await
        .expect("identity");
        sqlx::query(
            "INSERT INTO native_sessions (token_hash, pub_dress, expires_at, created_at, active) \
             VALUES (?, ?, 2000000000, 1, 1)",
        )
        .bind(vec![9_u8; 32])
        .bind(owner.as_str())
        .execute(&pool)
        .await
        .expect("session");
        // Test-only funding; production credits come from verified sources only.
        sqlx::query("INSERT INTO bond_seeds(owner_pub_dress, balance) VALUES (?, ?)")
            .bind(owner.as_str())
            .bind(balance)
            .execute(&pool)
            .await
            .expect("balance");
        repository
            .create_market_visit(
                VISIT,
                &owner,
                &[9; 32],
                CREATED_MS - 1000,
                CREATED_MS + 3_600_000,
            )
            .await
            .expect("visit");
        let market = FakeMarket::default();
        market
            .orders
            .lock()
            .unwrap()
            .insert("order-0001".to_owned(), order("order-0001"));
        Fixture {
            _directory: directory,
            repository,
            pool,
            owner,
            market,
        }
    }

    fn order(id: &str) -> IssuerOrder {
        IssuerOrder {
            id: id.to_owned(),
            attributes: IssuerOrderAttributes {
                sku: "0x1_special_1m".to_owned(),
                denomination: "seeds".to_owned(),
                issuer: "0x1".to_owned(),
                amount_minor: 640,
                price_id: 7,
                recipient_subject: "0x0sky".to_owned(),
                link_provider: "nilx_visit".to_owned(),
                link_provider_user_id: VISIT.to_owned(),
                status: "pending".to_owned(),
                created_at: "2026-10-29T14:05:00.123000Z".to_owned(),
                term: Some(IssuerOrderTerm {
                    months: 1,
                    policy: TERM_POLICY.to_owned(),
                }),
            },
        }
    }

    fn worker(fixture: &Fixture, now_ms: u64) -> MarketIssuerWorker<&FakeMarket> {
        MarketIssuerWorker::new(
            fixture.repository.clone(),
            &fixture.market,
            Arc::new(move || now_ms),
        )
    }

    async fn balance(fixture: &Fixture) -> u64 {
        fixture
            .repository
            .read_seeds_balance(&fixture.owner)
            .await
            .expect("balance")
    }

    #[tokio::test]
    async fn a_pending_order_is_debited_granted_and_confirmed_once() {
        let fixture = fixture(1000).await;
        worker(&fixture, NOW_MS).tick().await.expect("tick");
        assert_eq!(fixture.market.status("order-0001"), "fulfilled");
        assert_eq!(balance(&fixture).await, 360);
        assert_eq!(
            fixture.market.confirmations.lock().unwrap().as_slice(),
            [IssuerConfirmation {
                issuer_reference: "identity:order-0001".to_owned(),
                amount_minor: 640,
                denomination: "seeds".to_owned(),
                recipient_subject: "0x0sky".to_owned(),
                term_months: 1,
            }]
        );
        let receipt = fixture
            .repository
            .find_special_seeds_receipt("order-0001")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(receipt.duration_ms, 31 * DAY);
        let clock = fixture
            .repository
            .read_subscription_clock(&fixture.owner, NOW_MS)
            .await
            .expect("clock");
        assert!(serde_json::to_string(&clock).unwrap().contains("special"));
        worker(&fixture, NOW_MS).tick().await.expect("idle tick");
        assert_eq!(balance(&fixture).await, 360);
    }

    #[tokio::test]
    async fn a_lost_confirmation_is_repeated_without_a_second_debit_even_after_the_visit() {
        let fixture = fixture(1000).await;
        *fixture.market.lose_next_fulfil.lock().unwrap() = true;
        worker(&fixture, NOW_MS).tick().await.expect("tick");
        assert_eq!(fixture.market.status("order-0001"), "processing");
        assert_eq!(balance(&fixture).await, 360);

        // Recovery after the visit's spend window still only confirms.
        worker(&fixture, NOW_MS + 7 * DAY)
            .tick()
            .await
            .expect("recovery");
        assert_eq!(fixture.market.status("order-0001"), "fulfilled");
        assert_eq!(balance(&fixture).await, 360);
        let confirmations = fixture.market.confirmations.lock().unwrap();
        assert_eq!(confirmations.len(), 2);
        assert_eq!(confirmations[0], confirmations[1]);
        assert!(fixture.market.rejections.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn refusals_reject_without_debiting() {
        type Setup = fn(&mut IssuerOrder);
        let cases: [(&str, i64, u64, Setup); 6] = [
            (rejection::INSUFFICIENT_SEEDS, 639, NOW_MS, |_| {}),
            (
                rejection::VISIT_EXPIRED,
                1000,
                CREATED_MS + 3_600_000,
                |_| {},
            ),
            (rejection::VISIT_UNKNOWN, 1000, NOW_MS, |o| {
                o.attributes.link_provider_user_id = "w".repeat(43)
            }),
            (rejection::RECIPIENT_MISMATCH, 1000, NOW_MS, |o| {
                o.attributes.recipient_subject = "0xother".to_owned()
            }),
            (rejection::INVALID_TERMS, 1000, NOW_MS, |o| {
                o.attributes.term.as_mut().unwrap().months = 3
            }),
            (rejection::INVALID_TERMS, 1000, NOW_MS, |o| {
                o.attributes.sku = "0x1_exclusive_1m".to_owned()
            }),
        ];
        for (code, funds, now_ms, setup) in cases {
            let fixture = fixture(funds).await;
            setup(
                fixture
                    .market
                    .orders
                    .lock()
                    .unwrap()
                    .get_mut("order-0001")
                    .unwrap(),
            );
            worker(&fixture, now_ms).tick().await.expect("tick");
            assert_eq!(
                fixture.market.rejections.lock().unwrap().as_slice(),
                [code.to_owned()],
                "{code}"
            );
            assert_eq!(
                balance(&fixture).await,
                u64::try_from(funds).unwrap(),
                "{code}"
            );
            assert!(
                fixture
                    .repository
                    .find_special_seeds_receipt("order-0001")
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[tokio::test]
    async fn a_changed_active_session_cannot_spend() {
        let fixture = fixture(1000).await;
        sqlx::query("UPDATE native_sessions SET active = 0")
            .execute(&fixture.pool)
            .await
            .expect("takeover");
        worker(&fixture, NOW_MS).tick().await.expect("tick");
        assert_eq!(
            fixture.market.rejections.lock().unwrap().as_slice(),
            [rejection::SESSION_CHANGED.to_owned()]
        );
        assert_eq!(balance(&fixture).await, 1000);
    }

    #[tokio::test]
    async fn an_expired_authorization_is_never_settled() {
        let fixture = fixture(1000).await;
        *fixture.market.expire_on_claim.lock().unwrap() = true;
        let outcome = worker(&fixture, NOW_MS)
            .handle(&order("order-0001"))
            .await
            .expect("handle");
        assert_eq!(outcome, Settlement::Skipped);
        assert_eq!(balance(&fixture).await, 1000);
        assert!(fixture.market.confirmations.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_receipt_that_disagrees_with_the_order_is_held_for_an_operator() {
        let fixture = fixture(1000).await;
        worker(&fixture, NOW_MS).tick().await.expect("tick");
        let mut changed = fixture.market.set_status("order-0001", "processing");
        changed.attributes.price_id = 8;
        let outcome = worker(&fixture, NOW_MS)
            .handle(&changed)
            .await
            .expect("handle");
        assert_eq!(outcome, Settlement::Held);
        assert_eq!(balance(&fixture).await, 360);
        assert!(fixture.market.rejections.lock().unwrap().is_empty());
    }
}
