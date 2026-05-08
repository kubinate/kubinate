//! # kubinate-billing
//!
//! Billing bounded context (see brief §4).
//!
//! Entities: `Subscription`, `Invoice`, `UsageRecord`, `PaymentMethod`.
//! Stripe is the initial payment processor; the interface is
//! abstracted so a second processor can slot in later.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod model;
pub mod repository;
pub mod service;
pub mod stripe;
pub mod subscription;
pub mod usage;
