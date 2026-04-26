# Infisical Secrets

Brivva uses Infisical as local-dev secret source. Project link lives in `.infisical.json`; project ID is not secret.

## Projects

- `brivva` Infisical project: linked to this repo
- `spiko` Infisical project: separate app/project

## Local Dev

Run full local stack with cloud-synced secrets:

```bash
bun run dev:infisical
```

Auto-restart when Infisical secrets change:

```bash
bun run dev:infisical:watch
```

Run local stack smoke test with Infisical secrets:

```bash
bun run dev:infisical:smoke
```

## Compatibility Exports

Wrangler still has a file-based compatibility path. Treat generated files as cache, not source of truth.

```bash
bun run infisical:export:workers
```

Generated files stay gitignored:

- `workers/.dev.vars`

## Current Secret Split

- Workers needs `ELEVENLABS_API_KEY`, `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `JWT_SECRET`, `INTERNAL_SECRET`. `GRIP_ACCESS_KEY`, `GRIP_SECRET_KEY`, and `STRIPE_WEBHOOK_SECRET` are optional until those integrations are live.
- Fargate needs `SONIOX_API_KEY`, `ELEVENLABS_API_KEY`, `JWT_SECRET`, `INTERNAL_SECRET`, optional `TUNNEL_CREDS`.
- GitHub Actions secrets remain in GitHub: Cloudflare deploy token/account and AWS deploy account/role values.

## Prod Caution

Do not copy `dev` to `prod` blindly. `DEV_AUTH_BYPASS` and localhost URLs are dev-only. Push prod only after checking values against AWS Secrets Manager and Cloudflare Workers secrets.

## Prod Sync

Infisical `prod` is the intended source of truth.

`deploy.sh` and the GitHub `Deploy` workflow sync secrets automatically before
rolling production. Manual sync is only needed after changing secrets without a
code deploy.

Sync prod to AWS Secrets Manager:

```bash
bun run infisical:sync:aws:prod
```

AWS/Fargate do not read Infisical directly. ECS reads AWS Secrets Manager. This
script copies Infisical prod values into `brivva/env`.

Sync prod to Cloudflare Workers secrets:

```bash
bun run infisical:sync:workers:prod
```

Cloudflare Workers do not read Infisical directly. This script copies Infisical
prod values into Wrangler secrets.

Workers sync fails if required Worker secrets are missing from Infisical prod. Optional secrets are synced only when present and non-empty in Infisical prod; missing optional keys are skipped.

GitHub Actions needs an `INFISICAL_TOKEN` repository secret with read access to
the `brivva` project `prod` environment.

## Terraform Boundary

Terraform owns infrastructure only:

- Secrets Manager secret container: `brivva/env`
- IAM permissions for ECS to read it
- ECS task wiring that references secret keys

Terraform does not own secret values. `aws_secretsmanager_secret_version.env`
has `ignore_changes = [secret_string]`, so `tofu apply` must not overwrite
Infisical-managed values.

Generate local non-secret Terraform overrides with:

```bash
infisical run --env=prod -- ./infra/load-env.sh
```

Generated `infra/terraform.tfvars` should only contain `enable_cloudflared` and
`tunnel_id`.
