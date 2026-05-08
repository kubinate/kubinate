# Runbook: Hetzner API 5xx surge

**Labels**: `area/sre`, `area/runbooks`, `sprint-1`
**Size**: 1

## Context

Phase-1 runbook stub. We need the runbook file in place before on-call
rotation starts (end of Phase 2).

## Acceptance criteria

- **Given** an engineer paged by a Hetzner-5xx alert, **when** they
  open `docs/runbooks/hetzner-5xx-surge.md`, **then** they can in
  under 60 seconds: confirm the Hetzner status page, identify whether
  to pause provisioning workflows, and find the "pause" command.
- Runbook follows the template.

## DoD
- [ ] Runbook reviewed by second engineer
- [ ] Linked from the provisioning-workflow-stuck runbook
