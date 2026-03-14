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
