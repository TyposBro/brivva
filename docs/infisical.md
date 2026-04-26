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

Wrangler and Terraform still have file-based compatibility paths. Treat generated files as cache, not source of truth.

```bash
bun run infisical:export:workers
bun run infisical:export:terraform
```

Generated files stay gitignored:

- `workers/.dev.vars`
- `infra/terraform.tfvars`

## Current Secret Split

- Workers needs `ELEVENLABS_API_KEY`, `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `JWT_SECRET`, `INTERNAL_SECRET`, `GRIP_ACCESS_KEY`, `GRIP_SECRET_KEY`, optional `STRIPE_WEBHOOK_SECRET`.
- Fargate needs `SONIOX_API_KEY`, `ELEVENLABS_API_KEY`, `JWT_SECRET`, `INTERNAL_SECRET`, optional `TUNNEL_CREDS`.
- GitHub Actions secrets remain in GitHub: Cloudflare deploy token/account and AWS deploy account/role values.

## Prod Caution

Do not copy `dev` to `prod` blindly. `JWT_SECRET` and `INTERNAL_SECRET` may be dev-only. Push prod only after checking values against AWS Secrets Manager and Cloudflare Workers secrets.
