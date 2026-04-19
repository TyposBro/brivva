import createClient from "openapi-fetch";
import type { paths } from "./workers-api";

const API_BASE =
  import.meta.env.VITE_API_URL ??
  import.meta.env.VITE_WORKER_URL ??
  "http://localhost:3000";

export const client = createClient<paths>({
  baseUrl: API_BASE,
  headers: {
    "Content-Type": "application/json",
  },
});
