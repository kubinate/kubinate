//! # kubinate-integrations
//!
//! Adapters to external systems. This crate's role is to own the
//! ugly parts — HTTP clients, SSH orchestration, error translation —
//! so domain crates remain testable with mock implementations of the
//! traits defined here.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod github;
pub mod helm;
pub mod hetzner;
pub mod kubectl;
pub mod ssh;
pub mod stripe;
pub mod temporal;
