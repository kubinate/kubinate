# Sprint 1 Backlog

The files in this directory are **seed GitHub Issues** for Sprint 1.
Each file is a ready-to-paste issue body with:

- **Title** (H1)
- **Labels** (suggested GitHub labels)
- **Size** (Fibonacci points: 1, 2, 3, 5, 8)
- **Acceptance criteria** (Given/When/Then)
- **DoR / DoD checklists**

Sprint 1 scope is scoped to the Phase-1 thin slice (brief §9):
"a single user can provision one k3s cluster on their own Hetzner
account and receive a kubeconfig."

## Sprint 1 goals

- **Primary**: ship the end-to-end "create cluster" happy path
  behind a feature flag, against a test Hetzner project.
- **Secondary**: establish the nightly E2E test harness so that we
  cannot regress the happy path unknowingly.

## Sprint 1 issues

| File | Title | Size | Area |
|---|---|---|---|
| [01-hetzner-credential-storage.md](./01-hetzner-credential-storage.md) | Store and retrieve Hetzner API tokens under envelope encryption | 5 | identity / integrations |
| [02-cluster-create-form.md](./02-cluster-create-form.md) | Dashboard form to submit a cluster creation request | 3 | frontend / api |
| [03-provision-workflow-happy-path.md](./03-provision-workflow-happy-path.md) | Implement `ProvisionClusterWorkflow` happy path in Temporal | 8 | workflows / cluster / integrations |
| [04-kubeconfig-delivery.md](./04-kubeconfig-delivery.md) | Encrypt, store, and surface the cluster kubeconfig | 3 | cluster / platform |
| [05-destroy-cluster.md](./05-destroy-cluster.md) | Implement `DestroyClusterWorkflow` | 5 | workflows / cluster |
| [06-cluster-status-ui.md](./06-cluster-status-ui.md) | Show cluster status on the dashboard with real-time updates | 3 | frontend / api |
| [07-github-oidc-login.md](./07-github-oidc-login.md) | GitHub OIDC login + session cookie issuance | 5 | identity / api |
| [08-nightly-e2e-harness.md](./08-nightly-e2e-harness.md) | Nightly E2E harness: provision + destroy, real Hetzner | 5 | ci / workflows |
| [09-structured-audit-log.md](./09-structured-audit-log.md) | Persist audit log entries for token and cluster mutations | 2 | identity / platform |
| [10-runbook-hetzner-5xx.md](./10-runbook-hetzner-5xx.md) | Runbook: Hetzner API 5xx surge | 1 | sre |

**Total**: 40 points. Cap for 2 engineers × 2 weeks = 30-40 points
depending on velocity. We expect to carry the nightly-E2E and
destroy-cluster tickets into Sprint 2 if anything slips.

## Dependency order

```
07 (login) ─┬─► 02 (form) ─┬─► 03 (workflow) ─► 04 (kubeconfig) ─► 06 (status)
            │              │                                         
            └─► 01 (creds)─┘                          05 (destroy) ─►
                                                     08 (nightly E2E)
                                                     09 (audit) ───►
                                                     10 (runbook) ──►
```

## Definitions

- **DoR** (Definition of Ready): see `/docs/process.md` §4.
- **DoD** (Definition of Done): see `/docs/process.md` §5.
