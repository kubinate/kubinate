# Encrypt, store, and surface the cluster kubeconfig

**Labels**: `area/cluster`, `area/platform`, `area/security`, `sprint-1`
**Size**: 3

## Context

Once the cluster is provisioned, the user needs a working kubeconfig.
It is very sensitive — a kubeconfig grants cluster-admin. Threat model
Flow 3 covers the handling; ADR-0007 covers the storage.

## Acceptance criteria

- **Given** a completed provisioning workflow, **when** the workflow
  finalizes, **then** the collected kubeconfig is stored via
  `SecretStore::put` (pgcrypto envelope).
- **Given** a logged-in user who owns the cluster, **when** they click
  "Download kubeconfig", **then** the API serves the plaintext
  kubeconfig with `Content-Disposition: attachment`, with an audit
  log entry for the retrieval.
- **Given** a second download after 10 minutes, **then** the same
  kubeconfig is served (no rotation in v1).

## DoD
- [ ] Unit test for redaction (see #01)
- [ ] Audit entry for every retrieval
- [ ] Download endpoint rate-limited (5/min per user)
