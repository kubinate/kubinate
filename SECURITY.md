# Security Policy

Kubinate operates infrastructure on behalf of customers. We take
security reports seriously and we respond quickly.

## Supported versions

Pre-1.0: only the current `main` branch is supported. Once we ship
1.0 we will maintain a standard support window documented here.

## Reporting a vulnerability

**Do not open a public GitHub issue, discussion, or pull request for
security vulnerabilities.** Public disclosure before a fix is
available puts other users at risk.

### Preferred channel

Email **security@kubinate.io** with:

- A description of the issue and its impact.
- Steps to reproduce, or a proof of concept.
- The affected component (API, agent, workflow, infra).
- Any suggested remediation, if you have one.
- Whether you plan to publish research about the issue, and if so,
  on what timeline.

If you would like to encrypt your report, our PGP key is available at
<https://kubinate.io/.well-known/security.asc> (fingerprint published
with the key).

### What to expect

- **Acknowledgement** within 2 business days.
- **Triage** within 5 business days, including an initial severity
  assessment (CVSS 3.1).
- **Remediation timeline** communicated within 10 business days.
  Critical and high-severity issues are typically resolved within
  30 days; medium within 90; low on a best-effort basis.
- **Credit** in the release notes and, for impactful reports, on our
  public acknowledgements page — if you want it.

### Scope

In scope:

- `kubinate.io` and any subdomain we operate.
- The `kubinate-api`, `kubinate-agent`, and other binaries from this
  repository, in their default configuration.
- Our Temporal deployment, Postgres, Vault, and object storage as
  operated by us.

Out of scope:

- Denial of service via volumetric attacks against our public
  endpoints. Please test against a disposable account.
- Issues in customer-owned clusters that result from customer
  misconfiguration of their Hetzner account or their own cluster.
- Issues in third-party services we integrate with (report those to
  the respective vendor).
- Social engineering of our staff.

### Safe harbor

We will not pursue legal action against researchers who:

- Make a good-faith effort to avoid privacy violations, data
  destruction, or service degradation.
- Only interact with their own accounts or accounts they are
  explicitly authorized to test.
- Give us reasonable time to remediate before public disclosure
  (target: 90 days from acknowledgement, negotiable for complex
  issues).

## Dependency vulnerabilities

Our CI runs `cargo audit`, `cargo deny`, `npm audit`, and Trivy on
every pull request. If you spot a dependency CVE that our pipeline
missed, please let us know — that itself is a bug in our process.

## Multi-tenant isolation

Cross-tenant data leaks are treated as the highest-severity category
of issue. If you find a way to read, modify, or infer data belonging
to another organization, please report it via the process above.

---

Thank you for helping keep Kubinate and its users safe.
