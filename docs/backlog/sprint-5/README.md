# Sprint 5 plan

**Sprint length**: 2 weeks (Phase 3 cadence; weeks 5–6 of 14).
**Capacity target**: 30–40 points; Sprint 5 deliberately
under-fills at ~30 to absorb dogfood-cluster surprises (this
is the first migration of the control plane onto k3s — first
runs always discover something).

## Goal

> Move the control plane onto the dogfood k3s cluster, run
> the deferred Sprint 4 hardening that was gated on a live
> deployment (Vault migration binary, agent mTLS PKI,
> production observability proxy), and close out Sprint 4
> ticket 05's two leftover rows so WebAuthn enforcement is
> fully shipped instead of "shipped except".

## Scope

| File | Status | Pts | What ships |
|---|---|---|---|
| [01-run-dogfood-spike.md](./01-run-dogfood-spike.md) | full ship | 5 | Run M1–M4 measurements from `docs/decisions/sprint-4-dogfood-migration.md`; ratify the recommendation column; resolve every `<<measurement needed>>` marker. **Gates the rest of the sprint.** |
| 02-stand-up-dogfood-cluster.md *(stub at planning)* | full ship | 8 | Hetzner-provisioned 1-CP + 2-worker k3s cluster under `infra/ansible/`'s new `dogfood-*` roles; cert-manager + ingress-nginx + the operator-bootstrap conventions already used by tenant clusters. Detailed at planning once the spike fixes the cluster shape. |
| 03-migrate-api-to-dogfood.md *(stub at planning)* | full ship | 8 | `kubinate-api` running on k3s as a Deployment; Cloudflare Tunnel cutover; rollback plan rehearsed once before cutover. Order (A) per the spike doc — API-first while VPS Postgres stays authoritative. |
| [04-finish-vault-migration.md](./04-finish-vault-migration.md) | full ship | 5 | Vault deployed via the Sprint 4 Ansible role; `kubinate-vault-migrate` binary + dry-run + idempotency; production cutover via the Sprint 4 runbook stub (now filled in); `KUBINATE__KEK` retired from CI. |
| [05-agent-mtls-pki.md](./05-agent-mtls-pki.md) | full ship | 8 | Vault PKI engine issuing per-cluster certs at cluster-create; `agent/src/main.rs` replaces its stub with the real connect+dispatch loop; cert rotation via the `GracefulRestart` command. All five Security DoD rows of Sprint 4 ticket 03 close. |
| 06-observability-proxy-production.md *(stub at planning)* | full ship | 5 | VictoriaMetrics single-node deployed on the dogfood cluster; the proxy from Sprint 3 ticket 07 swaps `InMemoryMetricsStore` for the live VM client; agent's reverse-tunnel `MetricsRemoteWrite` lands on the proxy. Detailed at planning once the spike confirms the cluster shape. |
| [07-close-webauthn-ticket-05.md](./07-close-webauthn-ticket-05.md) | full ship | 3 | `requires_mfa` auto-flip on Owner/Admin promotion; remaining Owner/Admin route migrations (`workers`, `provision`, `addons`, `billing`); threat-model v2 row for the WebAuthn boundary. |

**Total: 42 points.** Slightly above the 40-pt ceiling; #01
and #02 can slip by half a sprint without breaking the rest of
the chain (#04 onwards depends on the cluster existing, but
each takes 2-3 days, so a one-week shift on #02 still lands
the rest within Sprint 5).

