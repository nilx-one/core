// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! A Bond's visit to 0xda-market and its return.
//!
//! The visit route hands the browser a one-time code to carry into Market. The
//! return route answers whether an order the tab came back with is confirmed;
//! the navigation itself is never payment proof. Neither request carries an
//! amount: the service snapshots its own wallet.

use serde::{Deserialize, Serialize};

use crate::value::InvalidValue;

/// `POST`: open a visit and get the code to carry into Market.
pub const VISIT_PATH: &str = "/api/v1/identity/market-visit";
/// `GET`: ask whether a returned order is confirmed. Query: [`ReturnQuery`].
pub const RETURN_PATH: &str = "/api/v1/identity/market-return";

/// The most characters a Market point's id may have.
pub const POINT_MAX_CHARS: usize = 64;
/// A visit reference is 32 random bytes, unpadded base64url.
pub const VISIT_ID_LEN: usize = 43;
/// The longest Market order id.
pub const ORDER_ID_MAX_LEN: usize = 128;
/// A handoff code is 43 characters of unpadded base64url.
pub const HANDOFF_CODE_LEN: usize = 43;

const DEFAULT_RETURN_PATH: &str = "/";

/// Whether `value` has the shape of a visit reference.
#[must_use]
pub fn is_market_visit_id(value: &str) -> bool {
    value.len() == VISIT_ID_LEN
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

/// Whether `value` has the shape of a Market order id.
#[must_use]
pub fn is_market_order_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= ORDER_ID_MAX_LEN
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b':' | b'-'))
}

/// Whether `path` is a 0x1 host page a visit may return to: `/` or one
/// lowercase segment such as `/telegram/`. Never a URL.
#[must_use]
pub fn is_market_return_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix('/') else {
        return false;
    };
    rest.is_empty()
        || rest.strip_suffix('/').is_some_and(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        })
}

refusals! {
    /// Why a visit was not opened.
    VisitRefusal {
        InvalidMarketContext => ("invalid_market_context", 422),
        ItemNotAvailable => ("item_not_available", 409),
        MarketVisitLimit => ("market_visit_limit", 429),
    }
}

refusals! {
    /// Why a return was not confirmed.
    ReturnRefusal {
        PurchaseNotConfirmed => ("purchase_not_confirmed", 404),
        MarketReturnLimit => ("market_return_limit", 429),
    }
}

/// Where the Avaia stands, in degrees.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Position {
    /// Degrees east, within ±180.
    pub longitude: f64,
    /// Degrees north, within ±90.
    pub latitude: f64,
}

/// `POST` [`VISIT_PATH`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisitRequest {
    /// The Market point being visited.
    pub point: String,
    /// A paid item this visit collects, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pickup_order_id: Option<String>,
    /// A balance older clients sent. The service ignores it; do not send it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seeds: Option<String>,
    /// Where the Avaia stands.
    pub avaia: Position,
    /// The host page to come back to, such as `/telegram/`; `/` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_path: Option<String>,
}

impl VisitRequest {
    /// A visit to `point` from `avaia`, returning to `/`.
    #[must_use]
    pub fn new(point: impl Into<String>, avaia: Position) -> Self {
        Self {
            point: point.into(),
            pickup_order_id: None,
            seeds: None,
            avaia,
            return_path: None,
        }
    }

    /// Checks the point, the place and the return page, and gives the page the
    /// visit returns to.
    ///
    /// # Errors
    ///
    /// [`VisitRefusal::InvalidMarketContext`] when the point is empty, longer
    /// than [`POINT_MAX_CHARS`] or not letters, digits, `-` and `_`; when the
    /// place is not on Earth; or when the return page is not
    /// [`is_market_return_path`].
    pub fn validate(&self) -> Result<&str, VisitRefusal> {
        let point_ok = !self.point.is_empty()
            && self.point.chars().count() <= POINT_MAX_CHARS
            && self
                .point
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_');
        let place_ok = (-180.0..=180.0).contains(&self.avaia.longitude)
            && (-90.0..=90.0).contains(&self.avaia.latitude);
        let return_path = self.return_path.as_deref().unwrap_or(DEFAULT_RETURN_PATH);
        if point_ok && place_ok && is_market_return_path(return_path) {
            Ok(return_path)
        } else {
            Err(VisitRefusal::InvalidMarketContext)
        }
    }
}

/// The one-time code Market exchanges for a browser session.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct HandoffCode(String);

impl HandoffCode {
    /// The code as written on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for HandoffCode {
    type Error = InvalidValue;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let shaped = value.len() == HANDOFF_CODE_LEN
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-');
        if shaped {
            Ok(Self(value))
        } else {
            Err(InvalidValue("not a handoff code"))
        }
    }
}

impl From<HandoffCode> for String {
    fn from(code: HandoffCode) -> Self {
        code.0
    }
}

/// The answer to [`VisitRequest`], with HTTP 201.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VisitAnswer {
    /// The code to post to `action`.
    pub code: HandoffCode,
    /// The visit a purchase is bound to; `null` when this sign-in can browse
    /// but not buy.
    pub visit_id: Option<String>,
    /// The Bond's public name.
    pub owner: String,
    /// Market's exchange address the browser posts the code to.
    pub action: String,
}

/// The query of `GET` [`RETURN_PATH`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnQuery {
    /// The visit the tab left on.
    pub visit_id: String,
    /// The order it came back with.
    pub order_id: String,
}

impl ReturnQuery {
    /// Checks both ids have their shape.
    ///
    /// # Errors
    ///
    /// [`ReturnRefusal::PurchaseNotConfirmed`]: a malformed id confirms nothing.
    pub fn validate(&self) -> Result<(), ReturnRefusal> {
        if is_market_visit_id(&self.visit_id) && is_market_order_id(&self.order_id) {
            Ok(())
        } else {
            Err(ReturnRefusal::PurchaseNotConfirmed)
        }
    }
}

/// The answer to [`ReturnQuery`]: the order is confirmed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReturnAnswer {
    /// Always `true`: an unconfirmed return answers
    /// [`ReturnRefusal::PurchaseNotConfirmed`].
    pub confirmed: bool,
    /// The visit asked about.
    pub visit_id: String,
    /// The order asked about.
    pub order_id: String,
    /// The Bond's public name.
    pub owner: String,
    /// Whether the order delivers an item to collect instead of a term.
    pub item_delivery: bool,
}

#[cfg(test)]
mod tests;
