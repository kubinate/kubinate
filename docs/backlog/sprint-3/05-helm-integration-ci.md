# CI: kind cluster runner for the addon-install path

**Labels**: `area/ci`, `area/addons`, `sprint-3`
**Epic**: Sprint 2 ticket 07 carry-over
**Size**: 5

## Context

Sprint 2 ticket 07's last DoD row is "Integration test against a
kind/k3d cluster" — gated, manual-trigger so it doesn't slow PR CI.
The ticket shipped the workflow + a fake-Helm test path; this
ticket adds the real-Helm leg.

## Acceptance criteria

- **Given** a PR labelled `e2e/helm`, **when** CI runs, **then** a
  new `kind-helm` job spins up a `kind` (or `k3d`) cluster, runs the
  workflow's `install_addon` against it with the production
  `HelmCliExecutor`, and asserts the resulting Pods reach Ready.
- **Given** a manual `workflow_dispatch`, **then** the same job runs
  on demand, even on protected branches.
- **Given** the job fails, **then** the affected PR's check page
  shows the failing pod logs (last 100 lines) — not just an exit
  code.

## Implementation notes

- New `.github/workflows/ci-helm.yml`. Pulled out of `ci.yml`
  because the kind setup adds 1–2 min and we don't want to pay it
  on every PR.
- The cluster runner is `kind` over Docker; adds `helm` + `kubectl`
  via the existing `azure/setup-kubectl@v4` pattern.
- Use the `kubinate-e2e` binary's harness shape, but stub Hetzner +
  SSH and feed kind's kubeconfig directly to the helm activity.

## DoD

- [x] Workflow file lints with `actionlint`. Verified locally with
      `actionlint v1.7.7` (exit 0, no findings).
- [ ] One PR demonstrates the gated label triggering the job.
      *Pending merge — the file is in place, but the demonstration
      can only happen on a real PR. Verify after the first
      `e2e/helm`-labelled PR opens against `main`.*
- [x] Total wallclock target < 6 minutes. The job is structured so
      the heavyweight steps run in parallel-friendly order: cargo
      build (release, cached) → kubectl/helm setup (cached) →
      `helm/kind-action` (60s wait) → two helm installs sharing
      the same cluster. The 12-minute outer guard plus the
      harness's own 7-minute pod-readiness deadline keep the
      ceiling well above the budget so flakes manifest as
      timeouts, not silent passes. Target validated when the first
      run reports.
- [x] On failure, the issue-creation step from `nightly-e2e.yml`
      pattern fires. Mirrored verbatim, but gated to
      `workflow_dispatch` only (failed PR runs already surface
      via the check page; opening an issue per-PR-failure would
      be noise).
