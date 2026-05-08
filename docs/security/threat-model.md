# Threat Model v0

**Status**: living document. v0 is the Phase-0 baseline; revised at
Phase 2 exit and before the Phase 4 pen test.

## Scope

This threat model covers the Kubinate system as described in
[`../architecture/containers.md`](../architecture/containers.md). It
identifies threats by applying [STRIDE](https://learn.microsoft.com/en-us/azure/security/develop/threat-modeling-tool-threats)
to each of the top-5 data flows — flows that are either on the critical
path of the core product experience or whose failure would be an
existential incident for the business.

STRIDE legend:

- **S**poofing — pretending to be someone else.
- **T**ampering — modifying data in transit or at rest.
- **R**epudiation — denying an action was taken.
- **I**nformation disclosure — unintended data leakage.
- **D**enial of service — making the system unavailable.
- **E**levation of privilege — gaining rights beyond those granted.

## Method

For each flow we list the actors, the data in motion, the trust
boundaries crossed, and the STRIDE threats we consider *in scope* for
v0. "Out of scope" threats are recorded but deferred — they are real,
just not in the v0 remediation plan.

Each threat has a control. A threat without a control is a gap and is
tracked as a backlog item.

---

## Flow 1 — User signup and login via OIDC

**Actors**: end user (browser), Kubinate API, external OIDC IdP
(GitHub / Google / Microsoft).
**Data**: OAuth authorization codes, ID tokens, session cookies, user
email and display name.
**Boundaries crossed**: browser ↔ Cloudflare ↔ API ↔ IdP.

### STRIDE

| Threat | Description | Control |
|---|---|---|
| **S** | Attacker forges an IdP response to log in as another user. | Verify ID token signature against IdP's JWKS; validate `iss`, `aud`, `exp`, `nonce`. Use PKCE on the code flow. |
| **S** | Session cookie theft via XSS. | HttpOnly + Secure + SameSite=Strict cookie. Strict CSP. SvelteKit auto-escaping + no `@html` with user input. Subresource integrity on third-party scripts. |
| **T** | OIDC state parameter tampered mid-flow. | Cryptographically random `state`, stored server-side, single-use, 10-minute TTL. |
| **R** | User denies performing a privileged action. | Audit log entry for every privileged action, including login and session issuance. Hash-chained for tamper-evidence. |
| **I** | IdP leaks more profile data than needed. | Request minimum scopes (`openid email profile` only). Store only what we use. |
| **D** | IdP outage breaks login. | Support multiple IdPs; a user with linked GitHub + Google can fall back. Document in runbook. |
| **E** | Session fixation — attacker sets victim's session ID before login. | Rotate session ID on successful authentication. Bind session to user agent and IP prefix for anomaly alerting. |

### Open gaps

- MFA (WebAuthn) not yet enforced for Admin/Owner roles — Phase 3
  scope.

---

## Flow 2 — User supplies a Hetzner API token

**Actors**: authenticated user, Kubinate API, Postgres (storage),
pgcrypto (Phase 0–2) or Vault (Phase 3+) for envelope encryption.
**Data**: raw Hetzner API token (extremely sensitive — grants full
control of the user's Hetzner project; ability to spin up arbitrary VPS
and incur costs).

### STRIDE

| Threat | Description | Control |
|---|---|---|
| **S** | Attacker impersonates user to submit or overwrite a token. | Authenticated session + CSRF double-submit cookie. Idempotency key on write. |
| **T** | Token ciphertext modified in DB. | Envelope encryption with authenticated mode (AES-GCM); decryption fails on tamper. |
| **T** | Token swapped in transit between API and Hetzner. | TLS 1.3, certificate validation, Hetzner's hostname pinned. |
| **R** | User denies having submitted a malicious token. | Audit log entry with IP, UA, request ID on token creation/rotation. |
| **I** | Token leaked via logs. | `SecretString` wrapper with `Debug` redaction; log filter allowlist; PII redaction middleware; CI grep for accidental `{:?}` on secret types. |
| **I** | Token leaked via error responses. | Errors from Hetzner client are mapped to generic `PlatformError::Internal` before returning. |
| **I** | Plaintext token resident in memory longer than necessary. | `SecretRef` handle pattern: plaintext materialized only inside `integrations::hetzner` call scope, zeroized on drop. |
| **D** | Attacker floods token validation to lock out the user. | Rate limit per-user and per-IP on submit endpoint. |
| **E** | Compromised application process exfiltrates all tokens. | Defense in depth: Vault with short-lived DB creds (Phase 3+); KEK not in memory long-term; audit log for bulk token access patterns. |

### Open gaps

- Phase 0–2 keeps the KEK in an env var. Rotation is manual. Addressed
  by Vault migration (ADR-0007) at Phase 3.
- We do not yet detect or alert on anomalous use of a stored token
  (e.g. provisioning in a region the user has never used). Backlog.

### Incident response

- Token compromise / customer-requested rotation:
  [`docs/runbooks/credential-rotation.md`](../runbooks/credential-rotation.md).

### Audit coverage

- `hetzner_credentials` mutations: per-tenant hash chain via the
  Sprint 1 ticket 09 trigger.
- `kubeconfig.retrieved` events: explicit-append per Sprint 1
  ticket 04 (`audit_log_append_explicit`).
- `organizations.plan` / `organizations.stripe_customer_id`
  changes: column-aware trigger from Sprint 3 ticket 06; emits a
  `organizations.updated` audit row with old/new values in
  `metadata`. Non-billing column updates are intentionally not
  audited.

---

## Flow 3 — Cluster provisioning workflow

**Actors**: Cluster Orchestrator (Temporal worker), Hetzner API, target
user's Hetzner project, Postgres, shadow state table.
**Data**: cloud-init user-data (may contain bootstrap tokens),
ephemeral SSH keys, kubeconfig (VERY sensitive — full cluster-admin).
**Boundaries crossed**: control plane → Hetzner; control plane → newly
created VPS (SSH) during bootstrap only.

### STRIDE

| Threat | Description | Control |
|---|---|---|
| **S** | Attacker-in-the-middle during SSH bootstrap intercepts k3s join token. | Use known-host fingerprints collected from Hetzner metadata after boot; refuse to connect to hosts without a verified fingerprint. |
| **S** | Workflow replay — attacker replays a provisioning activity against a different tenant's project. | Temporal activity input includes a tenant-bound correlation ID; Hetzner client refuses operations whose target project does not match the workflow's tenant claim. |
| **T** | Provisioned node receives a tampered cloud-init payload, allowing persistence. | Cloud-init is generated server-side, signed, and fetched by the node over TLS with certificate pinning. Deterministic workflow versioning prevents silent behavior change. |
| **R** | Orchestrator side effects lose their link to the initiating user. | Every workflow carries an `actor` attribute propagated to audit log activities. |
| **I** | Kubeconfig captured from logs or filesystem. | Never logged; encrypted on first byte; stored via envelope encryption. |
| **I** | Hetzner API error messages leak into user-visible error. | Error mapping layer in `integrations::hetzner`; only sanitized, classified errors reach the user. |
| **D** | Rapid creation/destruction exhausts Hetzner API quota. | Per-tenant provisioning concurrency limit (max 3 in-flight in v1); exponential backoff on 429. |
| **E** | An attacker who compromises one Hetzner project escalates to the control plane. | Control plane has no persistent credentials on user nodes; bootstrap SSH key is revoked before workflow completion. Agent-initiated reverse tunnel after bootstrap. |

### Open gaps

- Reverse-tunnel agent design (brief §12) not yet finalized. Until it
  is, the period between bootstrap SSH revocation and agent connection
  is a gap. Tracked as P1 spike.

---

## Flow 4 — Cross-tenant query through the API

**Actors**: authenticated user, API, Postgres.
**Data**: any tenant-scoped data (clusters, members, audit).
**Boundaries crossed**: API ↔ Postgres.

This flow is the "one bug, one incident" risk. It is the flow RLS
defends in depth.

### STRIDE

| Threat | Description | Control |
|---|---|---|
| **S** | User submits a request body naming another tenant's resource ID. | Service layer resolves ID within the caller's tenant scope; a not-found from that scope returns 404 (not 403, to avoid confirming the existence of the target). |
| **T** | (Internal) developer writes a query that forgets the tenant filter. | RLS catches it at the DB layer. Integration test matrix deliberately exercises "forgot the filter" cases against a seeded two-tenant DB. |
| **R** | (Internal) engineer uses bypass role and later denies it. | Bypass role use is logged to a separate, append-only audit stream with the human engineer's identity. |
| **I** | Timing side-channels leak existence of a cross-tenant resource. | Response timing constant-ish for "not found in my tenant" vs "does not exist"; we do not currently attempt hard timing-attack resistance. |
| **D** | Slow query on a large tenant's data monopolizes the connection pool. | Per-request statement timeout; per-tenant connection pool share. |
| **E** | A SQL injection bypasses RLS. | Parameterized queries only (sqlx macros enforce this at compile time). `SET LOCAL` is parameterized via a whitelisted UUID format, not string interpolation. CI audit for any `sqlx::query(...)` that concatenates. |

### Open gaps

- Timing-side-channel hardening is not in scope for v1.
- We do not yet have a static analyzer that flags raw `PgConnection`
  use outside `TenantScopedTransaction`. Backlog.

---

## Flow 5 — Agent-initiated reverse tunnel

**Actors**: agent (in user cluster), API.
**Data**: health reports, metrics and log push, command acknowledgements.
**Boundaries crossed**: user's public internet egress → control plane.

### STRIDE

| Threat | Description | Control |
|---|---|---|
| **S** | Attacker presents a forged agent certificate. | mTLS with our internal CA; certificates issued at cluster creation, bound to a cluster ID; CA only signs certs originating from a valid workflow. |
| **S** | Attacker redirects agent to a fake control plane. | Agent pins our CA public key; agent config is bootstrapped during provisioning from a known endpoint and not mutable thereafter without re-provisioning. |
| **T** | Command from API to agent is tampered mid-tunnel. | mTLS provides integrity. Commands also carry a server-signed token identifying the workflow/activity attempt; agent rejects unsigned commands. |
| **R** | Agent denies having executed a command. | Agent signs an acknowledgement with its client cert; the ack is persisted in the workflow history. |
| **I** | A compromised agent exfiltrates metrics containing customer data. | Metrics are already the customer's own data; we treat them as such and apply our tenant-enforcement at the proxy (see Flow 4 controls). |
| **D** | Mass agent reconnects after a control plane restart overwhelm it. | Agents back off exponentially with jitter; server advertises a backoff hint. |
| **E** | Agent is used to pivot from the control plane back into the customer cluster. | Agent runs with a minimal ServiceAccount (RBAC least-privilege); commands are in a fixed allowlist; no arbitrary exec. Future: document the SA permissions and include an in-cluster readable `kubinate-permissions.yaml` so operators can audit. |

### Open gaps

- The allowlist of commands the agent will execute needs a formal
  document before Phase 2 sign-off.
- We need a revocation story for agent certificates independent of
  cluster destruction. Short-lived certs with rotation is the target.

---

## Cross-cutting concerns

- **Supply chain**: `cargo audit` + `cargo deny` in CI, Dependabot,
  `gitleaks` pre-commit + CI, Trivy on filesystem and container images,
  SBOM with Syft, signed release artefacts with `cosign` (Phase 3+).
- **Logging**: structured JSON with PII redaction allowlist; no
  `Debug` on secret types; correlation ID on every request; WAL-ship
  audit log to object storage daily.
- **Backups**: Postgres WAL shipping + nightly base backup; monthly
  restore drill with a measured RTO/RPO, documented in runbook.
- **Key management**: KEK rotation every 90 days (Phase 3 via Vault).
- **Disaster recovery**: quarterly DR drill starting Phase 4.

## Revision schedule

- **v0** — Phase 0 exit (this document).
- **v1** — Phase 2 exit (post-organizations, pre-RLS hardening).
- **v2** — Phase 3 exit (post-Vault, post-observability-proxy).
- **v3** — before external pen test at Phase 4.

## v2 open requirements (tracked here, addressed at v2 cut)

- **Secret-store boundary moves from pgcrypto to Vault.** Sprint 4
  ticket 02 delivers the partial scope (`VaultStore` impl, parity
  tests, Ansible role, runbook stub at
  [`docs/runbooks/secrets-migration.md`](../runbooks/secrets-migration.md)).
  Sprint 5+ delivers the live migration. v2 of this document
  re-runs Flow 2's STRIDE table with Vault as the authoritative
  store and the pgcrypto column retired; the KEK-in-env open gap
  in Flow 2 closes at that point. Until v2 ships, Flow 2 still
  reflects the pgcrypto reality. Cross-reference:
  [ADR-0007](../adr/0007-secret-management.md) §Long term.
- **Agent reverse-tunnel auth boundary.** Tracked separately under
  [ADR-0014](../adr/0014-agent-reverse-tunnel-wire-format.md);
  v2 of Flow 5 will replace the placeholder STRIDE entries with the
  real mTLS handshake once the live cert-issuance path lands.
