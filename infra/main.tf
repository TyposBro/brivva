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

# Prebuilt ffmpeg amd64 image with --enable-librtmp, referenced by the
# server-runtime-base image. Decouples the 2+hr ffmpeg source build from
# every deploy — rebuilt only when the FFMPEG_VERSION pin or
# infra/ffmpeg-base/Dockerfile changes.
resource "aws_ecr_repository" "ffmpeg_base" {
  name = "${var.project}/ffmpeg-base"
  # MUTABLE so the CI workflow can advance `latest` to the newest build.
  # The version-suffixed tag (e.g. `6.1.2-librtmp`) is treated as
  # immutable-by-convention: bump FFMPEG_VERSION rather than rebuilding
  # over an existing tag.
  image_tag_mutability = "MUTABLE"
  image_scanning_configuration {
    scan_on_push = true
  }
}

resource "aws_ecr_lifecycle_policy" "ffmpeg_base" {
  repository = aws_ecr_repository.ffmpeg_base.name
  policy = jsonencode({
    rules = [{
      rulePriority = 1
      description  = "Keep last 5 versioned images — rebuilds are rare"
      selection = {
        tagStatus   = "any"
        countType   = "imageCountMoreThan"
        countNumber = 5
      }
      action = { type = "expire" }
    }]
  })
}

# Prebuilt Rust build toolchain for server-rs Docker builds. Keeps normal
# deploys from reinstalling cargo-chef, cargo-zigbuild, ziglang, and C
# build dependencies every time the GitHub Actions cache is cold.
resource "aws_ecr_repository" "server_build_base" {
  name                 = "${var.project}/server-build-base"
  image_tag_mutability = "MUTABLE"
  image_scanning_configuration {
    scan_on_push = true
  }
}

resource "aws_ecr_lifecycle_policy" "server_build_base" {
  repository = aws_ecr_repository.server_build_base.name
  policy = jsonencode({
    rules = [{
      rulePriority = 1
      description  = "Keep last 5 base images"
      selection = {
        tagStatus   = "any"
        countType   = "imageCountMoreThan"
        countNumber = 5
      }
      action = { type = "expire" }
    }]
  })
}

# Prebuilt runtime foundation for server-rs. Contains Debian runtime libs,
# CJK fonts, and the custom ffmpeg/ffprobe binaries copied from ffmpeg-base.
resource "aws_ecr_repository" "server_runtime_base" {
  name                 = "${var.project}/server-runtime-base"
  image_tag_mutability = "MUTABLE"
  image_scanning_configuration {
    scan_on_push = true
  }
}

