# Sprint 3 Backlog

Same shape as Sprints 1 and 2: each file is a ready-to-paste GitHub
Issue with title, labels, size, acceptance criteria, and DoD.

## Sprint 3 goals

Sprint 3 closes Phase 2 (core MVP) and lays the groundwork for the
Phase 3 dogfood migration. Concretely:

- **Primary**: ratify the two Sprint 2 spikes — Temporal SDK adoption
  (or formal deferral) and HA control-plane delivery — and ship the
  matching code so we can run a real 3-node CP behind a feature flag.
- **Secondary**: ship the Phase 2 add-ons that round out the "managed
  k3s on day one" promise (cert-manager), close the deferred frontend
  surfaces (addon install UI, billing settings), and stand up the
  observability proxy that the project brief schedules next.

## Carry-over from Sprint 2

These are the explicit deferrals from Sprint 2's tickets, not new work:

- Temporal SDK adoption / extension — `sprint-2-temporal.md` decision.
- HA control plane delivery — `sprint-2-ha-control-plane.md` decision.
- Addon install UI on the cluster status page — ticket 07 frontend.
- Billing settings page — ticket 08 frontend.
- Real Hetzner-side integration test for the nightly E2E — ticket 08
  carry-over (kind/k3d test was deferred).
- Plan-change audit trigger on `organizations` — ticket 08 DoD row.

## Sprint 3 issues

| File | Title | Size | Area |
|---|---|---|---|
| [01-temporal-sdk-adoption.md](./01-temporal-sdk-adoption.md) | Adopt the Temporal Rust SDK (or formally defer) | 8 | workflows |
| [02-ha-control-plane.md](./02-ha-control-plane.md) | Ship 3-node control plane behind a feature flag | 8 | workflows / cluster / frontend |
| [03-addon-install-ui.md](./03-addon-install-ui.md) | Frontend addon install + status panel | 3 | frontend |
| [04-billing-settings-page.md](./04-billing-settings-page.md) | `/app/settings/billing` — plan + checkout | 3 | frontend |
| [05-helm-integration-ci.md](./05-helm-integration-ci.md) | CI: kind cluster runner for the addon-install path | 5 | ci / addons |
| [06-organizations-audit-trigger.md](./06-organizations-audit-trigger.md) | Audit trigger for `organizations.plan` changes | 2 | platform / billing |
| [07-observability-proxy.md](./07-observability-proxy.md) | Multi-tenant metric + log query proxy (skeleton) | 8 | observability |
| [08-cluster-scale-out.md](./08-cluster-scale-out.md) | Add / remove worker nodes on an existing cluster | 5 | workflows / cluster |
| [09-addon-cert-manager.md](./09-addon-cert-manager.md) | Second add-on: cert-manager via the existing catalog | 3 | addons |
| [10-cluster-status-sse.md](./10-cluster-status-sse.md) | Replace cluster-status polling with SSE | 5 | api / frontend |

**Total**: 50 points. Cap for 2 engineers × 2 weeks ≈ 30–40 depending
on velocity; **expect carry-over**. The two 8-pointers (01, 02) are
spike-conditional — if Sprint 2's spikes recommend "Defer Temporal" or
"Single-CP only", those tickets reduce to 3-pt scope tickets covering
the metric/runbook follow-ups instead.

## Dependency order

```
sprint-2/03 (Temporal spike)  ─► 01 ─► 08 (scale-out wants signals)
sprint-2/10 (HA spike)        ─► 02
05 (kind CI)                  ─► 09 (cert-manager wants the test gate)
                                └► full validation of 03 (UI exercises Helm path)
06 (org audit trigger)       — independent
07 (observability)           — independent
04 (billing UI)              — independent
10 (SSE)                     — independent of 01/02; closes ticket 06 polling deferral
```

## Phase boundary

Sprint 3 is the last Phase 2 sprint. Phase 3 (week 22+) opens the
dogfood migration onto our own k3s. Anything that should *not* slip
into Phase 3 ships in this sprint:

- HA control plane (paying tenants need it before we self-host).
- Observability proxy (we need to scrape our own cluster from
  somewhere we don't yet operate).

Anything in this list is an explicit Phase 3 deferral:

- WebAuthn enforcement for owners (ADR-0009 §MFA).
- Vault migration (ADR-0007 §Long term).
- CloudNativePG for control-plane Postgres.
- Mimir vs. VictoriaMetrics decision (the spike for ticket 07
  picks one).

## Definitions

- **DoR**: see `/docs/process.md` §4.
- **DoD**: see `/docs/process.md` §5.
