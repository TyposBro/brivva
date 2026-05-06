output "account_id" {
  value = local.account_id
}

output "region" {
  value = var.region
}

output "ecr_server_url" {
  value = aws_ecr_repository.server.repository_url
}

output "ecr_ffmpeg_base_url" {
  description = "Repo URL for the prebuilt amd64 ffmpeg base image used by server-runtime-base."
  value       = aws_ecr_repository.ffmpeg_base.repository_url
}

output "ecr_ffmpeg_gpu_base_url" {
  description = "Repo URL for the prebuilt amd64 FFmpeg GPU/NVENC base image."
  value       = aws_ecr_repository.ffmpeg_gpu_base.repository_url
}

output "ecr_server_build_base_url" {
  description = "Repo URL for the prebuilt amd64 server-rs build toolchain image."
  value       = aws_ecr_repository.server_build_base.repository_url
}

output "ecr_server_runtime_base_url" {
  description = "Repo URL for the prebuilt amd64 server-rs runtime foundation image."
  value       = aws_ecr_repository.server_runtime_base.repository_url
}

output "secret_arn" {
  description = "Primary production secret used by brivva service. Bad-provider drills must never mutate this."
  value       = aws_secretsmanager_secret.env.arn
}

output "gpu_drill_secret_arn" {
  description = "Isolated secret used only by brivva-gpu when gpu_bad_provider_drill != none. Null otherwise."
  value       = local.gpu_bad_provider_enabled ? aws_secretsmanager_secret.gpu_drill[0].arn : null
}

output "log_group" {
  value = aws_cloudwatch_log_group.app.name
}

output "cluster_name" {
  value = aws_ecs_cluster.app.name
}

output "service_name" {
  value = aws_ecs_service.app.name
}

output "cloudflared_enabled" {
  value = local.enable_cloudflared
}

output "dashboard_url" {
  description = "Deep link to the CloudWatch ops dashboard. Null when alarm_enabled=false."
  value       = var.alarm_enabled ? "https://${var.region}.console.aws.amazon.com/cloudwatch/home?region=${var.region}#dashboards:name=${aws_cloudwatch_dashboard.app[0].dashboard_name}" : null
}

output "alarm_topic_arn" {
  description = "SNS topic ARN for all Brivva alarms. Subscribe extra endpoints (Slack webhook, PagerDuty) against this."
  value       = var.alarm_enabled ? aws_sns_topic.alarms[0].arn : null
}
