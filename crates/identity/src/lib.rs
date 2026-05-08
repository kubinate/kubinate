//! # kubinate-identity
//!
//! Identity & Access bounded context (see brief §4).
//!
//! Entities owned by this crate: `User`, `Organization`, `Membership`,
//! `Role`, `Session`, `ApiKey`, `AuditLogEntry`, `OidcProvider`.
//!
//! This crate is the authoritative source of the `Actor` type that
//! the `authz` module uses for permission checks (ADR-0010).

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod actor;
pub mod authz;
pub mod model;
pub mod oidc;
pub mod recovery_codes;
pub mod repository;
pub mod service;
pub mod session;
pub mod webauthn;
