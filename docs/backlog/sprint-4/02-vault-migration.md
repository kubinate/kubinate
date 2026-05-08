# Migrate tenant secrets from pgcrypto to Vault

**Labels**: `area/platform`, `area/security`, `sprint-4`
**Epic**: ADR-0007 §Long term
**Size**: 13 (high-confidence; the migration is mechanically large
even though the new code is small)

## Context

[ADR-0007 §Long term](../../adr/0007-secret-management.md) commits
us to **HashiCorp Vault** (self-hosted inside the dogfooded k3s
cluster) as the authoritative secret store from Phase 3 onward.
The Phase 0–2 implementation lives at
[`crates/platform/src/secrets.rs`](../../../crates/platform/src/secrets.rs)
behind a `SecretStore` trait + a `PgcryptoStore` impl that uses a
single env-loaded `KUBINATE_KEK`. The trait is the seam this
ticket exploits — domain code stays unchanged; only the impl
swaps.

The roadmap names "Vault replaces pgcrypto for tenant secrets" as
a Phase 3 exit gate (`docs/roadmap.md`).

## When this opens

Concretely: **before the production observability proxy ticket
(04)** and **before the agent reverse-tunnel ticket (03) reaches
production**, because both want short-lived credentials issued by
Vault rather than baked-in pgcrypto-encrypted blobs.

Soft prerequisite: the dogfood cluster has to exist. Vault on the
single-VPS deploy is doable but defeats the point of the
migration (no separation between control-plane host and secret
store). Land this **after** the dogfood-cluster Sprint 4 ticket
opens it up.

## Sprint 4 partial scope (~6 of 13 pts)

The Phase 3 dogfood cluster does not yet exist (Sprint 5+ work
per `docs/decisions/sprint-4-dogfood-migration.md`). Sprint 4
ships the parts of this ticket that don't depend on a running
Vault deploy:

- [x in Sprint 4] `VaultStore` impl behind the `SecretStore`
  trait, with parity tests against `PgcryptoStore` (every
  existing test passes against both backends).
- [x in Sprint 4] Ansible role under `infra/ansible/roles/vault/`
  that *will* deploy Vault on the dogfood cluster when Sprint 5
  brings it up. Tasks: render config + compose fragment, pull
  image, gated init (one-shot, sentinel-flagged), post-unseal
  bootstrap (transit + audit + AppRole policy + AppRole role
  config). The role is **not** invoked from `site.yml` today
  (sprint 5+ adds the playbook entry). Molecule scenario lands
  alongside the live `site.yml` invocation in Sprint 5+.
- [x in Sprint 4] `KUBINATE_SECRETS_BACKEND` env-var added with
  default `pgcrypto`; CI's existing `KUBINATE_KEK` row stays.

Deferred to Sprint 5+ (when the dogfood cluster exists):

- [ ] Migration binary (`kubinate-vault-migrate`) — needs a
      live Vault to integration-test against.
- [ ] Production cutover — env-var flip + KEK retirement.
- [ ] Dropping the pgcrypto extension — separate sprint after
      a clean week on the new backend.

The DoD list below applies to the **full** ticket; the Sprint
4 close only ticks the rows above. The migration runbook stub
should land in Sprint 4 too (so the cutover-plan section has
somewhere to live as Sprint 5 fills it in).

## Acceptance criteria

- **Given** the dogfood k3s cluster is running, **when** Vault is
  deployed via `infra/ansible/` (new role) or via a Helm chart on
  the dogfooded cluster, **then** the Vault transit + database
  secrets engines are enabled and audit-logging is enabled to a
  k3s PVC.
- **Given** `crates/platform/src/secrets.rs` ships a `VaultStore`
  impl of the `SecretStore` trait, **when** the API is configured
  with `KUBINATE_SECRETS_BACKEND=vault`, **then** every
  `SecretStore::store / get / delete` call routes through Vault's
  transit engine. The pgcrypto path stays compiled in (gated by
  `KUBINATE_SECRETS_BACKEND=pgcrypto`) until the migration lands
  in production.
