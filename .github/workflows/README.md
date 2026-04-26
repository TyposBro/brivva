# GitHub Actions

## Workflows

### `ci.yml` — runs on every push + PR to `main` or `prod`
- **server-rs**: `cargo check` + `cargo clippy -W dead_code -D warnings` + `cargo test`
- **workers**: `bun run typecheck` + `bun run test` (vitest with in-memory D1)
- **frontend**: `bun run typecheck` + `bun run test` + `bun run build` (smoke)

Fail-fast per job; all three must be green. Also callable as a reusable
workflow from `deploy.yml`.

### `deploy.yml` — runs on push to `prod`
`main` is the dev trunk. To ship: merge `main → prod` and push `prod`.
Re-runs `ci.yml` as a gate, then three parallel deploys:
1. **Workers → brivva-api** via `wrangler deploy` + `migrate:prod`
2. **Pages → brivva.pages.dev** via `wrangler pages deploy dist --branch=main` (Pages production alias)
3. **Fargate → us-east-1** via a prebuilt amd64 ffmpeg base, `docker buildx` server image build, task-definition registration, `services-stable` wait, and tunnel smoke test

Manual re-deploy available via the Actions tab (`workflow_dispatch`).

### `smoke.yml` — nightly + PR gate
End-to-end media pipeline smoke. Runs nightly (09:00 UTC) and on PRs that
touch `server-rs/**` or `tests/e2e/**`. See `tests/e2e/README.md`.

### `ffmpeg-base.yml` — custom FFmpeg image
Builds and pushes the amd64 `brivva/ffmpeg-base:<version>-librtmp` image.
Normal deploys reuse this pinned image instead of rebuilding FFmpeg every
time. Re-run this workflow, or run `./deploy.sh --build-ffmpeg-base`, only
when `infra/ffmpeg-base/**` or `FFMPEG_VERSION` changes.

## Required secrets

Set these in the repo's **Settings → Secrets and variables → Actions**:

| Secret | Scope | How to get it |
|---|---|---|
| `CLOUDFLARE_API_TOKEN` | Workers + Pages + D1 | Cloudflare dashboard → My Profile → API Tokens → Create Token. Permissions: `Account: Workers Scripts: Edit`, `Account: Cloudflare Pages: Edit`, `Account: D1: Edit`. |
| `CLOUDFLARE_ACCOUNT_ID` | same | Cloudflare dashboard → Workers & Pages → right sidebar. |
| `AWS_ACCESS_KEY_ID` | Fargate | IAM user with ECR push + ECS update-service permissions. See policy below. |
| `AWS_SECRET_ACCESS_KEY` | same | (paired with the access key id) |
| `AWS_ACCOUNT` | same | 12-digit account number (used to build the ECR registry URL). |

### Minimum AWS IAM policy

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "ecr:GetAuthorizationToken",
        "ecr:BatchCheckLayerAvailability",
        "ecr:GetDownloadUrlForLayer",
        "ecr:BatchGetImage",
        "ecr:InitiateLayerUpload",
        "ecr:UploadLayerPart",
        "ecr:CompleteLayerUpload",
        "ecr:PutImage"
      ],
      "Resource": "arn:aws:ecr:us-east-1:*:repository/brivva/*"
    },
    {
      "Effect": "Allow",
      "Action": [
        "ecs:UpdateService",
        "ecs:DescribeServices"
      ],
      "Resource": "arn:aws:ecs:us-east-1:*:service/brivva/brivva"
    }
  ]
}
```

Attach this policy to a fresh IAM user `brivva-ci`, generate an access key, and
drop the key pair into the GitHub secrets above. **Do not** reuse the root
credentials — rotate to an IAM user before enabling this workflow.

## Skipping deploy for a push

Add `[skip deploy]` or `[skip ci]` to the commit message. Or push to a branch
other than `main` (deploys only trigger on `main`).