resource "aws_ecr_lifecycle_policy" "server_runtime_base" {
  repository = aws_ecr_repository.server_runtime_base.name
  policy = jsonencode({
    rules = [{
      rulePriority = 1
      description  = "Keep last 5 base images"
      selection = {
        tagStatus   = "any"
        countType   = "imageCountMoreThan"
        countNumber = 5
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
    JWT_SECRET           = var.jwt_secret
    INTERNAL_SECRET      = var.internal_secret
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
# HTTP ingress stays private to cloudflared. WebRTC media needs direct UDP
# because Cloudflare Tunnel only carries the WHIP HTTP signaling path.

resource "aws_security_group" "task" {
  name        = "${var.project}-task"
  description = "Egress-only for Fargate task (cloudflared handles ingress)."
  vpc_id      = data.aws_vpc.default.id

  ingress {
    description = "WebRTC ICE/SRTP media"
    from_port   = var.webrtc_udp_port_min
    to_port     = var.webrtc_udp_port_max
    protocol    = "udp"
    cidr_blocks = ["0.0.0.0/0"]
  }

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

  # Required for ECS/ContainerInsights metrics (RunningTaskCount, etc.)
  # that the brivva-ecs-running-tasks-low alarm depends on.
  setting {
    name  = "containerInsights"
    value = "enabled"
  }
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
      # Phase 3: Fargate is stateless. State lives in D1 via the Workers API.
      # Fargate calls ${WORKERS_API_URL}/internal/* with INTERNAL_SECRET and
      # verifies host JWTs with JWT_SECRET — both shared with the Worker.
      { name = "BROADCAST_DELAY_MS", value = tostring(var.broadcast_delay_ms) },
      { name = "FRONTEND_URL", value = var.frontend_url },
      { name = "WORKERS_API_URL", value = var.workers_api_url },
      { name = "BRIVVA_WEBRTC_UDP_PORT_MIN", value = tostring(var.webrtc_udp_port_min) },
      { name = "BRIVVA_WEBRTC_UDP_PORT_MAX", value = tostring(var.webrtc_udp_port_max) },
      { name = "BRIVVA_WEBRTC_STUN_URLS", value = var.webrtc_stun_urls },
    ]
    secrets = [
      { name = "SONIOX_API_KEY", valueFrom = "${local.secret_arn}:SONIOX_API_KEY::" },
      { name = "ELEVENLABS_API_KEY", valueFrom = "${local.secret_arn}:ELEVENLABS_API_KEY::" },
      { name = "JWT_SECRET", valueFrom = "${local.secret_arn}:JWT_SECRET::" },
      { name = "INTERNAL_SECRET", valueFrom = "${local.secret_arn}:INTERNAL_SECRET::" },
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

  # Init container writes cloudflared config + creds to the shared volume.
  # cloudflared image is distroless (no sh) so we can't render config in it.
  cloudflared_init_container = {
    name       = "cloudflared-init"
    image      = "public.ecr.aws/docker/library/alpine:3"
    essential  = false
    entryPoint = ["sh", "-c"]
    command = [
      "echo \"$TUNNEL_CREDS\" > /shared/creds.json && printf 'tunnel: ${var.tunnel_id}\\ncredentials-file: /shared/creds.json\\ningress:\\n  - hostname: ${var.domain}\\n    service: http://localhost:3000\\n  - service: http_status:404\\n' > /shared/config.yml"
    ]
    mountPoints = [
      { sourceVolume = "cloudflared-config", containerPath = "/shared", readOnly = false }
    ]
    secrets = [
      { name = "TUNNEL_CREDS", valueFrom = "${local.secret_arn}:TUNNEL_CREDS::" }
    ]
    logConfiguration = {
      logDriver = "awslogs"
      options = {
        awslogs-group         = aws_cloudwatch_log_group.app.name
        awslogs-region        = var.region
        awslogs-stream-prefix = "cloudflared-init"
      }
    }
  }

  cloudflared_container = {
    name      = "cloudflared"
    image     = "cloudflare/cloudflared:latest"
    essential = true
    # Image ENTRYPOINT = ["cloudflared", "--no-autoupdate"].
    # We pass only the `tunnel ... run` subcommand as args.
    command = [
      "tunnel", "--config", "/shared/config.yml", "run"
    ]
    mountPoints = [
      { sourceVolume = "cloudflared-config", containerPath = "/shared", readOnly = true }
    ]
    dependsOn = [
      { containerName = "cloudflared-init", condition = "SUCCESS" },
      { containerName = "server-rs", condition = "START" }
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
    local.enable_cloudflared ? [local.cloudflared_init_container] : [],
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

  # x86_64 Fargate keeps production ffmpeg behavior aligned with Ubuntu dev.
  runtime_platform {
    operating_system_family = "LINUX"
    cpu_architecture        = "X86_64"
  }

  # Ephemeral shared volume for cloudflared config + creds handoff from init container.
  dynamic "volume" {
    for_each = local.enable_cloudflared ? [1] : []
    content {
      name = "cloudflared-config"
    }
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

  deployment_minimum_healthy_percent = 100
  deployment_maximum_percent         = 200

  # ECS auto-rolls back failed task-def revisions per docs/runbook.md. Set
  # manually in prod before this block existed; capturing here to match.
  deployment_circuit_breaker {
    enable   = true
    rollback = true
  }

  # deploy.sh runs `update-service --force-new-deployment` to roll new image.
  # Ignore runtime drift so `terraform apply` doesn't fight CI deploys.
  lifecycle {
    ignore_changes = [desired_count]
  }
}
