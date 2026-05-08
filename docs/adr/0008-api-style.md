# ADR-0008: API style, versioning, and error format

- **Status**: Accepted
- **Date**: 2026-04-24
- **Deciders**: Founding team
- **Tags**: api, contract, versioning

## Context

Kubinate exposes three API surfaces with different consumers and
constraints:

1. **Public API**, consumed by third parties (integrations, CI systems,
   power users who script against Kubinate). Must be stable,
   well-documented, and backwards-compatible over long windows.
2. **Frontend API**, consumed by the SvelteKit dashboard. Optimized for
   UI patterns (list + page + include-related-resources).
3. **Internal service-to-service and agent-to-control-plane**.
   Performance-sensitive, streaming-capable, no public documentation
   obligation.

## Decision

- **Public API** and **Frontend API**: REST over HTTPS, designed
  **spec-first** in OpenAPI 3.1. The Rust handlers are generated /
  derived from the spec (using `utoipa` or equivalent with manual
  authored route handlers — we do not go fully spec-code-first until we
  validate the tooling in Phase 0). Both APIs share the same Axum
  binary but may expose different subsets.
- **Internal and agent APIs**: gRPC via `tonic`, with `.proto` files
  in the `crates/platform/proto/` directory, consumed by both the
  service and the agent crates.
- **Error format**: REST responses use **RFC 7807 Problem Details**
  (`application/problem+json`), with a stable `type` URI per error
  family and optional `errors` array for field-level validation errors.
  gRPC errors use standard gRPC status codes with a typed
  `ErrorDetails` message.
- **Versioning**:
  - REST: URI-based, `/v1/...`. Breaking changes require a new major
    version; additive changes go into `/v1/` under a minor-version
    header advertised in the OpenAPI spec.
  - **Deprecation policy**: a deprecated endpoint or field is announced
    with `Deprecation` and `Sunset` HTTP headers per RFC 8594,
    maintained for **12 months minimum**, longer for endpoints used by
    paying customers.
  - gRPC: major-version package names (`kubinate.agent.v1`). Same 12-month
    deprecation policy.
- **Idempotency**: non-safe endpoints that create or modify long-lived
  resources require an **`Idempotency-Key`** request header. The server
  records the `(tenant_id, key)` pair for 24 hours and returns the
  cached response for duplicates. Documented in RFC-draft style (Stripe
  conventions are the reference).
- **Pagination**: cursor-based for list endpoints returning tenant data,
  opaque base64 cursor, `next` and `prev` links. Offset pagination is
  not exposed on public APIs.
- **Timestamps**: ISO 8601 UTC, string-serialized (`2026-04-24T13:00:00Z`).
  No epoch integers on the wire.

## Alternatives considered

- **GraphQL for the frontend API**: tempting because our dashboard has
  deeply nested views. Rejected because the added complexity (schema
  stitching, N+1 protection, caching, authorization per field) is not
  justified for a small team, and because Problem Details + REST +
  `include=` query params cover our needs.
- **JSON:API**: expressive, but the serialization surface is larger
  than we need and interoperability with common API clients is weaker
  than plain REST.
- **Single gRPC API exposed to the world**: rejected. Browser support,
  CLI ergonomics, and third-party tooling all favor REST for public
  APIs.

## Consequences

**We accept:**

- The burden of keeping the OpenAPI spec in sync with handlers. We
  enforce this in CI with a contract test that exercises the spec.
- Two RPC stacks (REST and gRPC) in one codebase. We mitigate by
  centralizing request/response types where they overlap (e.g. the
  `Cluster` DTO) and by keeping agent protocol surface minimal.
- Longer deprecation windows than we might strictly prefer. The
  commitment is public and it constrains us; that is the point.

**We assume (bets):**

- OpenAPI-driven handler generation in Rust is mature enough to use
  without pain. If not, we switch to manually authored handlers with
  CI-enforced spec contract tests (strictly more work, but acceptable).
- Problem Details is sufficient for our error surface. If customers
  demand structured error codes beyond what Problem Details affords, we
  extend the `type` URI scheme rather than invent a new envelope.

**Positive follow-ons:**

- A stable OpenAPI spec enables us to ship a generated TypeScript client
  for the frontend and community-maintained clients for Python, Go, etc.
- The gRPC agent API supports streaming for log and event fan-in, which
  we would otherwise have to retrofit onto REST + WebSockets.

## Revisit trigger

- Third-party integrators repeatedly complain that the REST surface is
  too chatty for their use cases, suggesting GraphQL or a richer
  include-expansion mechanism, **or**
- Our own frontend grows a sustained pattern of fetching-then-filtering
  large payloads, suggesting the REST boundary is mis-drawn.

## References

- [OpenAPI 3.1 specification](https://spec.openapis.org/oas/v3.1.0)
- [RFC 7807 — Problem Details for HTTP APIs](https://www.rfc-editor.org/rfc/rfc7807)
- [RFC 8594 — Sunset HTTP Header](https://www.rfc-editor.org/rfc/rfc8594)
- Stripe's idempotency pattern (reference for design, not a dependency).
- [tonic](https://github.com/hyperium/tonic) for gRPC in Rust.
