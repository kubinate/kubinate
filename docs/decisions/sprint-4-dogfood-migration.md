# Sprint 4 spike — dogfood-cluster migration plan

**Status**: Proposed (recommendation **incremental migration: API +
            observability proxy first, then Postgres + Temporal**,
            conditional on the measurements below).
**Decided**: <<2026-05-05; ratify after the spike runs in Sprint 5>>
**Spike owner**: Cluster Orchestrator team (owns the API runtime
            shape) + SRE (owns the cluster bring-up + cutover).
**Relates to**: ADR-0004 (deployment topology — single-VPS now,
            k3s in Phase 3 was the original commitment), ADR-0011
            (defer Temporal — affects the migration order: the
            in-process runner moves with the API rather than as
            its own service), `infra/ansible/` (the seven Phase 2
            roles that this migration retires or rehomes).

---

## ⚠️ Honest scope note

Same shape as `sprint-2-temporal.md`,
`sprint-2-ha-control-plane.md`, and
`sprint-3-observability-tsdb.md`: this file is a **decision-doc
starter**, not the spike itself. The empirical sections below are
marked `<<measurement needed>>` — the spike-runner fills those in
by actually executing the migration plan in a staging environment.
Do not ratify the recommendation until those markers resolve.

The structure here is the value: it picks the questions the spike
must answer, lists the tradeoffs we can already assert from prior
art, and pre-commits the team to a shape for the recommendation
so the spike doesn't drift into open-ended exploration.

---

## Context

The Kubinate control plane runs on a single Hetzner VPS today
(ADR-0004). `infra/ansible/site.yml` orchestrates seven roles
across that single host:

- `common` (base packages, hardening)
- `docker` (the container runtime)
- `postgres` (the control-plane database)
- `temporal` (the workflow server, currently un-used because
  ADR-0011 deferred SDK adoption — the in-process runner ships
  inside `kubinate-api`)
- `kubinate_api` (the Axum binary)
- `cloudflared` (the public ingress tunnel)
- `backup` (off-host snapshot rotation)

Phase 3 opens with the dogfood migration: running this same
control plane on our own k3s cluster, the way our customers'
clusters do. The forcing functions:

1. **Marketing milestone.** The brief's Phase 3 thesis is "we
   run on what we sell"; nothing else in Phase 3 starts before
   this lands.
2. **Production-shape host for the Sprint 4 hardening prep.**
   Vault (Sprint 4 ticket 02), the agent reverse tunnel (ticket
   03), and the production observability proxy (ticket 04 —
   deferred to Sprint 5+) all expect to land *on* the dogfood
   cluster, not on the single VPS.
3. **Threat model v2.** Its v1 footer names "post-Vault,
   post-observability-proxy" as the v2 trigger. Both ride on
   this migration.

The decision: **what migration shape, in what order, and against
what rollback story?**

The sub-question we are *not* answering here: *should we run on
k3s vs. another distribution* (k0s, microk8s, vanilla)? ADR-0004
already commits to k3s; we run our own product. Different question
if `kubinate-cluster::service` ever supports a non-k3s target,
but that's not Phase 3's question.

## Spike scope

Three to four days of focused work in Sprint 5, time-boxed.
Deliverable:

1. A staging copy of the Phase 2 VPS (the same Ansible playbook,
   one fresh Hetzner VPS).
2. A second Hetzner project running k3s (1 CP + 2 worker nodes
   for the spike — Phase 3 starts at this size; multi-CP is its
   own ADR-0012 trigger conversation).
3. The four measurements below executed against (1) → (2) in
   sequence.
4. A live rollback rehearsal that returns from (2) → (1) under
   60 minutes, recording the wallclock of every reverse step.

Use the existing nightly E2E harness shape (`crates/e2e/`) — do
not write a new one. The harness already drives the canonical
"clone → cluster Ready → destroy" flow, which is exactly the
shape the migrated control plane must keep delivering.

## What we can assert from prior art

These items don't need empirical measurement — they're stable
facts about the artifacts and ecosystem we're entering.

### What stays the same

- **Domain-crate behaviour.** Every test in `crates/cluster/`,
  `crates/workflows/`, `crates/observability/` runs identically
  on either deploy shape. `cargo test --workspace --lib` is
  the smoke that proves nothing in the migration changes the
  business logic.
- **Postgres schema.** Every migration in `migrations/` is
  append-only (CLAUDE.md invariant). The migration carries
  `migrations/` along; CloudNativePG (or self-managed Postgres
  on k3s — see M3 below) replays them on the new pod.
