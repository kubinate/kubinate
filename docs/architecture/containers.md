# Container Diagram (C4 Level 2)

This document is the authoritative container view of the Kubinate system.
Level 1 (system context) and Level 3 (component) diagrams live alongside
this file in the same directory and are linked at the bottom.

## Reading this diagram

- A **container** is a runtime unit — a process, a database, a managed
  service. Not a Docker container, though often one.
- **Trust boundaries** are drawn as colored regions. A message crossing
  a trust boundary must authenticate itself at the crossing.
- **Solid arrows** are synchronous calls. **Dashed arrows** are
  asynchronous (workflow, event, or agent-initiated reverse tunnel).

## Diagram

```mermaid
flowchart TB
  %% ============================================================
  %% Trust boundaries
  %% ============================================================
  subgraph INET["🌐 Untrusted internet"]
    User(["End user<br/>(browser)"])
    ThirdParty(["Third-party<br/>integrators"])
    Hetzner[(Hetzner Cloud API)]
    OIDC[(External OIDC IdPs<br/>GitHub / Google / MS)]
    Stripe[(Stripe API)]
  end

  subgraph CF["☁️ Cloudflare edge"]
    Pages["SvelteKit on<br/>Cloudflare Pages<br/>(SSR + CSR)"]
    CFProxy["Cloudflare<br/>proxy / WAF / DDoS"]
  end

  subgraph CP["🔒 Kubinate control plane (Hetzner)"]
    API["API / BFF<br/>kubinate-api (Axum)"]
    Orch["Cluster Orchestrator<br/>(Temporal worker)"]
    Addons["Add-on Manager<br/>(service)"]
    ObsProxy["Observability Proxy<br/>(multi-tenant query)"]
    Temporal[("Temporal<br/>server")]
    PG[("Postgres 16<br/>(app + temporal DBs)<br/>RLS enforced")]
    Redis[("Redis<br/>cache + sessions + pubsub")]
    Vault[("Vault<br/>(Phase 3+)")]
    Obj[("Hetzner S3-compatible<br/>Object Storage<br/>backups + audit archive")]
  end

  subgraph UC["👤 User-owned k3s cluster (Hetzner)"]
    Agent["kubinate-agent<br/>(DaemonSet)"]
    UserApps["User workloads<br/>+ baseline add-ons"]
    KubeAPI[("kube-apiserver")]
  end

  %% ============================================================
  %% Edges (solid = sync, dashed = async / reverse tunnel)
  %% ============================================================
  User   -->|HTTPS| Pages
  Pages  -->|HTTPS via Cloudflare Tunnel| CFProxy
  CFProxy -->|mTLS| API
  ThirdParty -->|HTTPS public API| CFProxy

  User   -->|OIDC redirect| OIDC
  API    -->|OIDC token exchange| OIDC
  API    -->|HTTPS| Stripe
  API    -->|SQL, SET LOCAL tenant| PG
  API    -->|TCP| Redis
  API    -->|secrets fetch| Vault
  API    -->|start workflow| Temporal

  Orch   -->|poll workflow tasks| Temporal
  Orch   -->|SQL| PG
  Orch   -->|HTTPS| Hetzner
  Orch   -.->|SSH during bootstrap only| UC

  Addons -->|enqueue install workflow| Temporal
  Addons -->|SQL| PG

  ObsProxy -->|SQL for tenant lookup| PG
  ObsProxy -->|push/query| Obj
  User     -->|Grafana embed| ObsProxy

  Agent -. mTLS gRPC reverse tunnel .-> API
  Agent -->|in-cluster| KubeAPI
  Agent -->|metrics + logs push| ObsProxy

  Temporal --> PG
  PG -. WAL + base backup .-> Obj
  API -. append .-> Obj

  %% ============================================================
  %% Styling
  %% ============================================================
  classDef inet fill:#ffe8e8,stroke:#b85c5c,color:#000
  classDef edge fill:#e8f2ff,stroke:#4c6ef5,color:#000
  classDef cp   fill:#e8ffe8,stroke:#2f9e44,color:#000
  classDef uc   fill:#fff4e6,stroke:#d9480f,color:#000
  classDef db   fill:#f1f3f5,stroke:#495057,color:#000

  class User,ThirdParty,Hetzner,OIDC,Stripe inet
  class Pages,CFProxy edge
  class API,Orch,Addons,ObsProxy,Temporal cp
  class Agent,UserApps,KubeAPI uc
  class PG,Redis,Vault,Obj db
```

## Containers — detail

| Container                  | Tech                       | Purpose                                                          | Scaling                 |
|----------------------------|----------------------------|------------------------------------------------------------------|-------------------------|
| SvelteKit (Pages)          | Svelte + Cloudflare edge   | UI, marketing, SSR; BFF for some flows                           | Global edge             |
| API / BFF                  | Rust / Axum                | Public + frontend REST, auth, rate limiting                      | Horizontal (stateless)  |
| Cluster Orchestrator       | Rust + Temporal worker     | Cluster lifecycle workflows                                      | Worker pool             |
| Add-on Manager             | Rust                       | Helm chart catalog, install workflows                            | Co-resident with API    |
| Observability Proxy        | Rust                       | Metric/log multi-tenant query proxy + TSDB                       | Horizontal              |
| Agent                      | Rust (static binary)       | In-cluster, reverse-tunnel to control plane                      | One per user cluster    |
| Temporal                   | Temporal server            | Workflow orchestration                                           | HA in Phase 3+          |
| Postgres                   | Postgres 16                | OLTP + Temporal persistence                                      | CNPG HA in Phase 3+     |
| Redis                      | Redis 7                    | Cache, rate limit, sessions, pubsub                              | Single writer           |
| Vault                      | HashiCorp Vault            | Secrets, dynamic creds (Phase 3+)                                | HA                      |
| Object Storage             | Hetzner S3-compatible      | Backups, audit archive                                           | Managed                 |

## Trust boundaries — crossings

| Crossing                                     | Authentication                                |
|----------------------------------------------|-----------------------------------------------|
| Browser → Cloudflare                         | TLS 1.3                                       |
| Cloudflare → API                             | Cloudflare Tunnel + mTLS                      |
| API → internal services                      | mTLS, service account tokens                  |
| Control plane → user cluster (bootstrap SSH) | Ephemeral SSH key, revoked after bootstrap    |
| User cluster → control plane (steady state)  | Agent-initiated mTLS reverse tunnel           |
| API → Hetzner                                | User's encrypted Hetzner API token            |
| API → OIDC IdP                               | OIDC client ID + PKCE                         |
| API → Stripe                                 | Restricted Stripe API key                     |

The **reverse-tunnel** property is load-bearing: a user's cluster does
not require inbound firewall rules from the internet. The control plane
never holds direct kubectl credentials to customer clusters. Commands
flow only over the authenticated, agent-initiated tunnel.

## Related diagrams

- [`context.md`](./context.md) — C4 Level 1 (system context)
  *(stub for Phase 0)*
- [`components-api.md`](./components-api.md) — C4 Level 3 of the API
  container *(filled in Phase 1)*
- [`components-orchestrator.md`](./components-orchestrator.md) —
  C4 Level 3 of the Cluster Orchestrator *(filled in Phase 1)*
