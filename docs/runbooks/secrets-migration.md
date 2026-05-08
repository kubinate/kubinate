# Runbook — pgcrypto → Vault secrets migration

**Severity**: SEV-3 planned (cutover) / SEV-2 if invoked as rollback
**Alert source(s)**:
- Planned change window — operator-initiated, **not** alert-driven.
- Rollback trigger: `KubinateSecretStoreUnavailable`
  (Vault unreachable for > 5 min during or after cutover).
**Owner**: Platform.
**Last reviewed**: 2026-05-08

## Summary

One-time migration of every encrypted secret in
[`crates/platform/src/secrets.rs`](../../crates/platform/src/secrets.rs)
from the Phase 0–2 `PgcryptoStore` (envelope-encrypted blobs in
`secrets.encrypted_blob` keyed off `KUBINATE_KEK`) to the Phase 3
`VaultStore` (HashiCorp Vault transit-engine ciphertext under
`secret/data/kubinate/<organization_id>/<secret_id>`). The
authoritative cutover signal is the `KUBINATE_SECRETS_BACKEND` env
var (`pgcrypto` → `vault`) on the API binary; the migration binary
(`kubinate-vault-migrate`, name TBC at Sprint 5 build) is what
actually moves the bytes.

> **This runbook is a stub.** Sprint 4 ticket 02 ships the
> `VaultStore` impl, the parity tests, and this scaffold — the
> migration binary itself is deferred to Sprint 5+ when the dogfood
> Vault deploy exists. Empirical sections below use
> `<<measurement needed>>` markers per the `docs/decisions/`
> convention; do not run a real migration off this stub before
> those sections are filled in by a Sprint 5 dry-run.

## Symptoms

You're running this runbook because either:

1. **Planned cutover.** The dogfood Vault is up, the Sprint 5+
   ticket 02 work has tagged this runbook as ready, and you're
   the operator carrying the change.
2. **Rollback during cutover.** The cutover started, something
   went wrong (Vault unreachable, AppRole token revoked, API
   pods unable to reach `KUBINATE_VAULT_ADDR`), and you need to
   flip back to `pgcrypto` without losing data.
3. **Forward verification.** You ran the cutover yesterday and
   today's checklist pass on the new backend.

If you got paged for a Vault outage that's NOT a cutover-window
incident, you are in the wrong runbook — `observability-proxy-down.md`
covers a Vault-adjacent failure mode but the storage path here is
distinct.

## Severity rubric

- **SEV-2**: cutover started, Vault unreachable from the API for
  > 5 minutes, **and** the rollback flip has not yet completed.
  The API is currently serving 5xx on any endpoint that touches
  a secret (Hetzner-credentials read, kubeconfig fetch).
- **SEV-3**: cutover is mid-flight but pgcrypto rows remain
  populated; an operator can flip
  `KUBINATE_SECRETS_BACKEND=pgcrypto` and the API self-heals
  within the rolling redeploy window.
- **SEV-4**: planned cutover under change-management control, no
  customer impact, you have a maintenance window. Most cutovers.

The whole **point** of keeping the pgcrypto column populated for
≥1 week post-cutover is to keep this incident at SEV-3 instead of
SEV-2.

## Pre-flight (operator-side, before any change)

Run this checklist out-of-band, **before** starting the
maintenance window. Anything in this list that fails aborts the
cutover.

1. **Vault snapshot exists.** A fresh `vault operator raft
   snapshot save` from within the last 1h, stored to the
   off-cluster S3 bucket the Ansible role configures. Confirm
   with `aws s3 ls s3://kubinate-vault-snapshots/ | tail -1`.
2. **AppRole token is short-lived and scoped.** The API's
   AppRole has `update` on `secret/data/kubinate/*` and
   `encrypt`/`decrypt` on the transit key
   `kubinate-tenant-secrets`. TTL ≤ 24h. Verify with
   `vault token lookup -accessor <accessor>`.
3. **Postgres backup is fresh.** A point-in-time recovery target
   ≤ 15 min stale. The PITR window is what backstops the rollback
   if a partial migration leaves the row state inconsistent.
4. **`KUBINATE_KEK` is recoverable.** The current pgcrypto KEK
   is committed to the operator vault (NOT the application Vault).
   If the migration somehow corrupts both backends, the KEK +
   the Postgres PITR is the disaster-recovery path.
5. **`KUBINATE_SECRETS_BACKEND=pgcrypto` is the live env var.**
   Confirm no rolling deploy has it flipped already.
6. **Maintenance banner posted.** Status page + dashboard banner
   noting "secret-store cutover in progress; brief 5xx blips
   possible on credential-touching endpoints."
7. `<<measurement needed>>` — Sprint 5 fills in: expected
   migration runtime per 1k secrets (from dry-run), saturation
   ceiling on the Vault transit endpoint.

## Steps (cutover)

```text
                pgcrypto-only           dual-write              vault-only
   t0 ─────────────────┼─────────────────────┼─────────────────────┼──────►
                       a                     b                     c
   a: migration binary --dry-run + --apply
   b: KUBINATE_SECRETS_BACKEND=vault rolling deploy
   c: pgcrypto column drop (separate sprint)
```

### Step 1 — dry-run

```bash
kubinate-vault-migrate \
  --dry-run \
  --pg-url "$KUBINATE_DATABASE_URL" \
  --vault-addr "$KUBINATE_VAULT_ADDR" \
  --vault-token "$KUBINATE_VAULT_TOKEN" \
  --concurrency 8 \
  | tee migrate-dry-run-$(date -u +%Y%m%dT%H%M%SZ).log
```

