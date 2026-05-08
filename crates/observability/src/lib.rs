//! # kubinate-observability
//!
//! Observability bounded context (see brief §4).
//!
//! This crate proxies metric and log queries from user clusters,
//! enforcing tenant scoping on every request. The choice of
//! multi-tenant TSDB (Mimir vs `VictoriaMetrics`) is pending the
//! Phase 3 benchmark spike (brief §12).

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod logs;
pub mod metrics;
pub mod proxy;
