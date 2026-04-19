import { defineConfig, devices } from "@playwright/test";

// Ports are env-overridable so a local dev can run the e2e suite against
// alternate ports when their Cloudflare wrangler dev is holding 8787 (the
// default). CI keeps the canonical pair via the defaults below.
const MOCK_PORT = Number(process.env.PLAYWRIGHT_MOCK_PORT ?? 8787);
const APP_PORT = Number(process.env.PLAYWRIGHT_APP_PORT ?? 5174);

export default defineConfig({
  testDir: "./e2e",
  testMatch: /.*\.e2e\.ts$/,
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [["list"]],
  use: {
    baseURL: `http://localhost:${APP_PORT}`,
    trace: "retain-on-failure",
    launchOptions: {
      args: [
        "--use-fake-device-for-media-stream",
        "--use-fake-ui-for-media-stream",
        "--autoplay-policy=no-user-gesture-required",
      ],
    },
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
  ],
  webServer: [
    {
      command: `node e2e/mock-server.mjs`,
      port: MOCK_PORT,
      reuseExistingServer: !process.env.CI,
      env: { MOCK_PORT: String(MOCK_PORT), PLAYWRIGHT_APP_PORT: String(APP_PORT) },
    },
    {
      command: `vite --port ${APP_PORT} --strictPort`,
      port: APP_PORT,
      reuseExistingServer: !process.env.CI,
      env: {
        VITE_API_URL: `http://localhost:${MOCK_PORT}`,
        VITE_WORKER_URL: `http://localhost:${MOCK_PORT}`,
      },
    },
  ],
});
