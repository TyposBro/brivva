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

data "aws_subnets" "gpu" {
  filter {
    name   = "vpc-id"
    values = [data.aws_vpc.default.id]
  }

  filter {
    name   = "availability-zone"
    values = var.gpu_availability_zones
  }
}

data "aws_ssm_parameter" "ecs_gpu_ami" {
  count = local.enable_gpu_capacity ? 1 : 0
  name  = "/aws/service/ecs/optimized-ami/amazon-linux-2/gpu/recommended"
}

locals {
  account_id          = data.aws_caller_identity.current.account_id
  enable_cloudflared  = nonsensitive(length(var.tunnel_creds) > 0) && length(var.tunnel_id) > 0
  ecr_server          = aws_ecr_repository.server.repository_url
  secret_arn          = aws_secretsmanager_secret.env.arn
  use_ec2_gpu         = var.ecs_launch_type == "EC2_GPU"
  enable_gpu_capacity = var.gpu_capacity_enabled || local.use_ec2_gpu || var.gpu_rehearsal_service_enabled
  ecs_gpu_ami_id      = local.enable_gpu_capacity ? jsondecode(data.aws_ssm_parameter.ecs_gpu_ami[0].value).image_id : null
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

# Prebuilt ffmpeg amd64 image with native RTMP/RTMPS, referenced by the
# server-runtime-base image. Decouples the 2+hr ffmpeg source build from
# every deploy — rebuilt only when the FFMPEG_VERSION pin or
# infra/ffmpeg-base/Dockerfile changes.
resource "aws_ecr_repository" "ffmpeg_base" {
  name = "${var.project}/ffmpeg-base"
  # MUTABLE so the CI workflow can advance `latest` to the newest build.
  # The version-suffixed tag (e.g. `7.1.1-native-rtmp`) is treated as
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

# GPU/NVENC FFmpeg base for ECS-on-EC2 launch path. Kept separate from the
# CPU/native-RTMP base so Fargate fallback never depends on NVIDIA headers.
resource "aws_ecr_repository" "ffmpeg_gpu_base" {
  name                 = "${var.project}/ffmpeg-gpu-base"
  image_tag_mutability = "MUTABLE"
  image_scanning_configuration {
    scan_on_push = true
  }
}

resource "aws_ecr_lifecycle_policy" "ffmpeg_gpu_base" {
  repository = aws_ecr_repository.ffmpeg_gpu_base.name
  policy = jsonencode({
    rules = [{
      rulePriority = 1
      description  = "Keep last 5 GPU ffmpeg images — rebuilds are rare"
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
      description  = "Keep last 5 build base images — rebuilds are rare"
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
      description  = "Keep last 5 runtime base images — rebuilds are rare"
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

resource "aws_iam_role" "ecs_gpu_instance" {
  count = local.enable_gpu_capacity ? 1 : 0
  name  = "${var.project}-ecs-gpu-instance"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "ec2.amazonaws.com" }
      Action    = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy_attachment" "ecs_gpu_instance_ecs" {
  count      = local.enable_gpu_capacity ? 1 : 0
  role       = aws_iam_role.ecs_gpu_instance[0].name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonEC2ContainerServiceforEC2Role"
}

resource "aws_iam_role_policy_attachment" "ecs_gpu_instance_ssm" {
  count      = local.enable_gpu_capacity ? 1 : 0
  role       = aws_iam_role.ecs_gpu_instance[0].name
  policy_arn = "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"
}

resource "aws_iam_instance_profile" "ecs_gpu" {
  count = local.enable_gpu_capacity ? 1 : 0
  name  = "${var.project}-ecs-gpu"
  role  = aws_iam_role.ecs_gpu_instance[0].name
}

# ── Security group (Fargate task / ECS GPU instance) ───────
# HTTP ingress is via cloudflared. WebRTC media is not HTTP/WebSocket; the
# browser and Fargate peer need direct ICE/UDP connectivity. server-rs pins ICE
# UDP sockets to 40000-40100 and advertises server-reflexive candidates via
# STUN, so expose only that bounded range.

resource "aws_security_group" "task" {
  name        = "${var.project}-task"
  description = "Egress-only for Fargate task (cloudflared handles ingress)."
  vpc_id      = data.aws_vpc.default.id

  ingress {
    description = "WebRTC ICE UDP media from host browsers"
    from_port   = 40000
    to_port     = 40100
    protocol    = "udp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  ingress {
    description = "Direct ECS GPU rehearsal HTTP/WSS signaling"
    from_port   = 3000
    to_port     = 3000
    protocol    = "tcp"
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

resource "aws_launch_template" "ecs_gpu" {
  count         = local.enable_gpu_capacity ? 1 : 0
  name_prefix   = "${var.project}-ecs-gpu-"
  image_id      = local.ecs_gpu_ami_id
  instance_type = var.gpu_instance_type

  iam_instance_profile {
    name = aws_iam_instance_profile.ecs_gpu[0].name
  }

  network_interfaces {
    associate_public_ip_address = true
    security_groups             = [aws_security_group.task.id]
  }

  user_data = base64encode(<<-EOF
    #!/bin/bash
    echo ECS_CLUSTER=${aws_ecs_cluster.app.name} >> /etc/ecs/ecs.config
    echo ECS_ENABLE_GPU_SUPPORT=true >> /etc/ecs/ecs.config
  EOF
  )
}

resource "aws_autoscaling_group" "ecs_gpu" {
  count               = local.enable_gpu_capacity ? 1 : 0
  name                = "${var.project}-ecs-gpu"
  vpc_zone_identifier = data.aws_subnets.gpu.ids
  min_size            = 0
  max_size            = 1
  desired_capacity    = var.gpu_desired_capacity

  launch_template {
    id      = aws_launch_template.ecs_gpu[0].id
    version = "$Latest"
  }

  tag {
    key                 = "Name"
    value               = "${var.project}-ecs-gpu"
    propagate_at_launch = true
  }

  lifecycle {
    # ECS capacity provider adds AmazonECSManaged. Don't fight it.
    ignore_changes = [tag]
  }
}

resource "aws_ecs_capacity_provider" "gpu" {
  count = local.enable_gpu_capacity ? 1 : 0
  name  = "${var.project}-gpu"

  auto_scaling_group_provider {
    auto_scaling_group_arn         = aws_autoscaling_group.ecs_gpu[0].arn
    managed_termination_protection = "DISABLED"

    managed_scaling {
      status          = "ENABLED"
      target_capacity = 100
    }
  }
}

resource "aws_ecs_cluster_capacity_providers" "app" {
  count        = local.enable_gpu_capacity ? 1 : 0
  cluster_name = aws_ecs_cluster.app.name

  capacity_providers = [aws_ecs_capacity_provider.gpu[0].name]

  default_capacity_provider_strategy {
    capacity_provider = aws_ecs_capacity_provider.gpu[0].name
    weight            = 1
  }
}

locals {
  server_container = {
    name      = "server-rs"
    image     = "${local.ecr_server}:latest"
    essential = true
    resourceRequirements = local.use_ec2_gpu ? [
      { type = "GPU", value = "1" }
    ] : []
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
      { name = "BRIVVA_SESSION_LOGS", value = var.session_logs_enabled ? "1" : "0" },
      { name = "BRIVVA_SESSION_LOG_VERBOSE", value = var.session_logs_verbose ? "1" : "0" },
      { name = "BRIVVA_WEBRTC_STUN_URLS", value = "stun:stun.l.google.com:19302" },
      { name = "BRIVVA_WEBRTC_UDP_PORT_MIN", value = "40000" },
      { name = "BRIVVA_WEBRTC_UDP_PORT_MAX", value = "40100" },
      { name = "BRIVVA_VIDEO_ENCODER", value = local.use_ec2_gpu ? "nvenc" : "x264" },
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

  gpu_rehearsal_server_container = merge(local.server_container, {
    resourceRequirements = [{ type = "GPU", value = "1" }]
    environment = [
      { name = "BROADCAST_DELAY_MS", value = tostring(var.broadcast_delay_ms) },
      { name = "FRONTEND_URL", value = var.frontend_url },
      { name = "WORKERS_API_URL", value = var.workers_api_url },
      { name = "BRIVVA_SESSION_LOGS", value = var.session_logs_enabled ? "1" : "0" },
      { name = "BRIVVA_SESSION_LOG_VERBOSE", value = var.session_logs_verbose ? "1" : "0" },
      { name = "BRIVVA_WEBRTC_STUN_URLS", value = "stun:stun.l.google.com:19302" },
      { name = "BRIVVA_WEBRTC_UDP_PORT_MIN", value = "40000" },
      { name = "BRIVVA_WEBRTC_UDP_PORT_MAX", value = "40100" },
      { name = "BRIVVA_VIDEO_ENCODER", value = "nvenc" },
    ]
    logConfiguration = {
      logDriver = "awslogs"
      options = {
        awslogs-group         = aws_cloudwatch_log_group.app.name
        awslogs-region        = var.region
        awslogs-stream-prefix = "gpu-server-rs"
      }
    }
  })

  gpu_rehearsal_containers = [local.gpu_rehearsal_server_container]
}

resource "aws_ecs_task_definition" "app" {
  family                   = var.project
  requires_compatibilities = [local.use_ec2_gpu ? "EC2" : "FARGATE"]
  network_mode             = local.use_ec2_gpu ? "host" : "awsvpc"
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

  lifecycle {
    # deploy.sh owns runtime task-definition revisions (image, env drift). Terraform
    # owns launch compatibility/network mode when intentionally switching Fargate ↔ EC2_GPU.
    ignore_changes = [container_definitions, tags, volume]
  }
}

resource "aws_ecs_task_definition" "gpu_rehearsal" {
  count                    = var.gpu_rehearsal_service_enabled ? 1 : 0
  family                   = "${var.project}-gpu"
  requires_compatibilities = ["EC2"]
  network_mode             = "host"
  cpu                      = var.gpu_task_cpu
  memory                   = var.gpu_task_memory
  execution_role_arn       = aws_iam_role.exec.arn

  runtime_platform {
    operating_system_family = "LINUX"
    cpu_architecture        = "X86_64"
  }

  container_definitions = jsonencode(local.gpu_rehearsal_containers)

  lifecycle {
    ignore_changes = [container_definitions, tags]
  }
}

resource "aws_ecs_service" "gpu_rehearsal" {
  count           = var.gpu_rehearsal_service_enabled ? 1 : 0
  name            = "${var.project}-gpu"
  cluster         = aws_ecs_cluster.app.id
  task_definition = aws_ecs_task_definition.gpu_rehearsal[0].arn
  desired_count   = var.gpu_rehearsal_desired_count

  capacity_provider_strategy {
    capacity_provider = aws_ecs_capacity_provider.gpu[0].name
    weight            = 1
  }

  deployment_minimum_healthy_percent = 0
  deployment_maximum_percent         = 100

  deployment_circuit_breaker {
    enable   = true
    rollback = true
  }

}

resource "aws_ecs_service" "app" {
  name            = var.project
  cluster         = aws_ecs_cluster.app.id
  task_definition = aws_ecs_task_definition.app.arn
  desired_count   = 1
  launch_type     = local.use_ec2_gpu ? null : "FARGATE"

  dynamic "capacity_provider_strategy" {
    for_each = local.use_ec2_gpu ? [1] : []
    content {
      capacity_provider = aws_ecs_capacity_provider.gpu[0].name
      weight            = 1
    }
  }

  dynamic "network_configuration" {
    for_each = local.use_ec2_gpu ? [] : [1]
    content {
      subnets          = data.aws_subnets.default.ids
      security_groups  = [aws_security_group.task.id]
      assign_public_ip = true
    }
  }

  # Account Fargate quota is 30 vCPU and this task is 16 vCPU, so a normal
  # 100/200 rolling deploy cannot place old+new tasks concurrently.
  deployment_minimum_healthy_percent = 0
  deployment_maximum_percent         = 100

  # ECS auto-rolls back failed task-def revisions per docs/runbook.md. Set
  # manually in prod before this block existed; capturing here to match.
  deployment_circuit_breaker {
    enable   = true
    rollback = true
  }

  # deploy.sh runs `update-service --force-new-deployment` to roll new image.
  # Ignore runtime drift so `terraform apply` doesn't fight CI deploys.
  lifecycle {
    ignore_changes = [desired_count, task_definition]
  }
}
