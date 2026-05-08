# Control-plane configuration (Ansible)

Pairs with [`infra/terraform/`](../terraform). Terraform provisions
the Hetzner VPS; this playbook turns the freshly cloud-init'd host
into a running Kubinate control plane: Postgres + Temporal + the API
binary in Docker Compose, behind a Cloudflare Tunnel, with WAL
shipping to Hetzner Object Storage.

## What this playbook does

| Role             | Purpose                                                                |
|------------------|------------------------------------------------------------------------|
| `common`         | Timezone, unattended-upgrades, base packages.                          |
| `docker`         | Converge check (cloud-init installed Docker; this verifies it stayed). |
| `postgres`       | Mounts the Hetzner data volume, prepares the multi-db init script.    |
| `temporal`       | Stages temporal data dir.                                              |
| `kubinate_api`   | Renders `docker-compose.yml` + the `api.env` (mode 0400) + brings the stack up. |
| `cloudflared`    | systemd unit for the tunnel connector — no public 80/443 ingress.     |
| `backup`         | Cron entry for `pg_dumpall` → Hetzner S3 every 15 minutes.            |

## Prerequisites

- Operator workstation with `ansible-core >= 2.16` and the
  `community.docker`, `community.general`, `ansible.posix` collections
  (`ansible-galaxy collection install community.docker community.general ansible.posix`).
- A Terraform-provisioned Hetzner VPS reachable on SSH from the
  operator. The `kubinate` user is created by cloud-init.
- A Cloudflare Tunnel **named tunnel** already created via
  `cloudflared tunnel create kubinate-prod`; the resulting token goes
  into `vault.yml`.
- A Hetzner Object Storage bucket for backups + an S3-compatible
  access/secret pair.
- A Stripe webhook endpoint pointed at `/v1/billing/webhook` (Sprint 2
  ticket 08); the signing secret goes into `vault.yml`.

## First-time setup

```bash
cd infra/ansible

# 1. Inventory
cp inventory.example.ini inventory.ini
$EDITOR inventory.ini

# 2. Secrets — pull from the Kubinate Production Secrets vault in 1Password
cp group_vars/all/vault.yml.example group_vars/all/vault.yml
$EDITOR group_vars/all/vault.yml
ansible-vault encrypt group_vars/all/vault.yml

# 3. Apply
ansible-playbook -i inventory.ini site.yml --ask-vault-pass
```

Expected first-run wallclock: 2–4 minutes (image pulls dominate).

## Re-runs are idempotent

Re-running the playbook against a healthy host should report **0
changed** in under 30 seconds (DoD row 2 from Sprint 2 ticket 05). If
you see drift on a "should be no-op" run:

- A new Docker image tag landed in `group_vars/all/main.yml` →
  expected.
- A vault secret rotated → expected; the affected handler restarts
  only the consuming containers, not the whole stack.
- Anything else → file an SRE ticket and treat the host as suspect.

## Rotating a secret

1. `ansible-vault edit group_vars/all/vault.yml` → save.
2. `ansible-playbook -i inventory.ini site.yml --tags handlers,kubinate_api --ask-vault-pass`
   restarts only the api container. Postgres / Temporal restarts
   require a deeper rotation — see
   [`docs/runbooks/control-plane-redeploy.md`](../../docs/runbooks/control-plane-redeploy.md).

## Rolling back to a previous binary

The image tag in `group_vars/all/main.yml` is the rollback handle.
Edit, commit, re-apply with `--tags kubinate_api`. The compose stack
pulls the older image, restarts only the api container, and reports
back via the `/healthz` retry loop in the role's tasks. If the
previous run had an in-flight migration, see the redeploy runbook.

## What this playbook deliberately does **not** do

- **Provision the VPS.** That's `infra/terraform/`.
- **Ship the api binary.** The image is built + pushed via CI; this
  playbook only pulls a tag.
- **Provision Cloudflare DNS / WAF rules.** Out-of-band via the
  Cloudflare provider in a future ticket.
- **Manage k3s on user clusters.** That's the provisioning workflow
  (Sprint 1 ticket 03).
