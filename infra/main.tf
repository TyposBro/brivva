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
  account_id               = data.aws_caller_identity.current.account_id
  enable_cloudflared       = nonsensitive(length(var.tunnel_creds) > 0) && length(var.tunnel_id) > 0
  ecr_server               = aws_ecr_repository.server.repository_url
  secret_arn               = aws_secretsmanager_secret.env.arn
  gpu_bad_provider_enabled = var.gpu_bad_provider_drill != "none"
  gpu_secret_arn           = local.gpu_bad_provider_enabled ? aws_secretsmanager_secret.gpu_drill[0].arn : aws_secretsmanager_secret.env.arn
  enable_gpu_capacity      = true
  ecs_gpu_ami_id           = local.enable_gpu_capacity ? jsondecode(data.aws_ssm_parameter.ecs_gpu_ami[0].value).image_id : null

  # Phase 6C private GPU rehearsal keeps endpoint blast radius/cost bounded to
  # one selected GPU subnet/AZ. The S3 gateway endpoint route table IDs are
  # explicit because Terraform cannot safely infer only the GPU subnet route
  # table in the default VPC without risking unrelated/customer paths.
  gpu_subnet_ids = var.gpu_private_endpoints_enabled ? slice(data.aws_subnets.gpu.ids, 0, 1) : data.aws_subnets.gpu.ids
  gpu_private_endpoint_services = toset(concat(
    ["ecs", "ecs-agent", "ecs-telemetry", "ecr.api", "ecr.dkr", "logs", "secretsmanager"],
    var.gpu_private_endpoint_debug_services_enabled ? ["ssm", "ec2messages", "ssmmessages"] : []
  ))
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

# GPU/NVENC FFmpeg base for the ECS-on-EC2 production launch path.
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
    TURN_USER            = var.turn_user
    TURN_PASSWORD        = var.turn_password
  })
}

resource "aws_secretsmanager_secret" "gpu_drill" {
  count                   = local.gpu_bad_provider_enabled ? 1 : 0
  name                    = "${var.project}/env-gpu-drill"
  description             = "Isolated brivva-gpu bad-provider drill secret. Primary brivva never references this secret."
  recovery_window_in_days = 0
}

