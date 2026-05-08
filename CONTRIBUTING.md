# Contributing to Kubinate

Thanks for considering a contribution. This document covers what we
expect from code, commits, PRs, and documentation.

## Table of contents

- [Code of conduct](#code-of-conduct)
- [Getting set up](#getting-set-up)
- [The decision hierarchy](#the-decision-hierarchy)
- [Branches and commits](#branches-and-commits)
- [Pull requests](#pull-requests)
- [Definition of Done](#definition-of-done)
- [Coding conventions](#coding-conventions)
- [Writing an ADR](#writing-an-adr)
- [Reporting security issues](#reporting-security-issues)

## Code of conduct

This project follows [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md). All
contributors, maintainers, and users are expected to abide by it.

## Getting set up

See the [`README`](README.md#local-development) for local-dev
prerequisites and commands. If anything in that section does not work
on a fresh clone, that itself is a bug worth filing — the Phase 0 exit
criterion is that any engineer is productive in under 30 minutes.

## The decision hierarchy

Before making a non-trivial change, ask yourself:

1. **Is this an architecturally significant decision?** If yes, it
   needs an ADR. Open a PR with the ADR in `Proposed` status *before*
   the implementation PR. See [Writing an ADR](#writing-an-adr) below.
2. **Does this change a public API or the database schema?** If yes,
   the PR must include a migration plan and, for APIs, an
   `Deprecation` / `Sunset` story for anything removed. See
   [ADR-0008](docs/adr/0008-api-style.md).
3. **Is it a bugfix or a small feature that fits inside an existing
   module?** Go ahead — just open a PR.

When in doubt, ask in the PR description and we'll sort it out
together. The worst outcome is silently shipping a decision that a
future reader will have to archaeologically reconstruct.

## Branches and commits

- Branch from `main`. Name branches `type/short-description`, e.g.
  `feat/cluster-provision-workflow`, `fix/session-revocation-race`.
- Keep branches short-lived. A branch older than two weeks is a signal
  that the work needs to be broken down.
- Commits follow [Conventional Commits](https://www.conventionalcommits.org/).
  Types we use: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`,
  `build`, `ci`, `perf`. Breaking changes are marked with a `!` after
  the type and include a `BREAKING CHANGE:` footer.

```
feat(cluster): add HA control plane provisioning

Implements the HA variant described in §2 Phase 2. Closes #123.
```

Conventional Commits are enforced in CI via `commitlint`.

## Pull requests

### Size

Aim for PRs that a reviewer can thoroughly read in under 20 minutes.
If your PR is larger than ~400 changed lines (excluding generated
code, lock files, and migrations), consider breaking it up.

### Description

The PR template asks for:

- **What** — the change in one sentence.
- **Why** — the motivation, with a link to the issue or ADR.
- **How** — a brief tour for the reviewer. Call out anything
  non-obvious.
- **Risk** — what could break in production, and how we would notice.
- **Out of scope** — things you deliberately did not do.

### Review

Every PR requires one approving review from someone other than the
author. Security-sensitive changes (authn, authz, secrets, tenant
isolation) require **two** approvals and one of them must be from a
maintainer with the `security` role.

## Definition of Done

A change is "done" when all of these are true. Protect this list —
shortcuts compound into incidents.

- [ ] All acceptance criteria verified.
- [ ] Unit tests for new logic (coverage targets: 70% on domain
      crates, 50% overall).
- [ ] At least one integration test for the happy path.
- [ ] ADR written and merged if an architectural decision was made.
- [ ] User-facing docs updated if public API or UI changed.
- [ ] Runbook updated if a new operational concern was introduced.
- [ ] Security review completed by a second engineer for authn/authz
      changes.
- [ ] PR merged through green CI, reviewed by at least one other
      engineer.
- [ ] Feature flag gate in place for anything shippable but not yet
      public.
- [ ] Observability in place — a metric and a log line for new code
      paths.

## Coding conventions

### Rust

- `cargo fmt` on every commit; `cargo clippy --all-targets
  --all-features -- -D warnings` passes.
- Crates are organized around bounded contexts (see project brief §4).
  A domain crate never depends on `api/`. Cross-domain interaction
  goes through traits defined in `platform` or through domain events.
- Errors in domain crates return `PlatformError` (or a domain-scoped
  error that converts into it); the API edge translates to Problem
  Details (ADR-0008).
- `unsafe` is forbidden at the crate level (`#![forbid(unsafe_code)]`).
  If you have a genuine need, remove the forbid and justify in the PR.
- Database access goes through `TenantScopedTransaction` for tenant
  data. Bypassing it requires a comment linking to an audit-logged
  exception path (ADR-0006).
- Secrets are handled exclusively through `SecretRef` and the
  `SecretStore` trait (ADR-0007). Raw plaintext does not cross module
  boundaries.

### TypeScript / Svelte

- Prettier on every commit; ESLint passes.
- Type-check with `svelte-check` in CI.
- Prefer server-side rendering for public pages; client-rendered SPA
  for dashboard routes.
- API access goes through the generated OpenAPI client; do not hand-
  roll fetch calls in components.

### SQL / migrations

- `migrations/` is append-only. Never edit a shipped migration.
- Every tenant-scoped table gets `organization_id`, RLS enabled, and
  a `tenant_scope` policy. Reviewers should catch missing RLS on
  first review — it's the single most important invariant.
- Use `UUID v7` for primary keys. Time fields are `TIMESTAMPTZ`.

## Writing an ADR

1. Copy `docs/adr/template.md` to `docs/adr/NNNN-short-title.md`.
2. Open a PR with the ADR in `Proposed` status. Do not implement yet.
3. Solicit review; discuss on the PR itself.
4. On merge, status becomes `Accepted`. The ADR is immutable.
5. To change a decision later, write a new ADR that **Supersedes**
   the old one. Update the old ADR's status header.

Keep one ADR per decision. Keep the Context section honest about the
state of the world at decision time — do not rewrite it later.

## Reporting security issues

**Do not file a public GitHub issue for security vulnerabilities.**
See [`SECURITY.md`](SECURITY.md) for our disclosure process.

---

Thanks again for contributing.
