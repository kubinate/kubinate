locals {
  name = "kubinate-${var.environment}"

  base_labels = merge(
    {
      "app"         = "kubinate"
      "environment" = var.environment
      "managed-by"  = "terraform"
    },
    var.labels,
  )
}

# --- SSH key registered in the Hetzner project -------------------------------
resource "hcloud_ssh_key" "operator" {
  name       = "${local.name}-operator"
  public_key = var.ssh_public_key
  labels     = local.base_labels
}

# --- Private network + subnet (Phase 3 adds more nodes to this network) ------
resource "hcloud_network" "cp" {
  name     = "${local.name}-cp"
  ip_range = "10.20.0.0/16"
  labels   = local.base_labels
}

resource "hcloud_network_subnet" "cp" {
  network_id   = hcloud_network.cp.id
  type         = "cloud"
  network_zone = "eu-central"
  ip_range     = "10.20.1.0/24"
}

# --- Firewall ----------------------------------------------------------------
# Inbound: SSH from admin CIDRs only. No 80/443 exposure — all user traffic
# reaches the origin through Cloudflare Tunnel (outbound-initiated, see
# ADR-0004). ICMP is allowed for operational troubleshooting.
resource "hcloud_firewall" "cp" {
  name   = "${local.name}-cp"
  labels = local.base_labels

  rule {
    direction  = "in"
    protocol   = "tcp"
    port       = "22"
    source_ips = var.admin_ssh_cidrs
  }

  rule {
    direction  = "in"
    protocol   = "icmp"
    source_ips = ["0.0.0.0/0", "::/0"]
  }
}

# --- Data volume (Postgres PGDATA + backup staging) --------------------------
resource "hcloud_volume" "data" {
  name      = "${local.name}-data"
  size      = var.volume_size_gb
  location  = var.location
  format    = "ext4"
  labels    = local.base_labels

  lifecycle {
    prevent_destroy = true
  }
}

# --- Control-plane VPS -------------------------------------------------------
resource "hcloud_server" "cp" {
  name         = "${local.name}-cp-01"
  image        = var.image
  server_type  = var.server_type
  location     = var.location
  ssh_keys     = [hcloud_ssh_key.operator.id]
  firewall_ids = [hcloud_firewall.cp.id]
  user_data    = file("${path.module}/cloud-init.yaml")
  labels       = local.base_labels

  network {
    network_id = hcloud_network.cp.id
    ip         = "10.20.1.10"
  }

  public_net {
    ipv4_enabled = true
    ipv6_enabled = true
  }

  depends_on = [hcloud_network_subnet.cp]
}

resource "hcloud_volume_attachment" "data" {
  volume_id = hcloud_volume.data.id
  server_id = hcloud_server.cp.id
  automount = true
}
