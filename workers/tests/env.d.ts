/// <reference types="@cloudflare/workers-types" />
/// <reference types="@cloudflare/vitest-pool-workers/types" />

// Augment the global `Cloudflare.Env` that @cloudflare/vitest-pool-workers
// exposes via `import { env } from "cloudflare:test"`. These are the bindings
// our app + tests read, so tsc should know about all of them.
declare global {
  namespace Cloudflare {
    interface Env {
      DB: D1Database;
      ELEVENLABS_API_KEY: string;
      GOOGLE_CLIENT_ID: string;
      GOOGLE_CLIENT_SECRET: string;
      JWT_SECRET: string;
      INTERNAL_SECRET: string;
      OAUTH_REDIRECT_URI: string;
      GOOGLE_SIGNIN_REDIRECT_URI: string;
      FRONTEND_URL: string;
      STRIPE_WEBHOOK_SECRET: string;
      GRIP_ACCESS_KEY: string;
      GRIP_SECRET_KEY: string;
      SESSION_LOGS_ENABLED: string;
      SESSION_LOG_CONSOLE: string;
      TEST_MIGRATIONS: D1Migration[];
    }
  }
}

export {};
