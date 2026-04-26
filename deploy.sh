#!/usr/bin/env bash
# Sync prod secrets → build server-rs image → push to ECR → register task def → deploy ECS.
# Infra (cluster, service, task def, secrets) is owned by terraform — see infra/.
# Run `terraform -chdir=infra apply` first.

set -euo pipefail

# ── Config ────────────────────────────────────────────────
AWS_REGION="${AWS_REGION:-us-east-1}"
CLUSTER="${CLUSTER:-brivva}"
SERVICE="${SERVICE:-brivva}"
PROJECT="${PROJECT:-brivva}"
FFMPEG_VERSION="${FFMPEG_VERSION:-7.1.1}"
SERVER_BUILD_BASE_TAG="${SERVER_BUILD_BASE_TAG:-rust-1.88-slim-zigbuild-v1}"
SERVER_RUNTIME_BASE_TAG="${SERVER_RUNTIME_BASE_TAG:-bookworm-ffmpeg-7.1.1-v1}"
TASK_FAMILY="${TASK_FAMILY:-$PROJECT}"
ECS_CONTAINER="${ECS_CONTAINER:-server-rs}"

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
SKIP_SECRET_SYNC=false
BUILD_FFMPEG_BASE=false
BUILD_SERVER_BASES=false

usage() {
    echo "Usage: $0 [--skip-build] [--skip-secret-sync] [--build-ffmpeg-base] [--build-server-bases]"
    echo ""
    echo "  --skip-build          Skip Docker build, deploy server-rs:latest"
    echo "  --skip-secret-sync    Do not sync Infisical prod to AWS/Cloudflare before deploy"
    echo "  --build-ffmpeg-base   Rebuild and push the pinned ffmpeg-base image"
    echo "  --build-server-bases  Rebuild and push server build/runtime base images"
    echo ""
    echo "Env overrides: AWS_REGION, AWS_ACCOUNT, CLUSTER, SERVICE, PROJECT, FFMPEG_VERSION,"
    echo "               SERVER_BUILD_BASE_TAG, SERVER_RUNTIME_BASE_TAG"
    exit 1
}

while [[ $# -gt 0 ]]; do
    case $1 in
        --skip-build) SKIP_BUILD=true; shift ;;
        --skip-secret-sync) SKIP_SECRET_SYNC=true; shift ;;
        --build-ffmpeg-base) BUILD_FFMPEG_BASE=true; shift ;;
        --build-server-bases) BUILD_SERVER_BASES=true; shift ;;
        -h|--help) usage ;;
        *) echo "Unknown arg: $1"; usage ;;
    esac
done

if [[ "$SKIP_BUILD" == true && ( "$BUILD_FFMPEG_BASE" == true || "$BUILD_SERVER_BASES" == true ) ]]; then
    echo "--skip-build cannot be combined with base image rebuild flags" >&2
    exit 1
fi

# ── Sync secrets from Infisical ─────────────────────────────
if [[ "$SKIP_SECRET_SYNC" == false ]]; then
    if ! command -v infisical >/dev/null 2>&1; then
        echo "infisical CLI not found; install it or pass --skip-secret-sync" >&2
        exit 1
    fi

    echo "==> Syncing Infisical prod → AWS Secrets Manager"
    ./scripts/sync-infisical-prod-aws.sh

    echo "==> Syncing Infisical prod → Cloudflare Workers secrets"
    ./scripts/sync-infisical-prod-workers.sh
fi