resource "aws_secretsmanager_secret_version" "gpu_drill" {
  count     = local.gpu_bad_provider_enabled ? 1 : 0
  secret_id = aws_secretsmanager_secret.gpu_drill[0].id
  secret_string = jsonencode({
    SONIOX_API_KEY       = var.gpu_bad_provider_drill == "soniox" ? "brivva-gpu-drill-bad-soniox" : var.soniox_api_key
    ELEVENLABS_API_KEY   = var.gpu_bad_provider_drill == "elevenlabs" ? "brivva-gpu-drill-bad-elevenlabs" : var.elevenlabs_api_key
    GOOGLE_CLIENT_ID     = var.google_client_id
    GOOGLE_CLIENT_SECRET = var.google_client_secret
    TUNNEL_CREDS         = ""
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
      Effect = "Allow"
      Action = ["secretsmanager:GetSecretValue"]
      Resource = concat(
        [aws_secretsmanager_secret.env.arn],
        local.gpu_bad_provider_enabled ? [aws_secretsmanager_secret.gpu_drill[0].arn] : []
      )
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

# ── Security group (ECS GPU instance) ──────────────────────
# HTTP ingress is via cloudflared. WebRTC media is not HTTP/WebSocket; the
# browser and GPU host need direct ICE/UDP connectivity. server-rs pins ICE
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
    description = "Private ECS GPU rehearsal HTTP/WSS signaling inside VPC only"
    from_port   = 3000
    to_port     = 3000
    protocol    = "tcp"
    cidr_blocks = [data.aws_vpc.default.cidr_block]
  }

  ingress {
    description = "TURN/STUN UDP for restrictive NAT WebRTC clients"
    from_port   = 3478
    to_port     = 3478
    protocol    = "udp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  ingress {
    description = "TURN TCP fallback for restrictive NAT WebRTC clients"
    from_port   = 3478
    to_port     = 3478
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  ingress {
    description = "TURN UDP relay ports"
    from_port   = var.turn_relay_port_min
    to_port     = var.turn_relay_port_max
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

resource "aws_security_group" "gpu_private_endpoints" {
  count       = var.gpu_private_endpoints_enabled ? 1 : 0
  name        = "${var.project}-gpu-private-endpoints"
  description = "Phase 6C private ECS GPU rehearsal AWS service endpoints."
  vpc_id      = data.aws_vpc.default.id

  ingress {
    description     = "HTTPS from private GPU ECS instances/tasks"
    from_port       = 443
    to_port         = 443
    protocol        = "tcp"
    security_groups = [aws_security_group.task.id]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

resource "aws_vpc_endpoint" "gpu_private_interface" {
  for_each            = var.gpu_private_endpoints_enabled ? local.gpu_private_endpoint_services : toset([])
  vpc_id              = data.aws_vpc.default.id
  service_name        = "com.amazonaws.${var.region}.${each.key}"
  vpc_endpoint_type   = "Interface"
  subnet_ids          = local.gpu_subnet_ids
  security_group_ids  = [aws_security_group.gpu_private_endpoints[0].id]
  private_dns_enabled = true

  tags = {
    Name  = "${var.project}-gpu-${each.key}"
    Scope = "phase-6c-gpu-rehearsal"
  }
}

resource "aws_vpc_endpoint" "gpu_private_s3" {
  count             = var.gpu_private_endpoints_enabled ? 1 : 0
  vpc_id            = data.aws_vpc.default.id
  service_name      = "com.amazonaws.${var.region}.s3"
  vpc_endpoint_type = "Gateway"
  route_table_ids   = var.gpu_private_endpoint_route_table_ids

  lifecycle {
    precondition {
      condition     = length(var.gpu_private_endpoint_route_table_ids) > 0
      error_message = "gpu_private_endpoint_route_table_ids must explicitly list the GPU subnet route table(s); do not infer/mutate all default VPC route tables."
    }
  }

  tags = {
    Name  = "${var.project}-gpu-s3"
    Scope = "phase-6c-gpu-rehearsal"
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
  vpc_zone_identifier = local.gpu_subnet_ids
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
    resourceRequirements = [
      { type = "GPU", value = "1" }
    ]
    portMappings = [
      { containerPort = 3000, hostPort = 3000, protocol = "tcp" }
    ]
    environment = [
      # Phase 3: server-rs is stateless. State lives in D1 via the Workers API.
      # server-rs calls ${WORKERS_API_URL}/internal/* with INTERNAL_SECRET and
      # verifies host JWTs with JWT_SECRET — both shared with the Worker.
      { name = "BROADCAST_DELAY_MS", value = tostring(var.broadcast_delay_ms) },
      { name = "FRONTEND_URL", value = var.frontend_url },
      { name = "WORKERS_API_URL", value = var.workers_api_url },
      { name = "BRIVVA_SESSION_LOGS", value = var.session_logs_enabled ? "1" : "0" },
      { name = "BRIVVA_SESSION_LOG_VERBOSE", value = var.session_logs_verbose ? "1" : "0" },
      { name = "BRIVVA_WEBRTC_STUN_URLS", value = "stun:stun.l.google.com:19302" },
      { name = "BRIVVA_WEBRTC_ICE_SERVERS", value = var.webrtc_ice_servers },
      { name = "BRIVVA_WEBRTC_UDP_PORT_MIN", value = "40000" },
      { name = "BRIVVA_WEBRTC_UDP_PORT_MAX", value = "40100" },
      # ECS GPU task assignment exposes /dev/nvidia*, but NVENC libraries are
      # mounted only when the NVIDIA runtime includes the video capability.
      { name = "NVIDIA_DRIVER_CAPABILITIES", value = "video,compute,utility" },
      { name = "BRIVVA_VIDEO_ENCODER", value = "nvenc" },
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

  turn_container = {
    name       = "coturn"
    image      = "coturn/coturn:4.6"
    essential  = false
    entryPoint = ["sh", "-c"]
    command = [
      "turnserver -n --log-file=stdout --listening-port=3478 --min-port=${var.turn_relay_port_min} --max-port=${var.turn_relay_port_max} --realm=${var.turn_realm} --user=$${TURN_USER}:$${TURN_PASSWORD} --lt-cred-mech --fingerprint --no-multicast-peers --no-cli"
    ]
    portMappings = [
      { containerPort = 3478, hostPort = 3478, protocol = "udp" },
      { containerPort = 3478, hostPort = 3478, protocol = "tcp" },
    ]
    secrets = [
      { name = "TURN_USER", valueFrom = "${local.secret_arn}:TURN_USER::" },
      { name = "TURN_PASSWORD", valueFrom = "${local.secret_arn}:TURN_PASSWORD::" },
    ]
    logConfiguration = {
      logDriver = "awslogs"
      options = {
        awslogs-group         = aws_cloudwatch_log_group.app.name
        awslogs-region        = var.region
        awslogs-stream-prefix = "coturn"
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
    var.turn_enabled ? [local.turn_container] : [],
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
      { name = "BRIVVA_WEBRTC_ICE_SERVERS", value = var.webrtc_ice_servers },
      { name = "BRIVVA_WEBRTC_UDP_PORT_MIN", value = "40000" },
      { name = "BRIVVA_WEBRTC_UDP_PORT_MAX", value = "40100" },
      { name = "NVIDIA_DRIVER_CAPABILITIES", value = "video,compute,utility" },
      { name = "BRIVVA_VIDEO_ENCODER", value = "nvenc" },
      # Phase 6C cloud validation is shadow/fake-sink only. Production route
      # selection remains disabled until a later explicit live-route gate.
      { name = "BRIVVA_V2_GPU_WORKERS", value = "0" },
      { name = "BRIVVA_V2_GPU_REHEARSAL_MODE", value = "shadow" },
      { name = "BRIVVA_V2_GPU_REHEARSAL_SINK", value = "fake" },
      { name = "BRIVVA_GPU_BAD_PROVIDER_DRILL", value = var.gpu_bad_provider_drill },
    ]
    secrets = [
      { name = "SONIOX_API_KEY", valueFrom = "${local.gpu_secret_arn}:SONIOX_API_KEY::" },
      { name = "ELEVENLABS_API_KEY", valueFrom = "${local.gpu_secret_arn}:ELEVENLABS_API_KEY::" },
      { name = "JWT_SECRET", valueFrom = "${local.gpu_secret_arn}:JWT_SECRET::" },
      { name = "INTERNAL_SECRET", valueFrom = "${local.gpu_secret_arn}:INTERNAL_SECRET::" },
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
  requires_compatibilities = ["EC2"]
  network_mode             = "host"
  cpu                      = var.gpu_task_cpu
  memory                   = var.gpu_task_memory
  execution_role_arn       = aws_iam_role.exec.arn

  # x86_64 ECS-on-EC2 keeps production ffmpeg behavior aligned with Ubuntu dev.
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
    # owns launch compatibility/network mode for the GPU-only service.
    ignore_changes = [container_definitions, tags, volume]
  }
}

resource "aws_ecs_task_definition" "gpu_rehearsal" {
  count                    = (var.gpu_rehearsal_service_enabled || local.gpu_bad_provider_enabled) ? 1 : 0
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
    precondition {
      condition     = !local.gpu_bad_provider_enabled || var.gpu_rehearsal_service_enabled
      error_message = "Bad-provider drills are only allowed through the isolated brivva-gpu rehearsal task definition."
    }

    precondition {
      condition     = !local.gpu_bad_provider_enabled || local.gpu_secret_arn != local.secret_arn
      error_message = "Bad-provider drill must use separate brivva/env-gpu-drill secret, never shared brivva/env."
    }

    # Terraform owns rehearsal container definitions so bad-provider drill env/secrets
    # create a new isolated task revision. Primary app task defs are still handled
    # separately above with container_definitions ignored for deploy.sh.
    ignore_changes = [tags]
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
  desired_count   = var.gpu_service_desired_count
  capacity_provider_strategy {
    capacity_provider = aws_ecs_capacity_provider.gpu[0].name
    weight            = 1
  }

  # One g4dn.xlarge only: avoid old+new tasks during deploys.
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