The four ticket files written at sprint-plan time (#01, #04,
#05, #07) cover the Sprint-4-deferred work where context is
already nailed down. The three remaining tickets (#02, #03,
#06) stay as README rows because their shape depends on the
spike's M-outcome — detailing them before the spike runs is
guesswork that would have to be redone.

## Dependency graph

```
#01 spike ──► #02 cluster ──► #03 api migration ──► #04 vault ──► #05 agent mTLS
                                  │                     │              │
                                  └─► #06 observability ┘              │
                                                                       │
              #07 webauthn close-out (independent — runs in parallel)──┘
```

- **#01 → #02**: the spike's M3 (Postgres on k3s) and M4
  (single-CP failover RPO/RTO) measurements decide the cluster
  shape. A spike-side surprise (e.g. M2 rollback fails the
  data-loss criterion) can re-shape #02 entirely; #02 is sized
  assuming the spike's default recommendation holds.
- **#02 → #03 / #06**: both want the cluster running. #06
  doesn't strictly need #03 to land first — VictoriaMetrics on
  the cluster works whether the API has migrated or not — but
  the proxy's tenant-isolation tests want the API path live,
  which means #03 first in practice.
- **#03 → #04**: Vault on k3s assumes k3s exists. The
  migration binary itself runs from the API's deployment
  context; running it pre-cutover (against the VPS database,
  with Vault on k3s) is feasible but adds a "Vault reaches
  across the network back to the VPS" path that's
  operationally weird. We default to running it post-cutover
  with both API and Vault on k3s.
- **#04 → #05**: the agent's mTLS PKI rides Vault PKI engine.
  Sprint 4's ADR-0014 explicitly names this as the soft
  prerequisite ("with pgcrypto it's a self-rolled OpenSSL
  flow, with Vault landed [ticket 02], this becomes Vault PKI;
  pick one path"). We pick Vault, so #05 runs after #04.
- **#07 is independent**: same reasoning as Sprint 4 ticket
  05 — touches only `kubinate-identity` and the `crates/api`
  route handlers, no infra dependency. Schedules into any
  sprint window.

## Standing assumptions (named so they fail loudly if violated)

- **Sprint 4's spike doc recommendation holds.** Order (A) —
  API-first, then state. If the M1 cutover wallclock
  measurement comes back > 60s on order (A) but acceptable on
  order (B), #02/#03 swap order and the rest of the sprint
  shifts by ~3 days.
- **No active customer traffic.** The control plane is in
  pre-alpha; the cutover window plans a maintenance banner
  but customer impact is zero on either ordering. If a paying
  customer lands during Sprint 5, the cutover window grows
  from ~1h to a scheduled out-of-hours change.
- **Hetzner test project cost ceiling holds.** The dogfood
  cluster runs continuously (unlike the e2e harness which
  provisions + destroys per run). The standing-cost-per-month
  estimate the spike doc commits to is the gate; if the
  actual spend during Sprint 5 exceeds that ceiling we
  re-shape #02 (smaller node class).
- **`security`-role review is available.** Five of the seven
  tickets touch a security boundary (#04, #05, #07 explicitly;
  #03 carries the cutover audit-chain question; #06 carries
  the per-tenant query-path invariant). CONTRIBUTING.md
  mandates the review; if the security-role maintainer is
  unavailable for a sprint window the affected tickets stage
  as `Proposed` PRs without merging.

## What "done" looks like at sprint close

- [ ] `cargo test --workspace` — all green; no `#[ignore]`'d
      tests added during the sprint.
- [ ] `cargo clippy --workspace --all-targets` — no new
      structural warnings.
- [ ] `frontend/` — `npm run check`, `npm run lint`,
      `npm test -- --run` all green.
- [ ] Every Sprint 5 ticket's DoD rows checked off, or the
      ticket is explicitly re-scoped to Sprint 6 with the
      deferred rows annotated.
- [ ] `docs/decisions/sprint-4-dogfood-migration.md` — every
      `<<measurement needed>>` marker resolved; status flips
      to `Accepted` or a follow-up ADR records the divergence.
- [ ] `docs/runbooks/secrets-migration.md` — every
      `<<measurement needed>>` marker resolved by the actual
      cutover.
- [ ] `docs/runbooks/agent-heartbeat-missing.md` — diagnosis
      section updated with the live `agent_audit` table
      queries (Sprint 4 ticket 03 deferred row).
- [ ] **Threat-model v2 cut**: the v2 trigger ("post-Vault,
      post-observability-proxy") fires; an explicit revision
      ships under `docs/security/threat-model.md` with the
      Flow 2 / Flow 5 STRIDE tables redone against the new
      boundaries.
- [ ] Sprint state memory (`memory/sprint_state.md`) updated
      to reflect Sprint 5 close + Sprint 6 commitments.

## Phase 3 exit-gate coverage after Sprint 5

The roadmap names eight Phase 3 exit gates. Sprint 5 closes
**six** of them (one explicitly, two via Sprint 4 carry-over,
three new):

| Gate | Sprint 5 contribution |
|---|---|
| Vault replaces pgcrypto for tenant secrets | **Closed** via #04. |
| WebAuthn enforced for org owners | **Closed** via #07 (Sprint 4 carried 90% of this; #07 is the last 10%). |
| Per-cluster agent reverse tunnel shipped | **Closed** via #05. |
| Dogfood cluster running the control plane | **Closed** via #01–#03. |
| Production observability proxy in front of real VM | **Closed** via #06. |
| Threat model v2 published | **Closed** at sprint-close (the trigger fires once #04 + #06 land; the v2 revision is a sprint-close deliverable, not a separate ticket). |
| CloudNativePG operator manages control-plane Postgres | (no contribution; this is its own Sprint 6 ticket — the dogfood spike's M3 row pre-shapes it). |
| HA + Temporal revisits ratified | (no contribution; trigger-driven, none firing). |

So the realistic Sprint 6 entry state is: **two open Phase 3
exit gates** (CloudNativePG, HA/Temporal revisits), with the
spike-doc inputs on CloudNativePG already gathered as a
side-effect of Sprint 5's #03.

## Sprint 6 commitments inherited from this sprint

If Sprint 5 lands per the dependency graph, Sprint 6 picks up:

1. **CloudNativePG operator deploy** — the dogfood spike's M3
   measurements pre-size this. Rough size: 5 pts.
2. **HA control-plane revisit** — only fires if an ADR-0012
   trigger has tripped (sustained sev-2 traffic, data-residency
   request, Hetzner managed-etcd availability). If none
   fires the ticket stays parked; planning for it is a
   no-op.
3. **Backlog drain from Sprint 5 deferrals** — anything that
   slipped from the under-fill ceiling above.

Plus the standard "what's the next user-visible feature"
question that Sprint 4 anchored on WebAuthn — Sprint 6's
equivalent is open at planning time.

## Conventions inherited from Sprint 4

Sprint 5 follows the Sprint 4 conventions verbatim — listed
here as a reminder, not a re-derivation:

- **Conventional Commits** with `security`-role review for
  every PR that touches an auth, secrets, tenancy, or
  agent-tunnel boundary.
- **PR target**: reviewable in <20 minutes, ~400 changed
  lines max (excluding generated code, locks, migrations).
- **Empirical-marker convention** (`<<measurement needed>>`)
  for runbooks and decision docs whose accuracy depends on
  a not-yet-run change. Tickets #04 and #06 both carry stub
  runbooks from Sprint 4; the markers resolve as part of
  the cutover.
- **Single-CP invariant** (per ADR-0012's `validate_allowed`).
  Sprint 5 keeps this; the dogfood cluster runs one
  control-plane node + two workers, not three CPs.

## Definitions

- **DoR**: see `/docs/process.md` §4.
- **DoD**: see `/docs/process.md` §5.
