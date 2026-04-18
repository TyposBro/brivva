output "account_id" {
  value = local.account_id
}

output "region" {
  value = var.region
}

output "ecr_server_url" {
  value = aws_ecr_repository.server.repository_url
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
