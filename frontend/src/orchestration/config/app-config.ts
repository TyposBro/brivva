export const appConfig = {
  apiBaseUrl: import.meta.env.VITE_WORKER_URL ?? "http://localhost:3000",
} as const;