# ── Build & Push ──────────────────────────────────────────
if [[ "$SKIP_BUILD" == false ]]; then
    echo "==> ECR login ($AWS_REGION)"
    aws ecr get-login-password --region "$AWS_REGION" \
        | docker login --username AWS --password-stdin "$ECR_BASE"

    # Keep Fargate on x86_64 so production ffmpeg behavior matches Ubuntu dev.
    PLATFORM="linux/amd64"
    SHA="$(git rev-parse HEAD)"
    IMAGE="$ECR_BASE/$PROJECT/server-rs:$SHA"
    LATEST_IMAGE="$ECR_BASE/$PROJECT/server-rs:latest"
    FFMPEG_BASE_IMAGE="${FFMPEG_BASE_IMAGE:-$ECR_BASE/$PROJECT/ffmpeg-base:${FFMPEG_VERSION}-librtmp}"
    SERVER_BUILD_BASE_IMAGE="${SERVER_BUILD_BASE_IMAGE:-$ECR_BASE/$PROJECT/server-build-base:${SERVER_BUILD_BASE_TAG}}"
    SERVER_RUNTIME_BASE_IMAGE="${SERVER_RUNTIME_BASE_IMAGE:-$ECR_BASE/$PROJECT/server-runtime-base:${SERVER_RUNTIME_BASE_TAG}}"

    if [[ "$BUILD_FFMPEG_BASE" == true ]]; then
        echo "==> Building ffmpeg-base ($PLATFORM) → $FFMPEG_BASE_IMAGE"
        docker buildx build --platform "$PLATFORM" \
            -t "$FFMPEG_BASE_IMAGE" \
            -t "$ECR_BASE/$PROJECT/ffmpeg-base:latest" \
            --build-arg "FFMPEG_VERSION=$FFMPEG_VERSION" \
            -f infra/ffmpeg-base/Dockerfile --push infra/ffmpeg-base
    else
        echo "==> Verifying ffmpeg-base exists → $FFMPEG_BASE_IMAGE"
        aws ecr describe-images \
            --region "$AWS_REGION" \
            --repository-name "$PROJECT/ffmpeg-base" \
            --image-ids "imageTag=${FFMPEG_VERSION}-librtmp" \
            --query 'imageDetails[0].{digest:imageDigest,pushed:imagePushedAt}' \
            --output table
    fi

    if [[ "$BUILD_SERVER_BASES" == true ]]; then
        echo "==> Building server-build-base ($PLATFORM) → $SERVER_BUILD_BASE_IMAGE"
        docker buildx build --platform "$PLATFORM" \
            -t "$SERVER_BUILD_BASE_IMAGE" \
            -t "$ECR_BASE/$PROJECT/server-build-base:latest" \
            -f infra/server-build-base/Dockerfile --push infra/server-build-base

        echo "==> Building server-runtime-base ($PLATFORM) → $SERVER_RUNTIME_BASE_IMAGE"
        docker buildx build --platform "$PLATFORM" \
            -t "$SERVER_RUNTIME_BASE_IMAGE" \
            -t "$ECR_BASE/$PROJECT/server-runtime-base:latest" \
            --build-arg "FFMPEG_BASE_IMAGE=$FFMPEG_BASE_IMAGE" \
            -f infra/server-runtime-base/Dockerfile --push infra/server-runtime-base
    else
        for image in \
            "$PROJECT/server-build-base:${SERVER_BUILD_BASE_TAG}" \
            "$PROJECT/server-runtime-base:${SERVER_RUNTIME_BASE_TAG}"
        do
            repo="${image%:*}"
            tag="${image##*:}"
            echo "==> Verifying $repo:$tag exists"
            aws ecr describe-images \
                --region "$AWS_REGION" \
                --repository-name "$repo" \
                --image-ids "imageTag=$tag" \
                --query 'imageDetails[0].{digest:imageDigest,pushed:imagePushedAt}' \
                --output table
        done
    fi

    echo "==> Building server-rs ($PLATFORM) → $IMAGE"
    docker buildx build --platform "$PLATFORM" \
        -t "$IMAGE" \
        -t "$LATEST_IMAGE" \
        --build-arg "SERVER_BUILD_BASE_IMAGE=$SERVER_BUILD_BASE_IMAGE" \
        --build-arg "SERVER_RUNTIME_BASE_IMAGE=$SERVER_RUNTIME_BASE_IMAGE" \
        --cache-from "type=registry,ref=$ECR_BASE/$PROJECT/server-rs:buildcache" \
        --cache-to "type=registry,ref=$ECR_BASE/$PROJECT/server-rs:buildcache,mode=max,image-manifest=true,oci-mediatypes=true" \
        -f server-rs/Dockerfile --push .
else
    IMAGE="$ECR_BASE/$PROJECT/server-rs:latest"
fi

# ── Register task definition + roll deployment ─────────────
echo "==> Registering X86_64 task definition for $TASK_FAMILY"
TASK_DEF_JSON="$(mktemp)"
trap 'rm -f "$TASK_DEF_JSON"' EXIT

aws ecs describe-task-definition \
    --region "$AWS_REGION" \
    --task-definition "$TASK_FAMILY" \
    --query 'taskDefinition' \
    --output json \
  | jq --arg IMG "$IMAGE" --arg C "$ECS_CONTAINER" '
      .containerDefinitions |= map(
        if .name == $C then .image = $IMG else . end
      )
      | {family, networkMode, taskRoleArn, executionRoleArn,
         containerDefinitions, volumes, placementConstraints,
         requiresCompatibilities, cpu, memory, runtimePlatform, ephemeralStorage}
      | .runtimePlatform = {
          operatingSystemFamily: "LINUX",
          cpuArchitecture: "X86_64"
        }
      | with_entries(select(.value != null))
    ' \
  > "$TASK_DEF_JSON"

TASK_DEF_ARN="$(aws ecs register-task-definition \
    --region "$AWS_REGION" \
    --cli-input-json "file://$TASK_DEF_JSON" \
    --query 'taskDefinition.taskDefinitionArn' \
    --output text)"
echo "    $TASK_DEF_ARN"

echo "==> Updating $CLUSTER/$SERVICE"
aws ecs update-service \
    --region "$AWS_REGION" \
    --cluster "$CLUSTER" \
    --service "$SERVICE" \
    --task-definition "$TASK_DEF_ARN" \
    --query 'service.taskDefinition' \
    --output text

echo "==> Waiting for service to stabilize..."
aws ecs wait services-stable --region "$AWS_REGION" --cluster "$CLUSTER" --services "$SERVICE"

echo "==> Done. Tail logs:"
echo "    aws logs tail /ecs/$PROJECT --follow --region $AWS_REGION"
