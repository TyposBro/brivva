# GitHub Actions

## Workflows

### `ci.yml` — runs on every push + PR
- **server-rs**: `cargo check` + `cargo clippy -W dead_code -D warnings` + `cargo test`
- **workers**: `bun run typecheck` + `bun run test` (vitest with in-memory D1)
- **frontend**: `bun run typecheck` + `bun run test` + `bun run build` (smoke)

Fail-fast per job; all three must be green.

### `deploy.yml` — runs on push to `main`
Re-runs `ci.yml` as a gate, then three parallel deploys:
1. **Workers → brivva-api** via `wrangler deploy` + `migrate:prod`
2. **Pages → brivva.pages.dev** via `wrangler pages deploy dist`
3. **Fargate → us-east-1** via `docker buildx` (arm64) + `ecs update-service --force-new-deployment` + `services-stable` wait + tunnel smoke test

Manual re-deploy available via the Actions tab (`workflow_dispatch`).

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
