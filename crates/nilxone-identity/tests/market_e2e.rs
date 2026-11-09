// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! The Special-for-seeds purchase end to end (nilx-one/web#567): a real
//! 0xda-market (Ruby, PostgreSQL) on loopback, this crate's Identity store and
//! issuer worker. Market's checkout is driven exactly as a browser and the
//! Identity service drive it: a server handoff, the browser exchange, the
//! offer, the order; then the worker claims, debits, grants and confirms.
//!
//! It runs only where `.github/workflows/market-e2e.yml` started Market and
//! set `MARKET_E2E_URL` (and the shared `NILX_BROWSER_HANDOFF_TOKEN`); a plain
//! `cargo test` skips it. That Market's database is disposable: the workflow
//! activates the staged Special SKU there and nowhere else.

#![cfg(feature = "market-e2e")]

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use nilxone_identity::market_issuer::{
    HttpMarketIssuer, IssuerConfirmation, IssuerOrder, IssuerOrderPage, MarketIssuerApi,
    MarketIssuerConfig, MarketIssuerError, MarketIssuerWorker, system_now_ms,
};
use nilxone_identity::{IdentityRepository, ProviderIdentity, PubDress};
use serde_json::{Value, json};

const BROWSER: &str = "https://nilx.one";
const MARKET: &str = "https://0xda-market.nilx.one";

struct Setup {
    market_url: String,
    secret: String,
    http: reqwest::Client,
    repository: IdentityRepository,
    pool: sqlx::SqlitePool,
    _dir: tempfile::TempDir,
}

/// One Bond with a funded Identity wallet, an active session and a Market
/// visit bound to that session.
struct Buyer {
    owner: PubDress,
    session: Vec<u8>,
    cookie: String,
}

fn configured() -> Option<(String, String)> {
    let url = std::env::var("MARKET_E2E_URL").ok()?;
    let secret = std::env::var("NILX_BROWSER_HANDOFF_TOKEN").ok()?;
    Some((url.trim_end_matches('/').to_owned(), secret))
}

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0_u8; bytes];
    getrandom::fill(&mut buffer).expect("randomness");
    buffer.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn visit_id() -> String {
    // 43 base64url characters, as Identity mints them.
    random_hex(22)[..43].to_owned()
}

async fn setup() -> Option<Setup> {
    let (market_url, secret) = configured()?;
    let dir = tempfile::tempdir().expect("tempdir");
    let url = format!(
        "sqlite://{}?mode=rwc",
        dir.path().join("identity.db").display()
    );
    let repository = IdentityRepository::connect(&url)
        .await
        .expect("identity store");
    let pool = sqlx::SqlitePool::connect(&url).await.expect("fixture pool");
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("http client");
    Some(Setup {
        market_url,
        secret,
        http,
        repository,
        pool,
        _dir: dir,
    })
}

impl Setup {
    fn issuer(&self) -> HttpMarketIssuer {
        let config = MarketIssuerConfig::loopback_for_e2e(
            &self.secret,
            &format!("{}/issuer", self.market_url),
        )
        .expect("loopback issuer config");
        HttpMarketIssuer::new(config).expect("issuer client")
    }

    fn worker<M: MarketIssuerApi>(&self, market: M) -> MarketIssuerWorker<M> {
        MarketIssuerWorker::new(self.repository.clone(), market, Arc::new(system_now_ms))
    }

