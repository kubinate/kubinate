variable "hcloud_token" {
  description = "Hetzner Cloud API token with read+write scope."
  type        = string
  sensitive   = true
}

variable "environment" {
  description = "Deployment environment label (prod | staging)."
  type        = string
  default     = "prod"

  validation {
    condition     = contains(["prod", "staging"], var.environment)
    error_message = "environment must be one of: prod, staging."
  }
}

variable "location" {
  description = "Hetzner Cloud location for the VPS (e.g. nbg1, fsn1, hel1)."
  type        = string
  default     = "nbg1"
}

variable "server_type" {
  description = "Hetzner Cloud server type. ADR-0004 targets cpx41 (8 vCPU / 16 GB / 240 GB NVMe)."
  type        = string
  default     = "cpx41"
}

variable "image" {
  description = "Base OS image. Debian 12 is the Phase 0 baseline."
  type        = string
  default     = "debian-12"
}

variable "ssh_public_key" {
  description = "SSH public key (OpenSSH format) authorized for the bootstrap user."
  type        = string
}

variable "admin_ssh_cidrs" {
  description = <<-EOT
    CIDR blocks permitted to reach SSH on the control-plane VPS during
    bootstrap and break-glass. Steady-state management flows through
    Cloudflare Tunnel (see ADR-0004); SSH should be locked down to
    operator IPs only. Use ["0.0.0.0/0", "::/0"] temporarily if you
    have no static IP, and tighten immediately after first apply.
  EOT
  type        = list(string)
}

variable "volume_size_gb" {
  description = "Size of the attached data volume (Postgres WAL + object-storage stage)."
  type        = number
  default     = 100
}

variable "labels" {
  description = "Extra labels applied to every Hetzner resource."
  type        = map(string)
  default     = {}
}