- **Given** the migration tool runs against an existing Phase-2
  database, **when** it completes, **then** every
  `hetzner_credentials.secret_ref` row + every
  `cluster_kubeconfigs` blob references a Vault path; the
  `KUBINATE_KEK` env var can be removed; `pgcrypto` extension
  dropped (separate migration in a follow-up to keep the
  rollback story clean).
- **Given** the Postgres connection, **when** the API requests a
  fresh credential via Vault's database secrets engine, **then**
  the credential is short-lived (< 1h TTL) and rotated
  automatically.

## Implementation notes

- The `SecretStore` trait already returns/takes a `SecretRef`
  with a `(scope, name)` pair. Vault path mapping is a simple
  `secret/data/<scope>/<name>`; no schema change needed.
- Use the official `vaultrs` crate for the client; pin the
  version in workspace deps.
- Authentication: AppRole for the API workload (provisioned via
  Ansible), JWT for the agent (later, ticket 03).
- The migration tool is a one-shot `kubinate-vault-migrate` binary
  in `crates/e2e/` (or a new `crates/platform/bin/migrate.rs`).
  Reads every encrypted blob with the old `PgcryptoStore`,
  writes it under the same `SecretRef` to the new `VaultStore`,
  flags the row.
- **Rollback story**: keep the pgcrypto column (don't drop it in
  the same sprint). The `SecretStore` trait dispatches to
  whichever backend `KUBINATE_SECRETS_BACKEND` selects; an
  emergency rollback is one env-var flip + a redeploy. Drop the
  pgcrypto column in a separate sprint after a clean week.

## Migration runbook (DoD row)

Author `docs/runbooks/secrets-migration.md` covering:
- Pre-flight: a fresh Vault snapshot must exist + the
  application's AppRole token has `update` permission on
  `secret/data/*`.
- Steps: deploy the `vault` backend; run the migration binary
  with `--dry-run`; review the migration log; run for real;
  flip `KUBINATE_SECRETS_BACKEND`.
- Verification: every cluster's `kubeconfig` retrievable via
  the API on the new backend; no row in
  `hetzner_credentials.encrypted_token` non-null *and* no
  Vault path recorded.
- Rollback: flip the env var back, the old encrypted columns
  are still populated.

## DoD

- [ ] `VaultStore` impl with parity tests against the
      `PgcryptoStore` (every public test in
      `crates/platform/src/secrets.rs::tests` passes against
      both backends via the trait).
- [ ] Migration binary + dry-run mode + idempotency (re-running
      after a partial run is a no-op).
- [ ] `infra/ansible/` (or k3s manifest) deploys Vault with
      audit logging enabled; the API role is provisioned and
      its AppRole token rotated.
- [ ] Runbook merged: `docs/runbooks/secrets-migration.md` +
      a row in `docs/runbooks/README.md`.
- [ ] ADR-0007 is **not** superseded — the long-term section
      of the original ADR is what this ticket implements; the
      `pgcrypto retired` row is just an addition. The ADR's
      header gets a "Long-term plan implemented in Sprint 4
      ticket 02" footnote so future readers see the link.
- [ ] `KUBINATE_KEK` removed from `.env.example` and from CI
      env. CI's `KUBINATE_KEK=...` row drops, replaced by
      `KUBINATE_SECRETS_BACKEND=pgcrypto` for the test path
      (no real Vault in CI).
- [ ] Threat-model v2 row notes the secret-store boundary
      change (this is one of v2's open requirements per
      `docs/security/threat-model.md` footer).

## What this ticket deliberately does **not** do

- **Drop the `pgcrypto` extension or the encrypted columns.**
  That's a follow-up after a clean week on the new backend.
- **Migrate session tokens, audit chain HMAC keys, or any
  non-tenant secret.** Those are scoped under the same trait
  and can be migrated in a follow-up; this ticket's blast
  radius is "tenant data only."
- **Roll out short-lived Hetzner API tokens.** Vault dynamic
  secrets for Hetzner is an aspirational item in ADR-0007 —
  Hetzner's token model doesn't support short TTLs natively.
  Track separately.