    /// A Bond holding exactly `seeds`, signed in, visiting Market.
    async fn buyer(&self, name: &str, telegram: i64, seeds: i64) -> Buyer {
        let owner: PubDress = name.parse().expect("pub_dress");
        let now_s = system_now_ms() / 1000;
        self.repository
            .register(&owner, &ProviderIdentity::telegram(telegram), now_s)
            .await
            .expect("registration");
        let session = random_hex(32).into_bytes()[..32].to_vec();
        sqlx::query(
            "INSERT INTO native_sessions (token_hash, pub_dress, created_at, expires_at, active)
             VALUES (?, ?, ?, ?, 1)",
        )
        .bind(&session)
        .bind(owner.as_str())
        .bind(i64::try_from(now_s).unwrap())
        .bind(i64::try_from(now_s + 86_400).unwrap())
        .execute(&self.pool)
        .await
        .expect("session");
        // A server-funded wallet of exactly `seeds`; the starter credit is
        // recorded as issued so the fixture's balance is the whole balance.
        sqlx::query(
            "INSERT INTO bond_seed_credits
             (owner_pub_dress, source_kind, source_id, amount_minor, committed_at_ms)
             VALUES (?, 'starter', 'once', 10000, 0)",
        )
        .bind(owner.as_str())
        .execute(&self.pool)
        .await
        .expect("starter receipt");
        sqlx::query("INSERT INTO bond_seeds (owner_pub_dress, balance) VALUES (?, ?)")
            .bind(owner.as_str())
            .bind(seeds)
            .execute(&self.pool)
            .await
            .expect("wallet");

        let visit = visit_id();
        let now_ms = system_now_ms();
        self.repository
            .create_market_visit(&visit, &owner, &session, now_ms, now_ms + 3_600_000)
            .await
            .expect("visit binding");
        let handoff: Value = self
            .http
            .post(format!("{}/nilx-browser/handoffs", self.market_url))
            .bearer_auth(&self.secret)
            .json(&json!({
                "subject": random_hex(32),
                "point": "zoloti-vorota",
                "seeds": seeds.to_string(),
                "seeds_authority": "identity_snapshot",
                "avaia": { "longitude": 30.5131, "latitude": 50.4489 },
                "visit_id": visit,
                "recipient": owner.as_str(),
                "return_path": "/",
            }))
            .send()
            .await
            .expect("handoff")
            .json()
            .await
            .expect("handoff body");
        let code = handoff["code"].as_str().expect("handoff code");
        let exchanged = self
            .http
            .post(format!("{}/nilx-browser/exchange", self.market_url))
            .header("origin", BROWSER)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!("code={code}"))
            .send()
            .await
            .expect("exchange");
        assert_eq!(exchanged.status().as_u16(), 303, "browser exchange");
        // The cookie is `Secure`; loopback HTTP carries it by hand.
        let cookie = exchanged
            .headers()
            .get("set-cookie")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .expect("market session cookie")
            .to_owned();
        Buyer {
            owner,
            session,
            cookie,
        }
    }

    async fn market(
        &self,
        buyer: &Buyer,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let url = format!("{}/nilx-browser{path}", self.market_url);
        let request = match method {
            "GET" => self.http.get(url),
            _ => self
                .http
                .post(url)
                .header("origin", MARKET)
                .header("x-market-csrf", "1"),
        }
        .header("cookie", &buyer.cookie);
        let request = match body {
            Some(body) => request.json(&body),
            None => request,
        };
        let response = request.send().await.expect("market request");
        let status = response.status().as_u16();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    /// The 1-month Special offer this visit sees, priced in seeds.
    async fn special_offer(&self, buyer: &Buyer) -> Value {
        let (status, body) = self.market(buyer, "GET", "/offers", None).await;
        assert_eq!(status, 200, "offers: {body}");
        body["offers"]
            .as_array()
            .expect("offers")
            .iter()
            .find(|offer| offer["sku"] == "0x1_special_1m")
            .cloned()
            .expect("the staged 1-month Special is on offer in this disposable Market")
    }

    async fn order(&self, buyer: &Buyer, offer: &Value, key: &str) -> (u16, Value) {
        self.market(
            buyer,
            "POST",
            "/orders",
            Some(json!({
                "sku": offer["sku"], "price_id": offer["price_id"],
                "amount_minor": offer["amount_minor"], "idempotency_key": key,
            })),
        )
        .await
    }

    async fn balance(&self, buyer: &Buyer) -> u64 {
        self.repository
            .read_seeds_balance(&buyer.owner)
            .await
            .expect("wallet")
    }

    async fn debits(&self, order_id: &str) -> i64 {
        sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM bond_seed_subscription_debits WHERE order_id = ?1)
                  + (SELECT COUNT(*) FROM paid_special_items WHERE order_id = ?1)",
        )
        .bind(order_id)
        .fetch_one(&self.pool)
        .await
        .expect("receipts")
    }

    async fn status(&self, order_id: &str) -> String {
        self.issuer()
            .read_order(order_id)
            .await
            .expect("issuer order")
            .attributes
            .status
    }
}

