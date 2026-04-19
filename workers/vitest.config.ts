import { defineConfig, type ViteUserConfig } from "vitest/config";
import { cloudflareTest, readD1Migrations } from "@cloudflare/vitest-pool-workers";

// Tests run inside miniflare so D1 is a real in-memory SQLite honoring our
// migrations. Non-secret bindings are fake by design — they exist only so
// `c.env.FOO` dereferences work.
export default defineConfig(async (): Promise<ViteUserConfig> => {
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
            GOOGLE_SIGNIN_REDIRECT_URI:
              "https://test-api.example.com/auth/google/callback",
            FRONTEND_URL: "https://test-app.example.com",
            STRIPE_WEBHOOK_SECRET: "whsec_test_secret",
            // Grip Seller-API bindings. The orchestration layer only routes
            // through the Seller API when a destination carries `product_id`
            // AND both keys are set, so defaulting them here keeps the
            // manual-paste happy path unaffected while letting Task B
            // exercise the auto-provision branch.
            GRIP_ACCESS_KEY: "test-grip-access",
            GRIP_SECRET_KEY: "test-grip-secret",
            TEST_MIGRATIONS: migrations,
          },
        },
      }),
    ],
    test: {
      setupFiles: ["./tests/setup.ts"],
      // istanbul instruments source at transform time so it works inside the
      // workerd sandbox, where the v8 provider's `node:inspector` usage errors
      // with `ERR_METHOD_NOT_IMPLEMENTED`.
      coverage: {
        provider: "istanbul",
        reporter: ["text", "html"],
        include: ["src/**"],
        exclude: [
          "src/orchestration/openapi.ts",
          "src/**/*.d.ts",
        ],
      },
    },
  };
});
