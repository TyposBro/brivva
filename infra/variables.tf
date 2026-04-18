variable "region" {
  description = "AWS region. us-east-1 chosen for proximity to Soniox STT + ElevenLabs TTS."
  type        = string
  default     = "us-east-1"
}

variable "project" {
  description = "Name prefix for all resources (cluster, ECR repos, role, secret, log group)."
  type        = string
  default     = "brivva"
}

variable "domain" {
  description = "Hostname that cloudflared tunnel routes to server-rs:3000."
  type        = string
  default     = "brivva.spiko.uz"
}

variable "frontend_url" {
  description = "CORS origin + OAuth frontend. Cloudflare Pages URL."
  type        = string
  default     = "https://brivva.pages.dev"
}

variable "broadcast_delay_ms" {
  description = "End-to-end delay target. Trades latency for smoothness."
  type        = number
  default     = 5000
}

variable "task_cpu" {
  description = <<-EOT
    Fargate CPU units (1024 = 1 vCPU). Default 2048 = 2 vCPU.
    Sized for 1 concurrent stream with 1080p transcode + 2 translations + burn-in subtitles.
    See infra/README.md "Sizing" for the math and scale-out guidance.
  EOT
  type        = string
  default     = "2048"
}

variable "task_memory" {
  description = "Fargate memory MB. Default 4096 = 4 GB (paired with 2 vCPU)."
  type        = string
  default     = "4096"
}

variable "log_retention_days" {
  type    = number
  default = 7
}

# ── Secrets (sensitive, provided via terraform.tfvars from load-env.sh) ──

variable "soniox_api_key" {
  type      = string
  sensitive = true
}

variable "elevenlabs_api_key" {
  type      = string
  sensitive = true
}

variable "google_client_id" {
  type      = string
  sensitive = true
  default   = ""
}

variable "google_client_secret" {
  type      = string
  sensitive = true
  default   = ""
}

variable "tunnel_creds" {
  description = "Cloudflared tunnel credentials JSON (as a string). If empty, cloudflared sidecar is disabled."
  type        = string
  sensitive   = true
  default     = ""
}

variable "tunnel_id" {
  description = "Cloudflared tunnel UUID. Required if tunnel_creds is set."
  type        = string
  default     = ""
}
