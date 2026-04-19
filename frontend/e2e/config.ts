// Shared config so all e2e files agree on the mock-server URL. Mirrors the
// port override in playwright.config.ts — when PLAYWRIGHT_MOCK_PORT is set,
// the mock-server launches there AND tests talk to it at that port.
const MOCK_PORT = Number(process.env.PLAYWRIGHT_MOCK_PORT ?? 8787);
export const MOCK = `http://localhost:${MOCK_PORT}`;
