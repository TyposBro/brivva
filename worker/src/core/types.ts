import type { Context as HonoContext } from "hono";

export type Bindings = {
  AI: Ai;
  ENVIRONMENT: string;
  // Future: REPLICA_API_KEY: string;
};

export type Variables = {
  requestId: string;
};

export type AppContext = HonoContext<{ Bindings: Bindings; Variables: Variables }>;
