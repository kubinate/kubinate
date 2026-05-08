# Ansible playbook to configure the Phase 0 control-plane VPS

**Labels**: `area/sre`, `area/infra`, `sprint-2`
**Epic**: Sprint 1 carry-over
**Size**: 5

## Context

Phase 0 shipped the Terraform that provisions the control-plane VPS
(`infra/terraform/`) but ADR-0004 specifies "Docker Compose managed
via an Ansible playbook" for Phases 0–2. The Ansible layer doesn't
exist yet; today the cloud-init in `infra/cloud-init/` only installs
Docker. The actual Kubinate stack (API, Postgres, Temporal) has no
deployment path other than hand-running `docker compose up`.

## Acceptance criteria

- **Given** a freshly Terraform-provisioned VPS, **when** the
  operator runs `ansible-playbook infra/ansible/site.yml -i ...`,
  **then** the host runs the kubinate-api + Temporal + Postgres
  Docker Compose stack with the secrets pulled from the operator's
  1Password vault per ADR-0007.
- **Given** the playbook, **when** it runs against an
  already-configured host, **then** it converges idempotently with
  zero changes reported.
- **Given** the playbook, **when** secrets in `vault.yml` change,
  **then** re-running rotates them on disk + restarts only the
  containers that consume them.

## Implementation notes

- Layout: `infra/ansible/{inventory,roles,group_vars,site.yml}`.
- Roles to ship: `common` (timezone, unattended-upgrades), `docker`
  (already handled by cloud-init but leave a converge check),
  `kubinate-api`, `postgres`, `temporal`, `cloudflared` (Tunnel
  connector), `backup` (WAL shipping cron).
- Ansible Vault for secrets; the operator runbook documents how to
  pull from 1Password into vault.yml.

## DoD

- [ ] First successful end-to-end deploy of the Sprint 1 binary onto
      the Hetzner VPS via the playbook.
- [ ] Second run with no diff completes in < 30 s and reports zero
      changes.
- [ ] New runbook `docs/runbooks/control-plane-redeploy.md` references
      the playbook command + rollback path.