- **Tenant isolation.** RLS is a property of the *database*,
  not the host. Crossing from VPS-Postgres to k3s-Postgres
  preserves it.
- **The in-process runner.** ADR-0011 defers Temporal SDK
  adoption; the runner lives inside `kubinate-api` and moves
  with the API binary. There is no "migrate Temporal" step —
  the `temporal` Ansible role retires.
- **Customer-cluster fleet.** The migration is the *control*
  plane only. Customer k3s clusters on Hetzner are unchanged;
  they continue to be addressed via the existing Hetzner API
  surface.

### What changes structurally

- **Ingress.** `cloudflared` on a VPS becomes either
  `cloudflared` in a Deployment + Tunnel, or a k3s-native
  Ingress controller. Cloudflare Tunnel survives either way
  (ADR-0004 commitment), so this is a packaging choice not a
  vendor choice.
- **Secrets.** Today `KUBINATE__KEK` rides in
  `infra/ansible/group_vars/all/vault.yml` (Ansible Vault, not
  HashiCorp Vault). Post-migration the env var still exists
  but is sourced from a k8s Secret (or, after Sprint 4 ticket
  02 and Sprint 5+ Vault deploy, from Vault's transit
  engine). The transition is one env-var binding change.
- **Backups.** The `backup` Ansible role becomes a CronJob —
  or, if M3 picks CloudNativePG, the operator's PITR features
  replace it.
- **Observability.** The single-VPS deploy can scrape
  `/metrics` directly; the k3s deploy gets a Prometheus
  scrape annotation on the `kubinate-api` pod. The
  in-process `InMemoryMetricsStore` (Sprint 3 ticket 07)
  stays in service through the migration; replacing it with
  the production VictoriaMetrics-backed impl is the deferred
  Sprint 5+ ticket 04.

### Migration order matters

Two viable orderings exist. We pre-commit to one to keep the
spike scoped:

