//! # kubinate-workflows
//!
//! Temporal workflows and activities (see ADR-0003).
//!
//! Workflows are deterministic; all side effects must go through
//! activities. See ADR-0003 for the Rust-vs-Go SDK decision and the
//! escape hatch to Go workers if the Rust SDK proves inadequate.
//!
//! Top-level workflows shipped in Phase 1:
//! - `ProvisionClusterWorkflow`
//! - `DestroyClusterWorkflow`
//!
//! Added in Phase 2:
//! - `UpgradeClusterWorkflow`
//! - `ScaleNodePoolWorkflow`
//! - `InstallAddonWorkflow`

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod activities;
pub mod events;
pub mod runner;
pub mod workflows;
