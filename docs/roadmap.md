# Roadmap

This is a living summary of the phased delivery plan. The canonical
source for each phase's scope and exit criteria is the project brief
(§9). This file surfaces the current phase and the next few
milestones.

## Current phase

**Phase 2 closing → Phase 3 entry** (week 22).

Sprint 3 closed at 10/10 on 2026-05-04, finishing Phase 2. Phase 3
opens with the dogfood migration onto our own k3s. The two
spike-conditional Phase-2 finishers (HA control plane and Temporal
SDK adoption) shipped on their **defer paths** — see ADR-0011 and
ADR-0012; both name the trigger that re-opens them inside Phase 3.

## Phase exit ledger

| Phase | Weeks | Status | Headline |
|-------|-------|--------|----------|
| 0     | 1–3   | ✓ closed | Foundations (workspace, CI, ADR seed, threat model v0). |
| 1     | 4–11  | ✓ closed | Thin slice: one user, one cluster, happy path. |
| 2     | 12–21 | ✓ closed | Core MVP: orgs, add-ons, billing scaffolding, observability proxy scaffold. HA + Temporal both deferred (ADR-0011/0012). |
| 3     | 22–35 | **active**, opens with this commit | Multi-tenant hardening + dogfood migration. |
| 4     | 36–48 | not started | Enterprise readiness: SSO, SOC 2, second provider spike. |

## Phase 3 scope

Phase 3 is shaped by **three forcing functions**:

1. **Dogfood migration** — run the control plane on our own k3s
   (the marketing milestone the brief names). Forces a real-load
   test of every Phase-2 abstraction at once.
2. **Phase-2 spike ratifications** — the empirical sections of
   the four `docs/decisions/*.md` starters get filled in against
   the dogfooded environment. Default rec stances stay in place
   (defer Temporal, defer HA, VictoriaMetrics + S3) until the
   measurements either confirm or flip them.
3. **Hardening deferrals** — items the brief explicitly names as
   Phase 3:
   - WebAuthn enforcement for owners (ADR-0009 §MFA).
   - Vault migration (ADR-0007 §Long term — pgcrypto retires).
   - CloudNativePG for control-plane Postgres.
   - Mimir vs. VictoriaMetrics ratification (the spike at
     `docs/decisions/sprint-3-observability-tsdb.md`).

### Phase 3 exit criteria (draft)

These are the gates a planning session should pressure-test before
locking the Sprint 4–N plan. Each row points at the source of truth
that will tell a future operator whether the gate is open.

- [ ] **Dogfood cluster running the control plane.** Source of
      truth: `infra/ansible/` describes the k3s deploy; the API's
      `/healthz` answers from a pod, not a VPS.
- [ ] **Vault replaces pgcrypto for tenant secrets.** ADR-0007
      §Long term records the new mechanism + the data-migration
      runbook. The `KUBINATE_KEK` env var is removed.
- [ ] **CloudNativePG operator manages the control-plane Postgres.**
      The single-VPS Postgres container is gone; backups + PITR are
      operator-managed.
- [ ] **Production observability proxy in front of a real
      VictoriaMetrics.** The `InMemoryMetricsStore` in
      `crates/observability/` is replaced by a `VictoriaMetricsStore`
      that speaks Prometheus remote-write to a managed VM cluster.
      A new ADR ratifies VM (or supersedes the default rec with a
      different choice if the spike measurements force it).
- [ ] **WebAuthn enforced for org owners.** ADR-0009 §MFA gets a
      superseder that records the chosen WebAuthn library + the
      enrollment flow.
- [ ] **Threat model v2** published. The current v1 explicitly
      names "Phase 3 exit (post-Vault, post-observability-proxy)"
      as its revisit trigger; this is when that fires.
- [ ] **HA + Temporal revisits ratified.** Either re-opens with a
      new ADR superseding 0011 / 0012, or keeps them in place with
      a fresh "still defer, here's the updated evidence" note. No
      silent drift.
- [ ] **Per-cluster agent reverse tunnel shipped.** The Phase-0
      stub at `agent/src/main.rs` becomes a real binary with
      mTLS gRPC. Forced by the observability proxy and any future
      out-of-band command path.

## Key decisions still outstanding

These have **decision-doc starters or ADRs**, not yet ratifications:

| Question | Source of truth | Status |
|---|---|---|
| Temporal Rust SDK — adopt or keep deferring? | `docs/decisions/sprint-2-temporal.md` + ADR-0011 | Defer through Phase 2; revisit triggers fire in Phase 3. |
| HA control plane — same-region 3-CP, cross-region, or single-CP? | `docs/decisions/sprint-2-ha-control-plane.md` + ADR-0012 | Defer through Phase 2 + 3 alpha; revisits on data-residency customer / Hetzner managed etcd / sev-1. |
| Mimir vs. VictoriaMetrics for production observability | `docs/decisions/sprint-3-observability-tsdb.md` | Default rec **VM + S3**; needs M1–M4 measurements against the dogfooded cluster. |
| Reverse-tunnel agent architecture (mTLS gRPC) | brief §12 (no decision doc yet) | Spike opens in early Phase 3; agent stub at `agent/src/main.rs` is the parking lot. |
| SvelteKit + Cloudflare Pages SSR adapter | (PoC was Phase 0; verify it still holds with Svelte 5 + experimental.async) | Re-validate before any Phase 3 frontend work that needs SSR. |

## Sprint pipeline

| Sprint | Phase | Status | Backlog |
|---|---|---|---|
| 1     | 1 | closed | `docs/backlog/sprint-1/` |
| 2     | 2 | closed | `docs/backlog/sprint-2/` |
| 3     | 2 | closed (10/10) | `docs/backlog/sprint-3/` |
| 4     | 3 | not yet planned | `docs/backlog/sprint-4/` (one parked-revisit row: HA control plane) |

Sprint 4 planning is the next concrete user-driven step. The
parking lot at `docs/backlog/sprint-4/` already holds the HA
revisit ticket pre-sized off ADR-0012's revisit triggers; expand
with the Phase 3 deferrals listed above when you're ready to
plan.

## Success metrics tracking

These are the brief's targets; we don't yet measure them
automatically.

- Time from signup to first cluster ready: **target < 10 min P50**
- Cluster provisioning success rate: **target > 98%**
- Platform availability SLO: **99.5%** (internal target 99.9%)
- Monthly churn: **target < 5%**
- Paying tenants by month 12: **target 200**

The Phase 3 dogfooded observability proxy is what makes the first
three measurable in production; the billing surface (Sprint 2
ticket 08) is what makes the fourth measurable.

## Conventions

- Each phase ends with a **phase-exit review**: every checkbox
  above flipped or explicitly waived in a follow-up ADR.
- The roadmap is updated at the end of each sprint, not at the
  end of each ticket. If you're a future contributor reading this
  in Phase 4 and "Current phase" still says Phase 3, that's the
  signal to bump it (and the prior Phase's status to "✓ closed").
- **Spike-doc starters** in `docs/decisions/` carry
  `<<measurement needed>>` placeholders for the empirical
  sections; do not ratify their recommendation columns until
  those markers are resolved by a real spike runner. The
  starters are the structure; the spike is the work.
