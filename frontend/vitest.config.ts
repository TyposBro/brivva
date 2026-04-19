import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    globals: true,
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    css: false,
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    coverage: {
      provider: "v8",
      reporter: ["text", "html", "lcov"],
      reportsDirectory: "./coverage",
      include: ["src/**/*.{ts,tsx}"],
      exclude: [
        "src/**/*.{test,spec}.{ts,tsx}",
        "src/**/*.d.ts",
        "src/main.tsx",
        "src/test/**",
        "src/orchestration/bootstrap.ts",
        "src/core/contracts/workers-api.d.ts",
        // Legacy panels that are not mounted by any current route.
        "src/features/broadcast/presentation/pipeline-analysis.tsx",
        "src/features/public/presentation/privacy-page.tsx",
        "src/features/public/presentation/terms-page.tsx",
      ],
      thresholds: {
        // Global thresholds — see docs/testing-coverage.md for the rationale.
        // Page-level branch coverage is intentionally lower than line/stmt
        // coverage; the un-exercised branches are pageful UI permutations
        // that the Playwright e2e suite covers.
        lines: 80,
        functions: 70,
        branches: 65,
        statements: 80,
        // Critical-path modules — every branch must be exercised so a
        // regression in auth, voice capture, or the host state machine
        // can't sneak past the test suite.
        "src/shared/auth/auth-store.ts": {
          lines: 100,
          functions: 100,
          branches: 100,
          statements: 100,
        },
        "src/shared/auth/sign-in-gate.tsx": {
          lines: 100,
          functions: 100,
          branches: 100,
          statements: 100,
        },
        "src/shared/audio/voice-recorder.ts": {
          lines: 100,
          functions: 100,
          // Two defensive null guards on the cleanup path are unreachable
          // in the real call flow (start always sets recorderRef + tickRef
          // before stop runs); keep the safety nets but accept the dead
          // false-branches in the coverage metric.
          branches: 80,
          statements: 100,
        },
        "src/features/broadcast/presentation/reducer.ts": {
          lines: 100,
          functions: 100,
          branches: 100,
          statements: 100,
        },
        "src/core/config/stream-defaults.ts": {
          lines: 100,
          functions: 100,
          branches: 100,
          statements: 100,
        },
      },
    },
  },
});
