output "server_id" {
  description = "Hetzner Cloud server ID."
  value       = hcloud_server.cp.id
}

output "server_name" {
  description = "Server hostname."
  value       = hcloud_server.cp.name
}

output "ipv4_address" {
  description = "Public IPv4 address (SSH during bootstrap; steady-state traffic goes through Cloudflare Tunnel)."
  value       = hcloud_server.cp.ipv4_address
}

output "ipv6_address" {
  description = "Public IPv6 address."
  value       = hcloud_server.cp.ipv6_address
}

output "private_ipv4" {
  description = "Private network IP on the cp network."
  value       = one([for n in hcloud_server.cp.network : n.ip])
}

output "data_volume_id" {
  description = "ID of the attached data volume."
  value       = hcloud_volume.data.id
}

output "ssh_command" {
  description = "Convenience SSH command for the bootstrap user."
  value       = "ssh kubinate@${hcloud_server.cp.ipv4_address}"
}
