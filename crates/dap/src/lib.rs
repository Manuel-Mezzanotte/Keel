//! Debugging, with the Debug Adapter Protocol VS Code uses: the client
//! that talks to an adapter, the messages it reads, where the adapters
//! are, and a small one for the tests.

pub mod adapters;
pub mod client;
pub mod fake;
pub mod protocol;

pub use client::{Client, ClientEvent, Handler};
