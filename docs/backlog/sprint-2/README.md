# Sprint 2 Backlog

Same shape as Sprint 1: each file in this directory is a ready-to-paste
GitHub Issue with title, labels, size, acceptance criteria, and DoD.

## Sprint 2 goals

Sprint 2 spans the boundary between Phase 1 (thin slice) and Phase 2
(core MVP). Concretely:

- **Primary**: harden the Sprint 1 thin slice — wire audit context end
  to end, run the integration tests in CI, ship the Ansible layer that
  Phase 0 deferred — so we can dogfood a real Hetzner provisioning run
  with confidence.
- **Secondary**: start the Phase 2 surface area — multi-org membership
  management, the first add-on (ingress-nginx), and Stripe billing
  scaffolding — without committing to the HA / multi-region work that
  belongs to Sprint 3.

## Carry-over from Sprint 1

These were explicit deferrals in Sprint 1, not regressions. Each has a
ticket below.

- Audit context threading                → 01
- CI for integration tests               → 02
- Temporal SDK spike                     → 03
- Credential picker UX                   → 04
- Ansible playbook                       → 05
- Real GitHub OAuth e2e                  → 02 (rolled into the CI job)

## Sprint 2 issues

| File | Title | Size | Area |
|---|---|---|---|
| [01-audit-context-wiring.md](./01-audit-context-wiring.md) | Thread `AuditContext` from API edge through service mutations | 3 | identity / platform / api |
| [02-ci-integration-tests.md](./02-ci-integration-tests.md) | CI workflow: workspace integration tests with ephemeral Postgres | 3 | ci / sre |
| [03-temporal-sdk-spike.md](./03-temporal-sdk-spike.md) | Temporal Rust SDK spike + adopt/defer decision | 5 | workflows |
| [04-credential-picker-ux.md](./04-credential-picker-ux.md) | Replace the credential UUID text input with a picker | 2 | frontend |
| [05-ansible-vps-playbook.md](./05-ansible-vps-playbook.md) | Ansible playbook to configure the Phase 0 control-plane VPS | 5 | sre / infra |
| [06-membership-management.md](./06-membership-management.md) | Multi-org membership management — invite, list, role change | 5 | identity / api / frontend |
| [07-addon-ingress-nginx.md](./07-addon-ingress-nginx.md) | First add-on workflow: ingress-nginx via Helm | 5 | addons / workflows |
| [08-billing-stripe-scaffolding.md](./08-billing-stripe-scaffolding.md) | Stripe customer + plan column on organizations | 5 | billing / api |
| [09-runbook-credential-rotation.md](./09-runbook-credential-rotation.md) | Runbook: tenant Hetzner-token compromise / rotation | 1 | sre |
| [10-ha-control-plane-spike.md](./10-ha-control-plane-spike.md) | HA k3s control plane spike on Hetzner private network | 5 | workflows / cluster |

**Total**: 39 points. Cap for 2 engineers × 2 weeks ≈ 30–40 depending
on velocity. Tickets 03, 07, 10 are the spike-shaped ones — if any
runs over budget, expect 09 (smallest) to absorb the slack and 07 to
slip into Sprint 3.

## Dependency order

```
01 (audit ctx) ─► 06 (membership uses Actor)
02 (ci)        ─► 07 (addon needs CI to verify Helm install)
                 └► 03 (Temporal spike output gates the runner replacement)
05 (ansible)   ─► 08 (Stripe webhook needs the prod baseline)
04 (picker)    — independent
09 (runbook)   — independent
10 (HA spike)  — independent (informs Sprint 3 scope)
```

## Definitions

- **DoR** (Definition of Ready): see `/docs/process.md` §4.
- **DoD** (Definition of Done): see `/docs/process.md` §5.
