# Nightly E2E harness: provision + destroy, real Hetzner

**Labels**: `area/ci`, `area/workflows`, `sprint-1`
**Size**: 5

## Context

Phase 1 success criterion: "end-to-end automated test that creates
+ destroys a real cluster weekly (nightly in CI)". Runs against a
dedicated Hetzner project. Failure pages the on-call engineer.

## Acceptance criteria

- **Given** the nightly cron in GitHub Actions, **when** the workflow
  runs, **then** it provisions a 1-control-plane + 1-worker cluster,
  asserts the kubeconfig works (`kubectl get nodes` returns 2 nodes),
  installs one baseline add-on, then destroys.
- **Given** any step fails, **then** the workflow posts to the
  incidents channel with the failing step and the workflow ID, and
  leaves any residual Hetzner resources tagged for a follow-up
  manual cleanup.
- **Given** a successful run, **then** no servers exist in the test
  Hetzner project at workflow end.

## DoD
- [ ] Hetzner test project created and its token stored as
      `HETZNER_API_TOKEN_E2E` GitHub secret
- [ ] Workflow has `timeout-minutes: 60`
- [ ] Slack/Discord notification on failure
