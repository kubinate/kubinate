# Sprint 2 spike — HA k3s control plane on Hetzner

**Status**: Proposed (recommendation **Same-region 3-CP**, conditional
on the measurements below).
**Decided**: <<2026-04-28; ratify after the spike runs>>
**Spike owner**: Cluster Orchestrator team.
**Relates to**: ADR-0003 (cluster provisioning orchestration),
`crates/workflows/src/workflows.rs#provision_cluster` (single-CP
implementation we ship today).

---

## ⚠️ Honest scope note

Same as `sprint-2-temporal.md`: this file is a **decision-doc starter**.
Empirical sections are marked `<<measurement needed>>` and the
spike-runner fills them in by actually provisioning. Do not ratify
until those markers resolve.

---

## Context

Sprint 1's `provision_cluster` hard-codes `control_plane_count = 1`
([crates/cluster/src/service.rs](../../crates/cluster/src/service.rs)).
ADR-0003 §Context flagged "k3s HA etcd latency across Hetzner regions"
as an open question to spike in Phase 1; the project brief schedules
HA delivery for Phase 2. This spike answers: **what topology should
the HA workflow target, and what does the new workflow shape look like?**

The delivery question (which Sprint 3 ticket actually ships HA) is
out of scope here; the spike sizes it.

## Spike scope

Two days. Deliverable:

1. A throwaway `kubinate-ha-spike` binary that provisions:
   - 3 control-plane servers + 1 worker, all in the same Hetzner
     location (`nbg1`).
   - Same shape but spread across `nbg1` + `fsn1` (cross-region).
2. Latency probes against each topology:
   - `etcdctl endpoint health --cluster`
   - `etcdctl check perf --consistency l`
   - `kubectl create deployment` + `kubectl wait` round-trip.
3. Failure-mode probe: kill one CP, verify cluster stays writeable;
   start the killed CP, verify rejoin.
4. Tear down via the existing `destroy_cluster` workflow (Sprint 1
   ticket 05). The 60-minute outer timeout from the nightly E2E
   harness applies.

Use the existing nightly E2E harness shape — don't write a new one.

## What I can assert from prior art

### Why same-region first

- k3s embedded etcd uses Raft. Raft tolerates `(N-1)/2` failures
  with `N` nodes; a 3-node CP tolerates one failure, which is the
  Phase-2 SLO target.
- Same-region etcd writes hit ~1ms p99 in production deployments.
  Cross-AZ within the same Hetzner location adds a few hundred μs at
  most. **Cross-region** is the question — see `<<measurement needed>>`
  below.
- ADR-0004 already commits us to "Hetzner everywhere" in Phase 0–2,
  so multi-region in Phase 2 is a forcing-function choice rather
  than a default.

### What changes in the workflow

The structural changes from single-CP to 3-CP, regardless of where
the nodes sit:

- `control_plane_params` becomes `cp_params(index)` — the existing
  pattern used by `worker_params`. Activity bodies don't change.
- The k3s install activity changes shape:
  - First CP runs `k3s server --cluster-init`.
  - Second + third CPs run `k3s server --server <first-cp> --token <token>`.
  - Workers join unchanged.
- The kubeconfig still comes from the first CP. ADR-0004's "no
  public kube-apiserver" still holds; the load-balancer is a
  follow-on after Phase 3 dogfood.
- `cluster_servers` table already supports N rows per role — no
  schema change.

### What stays the same

- Destroy workflow: the existing idempotent `delete_server` loop
  already handles N-of-each-role.
- Audit + `cluster_addons`: unchanged.
- The `LocalRunner` orchestration: unchanged, just dispatches more
  activities.

## What needs measurement (the actual spike)

### M1 — Same-region 3-CP latency

Provision 3 CPs + 1 worker in `nbg1`:

- **etcdctl check perf summary**: <<measurement needed>>
- **`kubectl create deployment nginx --replicas=3` to "all 3 ready"**
  : <<measurement needed>>
- **CP-to-CP RTT (private network)**: <<measurement needed>>

Pass criterion: etcdctl perf "PASS", deployment ready < 30s.

### M2 — Cross-region (`nbg1` + `fsn1`) 3-CP latency

Same probes:

- **etcdctl check perf summary**: <<measurement needed>>
- **`kubectl create deployment` to all 3 ready**: <<measurement needed>>
- **CP-to-CP RTT**: <<measurement needed>>

