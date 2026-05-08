# Second add-on: cert-manager via the existing catalog

**Labels**: `area/addons`, `sprint-3`
**Epic**: Phase 2 finisher
**Size**: 3

## Context

Sprint 2 ticket 07 shipped ingress-nginx as the first cataloged add-on
and validated the `kubinate_addons::catalog` pattern.  cert-manager
is the natural second one — every ingress-nginx user wants automatic
TLS, and it exercises a slightly different chart shape (CRDs that
must install before the controller).

## Acceptance criteria

- **Given** a Ready cluster with ingress-nginx installed, **when**
  the user POSTs `/v1/clusters/:id/addons` with
  `{ "addon": "cert-manager", "version": "1.16.x" }`, **then** an
  install workflow runs that handles the CRD → controller order
  correctly via Helm's `--include-crds` flag (or the chart's
  `installCRDs: true` value, depending on the upstream).
- **Given** the cert-manager pod is Ready, **when** the user
  applies an `Issuer` manifest (via their own kubectl), **then** it
  reconciles. Sprint 3's scope ends at "controller is up and ready
  to take an Issuer"; provisioning of the Issuer + actual cert
  issuance is out-of-scope.

## Implementation notes

- One entry in `crates/addons/src/catalog.rs::CATALOG`.
- The Helm activity already supports values YAML; the cert-manager
  upstream's recommended values for v1.16 set `installCRDs: true`.
- Run the `kind` integration test (ticket 05) against this addon to
  validate the CRD pre-install path.

## DoD

- [ ] `crates/addons/src/catalog.rs::tests` extended with a
      `cert_manager_is_on_the_allowlist` test.
- [ ] Ticket 05's gated CI job exercises this addon end-to-end.
- [ ] Runbook stub: `addon-install-stuck.md` mentions the CRD
      ordering edge case.
