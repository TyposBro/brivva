import { readFileSync } from "node:fs";
import { assertCleanMediaLogs } from "./media-assertions";

const paths = process.argv.slice(2);
if (!paths.length) {
  console.error("usage: bun assert-clean-media-logs.ts <log-file> [...]");
  process.exit(2);
}

for (const path of paths) {
  const logs = readFileSync(path, "utf-8");
  assertCleanMediaLogs(logs, path);
}

console.log(`[media-logs] OK ${paths.length} file(s)`);
