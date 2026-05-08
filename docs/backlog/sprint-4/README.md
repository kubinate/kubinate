# Sprint 4 plan

**Sprint length**: 2 weeks (matching Phase 2 cadence; Phase 3
is 14 weeks → 7 sprints to fit the brief's calendar).
**Capacity target**: 30–40 points; Sprint 4 deliberately
under-fills at ~22 to absorb phase-boundary surprises.

## Goal

> Ratify the dogfood-migration plan and ship hardening prep
> that's ready to flip on once Sprint 5's dogfood cluster
> exists; deliver one user-visible feature (WebAuthn for
> Owner / Admin) so the sprint isn't entirely backstage work.

## Scope

| File | Status | Pts | What ships |
|---|---|---|---|
| [05-webauthn-for-owners.md](./05-webauthn-for-owners.md) | full ship | 8 | `webauthn-rs` integration, `/app/settings/security` page, `requires_mfa` enforcement on Owner / Admin routes, recovery-codes runbook. **Headline user-visible feature.** |
| [02-vault-migration.md](./02-vault-migration.md) | partial (~6 of 13) | 6 | `VaultStore` impl with parity tests; Ansible role for the dogfood deploy; `KUBINATE_SECRETS_BACKEND` env-var. **No** migration binary, no production cutover. |
| [03-agent-reverse-tunnel.md](./03-agent-reverse-tunnel.md) | partial (~5 of 13) | 5 | New crate `crates/agent-proto/`; control-plane `/v1/agents/connect` endpoint accepting the bidirectional gRPC stream; in-process loopback test. **No** mTLS handshake, no real PKI, no agent-binary deploy. |
| [06-dogfood-migration-spike.md](./06-dogfood-migration-spike.md) | full ship | 3 | `docs/decisions/sprint-4-dogfood-migration.md` with M1–M4 questions, recommendation skeleton, revisit triggers. Pre-shapes Sprint 5 work. |
| [04-observability-proxy-production.md](./04-observability-proxy-production.md) | **deferred to Sprint 5+** | 0 | Hard-blocked on M1–M4 needing real VM on dogfood. The in-process `InMemoryMetricsStore` from Sprint 3 ticket 07 stays the production backend. |
| [01-ha-control-plane-revisit.md](./01-ha-control-plane-revisit.md) | **stays parked** | 0 | Opens only when an ADR-0012 revisit trigger fires. None are firing. |

**Total: 22 points.** ~30% slack vs the 30–40-pt ceiling.

## Dependency notes

- **#05 (WebAuthn)** has no infra dependency and lands in
  parallel with everything else. It's also the only ticket
  that requires `security`-role review per CONTRIBUTING.md
  — schedule that review early in the sprint, not at the
  end.
- **#02 (Vault) → #03 (agent tunnel)**: a soft dependency
  for the *production* path. The Sprint 4 partial scopes
  on both have no live dependency on each other — the
  `VaultStore` impl passes parity tests against
  `PgcryptoStore` without a live Vault, and the
  `crates/agent-proto/` crate compiles and round-trips
  in-process without certs. So Sprint 4 lets these run
  truly parallel.
- **#06 (spike doc)** drafts in any window during the
  sprint; it doesn't block on anything else and nothing
  blocks on it being merged before the sprint closes.

## What "done" looks like at sprint close

- [ ] `cargo test --workspace --lib` — all green.
- [ ] `cargo clippy --workspace --all-targets` — no new
      structural warnings (the housekeeping ledger from
      Sprint 3 close stays at zero).
- [ ] `frontend/` — `npm run check`, `npm run lint`,
      `npm test -- --run` all green; new `/app/settings/security`
      page covered by a Vitest spec.
- [ ] Each Sprint 4 partial-scope row in the four ticket
      files above is checked off; the deferred rows stay
      unchecked with the explicit "Sprint 5+" annotation.
- [ ] `docs/decisions/sprint-4-dogfood-migration.md` exists
      with `<<measurement needed>>` markers in M1–M4.
- [ ] `docs/runbooks/owner-passkey-lost.md` merged + indexed
      in the runbooks README.
- [ ] An ADR (next free, likely 0013) records the WebAuthn
      device-lifecycle decisions ADR-0009 explicitly
      deferred. Status `Accepted`.
- [ ] Sprint state memory (`memory/sprint_state.md`) updated
      to reflect Sprint 4 close + Sprint 5 commitments
      inherited from the spike doc.

## Phase 3 exit-gate coverage after Sprint 4

The roadmap names eight Phase 3 exit gates. Sprint 4
contributes to **three** of them — partial on two,
preparatory on one:

| Gate | Sprint 4 contribution |
|---|---|
| Vault replaces pgcrypto for tenant secrets | Partial: code + Ansible role compiled + tested, not deployed. |
| WebAuthn enforced for org owners | Full ship. |
| Per-cluster agent reverse tunnel shipped | Partial: protocol crate + in-process loopback, not deployed. |
| Dogfood cluster running the control plane | Spike-doc starter. **The migration itself is Sprint 5's commitment.** |
| Production observability proxy in front of real VM | (no contribution; deferred) |
| CloudNativePG operator manages control-plane Postgres | (not yet sized; will be a sub-question of the dogfood spike) |
| Threat model v2 published | (no contribution; rides on Vault + observability landing) |
| HA + Temporal revisits ratified | (no contribution; trigger-driven, none firing) |

## Sprint 5 commitments inherited from this sprint

If the spike doc (#06) lands on the default recommendation
("incremental migration: API + observability proxy first,
then Postgres, then anything else"), Sprint 5 picks up:

1. **Run the dogfood spike** (M1–M4 measurements). Rough
   size: 5 pts.
2. **Stand up the dogfood cluster** (Ansible/Terraform delta
   + initial k3s deploy). Rough size: 8 pts.
3. **Migrate `kubinate-api` onto the dogfood cluster**
   (statefulset / deployment shape decided by M4). Rough
   size: 8 pts.
4. **Deploy Vault** on the dogfood cluster + run the
   migration binary that Sprint 4 deferred. Rough size: 5
   pts.

That gets Sprint 5 to ~26 pts pre-loaded — leaves ~10 pts of
slack for the Sprint 5 planning session to tune.

## Definitions

- **DoR**: see `/docs/process.md` §4.
- **DoD**: see `/docs/process.md` §5.
- **Sprint 4 partial scope**: a section in a ticket file
  that names exactly what subset of that ticket lands in
  Sprint 4 vs. waits for the dogfood cluster. Tickets 02
  and 03 carry one. The full DoD list in those tickets
  applies to the eventual full ship; the Sprint 4 close
  ticks only the partial-scope rows.
