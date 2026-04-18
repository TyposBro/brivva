import { env, applyD1Migrations } from "cloudflare:test";
import { beforeAll, beforeEach } from "vitest";

// Apply D1 migrations once per worker process.
beforeAll(async () => {
  await applyD1Migrations(env.DB, env.TEST_MIGRATIONS);
});

// Wipe all tables between tests so each case starts from a known empty state.
// Runs in FK-safe order (children first).
beforeEach(async () => {
  await env.DB.batch([
    env.DB.prepare("DELETE FROM platform_credentials"),
    env.DB.prepare("DELETE FROM streams"),
    env.DB.prepare("DELETE FROM sessions"),
    env.DB.prepare("DELETE FROM voices"),
    env.DB.prepare("DELETE FROM users"),
  ]);
});