Expected output: a per-row line `org=<uuid> secret=<uuid>
algorithm=pgp_sym_v1 → vault_transit_v1 BYTES <n>`. **No** writes
to Vault. **No** writes to Postgres. Failures here are read-side
problems (KEK mismatch, RLS misconfiguration) and abort the
cutover.

### Step 2 — apply

```bash
kubinate-vault-migrate \
  --apply \
  --pg-url "$KUBINATE_DATABASE_URL" \
  --vault-addr "$KUBINATE_VAULT_ADDR" \
  --vault-token "$KUBINATE_VAULT_TOKEN" \
  --concurrency 8 \
  | tee migrate-apply-$(date -u +%Y%m%dT%H%M%SZ).log
```

Idempotency contract: re-running `--apply` after a partial run
must be a no-op for already-migrated rows (the binary checks the
`secrets.algorithm` column and skips rows already on
`vault_transit_v1`). The pgcrypto column stays populated — this
is the rollback safety net.

### Step 3 — flip the env var

Update `KUBINATE_SECRETS_BACKEND=vault` in the API's deployment
config and trigger a rolling restart. The `SecretStore` trait
dispatches at startup based on the env var, so existing pods
keep using pgcrypto until they're cycled. Watch
`kubinate_secret_store_dispatch_total{backend=...}` during the
roll: the `pgcrypto` series should drain to 0 and the `vault`
series take over within the rollout-deadline window.

### Step 4 — verify

- Every `cluster_kubeconfigs` row's `kubeconfig_secret_id`
  resolves through the API: `GET /v1/clusters/:id/kubeconfig`
  returns 200 with a non-empty body. Spot-check 1% of clusters
  (operator-only endpoint added in Sprint 5+).
- Every `hetzner_credentials.secret_ref` row similarly: any
  in-flight provision workflow's "fetch token" activity now
  sources from Vault. The `tracing` field
  `secret_store.backend=vault` confirms.
- `<<measurement needed>>` — Sprint 5 fills in: TSDB query that
  reports the dispatch counter ratio, expected steady-state
  latency on the Vault read path.

## Rollback

The whole reason the pgcrypto column stays populated is to make
this a one-line operation:

```bash
kubectl -n kubinate set env deployment/api KUBINATE_SECRETS_BACKEND=pgcrypto
kubectl -n kubinate rollout restart deployment/api
```

Caveats:

- Any secret **created** during the dual-write window (Step 2 →
  Step 3) lands in Vault only — the migration binary is
  forward-only. If you roll back, you also need to copy those
  back to pgcrypto. Sprint 5's binary will support a
  `--reverse` mode for exactly this case; until then a rollback
  loses any post-cutover writes. Keep the cutover window short
  enough that this matters less than the alternative.
- The KEK has to still be loadable. `KUBINATE_KEK` is preserved
  in the deployment config until the post-cutover-week sprint
  ticks the "drop pgcrypto" row. Don't remove it earlier.
- A rollback **does not** invalidate Vault writes. They sit there
  until a follow-up cleanup. Cost: storage only, but operationally
  ugly — flag for a follow-up.

## Verification (forward, the day after)

24h after cutover, run:

- `<<measurement needed>>` — Sprint 5 fills in: query that confirms
  `secrets.algorithm = 'vault_transit_v1'` for every row that's
  not a deleted-tombstone, and that the pgcrypto column hasn't
  been touched by writes (the existing audit chain is what proves
  this).
- The `kubinate_secret_store_dispatch_total{backend="pgcrypto"}`
  series should be 0 (or very low, only for explicitly retained
  rollback-test paths).
- `<<measurement needed>>` — Sprint 5 fills in: end-of-day Vault
  audit-log scan looking for unexpected `read` events outside the
  API's AppRole identity.

If any verification step fails, the rollback section is one
flip away. **Do not** drop the pgcrypto column until at least
seven calendar days of clean dispatch metrics. That's the
threshold the parent ticket commits to and the only honest way
to commit to "Vault is the authoritative store."

## Communications

- **During cutover:** status page banner ("secret store
  migration in progress; brief 5xx blips possible on
  credential-touching endpoints"). Post in
  `#kubinate-changes`.
- **On rollback:** explicit incident channel, even if no
  customer impact. The whole point of practicing the rollback
  is making sure the runbook is right.

## Postmortem

Required if the cutover goes SEV-2 (rollback fired, customer
impact). Stub draft within 48h, published within 5 business
days.

## Related

- [ADR-0007](../adr/0007-secret-management.md) — secret
  management strategy. The "Long term" section is what this
  runbook is the operational arm of.
- [`docs/backlog/sprint-4/02-vault-migration.md`](../backlog/sprint-4/02-vault-migration.md)
  — parent ticket.
- [`crates/platform/src/secrets.rs`](../../crates/platform/src/secrets.rs)
  — the `SecretStore` trait + `PgcryptoStore` / `VaultStore` impls.
- [`docs/runbooks/credential-rotation.md`](./credential-rotation.md)
  — sibling runbook for tenant-token rotation; touches the same
  storage layer but does not move bytes between backends.
- [`docs/runbooks/observability-proxy-down.md`](./observability-proxy-down.md)
  — runbook for Vault-adjacent platform outages outside a
  cutover window.
