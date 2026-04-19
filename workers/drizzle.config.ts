import { defineConfig } from "drizzle-kit";

// drizzle-kit generates SQL migration files from schema.ts changes. We keep
// the output path aligned with wrangler's migration dir so `wrangler d1
// migrations apply` picks up Drizzle-emitted files alongside the hand-written
// baseline (0001..0003 + the session-rename ops file) that already landed in
// production D1.
//
// Workflow:
//   1. Edit src/schema.ts.
//   2. `bun run db:generate --name=<snake_case_summary>` → emits SQL to ./migrations.
//   3. `bun run migrate:local` for miniflare, `bun run migrate:prod` for remote.
export default defineConfig({
  schema: "./src/core/schema.ts",
  out: "./migrations",
  dialect: "sqlite",
  // Wrangler applies the SQL itself — no direct DB credentials needed here.
});
