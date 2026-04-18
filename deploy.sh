#!/usr/bin/env bash
set -euo pipefail

# ── Config ────────────────────────────────────────────────
AWS_REGION="ap-northeast-2"
AWS_ACCOUNT="132593557399"
ECR_BASE="${AWS_ACCOUNT}.dkr.ecr.${AWS_REGION}.amazonaws.com"
CLUSTER="brivva"
SERVICE="brivva"
TASK_FAMILY="brivva"

# ── Parse args ────────────────────────────────────────────
SKIP_BUILD=false
ONLY=""

usage() {
    echo "Usage: $0 [--skip-build] [--only server-rs|stt-wrapper]"
    echo ""
    echo "  --skip-build    Skip Docker build, just update ECS task and deploy"
    echo "  --only NAME     Build and push only one image (server-rs or stt-wrapper)"
    echo ""
    echo "Examples:"
    echo "  $0                          # Build all, push, deploy"
    echo "  $0 --only stt-wrapper       # Rebuild only stt-wrapper, deploy"
    echo "  $0 --skip-build             # Just force new ECS deployment (same images)"
    exit 1
}

while [[ $# -gt 0 ]]; do
    case $1 in
        --skip-build) SKIP_BUILD=true; shift ;;
        --only) ONLY="$2"; shift 2 ;;
        -h|--help) usage ;;
        *) echo "Unknown arg: $1"; usage ;;
    esac
done

cd "$(dirname "$0")"

# ── ECR Login ─────────────────────────────────────────────
echo "==> ECR login"
aws ecr get-login-password --region "$AWS_REGION" \
    | docker login --username AWS --password-stdin "$ECR_BASE"

# ── Build & Push ──────────────────────────────────────────
if [[ "$SKIP_BUILD" == false ]]; then

    # Fargate requires linux/amd64 — cross-compile from Apple Silicon
    PLATFORM="linux/amd64"

    if [[ -z "$ONLY" || "$ONLY" == "server-rs" ]]; then
        echo "==> Building server-rs (${PLATFORM})"
        docker buildx build --platform "$PLATFORM" \
            -t "$ECR_BASE/brivva/server-rs:latest" \
            -f server-rs/Dockerfile --push .
    fi

    if [[ -z "$ONLY" || "$ONLY" == "stt-wrapper" ]]; then
        echo "==> Building stt-wrapper (${PLATFORM})"
        docker buildx build --platform "$PLATFORM" \
            -t "$ECR_BASE/brivva/stt-wrapper:latest" \
            -f stt-wrapper/Dockerfile --push stt-wrapper/
    fi

fi

# ── Register new task definition ──────────────────────────
echo "==> Registering new task definition"
aws ecs register-task-definition \
    --family "$TASK_FAMILY" \
    --requires-compatibilities FARGATE \
    --network-mode awsvpc \
    --cpu 1024 \
    --memory 2048 \
    --execution-role-arn "arn:aws:iam::${AWS_ACCOUNT}:role/brivva-ecs-execution" \
    --container-definitions '[
        {
            "name": "server-rs",
            "image": "'"$ECR_BASE"'/brivva/server-rs:latest",
            "essential": true,
            "portMappings": [{"containerPort": 3000, "hostPort": 3000, "protocol": "tcp"}],
            "environment": [
                {"name": "STT_HOST", "value": "localhost"},
                {"name": "STT_PORT", "value": "8766"},
                {"name": "DATABASE_URL", "value": "sqlite:/data/brivva.db?mode=rwc"},
                {"name": "BROADCAST_DELAY_MS", "value": "5000"},
                {"name": "GOOGLE_REDIRECT_URI", "value": "https://brivva-server.milliytechnology.org/auth/youtube/callback"},
                {"name": "FRONTEND_URL", "value": "https://brivva.pages.dev"}
            ],
            "secrets": [
                {"name": "ELEVENLABS_API_KEY", "valueFrom": "arn:aws:secretsmanager:ap-northeast-2:132593557399:secret:brivva/env-ItLiZU:ELEVENLABS_API_KEY::"},
                {"name": "GOOGLE_TRANSLATE_API_KEY", "valueFrom": "arn:aws:secretsmanager:ap-northeast-2:132593557399:secret:brivva/env-ItLiZU:GOOGLE_TRANSLATE_API_KEY::"},
                {"name": "GOOGLE_CLIENT_ID", "valueFrom": "arn:aws:secretsmanager:ap-northeast-2:132593557399:secret:brivva/env-ItLiZU:GOOGLE_CLIENT_ID::"},
                {"name": "GOOGLE_CLIENT_SECRET", "valueFrom": "arn:aws:secretsmanager:ap-northeast-2:132593557399:secret:brivva/env-ItLiZU:GOOGLE_CLIENT_SECRET::"}
            ],
            "logConfiguration": {
                "logDriver": "awslogs",
                "options": {
                    "awslogs-group": "/ecs/brivva",
                    "awslogs-region": "ap-northeast-2",
                    "awslogs-stream-prefix": "server-rs"
                }
            }
        },
        {
            "name": "stt-wrapper",
            "image": "'"$ECR_BASE"'/brivva/stt-wrapper:latest",
            "essential": true,
            "portMappings": [{"containerPort": 8766, "hostPort": 8766, "protocol": "tcp"}],
            "environment": [],
            "secrets": [
                {"name": "DEEPGRAM_API_KEY", "valueFrom": "arn:aws:secretsmanager:ap-northeast-2:132593557399:secret:brivva/env-ItLiZU:DEEPGRAM_API_KEY::"}
            ],
            "logConfiguration": {
                "logDriver": "awslogs",
                "options": {
                    "awslogs-group": "/ecs/brivva",
                    "awslogs-region": "ap-northeast-2",
                    "awslogs-stream-prefix": "stt-wrapper"
                }
            }
        },
        {
            "name": "cloudflared",
            "image": "'"$ECR_BASE"'/brivva/cloudflared:latest",
            "essential": true,
            "portMappings": [],
            "command": ["echo \"$TUNNEL_CREDS\" > /tmp/creds.json && printf '\''tunnel: b6e5239e-b304-4087-8879-97fb649e6ba1\\ncredentials-file: /tmp/creds.json\\ningress:\\n  - hostname: brivva-server.milliytechnology.org\\n    service: http://localhost:3000\\n  - service: http_status:404\\n'\'' > /tmp/config.yml && cloudflared tunnel --no-autoupdate --config /tmp/config.yml run"],
            "environment": [],
            "secrets": [
                {"name": "TUNNEL_CREDS", "valueFrom": "arn:aws:secretsmanager:ap-northeast-2:132593557399:secret:brivva/env-ItLiZU:TUNNEL_CREDS::"}
            ],
            "logConfiguration": {
                "logDriver": "awslogs",
                "options": {
                    "awslogs-group": "/ecs/brivva",
                    "awslogs-region": "ap-northeast-2",
                    "awslogs-stream-prefix": "cloudflared"
                }
            }
        }
    ]' \
    --query 'taskDefinition.revision' \
    --output text

# ── Deploy ────────────────────────────────────────────────
echo "==> Updating ECS service (force new deployment)"
aws ecs update-service \
    --cluster "$CLUSTER" \
    --service "$SERVICE" \
    --task-definition "$TASK_FAMILY" \
    --force-new-deployment \
    --query 'service.taskDefinition' \
    --output text

echo "==> Waiting for deployment to stabilize..."
aws ecs wait services-stable --cluster "$CLUSTER" --services "$SERVICE"

echo "==> Deploy complete!"
echo "    Check logs: aws logs tail /ecs/brivva --follow"
