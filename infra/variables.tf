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
    Fargate CPU units (1024 = 1 vCPU). Default 16384 = 16 vCPU.
    Sized for 1 concurrent stream with 720p/1080p transcode + up to 3 translations
    + passthrough + burn-in subtitles (May 10 "ultimate test" shape:
    En -> Ko+Zh+Ja + passthrough = ~3.45 cores under load, comfortable
    headroom for STT/translate/TTS orchestration).
    See infra/README.md "Sizing" for the math and scale-out guidance.
  EOT
  type        = string
  default     = "16384"
}

variable "task_memory" {
  description = "Fargate memory MB. Default 32768 = 32 GB (paired with 16 vCPU)."
  type        = string
  default     = "32768"
}

variable "ecs_launch_type" {
  description = "Media engine launch path: FARGATE for CPU fallback, EC2_GPU for May 10 GPU primary."
  type        = string
  default     = "FARGATE"
  validation {
    condition     = contains(["FARGATE", "EC2_GPU"], var.ecs_launch_type)
    error_message = "ecs_launch_type must be FARGATE or EC2_GPU."
  }
}

variable "gpu_capacity_enabled" {
  description = "Create zero-capacity ECS GPU ASG/capacity provider without switching the service off Fargate. No GPU bill while gpu_desired_capacity=0."
  type        = bool
  default     = true
}

variable "gpu_instance_type" {
  description = "EC2 GPU instance type for ecs_launch_type=EC2_GPU. g4dn.xlarge = cheapest NVIDIA T4 launch target."
  type        = string
  default     = "g4dn.xlarge"
}

variable "gpu_availability_zones" {
  description = "AZs allowed for GPU ASG. us-east-1e excludes g4dn.xlarge, so keep it out by default."
  type        = list(string)
  default     = ["us-east-1a", "us-east-1b", "us-east-1c", "us-east-1d", "us-east-1f"]
}

variable "gpu_desired_capacity" {
  description = "Number of ECS GPU EC2 instances. Keep 0 until ready to pay; set 1 for rehearsal/launch."
  type        = number
  default     = 0
}

variable "gpu_rehearsal_service_enabled" {
  description = "Create a parallel brivva-gpu ECS service for blue/green rehearsal without replacing the primary Fargate service."
  type        = bool
  default     = false
}

variable "gpu_rehearsal_desired_count" {
  description = "Desired task count for the parallel brivva-gpu rehearsal service. Use 1 only when gpu_desired_capacity is also 1+."
  type        = number
  default     = 0
}

variable "gpu_private_endpoints_enabled" {
  description = "Create private VPC endpoints required for no-public-IP ECS GPU rehearsal capacity. Keep false until the reviewed 6C endpoint apply."
  type        = bool
  default     = false
}

variable "gpu_private_endpoint_route_table_ids" {
  description = "Route table IDs for the S3 gateway endpoint used by private ECR image layer pulls. Intentionally explicit so 6C does not mutate unrelated route tables by discovery."
  type        = list(string)
  default     = []
}

variable "gpu_private_endpoint_debug_services_enabled" {
  description = "Also create SSM/EC2Messages/SSMMessages interface endpoints for private GPU debugging. Not required for base ECS registration."
  type        = bool
  default     = false
}

variable "gpu_task_cpu" {
  description = "CPU units for EC2 GPU task. g4dn.xlarge has 4096 CPU units."
  type        = string
  default     = "4096"
}

variable "gpu_task_memory" {
  description = "Memory MB for EC2 GPU task. Keep below g4dn.xlarge registered memory (~15.7GB)."
  type        = string
  default     = "14336"
}

variable "session_logs_enabled" {
  description = "Set BRIVVA_SESSION_LOGS=1 on server-rs to upload per-session NDJSON logs to Workers/D1."
  type        = bool
  default     = true
}

variable "session_logs_verbose" {
  description = "Set BRIVVA_SESSION_LOG_VERBOSE=1 on server-rs for high-volume debug session events."
  type        = bool
  default     = false
}

variable "log_retention_days" {
  type    = number
  default = 7
}

# ── Secrets (sensitive, provided at runtime by Infisical as TF_VAR_*) ──

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

variable "jwt_secret" {
  description = "HS256 secret shared with brivva-api Worker for JWT verification."
  type        = string
  sensitive   = true
}

variable "internal_secret" {
  description = "Shared secret for Fargate→Workers /internal/* HTTP calls."
  type        = string
  sensitive   = true
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
