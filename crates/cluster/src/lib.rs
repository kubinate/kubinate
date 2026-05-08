//! # kubinate-cluster
//!
//! Cluster Lifecycle bounded context (see brief §4).
//!
//! Entities: `Cluster`, `NodePool`, `Node`, `ProvisioningWorkflow`,
//! `ClusterCredential`, `ClusterEvent`.
//!
//! Note: the *source of truth* for in-flight provisioning workflow
//! state is Temporal. This crate maintains a shadow table for cheap
//! UI queries (see brief §5).

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod model;
pub mod repository;
pub mod service;
pub mod status;
