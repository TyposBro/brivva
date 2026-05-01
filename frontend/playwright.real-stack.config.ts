import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, devices } from "@playwright/test";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// Runs the `frontend/e2e/full-stack-live.e2e.ts` suite against the REAL
// local dev stack (server-rs + workers + frontend + real ElevenLabs +
// real Soniox + real Grip/YouTube RTMP ingestion). Not the mock server.
//
// Preconditions:
//   1. `./scripts/dev-all.sh` already running in another terminal.
//   2. `workers/.dev.vars` has `DEV_AUTH_BYPASS=true`.
//   3. `tests/e2e/.env` populated (see tests/e2e/.env.example).
//   4. `tests/e2e/fixtures/fake-cam.y4m` + `fake-mic.wav` present
//      (see tests/e2e/fixtures/README.md to regenerate).
//
// Launch via `./scripts/test-e2e-real.sh`, which sources the env file
// and points Playwright here.

const REPO_ROOT = path.resolve(__dirname, "..");
const FIXTURE_VIDEO = path.join(REPO_ROOT, "tests/e2e/fixtures/fake-cam.y4m");
const FIXTURE_AUDIO = path.join(REPO_ROOT, "tests/e2e/fixtures/fake-mic.wav");

const FRONTEND_URL =
	process.env.BRIVVA_DEV_FRONTEND_URL ?? "http://localhost:5173";
const RECORD_SECONDS = Number(process.env.E2E_RECORD_SECONDS ?? "90");
const TEST_TIMEOUT_MS = Math.max(5 * 60 * 1000, (RECORD_SECONDS + 180) * 1000);

const chromiumMediaArgs = [
	"--use-fake-device-for-media-stream",
	"--use-fake-ui-for-media-stream",
	"--autoplay-policy=no-user-gesture-required",
];
if (fs.existsSync(FIXTURE_VIDEO)) {
	chromiumMediaArgs.push(`--use-file-for-fake-video-capture=${FIXTURE_VIDEO}`);
}
if (fs.existsSync(FIXTURE_AUDIO)) {
	chromiumMediaArgs.push(`--use-file-for-fake-audio-capture=${FIXTURE_AUDIO}`);
}

export default defineConfig({
	testDir: "./e2e",
	// Real-stack tests only. All other *.e2e.ts files rely on the mock
	// server and break when pointed at real Workers.
	testMatch: /full-stack-live\.e2e\.ts$/,
	fullyParallel: false,
	workers: 1,
	// Long pipeline latencies (ElevenLabs clone, Soniox first-token, Grip
	// RTMP handshake). Scale with E2E_RECORD_SECONDS so 60–90 min burn-ins
	// do not fail at the old 5 min harness timeout.
	timeout: TEST_TIMEOUT_MS,
	retries: 0,
	// `printSteps: true` makes the list reporter emit a line per
	// `test.step(...)` call, so a long-running test surfaces its
	// progress live instead of going silent for minutes.
	reporter: [["list", { printSteps: true }]],
	use: {
		baseURL: FRONTEND_URL,
		trace: "retain-on-failure",
		video: "retain-on-failure",
		launchOptions: {
			args: chromiumMediaArgs,
		},
	},
	projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
	// No webServer block: the dev stack must already be running. Spawning
	// server-rs + workers inside Playwright would double-bind their ports
	// and burn compile time on every run.
});
