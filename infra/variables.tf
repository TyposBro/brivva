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

variable "webrtc_udp_port_min" {
  description = "First UDP port Fargate may bind for WebRTC ICE media."
  type        = number
  default     = 50000
}

variable "webrtc_udp_port_max" {
  description = "Last UDP port Fargate may bind for WebRTC ICE media."
  type        = number
  default     = 50100
}

variable "webrtc_stun_urls" {
  description = "Comma-separated STUN URLs used by server-rs to gather public ICE candidates."
  type        = string
  default     = "stun:stun.l.google.com:19302"
}

variable "task_cpu" {
  description = <<-EOT
    Fargate CPU units (1024 = 1 vCPU). Default 8192 = 8 vCPU.
    Sized for 1 concurrent stream with 1080p transcode + up to 3 translations
    + passthrough + burn-in subtitles (May 10 "ultimate test" shape:
    En -> Ko+Zh+Ja + passthrough = ~3.45 cores under load, comfortable
    headroom for STT/translate/TTS orchestration).
    See infra/README.md "Sizing" for the math and scale-out guidance.
  EOT
  type        = string
  default     = "8192"
}

variable "task_memory" {
  description = "Fargate memory MB. Default 16384 = 16 GB (paired with 8 vCPU)."
  type        = string
  default     = "16384"
}

variable "log_retention_days" {
  type    = number
  default = 7
}

# ── Runtime secrets are synced from Infisical, not Terraform ──

variable "enable_cloudflared" {
  description = "Enable the cloudflared sidecar. TUNNEL_CREDS must exist in AWS Secrets Manager, synced from Infisical."
  type        = bool
  default     = true
}

variable "tunnel_id" {
  description = "Cloudflared tunnel UUID. Required when enable_cloudflared is true."
  type        = string
  default     = ""
}

variable "workers_api_url" {
  description = "brivva-api Worker base URL (no trailing slash)."
  type        = string
  default     = "https://brivva-api.milliytechnology.workers.dev"
}

# ── Monitoring ──────────────────────────────────────────────

variable "alarm_enabled" {
  description = "Provision CloudWatch alarms + SNS topic. Disable for bare-bones dev stacks."
  type        = bool
  default     = true
}

variable "alarm_email" {
  description = "Email subscribed to the SNS alarm topic. Empty = alarms fire but nobody is paged. AWS sends a confirmation email on first apply."
  type        = string
  default     = ""
}
