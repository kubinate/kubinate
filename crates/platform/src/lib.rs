//! # kubinate-platform
//!
//! Shared primitives used by every Kubinate bounded context.
//!
//! This crate is deliberately thin. It holds things that every other
//! domain crate needs — error types, telemetry bootstrapping, database
//! pool, tenant-scoped transactions, configuration loading, and a few
//! newtypes for IDs — and nothing else.
//!
//! ## Module layout
//!
//! - [`config`]       — Layered configuration (env + file) loader.
//! - [`error`]        — Canonical platform error type, convertible into
//!                      Problem Details responses at the API edge.
//! - [`ids`]          — Typed ID newtypes (UUID v7 generators).
//! - [`telemetry`]    — `tracing` + OpenTelemetry bootstrap.
//! - [`db`]           — Postgres pool, migrations runner.
//! - [`tenant`]       — [`TenantScopedTransaction`] helper implementing
//!                      the RLS session variable dance described in
//!                      ADR-0006.
//! - [`clock`]        — Injectable clock for deterministic tests.
//! - [`secrets`]      — Envelope encryption primitives (pgcrypto in
//!                      Phases 0–2, Vault in Phase 3 — see ADR-0007).

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod audit;
pub mod clock;
pub mod config;
pub mod db;
pub mod error;
pub mod ids;
pub mod metrics;
pub mod secrets;
pub mod telemetry;
pub mod tenant;
