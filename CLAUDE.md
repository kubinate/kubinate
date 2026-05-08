# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Kubinate provisions and operates managed k3s clusters on user-owned Hetzner infrastructure. The control plane is a modular Rust monolith with a SvelteKit frontend and a per-cluster agent binary. Status: pre-alpha, Phase 0.

## Commands

### Backend (Rust workspace, toolchain pinned in `rust-toolchain.toml`)

```bash
# Local infra (Postgres 16, Redis 7, Temporal, optional Prometheus/Grafana under `--profile obs`)
docker compose up -d

# Migrations (requires sqlx-cli)
export DATABASE_URL=postgres://kubinate:kubinate@localhost:5432/kubinate
sqlx migrate run

# Run the API (binary crate `kubinate-api`, exposes /healthz, /readyz, /version)
cargo run -p kubinate-api

# Lint / format / test
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test -p <crate>               # single crate
cargo test -p <crate> <test_name>   # single test
```

Env vars use the `KUBINATE_` prefix with `__` as the nesting separator (e.g. `KUBINATE_DATABASE_URL`). Config layering: compiled defaults → `config/default.toml` → `config/{KUBINATE_ENV}.toml` → env vars. See `crates/platform/src/config.rs`.

### Frontend (`frontend/`, SvelteKit 2 + Svelte 5 + Vite, Node 22+)

```bash
cd frontend
npm install
npm run dev        # proxies /api/* to the backend
npm run check      # svelte-kit sync + svelte-check
npm run lint       # eslint + prettier --check
npm run format
npm run test       # vitest
npm run build
```

Frontend adapter is `@sveltejs/adapter-cloudflare` (deploys to Cloudflare Pages).

### Temporal UI: http://localhost:8233 · Postgres: `localhost:5432` (`kubinate`/`kubinate`).

## Architecture

### Bounded-context crate layout (`crates/`)

The workspace is a modular monolith organized by bounded context. **A domain crate never depends on `api/`.** Cross-domain interaction goes through traits defined in `platform` or through domain events.

- `platform/` — shared primitives: `PlatformError`, `TenantScopedTransaction`, config, telemetry, DB pool, IDs (UUID v7), clock, secrets.
- `identity/` — users, orgs, memberships, sessions, API keys, `Actor`, authz (owner of permission checks per ADR-0010).
- `cluster/` — cluster lifecycle domain model, repository, service.
- `addons/`, `billing/`, `observability/` — additional bounded contexts (observability includes the multi-tenant metric/log query proxy).
- `integrations/` — outbound clients: `hetzner`, `ssh`, `temporal`. External side effects live here; domain crates depend on traits, not concrete clients.
- `workflows/` — Temporal workflows and activities.
- `api/` — binary crate `kubinate-api`, the Axum HTTP API + BFF. Wires domain services and translates `PlatformError` to RFC 7807 Problem Details at the edge.
- `agent/` (workspace root, not under `crates/`) — in-cluster static binary `kubinate-agent`. Opens an **agent-initiated mTLS gRPC reverse tunnel** to the control plane; user clusters do not accept inbound connections from the control plane.
- `e2e/` — `kubinate-e2e` binary that drives a real `provision → kubectl → destroy → assert zero residue` cycle against a Hetzner test project. Wrapped by `.github/workflows/nightly-e2e.yml` (cron `0 4 * * *`) with a 60-minute outer timeout and a force-destroy backstop.

### Load-bearing invariants

These aren't optional conventions — they're the things that must not drift:

1. **Tenant isolation (ADR-0006).** Every tenant-scoped table has `organization_id UUID NOT NULL`, `ENABLE ROW LEVEL SECURITY`, and `FORCE ROW LEVEL SECURITY` with a policy keyed on `current_setting('app.current_tenant_id')`. All tenant data access goes through `TenantScopedTransaction::begin(pool, organization_id)` in `crates/platform/src/tenant.rs`, which issues `SET LOCAL app.current_tenant_id = '<uuid>'`. Bypassing RLS requires an explicit privileged role and an audit-logged exception path. **Missing RLS on a new tenant-scoped table is a P0 review blocker.**

2. **Error flow.** Domain crates return `PlatformError` (or a domain error that converts into it via `#[from]`). The API edge converts to Problem Details. `sqlx::Error` converts automatically; prefer that over `anyhow` inside domain code.

3. **Temporal (ADR-0003).** Workflows are deterministic — no direct I/O, no `SystemTime`, no RNG outside the SDK. All side effects go through activities in `crates/workflows/src/activities.rs`. Integration clients (Hetzner, SSH) live in `crates/integrations` and are invoked only from activities.

4. **Secrets (ADR-0007).** Raw plaintext secrets do not cross module boundaries. Use `SecretRef` + the `SecretStore` trait from `crates/platform/src/secrets.rs`. Phases 0–2 use pgcrypto envelope encryption; Phase 3+ uses Vault.

5. **Migrations.** `migrations/` is append-only. **Never edit a shipped migration.** Primary keys are UUID v7; time fields are `TIMESTAMPTZ`.

6. **`unsafe` is forbidden** at the crate level (`#![forbid(unsafe_code)]` on every crate).

### Request flow

Browser → Cloudflare (Pages + Tunnel/WAF) → `kubinate-api` (Axum) → domain services → `TenantScopedTransaction` → Postgres (RLS). Long-running operations (provision, destroy, upgrade, scale, addon install) are started from `api` and executed by the in-process `LocalRunner` in `crates/workflows/src/runner.rs` — Temporal SDK adoption was deferred to Phase 4+ per ADR-0011. Per-cluster live updates flow through a `tokio::sync::broadcast` hub (`crates/workflows/src/events.rs`) consumed by the `GET /v1/clusters/:id/events` SSE endpoint; the dashboard's 2s poll is a fallback only. See `docs/architecture/containers.md`.

## Decision records & docs

ADRs in `docs/adr/` are authoritative and **immutable once accepted** — to change a decision, write a new ADR that `Supersedes` the old one. Before making a non-trivial architectural change, check if an ADR already governs the area; if the change is itself architecturally significant, open an ADR PR in `Proposed` status *before* the implementation PR. Key ADRs to read before touching the respective area:

- 0001 backend language/framework · 0002 frontend · 0003 cluster provisioning · 0004 deployment topology · 0005 database · 0006 multi-tenancy/RLS · 0007 secret management · 0008 API style (RFC 7807) · 0009 authn · 0010 authz · 0011 defer Temporal SDK adoption (single-VPS in-process runner is the supported path; revisits at Phase 3) · 0012 defer HA control plane (single-CP invariant in `validate_allowed`; revisits on data-residency / Hetzner managed etcd / Phase 3 / sustained sev-2).

Other useful docs: `docs/architecture/containers.md` (C4 L2), `docs/security/threat-model.md`, `docs/runbooks/`, `docs/roadmap.md`. Spike-doc starters with `<<measurement needed>>` placeholders live in `docs/decisions/` — these are decision-doc skeletons, not the spikes themselves; do not ratify the recommendation columns until the empirical sections are filled in by a real spike runner.

## Conventions (from CONTRIBUTING.md)

- Conventional Commits (enforced in CI via commitlint); types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `build`, `ci`, `perf`. Breaking changes: `!` + `BREAKING CHANGE:` footer.
- Branch naming: `type/short-description`.
- PR target: reviewable in <20 minutes, ~400 changed lines max (excluding generated code/locks/migrations). Security-sensitive changes (authn, authz, secrets, tenancy) need **two** approvals including one `security`-role maintainer.
- Frontend rule: API calls go through the generated OpenAPI client, not hand-rolled `fetch`.
