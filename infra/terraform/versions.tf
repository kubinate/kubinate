terraform {
  required_version = ">= 1.8"

  required_providers {
    hcloud = {
      source  = "hetznercloud/hcloud"
      version = "~> 1.48"
    }
  }

  # Remote state: uncomment and configure once Hetzner Object Storage
  # credentials are provisioned (Phase 0 step 2). Until then, local state
  # is acceptable for a single-operator bootstrap.
  #
  # backend "s3" {
  #   bucket                      = "kubinate-tfstate"
  #   key                         = "prod/control-plane.tfstate"
  #   region                      = "eu-central"
  #   endpoints                   = { s3 = "https://nbg1.your-objectstorage.com" }
  #   skip_credentials_validation = true
  #   skip_region_validation      = true
  #   skip_metadata_api_check     = true
  #   skip_requesting_account_id  = true
  #   use_path_style              = true
  # }
}

provider "hcloud" {
  token = var.hcloud_token
}
