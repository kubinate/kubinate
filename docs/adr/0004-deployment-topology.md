# ADR-0004: Deployment topology

- **Status**: Accepted
- **Date**: 2026-04-24
- **Deciders**: Founding team
- **Tags**: infra, deployment, topology

## Context

Kubinate must host its own control plane somewhere. The options span
"as simple as possible" (a single VPS with Docker Compose) through
"fully managed k8s from day one" to "Cloudflare Workers edge-first".
Each has cost, complexity, and narrative implications. A critical
second-order factor: Kubinate sells managed k3s on Hetzner — it is
narratively valuable for us to run on Kubinate on Hetzner.

## Decision

We adopt a two-phase topology with a defined migration point.

**Short term (months 0–6), Phase 0 through Phase 2:**

- Frontend (SvelteKit) on **Cloudflare Pages** with edge-function SSR.
- Backend API, Temporal server + workers, Postgres, Redis, and local
  object storage run on a **single Hetzner dedicated VPS** (target:
  CPX41, ~€30/month, 8 vCPU / 16 GB RAM / 240 GB NVMe) with **Docker
  Compose** managed via an Ansible playbook.
- Postgres backups to Hetzner Object Storage every 15 minutes via WAL
  shipping; base backup nightly.
- **Cloudflare** in front of the backend as CDN, DDoS protection, and
  edge WAF. Origin exposed only over Cloudflare Tunnel; no public IP
  ingress to the VPS.

**Long term (month 6+), Phase 3:**

- Kubinate's backend, Temporal, and Postgres migrate onto a
  **Kubinate-provisioned k3s cluster** on Hetzner, multi-node with HA
  control plane. Postgres managed via the **CloudNativePG** operator
  with a three-replica cluster.
- Object storage remains Hetzner's S3-compatible service.
- Frontend remains on Cloudflare Pages.
- The migration is the "we dogfood ourselves" milestone and is
  customer-visible.

The long-term topology is the topology we commit to operationally. The
short-term topology is explicitly an on-ramp — we do not invest in
making the Docker Compose stack production-hardened beyond Phase 2
quality.

## Alternatives considered

- **Cloudflare Workers for the backend**: rejected. Workers cannot hold
  persistent SSH sessions to customer clusters, have tight CPU-time
  limits that do not suit provisioning orchestration, and the Rust
  support (workers-rs) is not production-grade for a complex multi-crate
  workspace. We may later use Workers as a targeted edge cache for
  hot-read endpoints.
- **Hetzner-managed Postgres**: rejected because Hetzner does not offer
  a fully managed Postgres product at acceptable latency and backup SLOs
  for our needs. CloudNativePG on our own k3s gives us more control.
- **Fly.io / Railway / Render**: good fits operationally but we commit
  to Hetzner for cost narrative and to keep the dogfooding story pure.
- **Full Kubernetes from day zero**: rejected as premature. We gain
  operational experience with the parts that matter (Hetzner API, SSH,
  k3s) in Phases 0–2 without also fighting our own kube-apiserver.

## Consequences

**We accept:**

- The Phase 3 migration is real work (a sprint to two sprints) and
  risky. We mitigate by doing it after we have already provisioned
  dozens of clusters for users — we trust the code path.
- Short-term Postgres lives on a single VPS. This is acceptable for
  alpha (Phase 1) and friends-and-family (Phase 2) but is a known
  single point of failure until the Phase 3 migration.
- Cloudflare is a hard dependency for DDoS and CDN. A Cloudflare outage
  degrades Kubinate. We accept this; the alternative (running our own
  edge) is infeasible.

**We assume (bets):**

- Hetzner availability is sufficient for our 99.5% SLO. Anecdotal data
  supports this but we will measure and publish monthly.
- CloudNativePG on k3s is operable by a 2–3 person team once we are in
  Phase 3. If not, we fall back to managed Postgres from a third party
  even if it compromises the "Hetzner everywhere" narrative.

**Positive follow-ons:**

- The dogfood story is excellent marketing — we ship blog posts about
  real operational incidents on our own cluster.
- The short-term single-VPS makes local dev parity (same Docker Compose)
  straightforward.

## Revisit trigger

- Hetzner Cloud availability measured over any rolling 90-day window
  falls below 99.9%, **or**
- The Phase 3 migration slips past week 35 without technical blockers,
  indicating the decision to go self-hosted was aspirational rather
  than grounded, **or**
- We acquire an enterprise customer whose compliance requirements force
  a specific topology (e.g. EU data residency in non-Hetzner regions).

## References

- [Cloudflare Pages documentation](https://developers.cloudflare.com/pages/)
- [Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/)
- [CloudNativePG](https://cloudnative-pg.io)
- Hetzner Cloud status history.
