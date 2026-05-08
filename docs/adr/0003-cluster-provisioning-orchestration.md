# ADR-0003: Cluster provisioning orchestration

- **Status**: Accepted
- **Date**: 2026-04-24
- **Deciders**: Founding team
- **Tags**: backend, workflow, orchestration, cluster-lifecycle

## Context

Cluster provisioning is the canonical long-running saga for Kubinate:

- Create N Hetzner servers across one or more locations.
- Wait for cloud-init to complete.
- SSH into each server, install k3s in the correct role (server /
  agent), join them together, and verify quorum on the embedded etcd.
- Install the CNI, then the baseline add-ons.
- Collect the kubeconfig, encrypt it, and hand it to the user.
- At every step, handle partial failure: retry idempotently, compensate
  (destroy half-provisioned servers), and surface clear progress to the
  user.

This saga can take 5–15 minutes on the happy path and, on failure, must
either complete successfully after transient errors or unwind cleanly so
the user is not billed by Hetzner for orphan resources.

In addition to provisioning, the same shape of problem appears for
upgrades, scale-out, scale-in, add-on installs, backup operations, and
disaster recovery. Under-investing in this layer now will produce
subtle, hard-to-debug production incidents later.

## Decision

We will use **Temporal** as the workflow orchestration engine in the
short and medium term (months 0–18+). Workflows and activities are
written in **Rust** via the official `temporal-sdk-rust`, with the
escape hatch that if the Rust SDK proves inadequate in the Phase 0 spike
(see ADR-0001 and Section 12 of the project brief), we fall back to the
**Go SDK** for workflow and activity workers, with the Rust API service
invoking them via Temporal's client API.

Activities encapsulate external side effects:

- `HetznerCreateServer`, `HetznerDeleteServer`, `HetznerAttachToNetwork`
- `SshProvisionK3sServer`, `SshJoinK3sAgent` (we wrap `k3sup` or a
  native equivalent; never ad-hoc shell scripts)
- `CollectKubeconfig`, `EncryptAndStoreKubeconfig`
- `InstallBaselineAddons`

The long-term direction (month 18+, conditional) is to migrate the
declarative "cluster spec" surface to a Kubernetes Operator pattern, so
that Kubinate's own control plane speaks CRDs. Temporal remains
well-suited for imperative multi-step operations; CRDs are better for
the "desired state reconciliation" experience power users will expect.
The two are compatible — an operator can trigger Temporal workflows for
non-trivial operations. We do not commit to this migration now.

## Alternatives considered

- **Redis-backed job queue (Sidekiq-style, BullMQ-style)**: rejected
  because we would rebuild Temporal's features (durability, visibility,
  compensation, signals, child workflows, versioning) poorly and
  incrementally. A 2–3 person team cannot afford this.
- **Cluster API (CAPI) with a Hetzner provider**: considered seriously.
  Rejected because the existing Hetzner provider targets vanilla k8s
  rather than k3s, embedding it would bring along assumptions about
  management clusters that conflict with our "customer owns the infra"
  model, and we would still need an orchestration layer above CAPI for
  everything that is not a cluster (billing, add-on installs, audit).
- **Write our own saga engine in Postgres (outbox + state machine
  rows)**: rejected for the same reasons as the Redis option, plus the
  additional engineering cost of getting durability and idempotency
  right.
- **AWS Step Functions / Google Workflows**: rejected because we are
  actively avoiding hyperscaler lock-in.

## Consequences

**We accept:**

- Running a Temporal cluster (server + history + matching + worker
  pools) in production. Operational burden is non-trivial; we offset it
  by using a single-instance Temporal deployment in Phase 0–1 and only
  going HA when we have the dogfooded k3s cluster in Phase 3.
- A new mental model for engineers: workflows must be deterministic,
  side-effecting code must live in activities, and workflow versioning
  is its own discipline. We invest in training and code review
  conventions for this.
- An additional Postgres schema (Temporal's) to back up and migrate.

**We assume (bets):**

- The Rust SDK is adequate; if not, we take the Go-workers fallback
  without changing the workflow boundaries.
- Temporal's licensing and hosted offerings remain stable. If
  Temporal Inc. makes a hostile licensing change, the self-hosted open
  source core under `.NET`/Go SDKs is still viable.

**Positive follow-ons:**

- "Replay from checkpoint" debugging is a superpower when a
  customer-visible provisioning run fails at minute 11.
- The Temporal UI gives us a visibility tool we do not have to build.
- Human-in-the-loop workflows (approvals, manual override for enterprise
  customers) become cheap later.

## Revisit trigger

- The Temporal server's operational cost (engineer hours per quarter)
  exceeds the cost of maintaining a home-grown state-machine engine for
  two consecutive quarters, **or**
- We have shipped the dogfooded k3s cluster and have built sufficient
  controller expertise that the CRD/operator migration for cluster spec
  is the obviously-right next step, **or**
- Temporal's licensing or project health degrades materially.

## References

- [Temporal documentation](https://docs.temporal.io)
- [temporal-sdk-rust](https://github.com/temporalio/sdk-core) (Rust SDK)
- Cluster API project: https://cluster-api.sigs.k8s.io
- [k3sup](https://github.com/alexellis/k3sup) — candidate tool for
  wrapping k3s installs within activities.
