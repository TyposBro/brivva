#!/usr/bin/env bash
# Build server-rs image → push to ECR → force new ECS deployment.
# Infra (cluster, service, task def, secrets) is owned by terraform — see infra/.
# Run `terraform -chdir=infra apply` first.

set -euo pipefail

# ── Config ────────────────────────────────────────────────
AWS_REGION="${AWS_REGION:-us-east-1}"
CLUSTER="${CLUSTER:-brivva}"
SERVICE="${SERVICE:-brivva}"
PROJECT="${PROJECT:-brivva}"

# Resolve account + ECR base. Prefer terraform output, fall back to STS.
cd "$(dirname "$0")"
if [[ -z "${AWS_ACCOUNT:-}" ]]; then
    if [[ -f infra/terraform.tfstate ]]; then
        AWS_ACCOUNT="$(terraform -chdir=infra output -raw account_id 2>/dev/null || true)"
    fi
fi
if [[ -z "${AWS_ACCOUNT:-}" ]]; then
    AWS_ACCOUNT="$(aws sts get-caller-identity --query Account --output text)"
fi
ECR_BASE="${AWS_ACCOUNT}.dkr.ecr.${AWS_REGION}.amazonaws.com"

# ── Parse args ────────────────────────────────────────────
SKIP_BUILD=false

usage() {
    echo "Usage: $0 [--skip-build]"
    echo ""
    echo "  --skip-build    Skip Docker build, just force a new ECS deployment"
    echo ""
    echo "Env overrides: AWS_REGION, AWS_ACCOUNT, CLUSTER, SERVICE, PROJECT"
    exit 1
}

while [[ $# -gt 0 ]]; do
    case $1 in
        --skip-build) SKIP_BUILD=true; shift ;;
        -h|--help) usage ;;
        *) echo "Unknown arg: $1"; usage ;;
    esac
done

# ── Build & Push ──────────────────────────────────────────
if [[ "$SKIP_BUILD" == false ]]; then
    echo "==> ECR login ($AWS_REGION)"
    aws ecr get-login-password --region "$AWS_REGION" \
        | docker login --username AWS --password-stdin "$ECR_BASE"

    # Fargate requires linux/amd64 — cross-compile from Apple Silicon
    PLATFORM="linux/amd64"
    IMAGE="$ECR_BASE/$PROJECT/server-rs:latest"

    echo "==> Building server-rs ($PLATFORM) → $IMAGE"
    docker buildx build --platform "$PLATFORM" \
        -t "$IMAGE" \
        -f server-rs/Dockerfile --push .
fi

# ── Roll deployment ───────────────────────────────────────
echo "==> Forcing new deployment on $CLUSTER/$SERVICE"
aws ecs update-service \
    --region "$AWS_REGION" \
    --cluster "$CLUSTER" \
    --service "$SERVICE" \
    --force-new-deployment \
    --query 'service.taskDefinition' \
    --output text

echo "==> Waiting for service to stabilize..."
aws ecs wait services-stable --region "$AWS_REGION" --cluster "$CLUSTER" --services "$SERVICE"

echo "==> Done. Tail logs:"
echo "    aws logs tail /ecs/$PROJECT --follow --region $AWS_REGION"
