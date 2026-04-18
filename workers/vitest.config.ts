import { defineConfig } from "vitest/config";
import { cloudflareTest, readD1Migrations } from "@cloudflare/vitest-pool-workers";

// Tests run inside miniflare so D1 is a real in-memory SQLite honoring our
// migrations. Non-secret bindings are fake by design — they exist only so
// `c.env.FOO` dereferences work.
export default defineConfig(async () => {
  const migrations = await readD1Migrations("./migrations");

  return {
    plugins: [
      cloudflareTest({
        miniflare: {
          compatibilityDate: "2026-04-01",
          compatibilityFlags: ["nodejs_compat"],
          d1Databases: ["DB"],
          bindings: {
            ELEVENLABS_API_KEY: "test-elevenlabs-key",
            GOOGLE_CLIENT_ID: "test-google-client",
            GOOGLE_CLIENT_SECRET: "test-google-secret",
            JWT_SECRET: "test-jwt-secret-at-least-32-chars-for-hs256-ok",
            INTERNAL_SECRET: "test-internal-secret",
            OAUTH_REDIRECT_URI:
              "https://test-api.example.com/auth/youtube/callback",
            FRONTEND_URL: "https://test-app.example.com",
            TEST_MIGRATIONS: migrations,
          },
        },
      }),
    ],
    test: {
      setupFiles: ["./tests/setup.ts"],
    },
  };
});
