# brivva-api

Cloudflare Worker + D1 for Brivva's control plane (users, voices, sessions,
streams, platform credentials, OAuth, JWT issuance).

## Schema + migrations

`src/schema.ts` is the source of truth. It drives both the type-safe Drizzle
query builder in `src/db.ts` **and** the SQL migrations that apply to D1.

### Running migrations (nothing changed about this flow)

```fish
bun run migrate:local    # apply to miniflare's local D1
bun run migrate:prod     # apply to production D1
```

`wrangler d1 migrations apply` reads every `*.sql` file in `./migrations/` and
runs the ones its own `d1_migrations` tracking table hasn't seen yet.

### Changing the schema

1. Edit `src/schema.ts`.
2. Generate a delta migration:
   ```fish
   bun run db:generate --name=add_something_descriptive
   ```
   Drizzle diffs your schema against `migrations/meta/0000_snapshot.json`
   (already captures the current shape) and emits exactly the `ALTER TABLE` /
   `CREATE INDEX` / etc. statements needed. File lands in `./migrations/`
   with the next index (starting at `0005_…` to avoid colliding with the
   hand-written baseline `0001..0004`).
3. Review the emitted SQL. Drizzle is usually right, but on SQLite some
   ALTERs require a table rebuild — double-check before shipping.
4. Apply:
   ```fish
   bun run migrate:local
   bun run test              # make sure query helpers still compile + pass
   bun run migrate:prod
   ```

### Why the journal has a phantom `0000__drizzle_baseline`

The first `drizzle-kit generate` ever run in this repo emitted a full
schema dump — we deleted the SQL file (prod already has everything from
hand-written `0001..0004`) but kept `migrations/meta/0000_snapshot.json` so
future `generate` calls diff against a valid baseline. The journal's `0001..
0004` entries are there only to reserve those indices; the matching SQL
files are the real hand-written migrations wrangler applies.

## Tests

```fish
bun run test         # all 58 cases (unit + integration + e2e)
bun run test:watch
```

Tests run inside miniflare with a real in-memory D1 that re-applies every
migration file on start, so the migration chain itself is exercised on every
run.

## Other scripts

| Script | What |
|---|---|
| `bun run dev` | `wrangler dev` — local worker on port 8787 |
| `bun run deploy` | `wrangler deploy` — ship to the `brivva-api` Worker |
| `bun run typecheck` | `tsc --noEmit` |
| `bun run db:check` | drizzle-kit schema sanity (no migration required) |
| `bun run db:studio` | drizzle studio GUI over local D1 |
