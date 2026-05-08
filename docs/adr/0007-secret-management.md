# ADR-0007: Secret management

- **Status**: Accepted
- **Date**: 2026-04-24
- **Deciders**: Founding team
- **Tags**: security, secrets, platform

> **Long-term plan implemented in [Sprint 4 ticket 02](../backlog/sprint-4/02-vault-migration.md).**
> Sprint 4 ships the partial scope (`VaultStore` impl behind the
> `SecretStore` trait, parity tests, Ansible role for the dogfood
> Vault deploy, runbook stub at
> [`docs/runbooks/secrets-migration.md`](../runbooks/secrets-migration.md)).
> The migration binary, production cutover, and pgcrypto-extension
> drop are explicitly Sprint 5+ work, gated on the dogfood cluster
> existing. This ADR is **not** superseded — the Long term section
> below is what the ticket implements.

## Context

Kubinate holds three categories of secret material:

1. **Customer secrets**: Hetzner API tokens, user-supplied OIDC IdP
   credentials (enterprise), cluster kubeconfigs.
2. **Platform internal secrets**: Postgres credentials, JWT signing
   keys, mTLS CA and issued certs, webhook signing secrets.
3. **Encryption keys**: DEKs (data encryption keys) and the KEK (key
   encryption key) used for envelope encryption of the above.

The worst-case outcome of mishandling category 1 is a compromise of
customer infrastructure (spin up crypto miners on the customer's
account). The worst-case for category 2 is a platform compromise. We
treat both as tier-1 risks.

## Decision

We adopt a **two-phase secret management strategy**.

**Short term (Phase 0 through Phase 2):**

- All secrets encrypted at rest in Postgres using **envelope
  encryption**:
  - Per-record DEK generated and wrapped by a single KEK.
  - KEK loaded from an environment variable on process start, sourced
    from the hosting VPS's encrypted filesystem (LUKS) and a deployment
    secret manager we operate by hand (a 1Password vault).
  - `pgcrypto` used for the primitive operations; we never handle
    unencrypted secret bytes outside of request scope in memory.
- KEK rotation is a manual runbook action in the short term.
- No secrets in environment variables at rest on disk; the systemd
  unit file pulls from a root-owned file with mode `0400`.

**Long term (Phase 3):**

- Migrate to **HashiCorp Vault** (self-hosted inside the dogfooded k3s
  cluster) as the authoritative store for categories 1, 2, and 3.
- Use Vault's **transit** secrets engine for envelope encryption
  (application never sees plaintext KEKs).
- Use Vault's **database** secrets engine to issue short-lived Postgres
  credentials to the application.
- Where Hetzner supports it, prefer short-lived project-scoped API
  tokens with automatic rotation via the Vault dynamic secrets pattern.
  (Hetzner's API token model is project-scoped, long-lived — we
  simulate rotation by periodic re-issuance coordinated with the user's
  UI flow.)
- Use Vault auth methods (JWT / k8s service account) for workloads to
  obtain secrets; no static Vault tokens on disk.

Regardless of phase: Hetzner API tokens supplied by users are encrypted
immediately on ingress and never logged. The raw token is present in
memory only for the duration of the specific API call to Hetzner. The
Hetzner client wrapper in the `integrations` crate enforces this by
accepting only a `SecretRef` handle and resolving it at call time.

## Alternatives considered

- **AWS KMS / GCP KMS / Azure Key Vault**: rejected as violating the
  hyperscaler-avoidance posture (ADR-0004). Also complicates on-prem or
  secondary-provider deployments later.
- **SOPS with age / PGP keys checked into git**: good for declarative
  infrastructure config, but wrong for dynamic per-tenant customer
  secrets.
- **Skip Vault, keep pgcrypto envelope encryption long term**: rejected
  because we lose dynamic credential issuance, centralized audit, and
  the cleaner separation of duties that Vault provides.

## Consequences

**We accept:**

- The short-term solution has single-person-risk: loss of the KEK
  renders all encrypted secrets unrecoverable. The runbook covers
  emergency recovery via an encrypted KEK backup stored offline.
- Operating Vault in Phase 3 is non-trivial. We plan the migration as a
  dedicated sprint and treat its operational burden as a first-class
  Phase 3 exit criterion.
- Migrating in place (Phase 2 → 3) requires re-encrypting existing
  ciphertext under Vault-managed keys. We schedule this during the
  broader Phase 3 migration.

**We assume (bets):**

- The pgcrypto-based envelope scheme is sufficient for the 6-month
  window. We validate this with a threat-model review in Phase 2.
- Vault's OSS edition remains viable. If licensing becomes an issue,
  OpenBao (a community fork) is a drop-in replacement.

**Positive follow-ons:**

- Vault's audit log gives us a separate source of truth for
  secret-access events, independent of our own audit log.
- Short-lived DB creds dramatically reduce the blast radius of any
  future credential leak.

## Revisit trigger

- A security incident involves secret material, **or**
- Hetzner introduces short-lived API token issuance, at which point we
  re-evaluate which category-1 secrets we need to store at all, **or**
- Vault's licensing or health regresses materially, in which case we
  migrate to OpenBao without re-opening the envelope-encryption model.

## References

- [HashiCorp Vault documentation](https://developer.hashicorp.com/vault/docs)
- [OpenBao](https://openbao.org) — Vault fork, contingency plan.
- [pgcrypto](https://www.postgresql.org/docs/current/pgcrypto.html)
- Envelope encryption patterns — NIST SP 800-57.
