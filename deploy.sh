#!/usr/bin/env bash
# Build GPU server-rs image → push to ECR → register task def → deploy ECS.
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
SERVER_RUNTIME_BASE_TAG="${SERVER_RUNTIME_BASE_TAG:-bookworm-ffmpeg-7.1.1-native-rtmp-v1}"
SERVER_RUNTIME_GPU_BASE_TAG="${SERVER_RUNTIME_GPU_BASE_TAG:-bookworm-ffmpeg-7.1.1-nvenc-v1}"
TASK_FAMILY="${TASK_FAMILY:-$PROJECT}"
ECS_CONTAINER="${ECS_CONTAINER:-server-rs}"
BRIVVA_SESSION_LOGS="${BRIVVA_SESSION_LOGS:-1}"
BRIVVA_SESSION_LOG_VERBOSE="${BRIVVA_SESSION_LOG_VERBOSE:-0}"
BRIVVA_WEBRTC_STUN_URLS="${BRIVVA_WEBRTC_STUN_URLS:-stun:stun.l.google.com:19302}"
BRIVVA_WEBRTC_ICE_SERVERS="${BRIVVA_WEBRTC_ICE_SERVERS:-}"
BRIVVA_WEBRTC_UDP_PORT_MIN="${BRIVVA_WEBRTC_UDP_PORT_MIN:-40000}"
BRIVVA_WEBRTC_UDP_PORT_MAX="${BRIVVA_WEBRTC_UDP_PORT_MAX:-40100}"
BRIVVA_FORCE_RTMP_NOT_RTMPS="${BRIVVA_FORCE_RTMP_NOT_RTMPS:-0}"
DEPLOY_MIN_HEALTH="${DEPLOY_MIN_HEALTH:-0}"
DEPLOY_MAX_PERCENT="${DEPLOY_MAX_PERCENT:-100}"

# Resolve account + ECR base. Prefer terraform output, fall back to STS.
cd "$(dirname "$0")"
if [[ -z "${AWS_ACCOUNT:-}" ]]; then
	if [[ -f infra/terraform.tfstate ]]; then
		tf_account="$(terraform -chdir=infra output -raw account_id 2>/dev/null || true)"
		if [[ "$tf_account" =~ ^[0-9]{12}$ ]]; then
			AWS_ACCOUNT="$tf_account"
		fi
	fi
fi
if [[ -z "${AWS_ACCOUNT:-}" ]]; then
	AWS_ACCOUNT="$(aws sts get-caller-identity --query Account --output text)"
fi
ECR_BASE="${AWS_ACCOUNT}.dkr.ecr.${AWS_REGION}.amazonaws.com"

# ── Parse args ────────────────────────────────────────────
SKIP_BUILD=false
BUILD_FFMPEG_BASE=false
BUILD_FFMPEG_GPU_BASE=false
BUILD_SERVER_BASES=false
BASES_ONLY=false
BUILD_ONLY=false
GPU_MODE=true

usage() {
	echo "Usage: $0 [--gpu] [--bases-only] [--build-only] [--skip-build] [--build-ffmpeg-base] [--build-ffmpeg-gpu-base] [--build-server-bases]"
	echo ""
	echo "  --gpu                 Build/deploy using NVENC GPU runtime base (default; kept for compatibility)"
	echo "  --bases-only          Build/verify requested base images, then exit before server deploy"
	echo "  --build-only          Build/push server-rs image, then exit before ECS deploy"
	echo "  --skip-build          Skip Docker build, deploy server-rs:latest"
	echo "  --build-ffmpeg-base   Rebuild and push the pinned ffmpeg-base image"
	echo "  --build-ffmpeg-gpu-base Rebuild and push the pinned ffmpeg-gpu-base image"
	echo "  --build-server-bases  Rebuild and push server build/runtime base images"
	echo ""
	echo "Env overrides: AWS_REGION, AWS_ACCOUNT, CLUSTER, SERVICE, PROJECT, FFMPEG_VERSION,"
	echo "               SERVER_BUILD_BASE_TAG, SERVER_RUNTIME_BASE_TAG, SERVER_RUNTIME_GPU_BASE_TAG"
	exit 1
}

while [[ $# -gt 0 ]]; do
	case $1 in
	--gpu)
		GPU_MODE=true
		shift
		;;
	--bases-only)
		BASES_ONLY=true
		shift
		;;
	--build-only)
		BUILD_ONLY=true
		shift
		;;
	--skip-build)
		SKIP_BUILD=true
		shift
		;;
	--build-ffmpeg-base)
		BUILD_FFMPEG_BASE=true
		shift
		;;
	--build-ffmpeg-gpu-base)
		BUILD_FFMPEG_GPU_BASE=true
		shift
		;;
	--build-server-bases)
		BUILD_SERVER_BASES=true
		shift
		;;
	-h | --help) usage ;;
	*)
		echo "Unknown arg: $1"
		usage
		;;
	esac
done

