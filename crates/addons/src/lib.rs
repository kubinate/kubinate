//! # kubinate-addons
//!
//! Add-on Catalog bounded context (see brief §4).
//!
//! Entities: `AddonDefinition`, `AddonInstallation`, `AddonVersion`,
//! `HelmValuesSchema`.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod catalog;
pub mod installation;
pub mod model;
pub mod repository;
pub mod service;
