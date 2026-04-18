data "aws_caller_identity" "current" {}

data "aws_vpc" "default" {
  default = true
}

data "aws_subnets" "default" {
  filter {
    name   = "vpc-id"
    values = [data.aws_vpc.default.id]
  }
}

locals {
  account_id         = data.aws_caller_identity.current.account_id
  ecr_base           = "${local.account_id}.dkr.ecr.${var.region}.amazonaws.com"
  enable_cloudflared = nonsensitive(length(var.tunnel_creds) > 0) && length(var.tunnel_id) > 0
  ecr_server         = aws_ecr_repository.server.repository_url
  secret_arn         = aws_secretsmanager_secret.env.arn
}

# ── ECR ────────────────────────────────────────────────────

resource "aws_ecr_repository" "server" {
  name                 = "${var.project}/server-rs"
  image_tag_mutability = "MUTABLE"
  image_scanning_configuration {
    scan_on_push = true
  }
}

resource "aws_ecr_lifecycle_policy" "server" {
  repository = aws_ecr_repository.server.name
  policy = jsonencode({
    rules = [{
      rulePriority = 1
      description  = "Keep last 10 images"
      selection = {
        tagStatus   = "any"
        countType   = "imageCountMoreThan"
        countNumber = 10
      }
      action = { type = "expire" }
    }]
  })
}

# ── CloudWatch Logs ────────────────────────────────────────

resource "aws_cloudwatch_log_group" "app" {
  name              = "/ecs/${var.project}"
  retention_in_days = var.log_retention_days
}

# ── Secrets Manager ────────────────────────────────────────

resource "aws_secretsmanager_secret" "env" {
  name                    = "${var.project}/env"
  recovery_window_in_days = 0
}

resource "aws_secretsmanager_secret_version" "env" {
  secret_id = aws_secretsmanager_secret.env.id
  secret_string = jsonencode({
    SONIOX_API_KEY       = var.soniox_api_key
    ELEVENLABS_API_KEY   = var.elevenlabs_api_key
    GOOGLE_CLIENT_ID     = var.google_client_id
    GOOGLE_CLIENT_SECRET = var.google_client_secret
    TUNNEL_CREDS         = var.tunnel_creds
  })
}

# ── IAM ────────────────────────────────────────────────────

resource "aws_iam_role" "exec" {
  name = "${var.project}-ecs-execution"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "ecs-tasks.amazonaws.com" }
      Action    = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy_attachment" "exec_base" {
  role       = aws_iam_role.exec.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

resource "aws_iam_role_policy" "secrets_read" {
  name = "${var.project}-secrets-read"
  role = aws_iam_role.exec.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["secretsmanager:GetSecretValue"]
      Resource = aws_secretsmanager_secret.env.arn
    }]
  })
}

# ── Security group (Fargate task) ──────────────────────────
# No public ingress — cloudflared sidecar connects outbound only.
# If you swap to ALB ingress, add inbound 3000 here.

resource "aws_security_group" "task" {
  name        = "${var.project}-task"
  description = "Egress-only for Fargate task (cloudflared handles ingress)."
  vpc_id      = data.aws_vpc.default.id

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

# ── ECS ────────────────────────────────────────────────────

resource "aws_ecs_cluster" "app" {
  name = var.project
}

locals {
  server_container = {
    name      = "server-rs"
    image     = "${local.ecr_server}:latest"
    essential = true
    portMappings = [
      { containerPort = 3000, hostPort = 3000, protocol = "tcp" }
    ]
    environment = [
      { name = "DATABASE_URL", value = "sqlite:/data/brivva.db?mode=rwc" },
      { name = "BROADCAST_DELAY_MS", value = tostring(var.broadcast_delay_ms) },
      { name = "GOOGLE_REDIRECT_URI", value = "https://${var.domain}/auth/youtube/callback" },
      { name = "FRONTEND_URL", value = var.frontend_url },
    ]
    secrets = [
      { name = "SONIOX_API_KEY", valueFrom = "${local.secret_arn}:SONIOX_API_KEY::" },
      { name = "ELEVENLABS_API_KEY", valueFrom = "${local.secret_arn}:ELEVENLABS_API_KEY::" },
      { name = "GOOGLE_CLIENT_ID", valueFrom = "${local.secret_arn}:GOOGLE_CLIENT_ID::" },
      { name = "GOOGLE_CLIENT_SECRET", valueFrom = "${local.secret_arn}:GOOGLE_CLIENT_SECRET::" },
    ]
    logConfiguration = {
      logDriver = "awslogs"
      options = {
        awslogs-group         = aws_cloudwatch_log_group.app.name
        awslogs-region        = var.region
        awslogs-stream-prefix = "server-rs"
      }
    }
  }

  cloudflared_container = {
    name      = "cloudflared"
    image     = "cloudflare/cloudflared:latest"
    essential = true
    command = [
      "sh", "-c",
      "echo \"$TUNNEL_CREDS\" > /tmp/creds.json && printf 'tunnel: ${var.tunnel_id}\\ncredentials-file: /tmp/creds.json\\ningress:\\n  - hostname: ${var.domain}\\n    service: http://localhost:3000\\n  - service: http_status:404\\n' > /tmp/config.yml && cloudflared tunnel --no-autoupdate --config /tmp/config.yml run"
    ]
    secrets = [
      { name = "TUNNEL_CREDS", valueFrom = "${local.secret_arn}:TUNNEL_CREDS::" }
    ]
    logConfiguration = {
      logDriver = "awslogs"
      options = {
        awslogs-group         = aws_cloudwatch_log_group.app.name
        awslogs-region        = var.region
        awslogs-stream-prefix = "cloudflared"
      }
    }
  }

  containers = concat(
    [local.server_container],
    local.enable_cloudflared ? [local.cloudflared_container] : [],
  )
}

resource "aws_ecs_task_definition" "app" {
  family                   = var.project
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = var.task_cpu
  memory                   = var.task_memory
  execution_role_arn       = aws_iam_role.exec.arn

  # Fargate Graviton (ARM64): native build on Apple Silicon → no QEMU, faster builds + ~20% cheaper.
  runtime_platform {
    operating_system_family = "LINUX"
    cpu_architecture        = "ARM64"
  }

  container_definitions = jsonencode(local.containers)
}

resource "aws_ecs_service" "app" {
  name            = var.project
  cluster         = aws_ecs_cluster.app.id
  task_definition = aws_ecs_task_definition.app.arn
  desired_count   = 1
  launch_type     = "FARGATE"

  network_configuration {
    subnets          = data.aws_subnets.default.ids
    security_groups  = [aws_security_group.task.id]
    assign_public_ip = true
  }

  deployment_minimum_healthy_percent = 0
  deployment_maximum_percent         = 100

  # deploy.sh runs `update-service --force-new-deployment` to roll new image.
  # Ignore runtime drift so `terraform apply` doesn't fight CI deploys.
  lifecycle {
    ignore_changes = [desired_count]
  }
}
