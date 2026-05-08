# Kubinate

**Managed k3s on infrastructure you own.**

Kubinate provisions and operates production-ready k3s Kubernetes
clusters on your own Hetzner Cloud account. Your hardware, your bill,
your full ownership — we run the control plane, the add-on catalog,
and observability on top.

> **Status**: pre-alpha. Phase 0 (foundations) is in progress. See
> [docs/roadmap.md](docs/roadmap.md) for phase-by-phase plans.

---

## Why Kubinate

- **Hyperscaler-free economics.** Flat monthly fee per cluster or per
  node, well below EKS/GKE/AKS.
- **You own the nodes.** Hetzner bills you directly. We can't
  hold your infrastructure hostage because we never hold it at all.
- **Managed ergonomics.** One form, ten minutes, a working cluster and
  a kubeconfig. Baseline add-ons (ingress, cert-manager, Prometheus,
  Loki, ArgoCD, Velero) one click away.
- **Real multi-tenancy.** Row-Level Security in Postgres plus
  per-tenant mTLS agent channels — covered in
  [the threat model](docs/security/threat-model.md).

## Architecture at a glance

See [docs/architecture/containers.md](docs/architecture/containers.md)
for the full C4 diagram. The short version:

- **Frontend**: SvelteKit on Cloudflare Pages.
- **Backend**: Rust (Axum, Tokio) as a modular monolith.
- **Orchestration**: Temporal workflows for cluster lifecycle.
- **Storage**: Postgres 16 with Row-Level Security for tenancy.
- **Agent**: per-cluster Rust binary, mTLS reverse tunnel to the
  control plane.

Architectural decisions are written down in
[docs/adr/](docs/adr/). Start with ADR-0001.

## Repo layout

```
kubinate/
├── Cargo.toml             # Rust workspace
├── crates/
│   ├── api/               # HTTP API + BFF (Axum)
│   ├── identity/          # Users, orgs, sessions, authz
│   ├── cluster/           # Cluster lifecycle domain
│   ├── addons/            # Add-on catalog
│   ├── observability/     # Metric + log proxy
│   ├── billing/           # Subscriptions, invoices
│   ├── integrations/      # Hetzner, SSH, Temporal clients
│   ├── workflows/         # Temporal workflows + activities
│   ├── platform/          # Shared primitives (errors, db, tenant)
│   └── e2e/               # Nightly Hetzner provision/destroy harness
├── agent/                 # In-cluster agent binary
├── frontend/              # SvelteKit app
├── migrations/            # sqlx migrations
├── infra/
│   └── terraform/         # Control-plane VPS baseline (ADR-0004)
├── docs/
│   ├── adr/               # Architecture Decision Records
│   ├── architecture/      # C4 diagrams
│   ├── security/          # Threat model
│   ├── runbooks/          # Operational runbooks
│   └── backlog/           # Sprint backlog (seed)
├── docker-compose.yml     # Local dev stack
└── .github/workflows/     # CI, nightly E2E
```

## Local development

### Prerequisites

- Rust, version pinned by `rust-toolchain.toml` (installed automatically
  by rustup if you have it).
- Node.js 22+ (for the frontend).
- Docker + Docker Compose.
- `sqlx-cli`: `cargo install sqlx-cli --no-default-features --features native-tls,postgres`

### First-time setup (under 30 minutes is the target)

```bash
# 1. Bring up Postgres, Redis, Temporal.
docker compose up -d

# 2. Apply database migrations.
export DATABASE_URL=postgres://kubinate:kubinate@localhost:5432/kubinate
sqlx migrate run

# 3. Copy the example env.
cp .env.example .env

# 4. Run the API (produces structured JSON logs on stdout).
cargo run -p kubinate-api

# 5. In another shell, run the frontend (proxies /api/* to the backend).
cd frontend
npm install
npm run dev
```

Visit http://localhost:3000.

The Temporal UI is at http://localhost:8233. Postgres is at
`localhost:5432` with username/password `kubinate`/`kubinate`.

### Running the tests

```bash
# Unit + integration tests (needs docker compose up).
cargo test --workspace

# Frontend tests.
cd frontend && npm run check && npm run test
```

## Engineering process

- 2-week sprints. Async standups.
- Conventional Commits. Signed commits encouraged.
- Every change: PR review by at least one other engineer, green CI,
  DoD satisfied.
- 20% of each sprint reserved for tech debt and runbook authoring.

## Contributing

Internal team for now; see [CONTRIBUTING.md](CONTRIBUTING.md) and
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## Security

To report a vulnerability, see [SECURITY.md](SECURITY.md). **Do not**
file public GitHub issues for security reports.

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).
