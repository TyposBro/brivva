import createClient, { type Client } from "openapi-fetch";
import type { paths } from "./workers-api";
import { appConfig } from "../config/app-config";

// Lazy — the client is constructed on first use so the orchestration
// bootstrap has had a chance to populate AppConfig. Do not reach for
// `import.meta.env` here; that is an orchestration concern.

let cached: Client<paths> | null = null;

export function client(): Client<paths> {
  if (!cached) {
    cached = createClient<paths>({
      baseUrl: appConfig().workersApiBase,
      headers: { "Content-Type": "application/json" },
    });
  }
  return cached;
}
