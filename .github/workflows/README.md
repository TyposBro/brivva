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
Re-runs `ci.yml` as a gate, ensures the immutable server image for the
commit exists in ECR, then deploys:
1. **Workers → brivva-api** via `wrangler deploy` + `migrate:prod`
2. **Pages → brivva.pages.dev** via `wrangler pages deploy dist --branch=main` (Pages production alias)
3. **Fargate → us-east-1** via the prebuilt `brivva/server-rs:<git-sha>` image, task-definition registration, `services-stable` wait, and tunnel smoke test

Manual re-deploy available via the Actions tab (`workflow_dispatch`).

### `server-image.yml` — server application image
Builds and pushes the amd64 `brivva/server-rs:<git-sha>` image. Pushes to
`main` prebuild the image, so the later `prod` deploy usually only verifies
that the tag exists and registers an ECS task definition. If the image is
missing, the reusable workflow builds it once with the GitHub Actions and
ECR registry caches.

### `smoke.yml` — nightly + PR gate
End-to-end media pipeline smoke. Runs nightly (09:00 UTC) and on PRs that
touch `server-rs/**` or `tests/e2e/**`. See `tests/e2e/README.md`.

### `ffmpeg-base.yml` — custom FFmpeg image
Builds and pushes the amd64 `brivva/ffmpeg-base:<version>-librtmp` image.
Normal deploys reuse this pinned image instead of rebuilding FFmpeg every
time. Re-run this workflow, or run `./deploy.sh --build-ffmpeg-base`, only
when `infra/ffmpeg-base/**` or `FFMPEG_VERSION` changes.

### `server-base.yml` — server build/runtime foundations
Builds and pushes the amd64 `server-build-base` and `server-runtime-base`
images. Normal deploys reuse these pinned images so they do not reinstall
Rust build tooling, Debian runtime libraries, subtitle fonts, or FFmpeg.
Re-run this workflow, or run `./deploy.sh --build-server-bases`, when
`infra/server-build-base/**`, `infra/server-runtime-base/**`, or the pinned
base tags change.

## Required secrets

Set these in the repo's **Settings → Secrets and variables → Actions**:

| Secret | Scope | How to get it |
|---|---|---|
| `CLOUDFLARE_API_TOKEN` | Workers + Pages + D1 | Cloudflare dashboard → My Profile → API Tokens → Create Token. Permissions: `Account: Workers Scripts: Edit`, `Account: Cloudflare Pages: Edit`, `Account: D1: Edit`. |
| `CLOUDFLARE_ACCOUNT_ID` | same | Cloudflare dashboard → Workers & Pages → right sidebar. |
| `AWS_ACCOUNT` | Fargate | 12-digit account number. GitHub Actions assumes `arn:aws:iam::<AWS_ACCOUNT>:role/github-actions-deploy` via OIDC. |
| `INFISICAL_TOKEN` | Workers + Fargate secret sync | Infisical service token or machine-identity token with read access to the `brivva` project `prod` environment. |

### Minimum AWS IAM permissions

The `github-actions-deploy` role needs ECR read/write for `brivva/*`,
ECS task-definition registration and service update/describe, IAM
`PassRole` for the task roles, and CloudWatch/log read access used by
smoke/debug steps. Do not use long-lived AWS access keys for Actions.

## Skipping deploy for a push

Add `[skip deploy]` or `[skip ci]` to the commit message. Or push to a branch
other than `prod` (deploys only trigger on `prod`).
