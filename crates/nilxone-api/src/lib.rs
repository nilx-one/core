// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! The 0x1 wire contract.
//!
//! One definition of every route a client calls: its path, its request and
//! answer types, its error codes and the limits a client can check before it
//! sends anything. The server host and every client (Web, iOS, a command-line
//! tool) use these types; none of them writes the JSON by hand.
//!
//! The crate is portable. It has no runtime, no HTTP stack and no server
//! dependency, so it can sit in a Wasm or Swift client graph. The `fixtures`
//! directory holds the JSON bodies the tests pin; a client in another language
//! checks its parser against the same files.
//!
//! Requests are strict: a body with a field the contract does not name is
//! refused. Answers are read tolerantly: a client ignores a field it does not
//! know, so the service can add one without breaking it.
//!
//! Only the routes of the purchase path so far: [`finds`], [`wallet`] and
//! [`market`]. See `docs/core-api-boundary.md` for the order of the rest.

macro_rules! refusals {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => ($code:literal, $status:literal)),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum $name { $($variant),+ }

        impl $name {
            /// Every refusal of this route, in a stable order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The code in the error body.
            #[must_use]
            pub const fn code(self) -> &'static str {
                match self { $(Self::$variant => $code),+ }
            }

            /// The HTTP status that carries it.
            #[must_use]
            pub const fn http_status(self) -> u16 {
                match self { $(Self::$variant => $status),+ }
            }

            /// Reads a code from an error body.
            #[must_use]
            pub fn from_code(code: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|refusal| refusal.code() == code)
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.code())
            }
        }
    };
}

pub mod error;
pub mod finds;
pub mod market;
pub mod value;
pub mod wallet;
