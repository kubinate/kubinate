# `vault` Ansible role

Deploys HashiCorp Vault as a systemd-managed Docker container,
configures the transit + audit secrets engines, and provisions an
AppRole for the `kubinate-api` workload.

## Status: **not yet invoked from `site.yml`**

This role exists in the parking lot because Sprint 4 ticket 02
([`docs/backlog/sprint-4/02-vault-migration.md`](../../../../docs/backlog/sprint-4/02-vault-migration.md))
ships the application-side `VaultStore` impl, but the migration
to **run** Vault in production rides on the dogfood-cluster
migration ([sprint-4 spike doc](../../../../docs/decisions/sprint-4-dogfood-migration.md)).

When Sprint 5+ stands up the dogfood cluster:

1. Add `kubinate-vault` to the `control_plane` group in
   `inventory.ini`.
2. Add `- role: vault` to `site.yml` after the `docker` role.
3. Run the role once to bootstrap. Subsequent runs are
   idempotent.
4. Run the migration binary (Sprint 5+ ticket — see partial scope)
   to re-key existing pgcrypto-encrypted secrets.
5. Flip `KUBINATE__SECRETS_BACKEND=vault` in
   `roles/kubinate_api/templates/api.env.j2`.

## What this role does

- Pulls the `hashicorp/vault:<version>` Docker image (version is
  pinned in `defaults/main.yml`; default `1.18.2`).
- Renders a minimal `config.hcl` enabling the file-storage
  backend on the existing data mount + the listener on
  `127.0.0.1:8200` (mTLS termination is at the Cloudflare Tunnel
  / k3s ingress — Vault itself listens loopback only).
- Renders a docker-compose fragment under
  `{{ kubinate_data_mount }}/compose/vault.yml` and merges into
  the parent compose project.
- Initialises Vault on first run (writes the unseal keys + root
  token to a file Ansible-vault-encrypted under
  `group_vars/all/vault_init.yml`; an operator unseals once,
  then auto-unseal config takes over).
- Enables the transit secrets engine.
- Enables the file audit device.
- Creates the `kubinate-api` policy and AppRole; emits the
  AppRole role-id + secret-id into a vault-encrypted file the
  `kubinate_api` role reads.

## What this role deliberately does **not** do

- **Initial unsealing of the root token.** That's an operator
  action — running it in Ansible would mean the unseal keys
  pass through Ansible Vault, which weakens the threat model
  (one compromise → both unseal keys *and* their encrypted
  storage). Ship the role, document the manual step.
- **Cluster-mode Vault.** This is a single-node deploy fitting
  the Phase 2 single-VPS topology. HA Vault on the dogfood
  cluster is its own ticket once we hit operational pressure
  on the single node.
- **Auto-rotate of the AppRole secret-id.** Phase 4+ ticket;
  for now the secret-id is long-lived (90 day TTL) and
  documented as such in the runbook stub
  `docs/runbooks/secrets-migration.md` (forthcoming).

## Variables

See [`defaults/main.yml`](./defaults/main.yml). The two
operationally-important ones:

- `vault_version` — pinned version of the upstream image.
- `vault_data_dir` — derived from `kubinate_data_mount`; the
  file-storage backend lives here. **Backups must include this
  directory** or a Vault restore loses every secret.

## Idempotency

The role is idempotent **after** the first-run init. The init
step is gated on the absence of
`{{ vault_data_dir }}/.kubinate-vault-initialised`; once present,
`vault operator init` is skipped. Manual deletion of that
sentinel + a re-run of the role is the supported re-init path.

## Testing

CI runs `molecule test` against this role. Manual runs:

```bash
cd infra/ansible/roles/vault
molecule test
```

The `molecule.yml` scenario provisions a Docker container,
applies the role, and asserts the transit engine is enabled +
the AppRole responds to a login. Sprint-4 partial scope ships
the role + scenario; the live-deploy invocation waits for
Sprint 5+.
