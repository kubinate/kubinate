# ADR-0001: Backend language and framework

- **Status**: Accepted
- **Date**: 2026-04-24
- **Deciders**: Founding team
- **Tags**: backend, language, platform

## Context

Kubinate is a security-sensitive multi-tenant B2B SaaS that orchestrates
infrastructure in customers' cloud accounts. The backend has three
defining properties:

1. **Long-running, stateful orchestration**: cluster provisioning is a
   10+ minute saga involving external APIs, SSH, and kubectl. Code lives
   in the process for minutes, not milliseconds.
2. **Handling of customer secrets**: Hetzner API tokens, kubeconfigs, and
   agent mTLS certificates all pass through this system. Memory safety
   matters. Type-driven encapsulation of "this string is a secret" matters.
3. **Small team**: 2–3 engineers must maintain a surprisingly wide surface
   area (API, workflow workers, agent, observability proxy).

The dominant alternatives are Go, TypeScript (Node.js), and Rust. Each is
capable of the task; the choice is about which set of trade-offs we take.

## Decision

We will use **Rust** with the **Axum** HTTP framework on the **Tokio**
async runtime. Supporting crates: `tower`/`tower-http` for middleware,
`serde` for serialization, `sqlx` for Postgres, `tracing` +
`tracing-opentelemetry` for observability, `tonic` for gRPC where we need
it (agent protocol, internal service-to-service).

## Alternatives considered

- **Go + chi/echo**: fastest path to a working prototype; large hiring
  pool; mature Kubernetes ecosystem (client-go). Rejected because the
  type system does not help us enforce secret handling, tenant isolation,
  or error-path correctness to the degree we need, and because we value
  "if it compiles, it is likely correct" as a force-multiplier for a
  small team.
- **TypeScript on Node.js (Fastify/NestJS)**: fastest iteration for a
  full-stack team, shared types with the frontend. Rejected because of
  weaker concurrency primitives for the agent and orchestrator, weaker
  memory-safety story for a process handling private keys, and a runtime
  whose failure modes under load (GC pauses, event-loop starvation) we do
  not want to operate.
- **Elixir/Phoenix**: excellent fit for long-running stateful workflows
  via OTP. Rejected because hiring is harder than Rust in our market, and
  the interoperability story with the Kubernetes/Helm/k3s tooling we must
  integrate with is weaker.

## Consequences

**We accept:**

- Slower early iteration velocity compared to Go or TypeScript. The
  compile/test loop is measurably longer. Engineers unfamiliar with Rust
  async will hit productivity cliffs for the first 4–8 weeks.
- A smaller pool of candidates when hiring engineers 4 and 5.
- Some ecosystems we depend on (e.g. Temporal SDK, see ADR-0003) are less
  mature in Rust than in Go. We will route around this where necessary.

**We assume (bets):**

- The type system pays back its costs by catching an outsized share of
  the bugs that would otherwise cause security incidents in a
  multi-tenant system.
- The Rust async ecosystem (Tokio, Axum, tower) is stable enough that we
  do not pay ongoing migration tax.
- We can find and retain engineers who want to write Rust; for our target
  profile (infra-flavoured, security-minded) this tends to be positive
  rather than negative selection.

**Positive follow-ons:**

- The agent binary (runs in customer clusters) benefits from a single
  static binary with a tiny footprint and no runtime dependencies.
- Sharing types between the API crate and the agent crate is possible
  inside the workspace.

## Revisit trigger

- Onboarding a new backend engineer takes longer than 4 weeks to first
  merged feature PR for two consecutive hires, **or**
- A single dependency we cannot replace (e.g. Temporal) forces a second
  language into the server-side stack for a sustained period (> 6 months
  in production), making the "one language" benefit a fiction.

## References

- [Axum documentation](https://docs.rs/axum)
- [Tokio documentation](https://tokio.rs)
- Internal spike: "Rust async ergonomics for long-running workflows"
  (Phase 0, week 1).