- **(A) API-first, then state.** Stand up k3s; deploy
  `kubinate-api` against the *existing* VPS Postgres (the
  cluster's network can reach back). Validate the API on k3s.
  Then migrate Postgres. Lower wallclock for the first user-
  visible cutover, but you carry an unusual "API on k3s,
  Postgres on VPS" intermediate state for some hours.
- **(B) Big-bang.** Stand up k3s with the full stack
  (Postgres, API, all sidecars), restore Postgres from a
  pg_dump snapshot, point Cloudflare at the new ingress, sunset
  the VPS. No intermediate state, but a single longer cutover
  window with the entire migration on the critical path.

We default-recommend **(A) API-first**. The intermediate state
is unusual but bounded — every sub-step has a 30-second-ish
reversibility because the VPS Postgres is still authoritative.
Big-bang's appeal is operational cleanness; its risk is that
you discover the migration's bugs after Postgres has cut over,
and the rollback is "restore from the latest snapshot" with the
data loss that implies.

## What needs measurement (the actual spike)

### M1 — Cutover wallclock

End-to-end "API stops serving on the VPS" → "API serves on
k3s" wallclock, measured from the Cloudflare tunnel cutover
forward.

- **Cutover wallclock under ordering (A)**: <<measurement needed>>
- **Cutover wallclock under ordering (B)**: <<measurement needed>>
- **Customer-visible 5xx duration** (the synthetic monitor
  cluster-create call sees the gap): <<measurement needed>>

Pass criterion: customer-visible gap < 60s for either
ordering. If neither hits 60s, the migration plan needs a
load-balancer-aware cutover (out of scope for this spike).

### M2 — Rollback rehearsal

Execute the migration to k3s, then deliberately roll back to
the VPS. Measure each reverse step's wallclock. The cluster
must converge to the original state with **zero data loss**.

- **Total rollback wallclock**: <<measurement needed>>
- **Cluster state diff (Postgres rows changed during the
  migration window) replayed onto the rolled-back VPS**:
  <<measurement needed>>
- **Did any tenant-visible row get lost** (specifically: a
  cluster row created during the migration)? <<measurement needed>>

Pass criterion: rollback < 60 minutes wallclock; **zero**
tenant-visible row loss. Rollback that loses data is no
rollback; we re-design the migration shape if M2 fails.

### M3 — Postgres on k3s

Two viable choices:
1. **CloudNativePG** — operator-managed, PITR-capable,
   future-proof.
2. **Self-managed `postgres:16-alpine` StatefulSet** — same
   image as today's container, smaller op surface, no PITR
   without separate work.

The roadmap names CloudNativePG as a Phase 3 exit gate, so
choice (1) is the long-term answer. The question this M
answers is *whether Sprint 5 can take it on at the same time
as the migration itself*, or whether we land on the
StatefulSet shape first and then re-platform inside Phase 3.

- **CloudNativePG operator deploy + Postgres restore from a
  Phase 2 pg_dump**: <<measurement needed (works first try
  yes/no, time)>>
- **Self-managed StatefulSet + same restore**: <<measurement needed>>
- **Backup story for each (CloudNativePG PITR vs.
  StatefulSet + CronJob `pg_dump`)**: <<measurement needed>>

Pass criterion: at least one of the two completes the restore
in < 30 minutes from a 1 GB-class snapshot. If neither, the
spike re-opens the conversation (likely option: keep Postgres
on a managed Hetzner Postgres if/when Hetzner ships one;
pre-empt by writing this footnote).

### M4 — `kubinate-api` runtime shape

The Phase 2 deploy runs `kubinate-api` as a container under
Docker on a VPS. On k3s, two viable shapes:

1. **Deployment** (multiple replicas, stateless). Fits the
   pattern of every other web service. The in-process runner
   becomes "any replica can run a workflow"; the existing
   `tokio::spawn` shape doesn't survive a pod restart, but
   neither did the VPS shape.
2. **StatefulSet** (single replica, stable identity). Closer
   to the Phase 2 "one VPS, one process" model; safer for the
   in-process runner during the migration, less idiomatic.

The headline tradeoff: a Deployment with replicas > 1 means
*two replicas can spawn the same workflow* if a request lands
on each — the in-process runner has no cross-replica
coordination today. ADR-0011's revisit trigger fires
exactly here: "sustained > 10 inflight workflows" wasn't
the only signal that re-opens it; "two replicas accidentally
running the same workflow" is the same forcing function.

- **Deployment with replicas=1 + Pod-ReadinessGate to delay
  rolling**: <<measurement needed (does the in-process runner
  cleanly hand off, or does a pod restart leak in-flight
  workflows?)>>
- **StatefulSet replicas=1**: <<measurement needed>>
- **Memory under one in-flight provision**: <<measurement
  needed>>
- **OTLP collector reachable from the API pod** (the
  `KUBINATE__OTLP_ENDPOINT` env var stays the same; what
  changes is whether a Tempo/Jaeger backend is reachable on
  the cluster network): <<measurement needed>>

Pass criterion: the in-process runner survives a single-pod
restart without leaking an in-flight workflow row in the
`provisioning_workflows` shadow table; OTLP traces show up
end-to-end.

## Recommendation skeleton

The recommendation flips on M1 / M2 / M3 / M4:

| M1 cutover | M2 rollback | M3 Postgres | M4 API shape | Recommendation |
|---|---|---|---|---|
| Both < 60s | < 60 min, zero loss | CloudNativePG works | Deployment passes restart test | **Default: incremental (A), CloudNativePG, Deployment replicas=1.** Sprint 5 commits the migration. |
| Both < 60s | < 60 min, zero loss | Only StatefulSet works | Deployment passes restart test | **Incremental (A), StatefulSet Postgres, Deployment API.** CloudNativePG re-spike in Sprint 6+. |
| Both < 60s | < 60 min, zero loss | Either works | Restart leaks workflows | **Incremental (A), StatefulSet API.** ADR-0011 revisit trigger fires; queue Temporal SDK as a parallel ticket. |
| (A) > 60s | n/a | n/a | n/a | **Big-bang (B) with a 5-minute maintenance window.** Status-page comms required. |
| (A) and (B) > 60s | n/a | n/a | n/a | **Defer migration to Sprint 6+.** Spike a load-balancer-aware cutover before re-attempting. |
| Rollback fails / loses data | n/a | n/a | n/a | **Stop.** Re-design the migration shape. The "what 'Defer to Sprint 6+' looks like" section below covers what the team does in the meantime. |

Default recommendation, until M1–M4 are filled in:
**incremental migration (A), CloudNativePG for Postgres,
Deployment with replicas=1 for the API**. The reasoning:

- Ordering (A) keeps the rollback story trivial as long as
  the intermediate state holds.
- CloudNativePG is the long-term answer per the roadmap; doing
  it once is cheaper than doing it twice (StatefulSet first,
  CloudNativePG later).
- Deployment is more idiomatic for Kubernetes; ADR-0011's
  revisit trigger is the right place for the in-process-runner
  multi-replica concern, not a deployment-shape work-around.

## What "Defer to Sprint 6+" looks like in practice

If M2 fails (rollback loses data) or M1 says no ordering hits
the 60s wallclock target, the migration doesn't ship in Sprint
5. Operationally:

- Sprint 5 ships **Vault on the VPS** (we already have an
  Ansible Vault role today — we land HashiCorp Vault as a
  systemd service alongside the existing `kubinate_api` role).
  This unblocks Sprint 4 ticket 02's full ship without
  requiring k3s.
- Sprint 5 ships **the agent reverse-tunnel binary deploy**
  via cloud-init — same VPS, just real production work for
  the customer-cluster path.
- Sprint 6 re-spikes the migration with a load-balancer
  shape (specifically: a Cloudflare Worker that proxies
  between VPS and k3s during a long cutover, sequencing
  the per-tenant DNS swap independently from the per-tenant
  data move).

This deferral path costs us the "we run on what we sell"
marketing milestone for two extra weeks, not the threat-model
v2 publication or the Vault rollout. **Both still happen** —
the dogfood migration is the *destination*, not the
prerequisite for those.

## Revisit trigger

This decision-doc starter re-opens automatically when **any**
of the following observables fire after Sprint 5:

1. **The in-process runner leaks an in-flight workflow during
   any production pod restart.** ADR-0011's revisit trigger
   fires; we adopt the Temporal SDK and the dogfood deploy
   becomes "Deployment replicas > 1 is finally safe."
2. **CloudNativePG's restore-from-snapshot path fails in
   production.** Open the StatefulSet fallback ticket; treat
   the failure as a SEV-2 with the dogfood-cluster runbook
   (yet to be written) covering the database-redeploy path.
3. **Cutover takes > 60s in production despite the spike's
   60s pass criterion.** The spike measured against staging
   load; production traffic flips the recommendation row to
   "big-bang or load-balancer-aware."

If a trigger fires, the team opens a follow-up ADR
ratifying the new shape (or a new spike doc starter for the
load-balancer-aware cutover). We do not quietly drift.

## What this spike deliberately does **not** do

- **Decide CloudNativePG vs. self-managed Postgres on k3s
  permanently.** M3 picks the *Sprint 5* answer; the
  roadmap's Phase 3 exit gate ("CloudNativePG operator
  manages the control-plane Postgres") is the eventual
  destination. If M3 picks StatefulSet, it's an explicit
  intermediate.
- **Address the Temporal SDK question.** ADR-0011 already
  governs that; M4 names the deploy-shape concern that *would*
  re-open ADR-0011, but doesn't pre-empt that revisit.
- **Decide the multi-cluster topology for the dogfood
  cluster itself.** Sprint 5 ships 1 CP + 2 workers — same
  thing customers get. ADR-0012 governs HA for customer
  clusters; whether the dogfood cluster eventually goes
  multi-CP is its own decision in a future ADR.
- **Cover Hetzner-managed Postgres or Hetzner-managed k3s.**
  Neither product exists at the time of writing. ADR-0007's
  "if Hetzner publishes managed etcd" trigger is the
  related case to track.
- **Specify the cluster autoscaler shape, multi-tenant
  isolation between dogfood-cluster pods, or the rate-limit
  story.** Phase 3 follow-on tickets, sized off the live
  migration's measurements.

## References

- ADR-0004: Deployment topology — the "k3s in Phase 3"
  commitment this spike executes against.
- ADR-0011: Defer the Temporal SDK adoption — the in-process
  runner's deploy shape concerns concentrate in M4.
- ADR-0012: Defer HA control plane — customer-cluster HA;
  this spike does not re-litigate that decision for the
  dogfood cluster.
- `infra/ansible/site.yml` + the seven roles under
  `infra/ansible/roles/`: the Phase 2 deploy that this
  migration retires (or rehomes).
- `infra/terraform/`: the VPS provisioner; survives the
  migration, just provisions a different host shape.
- `crates/e2e/`: the harness this spike reuses for the
  cluster-create smoke test.
- `docs/backlog/sprint-4/06-dogfood-migration-spike.md`: the
  ticket that opened this decision-doc starter.
- `docs/backlog/sprint-4/README.md` § "Sprint 5 commitments
  inherited from this sprint": the work that lands once the
  recommendation here is ratified.
