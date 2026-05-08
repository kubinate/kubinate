# Architecture Decision Records

This directory contains Architecture Decision Records (ADRs) for Kubinate,
following [Michael Nygard's format](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions).

## Why ADRs

An architecturally significant decision is one that is costly to reverse.
ADRs capture *why* we made such a decision so that a future contributor
(including ourselves) can evaluate whether the reasoning still holds. They
are not design documents and they are not status reports. One ADR, one
decision.

## Workflow

1. Copy `template.md` to `NNNN-short-title.md` using the next number.
2. Open the ADR in **Proposed** status and link it from the PR description.
3. Discuss and iterate on the ADR itself (not the implementation yet).
4. On merge, status becomes **Accepted**. The ADR is immutable afterwards.
5. To change a decision, write a new ADR that **Supersedes** the old one.
   Update the superseded ADR's status header with a link to the new one.
6. Deprecate (but do not delete) an ADR when the decision no longer applies
   but no replacement is needed.

## Scope

An ADR is warranted when a decision:

- Affects multiple bounded contexts or services.
- Constrains future technology choices.
- Carries non-trivial migration cost to reverse.
- Has security, privacy, or compliance implications.
- Locks in a vendor or open-source project dependency.

Everyday implementation choices (library preferences inside a module, code
style, variable naming) do not need ADRs.

## Seed set

| #    | Title                                                 | Status   |
|------|-------------------------------------------------------|----------|
| 0001 | Backend language and framework                        | Accepted |
| 0002 | Frontend framework                                    | Accepted |
| 0003 | Cluster provisioning orchestration                    | Accepted |
| 0004 | Deployment topology (short- and long-term)            | Accepted |
| 0005 | Primary database and secondary stores                 | Accepted |
| 0006 | Multi-tenancy isolation model                         | Accepted |
| 0007 | Secret management                                     | Accepted |
| 0008 | API style, versioning, and error format               | Accepted |
| 0009 | Authentication                                        | Accepted |
| 0010 | Authorization                                         | Accepted |
| 0011 | Defer the Temporal SDK adoption to Phase 3            | Accepted |
| 0012 | Defer HA control-plane delivery to Phase 4+           | Accepted |
| 0013 | WebAuthn device-lifecycle decisions                   | Accepted |
