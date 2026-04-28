//! LSP messaging for `lean --server`: display formatting, protocol extensions, and client methods.
//!
//! - [`lean`] — [`DisplayLspParams`] trait and `impl`s for human-readable debug output.
//! - [`extensions`] — Lean-specific LSP extension types (`$/lean/plainGoal`, etc.).
//! - [`client`] — Named send methods (`hover`, `did_open`, etc.) added to [`client::Client`].

#![allow(dead_code)]

mod client;
mod extensions;
pub mod lean;
pub mod turnstile;

pub use lean::DisplayLspParams;