fn order_id(body: &Value) -> String {
    body["order"]["id"]
        .as_str()
        .or_else(|| body["id"].as_str())
        .or_else(|| body["data"]["id"].as_str())
        .unwrap_or_else(|| panic!("an order id in {body}"))
        .to_owned()
}

/// A Market issuer whose first confirmation is lost on the way: Identity has
/// committed, Market never heard.
struct LosesFirstConfirmation {
    inner: HttpMarketIssuer,
    lost: AtomicBool,
}

impl MarketIssuerApi for LosesFirstConfirmation {
    async fn orders(
        &self,
        status: &str,
        cursor: Option<&str>,
    ) -> Result<IssuerOrderPage, MarketIssuerError> {
        self.inner.orders(status, cursor).await
    }
    async fn claim(&self, id: &str) -> Result<IssuerOrder, MarketIssuerError> {
        self.inner.claim(id).await
    }
    async fn fulfil(
        &self,
        id: &str,
        confirmation: &IssuerConfirmation,
    ) -> Result<IssuerOrder, MarketIssuerError> {
        if !self.lost.swap(true, Ordering::SeqCst) {
            return Err(MarketIssuerError::Unavailable);
        }
        self.inner.fulfil(id, confirmation).await
    }
    async fn reject(&self, id: &str, code: &str) -> Result<IssuerOrder, MarketIssuerError> {
        self.inner.reject(id, code).await
    }
}

