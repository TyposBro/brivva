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
  description = "Repo URL for the prebuilt arm64 ffmpeg base image. Pass as FFMPEG_BASE_IMAGE build-arg to server-rs/Dockerfile."
  value       = aws_ecr_repository.ffmpeg_base.repository_url
}

output "secret_arn" {
  value = aws_secretsmanager_secret.env.arn
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
  description = "Deep link to the CloudWatch ops dashboard."
  value       = "https://${var.region}.console.aws.amazon.com/cloudwatch/home?region=${var.region}#dashboards:name=${aws_cloudwatch_dashboard.app.dashboard_name}"
}

output "alarm_topic_arn" {
  description = "SNS topic ARN for all Brivva alarms. Subscribe extra endpoints (Slack webhook, PagerDuty) against this."
  value       = var.alarm_enabled ? aws_sns_topic.alarms[0].arn : null
}