BRIVVA_VIDEO_ENCODER="${BRIVVA_VIDEO_ENCODER:-nvenc}"
NVIDIA_DRIVER_CAPABILITIES="${NVIDIA_DRIVER_CAPABILITIES:-video,compute,utility}"

if [[ "$SKIP_BUILD" == true && ("$BUILD_FFMPEG_BASE" == true || "$BUILD_FFMPEG_GPU_BASE" == true || "$BUILD_SERVER_BASES" == true || "$BASES_ONLY" == true || "$BUILD_ONLY" == true) ]]; then
	echo "--skip-build cannot be combined with build/base image flags" >&2
	exit 1
fi

if [[ "$BASES_ONLY" == true && ("$BUILD_FFMPEG_BASE" != true && "$BUILD_FFMPEG_GPU_BASE" != true && "$BUILD_SERVER_BASES" != true) ]]; then
	echo "--bases-only requires at least one base image rebuild flag" >&2
	exit 1
fi

if [[ "$BASES_ONLY" == true && "$BUILD_ONLY" == true ]]; then
	echo "--bases-only and --build-only are mutually exclusive" >&2
	exit 1
fi

# ── Build & Push ──────────────────────────────────────────
if [[ "$SKIP_BUILD" == false ]]; then
	echo "==> ECR login ($AWS_REGION)"
	aws ecr get-login-password --region "$AWS_REGION" |
		docker login --username AWS --password-stdin "$ECR_BASE"

	# Keep ECS GPU on x86_64 so production ffmpeg behavior matches Ubuntu dev.
	PLATFORM="linux/amd64"
	SHA="$(git rev-parse HEAD)"
	IMAGE="$ECR_BASE/$PROJECT/server-rs:$SHA"
	LATEST_IMAGE="$ECR_BASE/$PROJECT/server-rs:latest"
	FFMPEG_GPU_BASE_IMAGE="${FFMPEG_GPU_BASE_IMAGE:-$ECR_BASE/$PROJECT/ffmpeg-gpu-base:${FFMPEG_VERSION}-nvenc}"
	SERVER_BUILD_BASE_IMAGE="${SERVER_BUILD_BASE_IMAGE:-$ECR_BASE/$PROJECT/server-build-base:${SERVER_BUILD_BASE_TAG}}"
	SERVER_RUNTIME_BASE_IMAGE="${SERVER_RUNTIME_BASE_IMAGE:-$ECR_BASE/$PROJECT/server-runtime-base:${SERVER_RUNTIME_GPU_BASE_TAG}}"

	if [[ "$BUILD_FFMPEG_BASE" == true ]]; then
		echo "--build-ffmpeg-base is obsolete: production deploy is GPU-only. Use --build-ffmpeg-gpu-base." >&2
		exit 1
	fi

	if [[ "$GPU_MODE" == true ]]; then
		if [[ "$BUILD_FFMPEG_GPU_BASE" == true ]]; then
			echo "==> Building ffmpeg-gpu-base ($PLATFORM) → $FFMPEG_GPU_BASE_IMAGE"
			docker buildx build --platform "$PLATFORM" \
				-t "$FFMPEG_GPU_BASE_IMAGE" \
				-t "$ECR_BASE/$PROJECT/ffmpeg-gpu-base:latest" \
				--build-arg "FFMPEG_VERSION=$FFMPEG_VERSION" \
				-f infra/ffmpeg-gpu-base/Dockerfile --push infra/ffmpeg-gpu-base
		else
			echo "==> Verifying ffmpeg-gpu-base exists → $FFMPEG_GPU_BASE_IMAGE"
			aws ecr describe-images \
				--region "$AWS_REGION" \
				--repository-name "$PROJECT/ffmpeg-gpu-base" \
				--image-ids "imageTag=${FFMPEG_VERSION}-nvenc" \
				--query 'imageDetails[0].{digest:imageDigest,pushed:imagePushedAt}' \
				--output table
		fi
	fi

	if [[ "$BUILD_SERVER_BASES" == true ]]; then
		echo "==> Building server-build-base ($PLATFORM) → $SERVER_BUILD_BASE_IMAGE"
		docker buildx build --platform "$PLATFORM" \
			-t "$SERVER_BUILD_BASE_IMAGE" \
			-t "$ECR_BASE/$PROJECT/server-build-base:latest" \
			-f infra/server-build-base/Dockerfile --push infra/server-build-base

		if [[ "$GPU_MODE" == true ]]; then
			echo "==> Building server-runtime-gpu-base ($PLATFORM) → $SERVER_RUNTIME_BASE_IMAGE"
			docker buildx build --platform "$PLATFORM" \
				-t "$SERVER_RUNTIME_BASE_IMAGE" \
				-t "$ECR_BASE/$PROJECT/server-runtime-base:gpu-latest" \
				--build-arg "FFMPEG_GPU_BASE_IMAGE=$FFMPEG_GPU_BASE_IMAGE" \
				-f infra/server-runtime-gpu-base/Dockerfile --push infra/server-runtime-gpu-base
		else
			echo "==> Building server-runtime-base ($PLATFORM) → $SERVER_RUNTIME_BASE_IMAGE"
			docker buildx build --platform "$PLATFORM" \
				-t "$SERVER_RUNTIME_BASE_IMAGE" \
				-t "$ECR_BASE/$PROJECT/server-runtime-base:latest" \
				--build-arg "FFMPEG_BASE_IMAGE=$FFMPEG_BASE_IMAGE" \
				-f infra/server-runtime-base/Dockerfile --push infra/server-runtime-base
		fi
	else
		for image in \
			"$PROJECT/server-build-base:${SERVER_BUILD_BASE_TAG}" \
			"${SERVER_RUNTIME_BASE_IMAGE#${ECR_BASE}/}"; do
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

	if [[ "$BASES_ONLY" == true ]]; then
		echo "==> Base image bootstrap complete; skipping server-rs build and ECS deploy (--bases-only)"
		exit 0
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

	if [[ "$BUILD_ONLY" == true ]]; then
		echo "==> Built and pushed $IMAGE; skipping ECS deploy (--build-only)"
		exit 0
	fi
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
	--output json |
	jq \
		--arg IMG "$IMAGE" \
		--arg C "$ECS_CONTAINER" \
		--arg SESSION_LOGS "$BRIVVA_SESSION_LOGS" \
		--arg SESSION_LOG_VERBOSE "$BRIVVA_SESSION_LOG_VERBOSE" \
		--arg WEBRTC_STUN_URLS "$BRIVVA_WEBRTC_STUN_URLS" \
		--arg WEBRTC_ICE_SERVERS "$BRIVVA_WEBRTC_ICE_SERVERS" \
		--arg WEBRTC_UDP_PORT_MIN "$BRIVVA_WEBRTC_UDP_PORT_MIN" \
		--arg WEBRTC_UDP_PORT_MAX "$BRIVVA_WEBRTC_UDP_PORT_MAX" \
		--arg FORCE_RTMP_NOT_RTMPS "$BRIVVA_FORCE_RTMP_NOT_RTMPS" \
		--arg NVIDIA_DRIVER_CAPABILITIES "$NVIDIA_DRIVER_CAPABILITIES" \
		--arg VIDEO_ENCODER "$BRIVVA_VIDEO_ENCODER" '
      .containerDefinitions |= map(
        if .name == $C then
          .image = $IMG
          | .environment = (
              ((.environment // [])
                | map(select(
                    .name != "BRIVVA_SESSION_LOGS"
                    and .name != "BRIVVA_SESSION_LOG_VERBOSE"
                    and .name != "BRIVVA_WEBRTC_STUN_URLS"
                    and .name != "BRIVVA_WEBRTC_ICE_SERVERS"
                    and .name != "BRIVVA_WEBRTC_UDP_PORT_MIN"
                    and .name != "BRIVVA_WEBRTC_UDP_PORT_MAX"
                    and .name != "BRIVVA_FORCE_RTMP_NOT_RTMPS"
                    and .name != "NVIDIA_DRIVER_CAPABILITIES"
                    and .name != "BRIVVA_VIDEO_ENCODER"
                  )))
              + [
                {name: "BRIVVA_SESSION_LOGS", value: $SESSION_LOGS},
                {name: "BRIVVA_SESSION_LOG_VERBOSE", value: $SESSION_LOG_VERBOSE},
                {name: "BRIVVA_WEBRTC_STUN_URLS", value: $WEBRTC_STUN_URLS},
                {name: "BRIVVA_WEBRTC_ICE_SERVERS", value: $WEBRTC_ICE_SERVERS},
                {name: "BRIVVA_WEBRTC_UDP_PORT_MIN", value: $WEBRTC_UDP_PORT_MIN},
                {name: "BRIVVA_WEBRTC_UDP_PORT_MAX", value: $WEBRTC_UDP_PORT_MAX},
                {name: "BRIVVA_FORCE_RTMP_NOT_RTMPS", value: $FORCE_RTMP_NOT_RTMPS},
                {name: "NVIDIA_DRIVER_CAPABILITIES", value: $NVIDIA_DRIVER_CAPABILITIES},
                {name: "BRIVVA_VIDEO_ENCODER", value: $VIDEO_ENCODER}
              ]
            )
        else . end
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
		>"$TASK_DEF_JSON"

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
	--deployment-configuration "minimumHealthyPercent=$DEPLOY_MIN_HEALTH,maximumPercent=$DEPLOY_MAX_PERCENT,deploymentCircuitBreaker={enable=true,rollback=true}" \
	--query 'service.taskDefinition' \
	--output text

echo "==> Waiting for service to stabilize..."
aws ecs wait services-stable --region "$AWS_REGION" --cluster "$CLUSTER" --services "$SERVICE"

echo "==> Done. Tail logs:"
echo "    aws logs tail /ecs/$PROJECT --follow --region $AWS_REGION"
