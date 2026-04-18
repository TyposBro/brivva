# Ops Migrations

This directory holds one-off operational SQL for already-deployed databases.

Current script:

- `live-session-id-migration.sql`
  - Rebuilds `sessions` so `room_id` becomes `live_session_id`.
  - Use this only for an existing D1 database created before the session naming cleanup.

Run it with Wrangler from the `workers/` directory:

```sh
wrangler d1 execute brivva --remote --file=ops/live-session-id-migration.sql
```

Use `--local` instead of `--remote` for local development databases.

Convenience scripts:

```sh
bun run ops:migrate-session-rename:local
bun run ops:migrate-session-rename:prod
```