#[tokio::test]
async fn special_for_seeds_end_to_end() {
    let Some(e2e) = setup().await else {
        eprintln!("MARKET_E2E_URL is not set: the Market end-to-end check is skipped");
        return;
    };

    // 1. A Bond with exactly 640 seeds buys 1-month Special.
    let sky = e2e.buyer("0x1sky", 9101, 640).await;
    let offer = e2e.special_offer(&sky).await;
    assert_eq!(offer["amount_minor"], 640);
    assert_eq!(offer["denomination"], "seeds");
    let (status, placed) = e2e.order(&sky, &offer, "e2e-sky-0001").await;
    assert_eq!(status, 201, "order: {placed}");
    let sky_order = order_id(&placed);
    // The same order again is the same order.
    let (status, replay) = e2e.order(&sky, &offer, "e2e-sky-0001").await;
    assert_eq!(status, 200, "replay: {replay}");
    assert_eq!(order_id(&replay), sky_order);

    let worker = e2e.worker(e2e.issuer());
    worker.tick().await.expect("tick");
    assert_eq!(e2e.status(&sky_order).await, "fulfilled");
    assert_eq!(e2e.balance(&sky).await, 0, "exactly 640 debited");
    assert_eq!(e2e.debits(&sky_order).await, 1, "one receipt");
    // Another pass, and a duplicate confirmation, change nothing.
    worker.tick().await.expect("tick");
    assert_eq!(e2e.balance(&sky).await, 0);
    assert_eq!(e2e.debits(&sky_order).await, 1);

    // 2. 639 seeds: rejected, nothing debited or granted.
    let poor = e2e.buyer("0x1poor", 9102, 639).await;
    let (status, placed) = e2e
        .order(&poor, &e2e.special_offer(&poor).await, "e2e-poor-0001")
        .await;
    assert_eq!(status, 201, "order: {placed}");
    let poor_order = order_id(&placed);
    worker.tick().await.expect("tick");
    assert_eq!(e2e.status(&poor_order).await, "rejected");
    assert_eq!(e2e.balance(&poor).await, 639);
    assert_eq!(e2e.debits(&poor_order).await, 0);

    // 3. Exclusive is not sold for seeds, and a changed price is refused.
    let (status, _) = e2e
        .market(
            &sky,
            "POST",
            "/orders",
            Some(json!({
                "sku": "0x1_exclusive_1m", "price_id": offer["price_id"],
                "amount_minor": 640, "idempotency_key": "e2e-sky-exclusive",
            })),
        )
        .await;
    assert!((400..500).contains(&status), "exclusive in seeds: {status}");
    let (status, _) = e2e
        .market(
            &sky,
            "POST",
            "/orders",
            Some(json!({
                "sku": "0x1_special_1m", "price_id": offer["price_id"],
                "amount_minor": 1, "idempotency_key": "e2e-sky-cheap",
            })),
        )
        .await;
    assert!((400..500).contains(&status), "changed price: {status}");

    // 4. A superseded client session cannot spend: rejected, no debit.
    let gone = e2e.buyer("0x1gone", 9103, 640).await;
    let (status, placed) = e2e
        .order(&gone, &e2e.special_offer(&gone).await, "e2e-gone-0001")
        .await;
    assert_eq!(status, 201, "order: {placed}");
    let gone_order = order_id(&placed);
    sqlx::query("UPDATE native_sessions SET active = 0 WHERE token_hash = ?")
        .bind(&gone.session)
        .execute(&e2e.pool)
        .await
        .expect("supersede session");
    worker.tick().await.expect("tick");
    assert_eq!(e2e.status(&gone_order).await, "rejected");
    assert_eq!(e2e.balance(&gone).await, 640);
    assert_eq!(e2e.debits(&gone_order).await, 0);

    // 5. Identity commits, the confirmation is lost: the order stays
    //    processing, and the next pass confirms it, debiting exactly once.
    let lost = e2e.buyer("0x1lost", 9104, 640).await;
    let (status, placed) = e2e
        .order(&lost, &e2e.special_offer(&lost).await, "e2e-lost-0001")
        .await;
    assert_eq!(status, 201, "order: {placed}");
    let lost_order = order_id(&placed);
    let flaky = e2e.worker(LosesFirstConfirmation {
        inner: e2e.issuer(),
        lost: AtomicBool::new(false),
    });
    flaky.tick().await.expect("tick");
    assert_eq!(e2e.status(&lost_order).await, "processing");
    assert_eq!(
        e2e.balance(&lost).await,
        0,
        "committed before the confirmation"
    );
    assert_eq!(e2e.debits(&lost_order).await, 1);
    // A restarted worker recovers it.
    e2e.worker(e2e.issuer()).tick().await.expect("tick");
    assert_eq!(e2e.status(&lost_order).await, "fulfilled");
    assert_eq!(e2e.balance(&lost).await, 0);
    assert_eq!(e2e.debits(&lost_order).await, 1);

    // 6. A cancelled order is never debited.
    let cancel = e2e.buyer("0x1cncl", 9105, 640).await;
    let (status, placed) = e2e
        .order(&cancel, &e2e.special_offer(&cancel).await, "e2e-cncl-0001")
        .await;
    assert_eq!(status, 201, "order: {placed}");
    let cancel_order = order_id(&placed);
    let (status, _) = e2e
        .market(
            &cancel,
            "POST",
            &format!("/orders/{cancel_order}/cancel"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 200, "cancel");
    worker.tick().await.expect("tick");
    assert_eq!(e2e.balance(&cancel).await, 640);
    assert_eq!(e2e.debits(&cancel_order).await, 0);
}
