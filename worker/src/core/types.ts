import type { Context as HonoContext } from "hono";

export type Bindings = {
  AI: Ai;
  ENVIRONMENT: string;
  CF_ACCOUNT_ID: string;
  CF_API_TOKEN: string;
  CF_AI_GATEWAY_ID: string;
  KOKORO_URL: string;
  ROOMS: DurableObjectNamespace;
  DEEPGRAM_API_KEY?: string;
};

export type Variables = {
  requestId: string;
};

export type AppContext = HonoContext<{ Bindings: Bindings; Variables: Variables }>;
