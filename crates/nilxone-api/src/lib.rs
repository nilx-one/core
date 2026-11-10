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
//! Only the routes of the purchase path so far: [`finds`]. See
//! `docs/core-api-boundary.md` for the order of the rest.

pub mod error;
pub mod finds;