Pass criterion: etcdctl perf "PASS" *and* RTT p99 < 25ms (Raft
heartbeat default tolerance). If the cross-region setup fails
either, recommend same-region only.

### M3 — Failure-mode behaviour

Run with the same-region 3-CP from M1:

- Kill one CP server (`hcloud server poweroff <id>`).
- Cluster stays writeable: <<measurement needed (yes/no + how long until next write succeeds)>>
- Start the killed server.
- Cluster reconverges to all 3 healthy: <<measurement needed (wallclock)>>
- Etcd reports member count = 3: <<measurement needed>>

Pass criterion: writes recover within 30s; member rejoin within 5min.

### M4 — Provisioning wallclock for 3-CP + 1 worker

End-to-end from `provision_cluster` start to `Ready`:

- **Same-region**: <<measurement needed>>
- **Cross-region**: <<measurement needed>>

Compares to today's single-CP wallclock (~10 min P50 per ADR-0003
§Context). Pass criterion: same-region 3-CP wallclock < 15 min.

## Recommendation skeleton

The recommendation flips on M1 / M2 / M3:

| M1 same-region | M2 cross-region | M3 failure recovery | Recommendation |
|---|---|---|---|
| PASS | PASS | <30s writeable | **Same-region 3-CP default; expose cross-region behind a feature flag** for enterprise. |
| PASS | FAIL | <30s writeable | **Same-region 3-CP only**; cross-region is a Phase 4 Vault/SOC 2 follow-on. |
| PASS | n/a | >30s writeable | **Single-CP for Phase 2**; HA after k3s upgrades or moves to managed etcd in Phase 3. |
| FAIL | n/a | n/a | **Single-CP only**; revisit when k3s ships an HA option that doesn't rely on embedded etcd at this latency. |

My **default recommendation is "Same-region 3-CP, cross-region
deferred"**. The reasoning:

- Same-region etcd is well-trodden ground; M1 + M3 are very likely
  to PASS based on industry data.
- Cross-region etcd is the experiment that's actually risky.
  Hetzner's `nbg1` ↔ `fsn1` private-network RTT is in the
  10–20ms range based on public benchmarks, which is *near*
  Raft's 25ms tolerance. The spike is *the* place to find out.
- Phase 2's customer base is "small teams". They don't need
  cross-region; they need "if one node hiccups, my cluster doesn't
  go down". Same-region 3-CP delivers that.

## Sprint 3 ticket sizing (post-spike)

If the recommendation is "Same-region 3-CP default":

- 5 pts — workflow change to provision N CPs (`cp_params(index)`,
  k3sup join semantics).
- 3 pts — frontend cluster-create form: control-plane-count picker
  (1 or 3, no other values).
- 3 pts — destroy workflow: validate it handles N-of-each-role
  correctly (likely already does — sanity-check).
- 2 pts — runbook: "etcd member unhealthy" — diagnostic +
  recovery commands.
- 2 pts — observability: per-cluster etcd metrics scraped via the
  agent (Sprint 1 reverse tunnel).

If the recommendation is "Cross-region behind a flag" the size
roughly doubles — add network setup, multi-AZ DNS, kubeconfig
endpoint selection.

## Revisit trigger

- A customer with a hard data-residency requirement that requires
  cross-region (multi-region within the EU). Until then, single-region
  is operationally the right choice.
- Hetzner publishes a managed etcd / Cluster API offering that
  removes the embedded-etcd risk entirely — re-spike against it.

## What this spike deliberately does **not** do

- Rolling upgrade of an existing 1-CP cluster to 3-CP. Out of
  scope; that's Sprint 4 (live migration of existing tenants is a
  separate forcing function).
- Changing `cluster_status` enum or shadow-table shape. The
  existing schema already accommodates N CPs.
- Adding load-balancer-in-front-of-API-servers. Phase 3 work,
  alongside the dogfood migration.

## References

- ADR-0003 §Context (k3s HA etcd latency open question).
- ADR-0004 (single-VPS now, dogfood k3s in Phase 3).
- `crates/cluster/src/service.rs#validate_allowed` —
  enforces `control_plane_count == 1` today; the gate to relax.
- `crates/workflows/src/workflows.rs#provision_cluster` — the workflow
  that grows a CP loop.
- `crates/workflows/src/activities.rs#ssh_install_k3s_server` — the
  single-CP install today; multi-CP requires the
  `--cluster-init` / `--server` split described above.
