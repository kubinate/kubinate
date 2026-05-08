# Control-plane infrastructure (Terraform)

Provisions the Phase 0–2 production baseline described in
[ADR-0004](../../docs/adr/0004-deployment-topology.md): a single Hetzner
Cloud VPS (`cpx41` by default) with a private network, firewall, and
attached data volume. Application configuration (Docker Compose stack,
Cloudflare Tunnel connector, Postgres backup job) is handled by the
Ansible playbook added in a later sprint.

## What this creates

| Resource                  | Purpose                                                              |
|---------------------------|----------------------------------------------------------------------|
| `hcloud_ssh_key.operator` | Bootstrap SSH key for the operator                                   |
| `hcloud_network.cp`       | Private network (`10.20.0.0/16`) — grows to multi-node in Phase 3    |
| `hcloud_network_subnet`   | `10.20.1.0/24` subnet in `eu-central`                                |
| `hcloud_firewall.cp`      | Inbound SSH (admin CIDRs) + ICMP only. No 80/443 ingress.            |
| `hcloud_volume.data`      | 100 GB ext4 data volume (Postgres PGDATA + backup staging)           |
| `hcloud_server.cp`        | The VPS itself, running the cloud-init bootstrap in this module      |

There is **no public HTTP ingress** to the origin. All user traffic
reaches the control plane through Cloudflare Tunnel, which is initiated
outbound from the VPS (configured by Ansible).

## Prerequisites

- Terraform `>= 1.8`.
- A Hetzner Cloud **project** (recommended: one per environment, e.g.
  `kubinate-prod`, `kubinate-staging`).
- A Hetzner Cloud **API token** with read+write scope in that project.
- An operator SSH keypair (ed25519 recommended). **Keep the private key
  out of this repo.**

## Bootstrap

```bash
cd infra/terraform

# 1. Inputs — copy the example and fill in your token, SSH key, admin IPs.
cp terraform.tfvars.example terraform.tfvars

# 2. Init. Provider plugins land in .terraform/ (gitignored); the
#    dependency lock file is committed.
terraform init

# 3. Review the plan.
terraform plan -out tfplan

# 4. Apply.
terraform apply tfplan

# 5. SSH into the node (cloud-init may still be running on first apply).
ssh kubinate@$(terraform output -raw ipv4_address)
```

Expected cold-start time: ~60 seconds for the Hetzner resources plus
~2 minutes for cloud-init (package update, Docker install, UFW).

## Remote state

The initial bootstrap uses **local state** because the Hetzner Object
Storage bucket it would live in does not exist yet. The intended flow:

1. Apply this module with local state to create the VPS and (in a
   follow-up) a Terraform-managed Hetzner Object Storage bucket.
2. Uncomment the `backend "s3"` block in `versions.tf`, pointing at that
   bucket.
3. `terraform init -migrate-state` to move state to the bucket.

Never commit `*.tfstate` — it contains sensitive outputs. See
`.gitignore`.

## Post-apply follow-ups (tracked elsewhere)

- Ansible playbook: Docker Compose stack, Cloudflare Tunnel, Postgres
  backup cronjob.
- Cloudflare Tunnel origin certificate and named tunnel (via the
  Cloudflare provider, separate state).
- Hetzner Object Storage bucket + credentials for WAL shipping and
  Terraform remote state.

## Destroying

```bash
terraform destroy
```

The `hcloud_volume.data` resource has `prevent_destroy = true` as a
safety net. To actually tear everything down you must remove that
lifecycle block in a dedicated PR and re-apply first. **Do this with
intent** — the volume holds Postgres data.
