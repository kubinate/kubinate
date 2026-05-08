# Finish the Vault migration deferred from Sprint 4

**Labels**: `area/platform`, `area/security`, `sprint-5`
**Epic**: ADR-0007 §Long term
**Size**: 5 (medium-confidence; binary is mechanically large
but the wire shape is fixed by Sprint 4's `VaultStore` impl)

## Context

Sprint 4 ticket 02 shipped the partial scope (`VaultStore`
impl behind the `SecretStore` trait, parity tests against
`PgcryptoStore`, Ansible role, runbook stub at
[`docs/runbooks/secrets-migration.md`](../../runbooks/secrets-migration.md))
but explicitly deferred the migration binary, the production
cutover, and the pgcrypto-extension drop because each needs a
live Vault to integration-test against. Sprint 5's #02 +
#03 stand up the dogfood cluster and migrate the API onto it;
that unlocks the deferred work.

This ticket closes Sprint 4 ticket 02's remaining DoD rows.
ADR-0007's "Long term" section is what's actually being
implemented here — the ADR isn't superseded; this is the
operational arm landing.

## Acceptance criteria

- **Given** the dogfood cluster is up (#02) and `kubinate-api`
  has migrated onto it (#03), **when** the operator runs the
  Sprint 4 Ansible Vault role from `infra/ansible/roles/vault/`,
  **then** Vault is deployed on the cluster with the transit
  + database secrets engines enabled, audit-logging enabled
  to a k3s PVC, and the API's AppRole token provisioned with
  the correct policy (`update` on `secret/data/kubinate/*`,
  `encrypt`/`decrypt` on the `kubinate-tenant-secrets` transit
  key).
- **Given** Vault is deployed, **when** `kubinate-vault-migrate`
  runs with `--dry-run`, **then** every encrypted blob in
  `secrets.encrypted_blob` (algorithm = `pgp_sym_v1`) is read
  via `PgcryptoStore::get` and reported as a candidate for
  migration; the binary writes nothing to Vault and writes
  nothing to Postgres.
- **Given** the dry-run output is reviewed, **when** the
  binary runs with `--apply`, **then** every candidate row
  has its plaintext re-encrypted via the Vault transit
  engine and written under `secret/data/kubinate/<organization_id>/<secret_id>`;
  the row's `algorithm` column flips to `vault_transit_v1`.
  The pgcrypto blob column stays populated for the rollback
  window (separate sprint drops it).
- **Given** every row is migrated, **when** the operator
  flips `KUBINATE_SECRETS_BACKEND=vault` and the API rolls,
  **then** every credential-touching endpoint reads from
  Vault and the `kubinate_secret_store_dispatch_total{backend="pgcrypto"}`
  series drains to 0 within the rollout deadline.
- **Given** the production cutover completes, **when** the
  operator inspects CI's env, **then** `KUBINATE_KEK` has
  been dropped from `.github/workflows/ci.yml` and replaced
  with `KUBINATE_SECRETS_BACKEND=pgcrypto` for the test path
  (no real Vault in CI; tests still need a backend).

## Implementation notes

- **Binary location.** `crates/platform/bin/migrate.rs` (the
  alternative — a separate `crates/vault-migrate/` — is
  rejected; the binary is small enough to share the
  `kubinate-platform` deps without a new crate). Workspace
  member registration in the root `Cargo.toml`.
- **Idempotency.** The binary skips rows already on
  `vault_transit_v1` — the column is the source of truth, not
  the dry-run state. Re-running `--apply` after a partial
  failure picks up where the last run stopped without
  re-writing already-migrated rows.
- **Concurrency.** Default `--concurrency 8` (configurable
  flag). The Vault transit engine handles per-key encrypt
  calls in parallel without coordination; Postgres writes
  use `SELECT … FOR UPDATE SKIP LOCKED` so two concurrent
  workers can't both try to migrate the same row.
- **Failure mode.** On any per-row failure (transit timeout,
  Postgres deadlock, KEK mismatch on the source read), the
  binary logs the row id + error + decision and continues.
  The summary at end of run lists every skipped row with the
  reason. **Do not** halt on first failure — a single corrupt
  row should not block the rest of the migration.
- **Dual-write window.** Between `--apply` completing and
  `KUBINATE_SECRETS_BACKEND=vault` taking effect on every
  pod, new secret writes go to pgcrypto (the live backend)
  while reads can come from either (the trait dispatches by
  `algorithm` column). The runbook section names this window
  explicitly; keep it under 30 minutes — long enough to roll
  the deployment, short enough that the "rollback loses
  post-cutover writes" caveat in the runbook stays bounded.

## Migration runbook (DoD row)

The runbook stub at
[`docs/runbooks/secrets-migration.md`](../../runbooks/secrets-migration.md)
lands every empirical section as part of this ticket — every
`<<measurement needed>>` marker resolves with the actual
numbers from the production cutover. Specifically:

- **Pre-flight measurement**: expected runtime per 1k secrets
  from the dry-run.
- **Forward-verification queries**: the actual TSDB query +
  expected steady-state latency on the Vault read path.
- **Vault audit-log scan query**: the exact log-query DSL the
  on-call uses 24h post-cutover.

## DoD

- [ ] Migration binary `crates/platform/bin/migrate.rs` lands
      with `--dry-run` + `--apply` modes, idempotency, and
      structured per-row logging.
- [ ] `infra/ansible/site.yml` adds the `vault` role to the
      dogfood-cluster playbook; Molecule scenario passes.
- [ ] `kubinate_secret_store_dispatch_total` metric is added
      to `crates/platform/src/secrets.rs` so the rollout's
      drain-down can be observed.
- [ ] Production cutover executed; runbook empirical markers
      resolved.
- [ ] `KUBINATE_KEK` removed from `.env.example` and CI; the
      docker-compose dev path keeps it for offline dev.
- [ ] ADR-0007 footnote at the head of the file (added in
      Sprint 4 #4) updated: "Long-term plan implemented in
      Sprint 4 ticket 02 (partial) + Sprint 5 ticket 04
      (production cutover)."
- [ ] Threat-model v2 cut at sprint-close picks up the
      secret-store boundary change (already pre-noted in the
      v2-open-requirements section that Sprint 4 #4 added).

## What this ticket deliberately does **not** do

- **Drop the pgcrypto extension or the encrypted columns.**
  Separate sprint after one clean week on the new backend —
  same scope-rule Sprint 4's parent ticket commits to.
- **Migrate session tokens, audit chain HMAC keys, or any
  non-tenant secret.** Same trait, different scope; track in
  a follow-up.
- **Roll out short-lived Hetzner API tokens.** Vault dynamic
  secrets for Hetzner is aspirational in ADR-0007; Hetzner's
  token model doesn't support short TTLs natively. Track
  separately.

## References

- [`docs/backlog/sprint-4/02-vault-migration.md`](../sprint-4/02-vault-migration.md)
  — Sprint 4 parent ticket; the deferred-to-Sprint-5+ section
  is exactly what this ticket closes.
- [`docs/runbooks/secrets-migration.md`](../../runbooks/secrets-migration.md)
  — the runbook stub.
- [ADR-0007](../../adr/0007-secret-management.md) — secret
  management decision; this ticket is the long-term arm.
- [`crates/platform/src/secrets.rs`](../../../crates/platform/src/secrets.rs)
  — `SecretStore` trait + both backends already shipped.
