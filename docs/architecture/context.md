# System Context (C4 Level 1)

This is the outermost view: who and what Kubinate interacts with as a
system. Everything inside the "Kubinate" box is expanded in
[`containers.md`](./containers.md).

```mermaid
flowchart LR
  U1(["Indie hacker<br/>Small SaaS team"])
  U2(["DevOps / platform<br/>engineer"])
  U3(["Third-party<br/>integrator"])

  subgraph K["Kubinate"]
    direction TB
    System["Self-service control plane<br/>for k3s on Hetzner"]
  end

  Hetzner[(Hetzner Cloud)]
  IDP[(OIDC IdPs)]
  Stripe[(Stripe)]
  CF[(Cloudflare)]

  U1 -->|signs up, creates clusters,<br/>installs add-ons| System
  U2 -->|manages org members,<br/>integrates with CI| System
  U3 -->|Terraform / scripts<br/>against public API| System

  System -->|provisions VPS,<br/>networks, LBs| Hetzner
  System -->|federated login| IDP
  System -->|billing| Stripe
  System -->|edge, DDoS| CF

  classDef person fill:#ffe8e8,stroke:#b85c5c
  classDef ext    fill:#f1f3f5,stroke:#495057
  classDef sys    fill:#e8ffe8,stroke:#2f9e44
  class U1,U2,U3 person
  class Hetzner,IDP,Stripe,CF ext
  class System sys
```
