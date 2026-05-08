# CI workflow: workspace integration tests with ephemeral Postgres

**Labels**: `area/ci`, `area/sre`, `sprint-2`
**Epic**: Sprint 1 carry-over
**Size**: 3

## Context

Sprint 1 wrote integration tests for envelope encryption, audit chain
verification, and the OIDC state store. None of them have ever been
executed because (a) the local Postgres port is occupied on the
maintainer's workstation, and (b) we don't have a CI workflow yet —
`.github/workflows/` only contains the nightly E2E from ticket 08.

## Acceptance criteria

- **Given** a pull request, **when** CI runs, **then** the workflow
  executes `cargo test --workspace` with a Postgres 16 service
  container, and the run fails if any test (lib, integration,
  doctest) fails.
- **Given** the test job, **when** it runs, **then**
  `scripts/check-no-token-logging.sh` runs as a separate step and the
  job fails on any hit.
- **Given** a frontend change, **when** CI runs, **then** `npm run
  check` and `npm run lint` execute against the SvelteKit app.

## Implementation notes

- New `.github/workflows/ci.yml` with two jobs: `rust` and `frontend`.
- Reuse the Postgres service container shape from
  `nightly-e2e.yml`; `DATABASE_URL` exported into the job env so
  `#[sqlx::test]` can use it.
- Cache cargo + node_modules.
- Trigger on `pull_request` against `main` and on `push` to `main`.

## DoD

- [ ] CI green on a PR that intentionally regresses one of the existing
      integration tests (turn it into a demonstration in the PR
      description, then revert).
- [ ] First-time CI run on `main` < 8 minutes wall clock.
- [ ] Failure surfaces the failing test name in the PR check summary
      (no need to dig through logs for the common case).
