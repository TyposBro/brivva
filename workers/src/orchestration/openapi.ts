import { CONTRACT_VERSION, OPENAPI_VERSION } from "@brivva/contracts/meta";

type SchemaObject = Record<string, unknown>;
type OperationObject = Record<string, unknown>;

function ref(name: string) {
  return { $ref: `#/components/schemas/${name}` };
}

function pathParam(name: string, description?: string) {
  return {
    in: "path",
    name,
    required: true,
    description,
    schema: { type: "string" },
  };
}

function queryParam(name: string, required = true, description?: string) {
  return {
    in: "query",
    name,
    required,
    description,
    schema: { type: "string" },
  };
}

function jsonBody(schema: SchemaObject) {
  return {
    required: true,
    content: {
      "application/json": {
        schema,
      },
    },
  };
}

function jsonResponse(schema: SchemaObject, description: string) {
  return {
    description,
    content: {
      "application/json": {
        schema,
      },
    },
  };
}

function redirectResponse(description: string) {
  return {
    description,
    headers: {
      Location: {
        description: "Redirect target",
        schema: { type: "string" },
      },
    },
  };
}

const schemas = {
  ErrorResponse: {
    type: "object",
    required: ["error"],
    properties: {
      error: { type: "string" },
      issues: {
        type: "array",
        items: {
          type: "object",
          required: ["path", "message"],
          properties: {
            path: { type: "string" },
            message: { type: "string" },
          },
        },
      },
    },
  },
  StatusResponse: {
    type: "object",
    required: ["status"],
    properties: {
      status: { type: "string" },
    },
  },
  HealthResponse: {
    type: "object",
    required: ["ok"],
    properties: {
      ok: { type: "boolean", enum: [true] },
    },
  },
  UserInfo: {
    type: "object",
    required: [
      "id",
      "youtube_connected",
      "youtube_channel_name",
      "youtube_channel_id",
      "email",
      "name",
      "picture",
      "onboarding_completed_at",
      "active_voice_id",
      "billing_tier",
      "bills_to",
      "created_at",
    ],
    properties: {
      id: { type: "string" },
      youtube_connected: { type: "boolean" },
      youtube_channel_name: { type: "string", nullable: true },
      youtube_channel_id: { type: "string", nullable: true },
      email: { type: "string", nullable: true },
      name: { type: "string", nullable: true },
      picture: { type: "string", nullable: true },
      onboarding_completed_at: { type: "integer", nullable: true },
      active_voice_id: { type: "string", nullable: true },
      billing_tier: { type: "string" },
      bills_to: { type: "string", nullable: true },
      created_at: { type: "integer" },
    },
  },
  InternalUser: {
    type: "object",
    required: [
      "id",
      "youtube_channel_id",
      "youtube_channel_name",
      "youtube_access_token",
      "youtube_refresh_token",
      "youtube_token_expires_at",
      "email",
      "name",
      "picture",
      "onboarding_completed_at",
      "active_voice_id",
      "billing_tier",
      "bills_to",
      "created_at",
    ],
    properties: {
      id: { type: "string" },
      youtube_channel_id: { type: "string", nullable: true },
      youtube_channel_name: { type: "string", nullable: true },
      youtube_access_token: { type: "string", nullable: true },
      youtube_refresh_token: { type: "string", nullable: true },
      youtube_token_expires_at: { type: "integer", nullable: true },
      email: { type: "string", nullable: true },
      name: { type: "string", nullable: true },
      picture: { type: "string", nullable: true },
      onboarding_completed_at: { type: "integer", nullable: true },
      active_voice_id: { type: "string", nullable: true },
      billing_tier: { type: "string" },
      bills_to: { type: "string", nullable: true },
      created_at: { type: "integer" },
    },
  },
  Voice: {
    type: "object",
    required: [
      "id",
      "user_id",
      "elevenlabs_voice_id",
      "name",
      "source_lang",
      "created_at",
    ],
    properties: {
      id: { type: "string" },
      user_id: { type: "string" },
      elevenlabs_voice_id: { type: "string" },
      name: { type: "string" },
      source_lang: { type: "string", nullable: true },
      created_at: { type: "integer" },
    },
  },
  Stream: {
    type: "object",
    required: [
      "id",
      "session_id",
      "lang",
      "platform",
      "platform_broadcast_id",
      "platform_stream_id",
      "stream_key",
      "rtmp_url",
      "status",
      "delay_ms",
      "host_gain",
      "created_at",
    ],
    properties: {
      id: { type: "string" },
      session_id: { type: "string" },
      lang: { type: "string" },
      platform: { type: "string" },
      platform_broadcast_id: { type: "string", nullable: true },
      platform_stream_id: { type: "string", nullable: true },
      stream_key: { type: "string", nullable: true },
      rtmp_url: { type: "string", nullable: true },
      status: { type: "string" },
      delay_ms: { type: "integer" },
      host_gain: { type: "number" },
      created_at: { type: "integer" },
      // Auto-populated for YouTube; absent on other platforms.
      watch_url: { type: "string", nullable: true },
    },
  },
  Session: {
    type: "object",
    required: [
      "id",
      "user_id",
      "voice_id",
      "title",
      "source_lang",
      "target_langs",
      "status",
      "live_session_id",
      "voice_preset",
      "created_at",
    ],
    properties: {
      id: { type: "string" },
      user_id: { type: "string" },
      voice_id: { type: "string", nullable: true },
      title: { type: "string" },
      source_lang: { type: "string" },
      target_langs: { type: "string" },
      status: { type: "string" },
      live_session_id: { type: "string", nullable: true },
      voice_preset: { type: "string", enum: ["cloned", "female", "male"] },
      created_at: { type: "integer" },
    },
  },
  PlatformCredential: {
    type: "object",
    required: [
      "id",
      "user_id",
      "platform",
      "rtmp_url",
      "stream_key",
      "display_name",
      "created_at",
      "updated_at",
    ],
    properties: {
      id: { type: "string" },
      user_id: { type: "string" },
      platform: { type: "string" },
      rtmp_url: { type: "string", nullable: true },
      stream_key: { type: "string", nullable: true },
      display_name: { type: "string", nullable: true },
      created_at: { type: "integer" },
      updated_at: { type: "integer" },
    },
  },
  AuthTokenRequest: {
    type: "object",
    required: ["user_id"],
    properties: {
      user_id: { type: "string" },
    },
  },
  AuthTokenResponse: {
    type: "object",
    required: ["token"],
    properties: {
      token: { type: "string" },
    },
  },
  PlatformConfig: {
    type: "object",
    required: ["platform"],
    properties: {
      platform: { type: "string" },
      lang: { type: "string" },
      rtmp_url: { type: "string" },
      stream_key: { type: "string" },
      delay_ms: { type: "integer" },
      host_gain: { type: "number" },
      product_id: { type: "string" },
    },
  },
  CreateSessionRequest: {
    type: "object",
    required: ["user_id", "title", "source_lang", "target_langs"],
    properties: {
      user_id: { type: "string" },
      title: { type: "string" },
      source_lang: { type: "string" },
      target_langs: { type: "array", items: { type: "string" }, minItems: 1 },
      voice_id: { type: "string" },
      privacy_status: { type: "string" },
      platforms: { type: "array", items: ref("PlatformConfig") },
    },
  },
  CreateSessionResponse: {
    type: "object",
    required: ["session", "streams"],
    properties: {
      session: ref("Session"),
      streams: { type: "array", items: ref("Stream") },
      errors: { type: "array", items: { type: "string" } },
    },
  },
  ListSessionsResponse: {
    type: "object",
    required: ["sessions"],
    properties: {
      sessions: { type: "array", items: ref("Session") },
    },
  },
  GetSessionResponse: {
    type: "object",
    required: ["session", "streams"],
    properties: {
      session: { allOf: [ref("Session")], nullable: true },
      streams: { type: "array", items: ref("Stream") },
    },
  },
  CloneSessionVoiceRequest: {
    type: "object",
    required: ["user_id", "audio_base64"],
    properties: {
      user_id: { type: "string" },
      audio_base64: { type: "string" },
      name: { type: "string" },
      source_lang: { type: "string" },
    },
  },
  CloneSessionVoiceResponse: {
    type: "object",
    required: ["voice"],
    properties: {
      voice: ref("Voice"),
    },
  },
  AddStreamRequest: {
    type: "object",
    required: ["lang", "platform", "rtmp_url", "stream_key"],
    properties: {
      lang: { type: "string" },
      platform: { type: "string" },
      rtmp_url: { type: "string" },
      stream_key: { type: "string" },
      delay_ms: { type: "integer" },
      host_gain: { type: "number" },
    },
  },
  ListVoicesResponse: {
    type: "object",
    required: ["voices"],
    properties: {
      voices: { type: "array", items: ref("Voice") },
    },
  },
  CreateVoiceRequest: {
    type: "object",
    required: ["user_id", "name", "audio_base64"],
    properties: {
      user_id: { type: "string" },
      name: { type: "string" },
      audio_base64: { type: "string" },
      source_lang: { type: "string" },
    },
  },
  ListCredentialsResponse: {
    type: "object",
    required: ["credentials"],
    properties: {
      credentials: { type: "array", items: ref("PlatformCredential") },
    },
  },
  SaveCredentialRequest: {
    type: "object",
    required: ["user_id", "platform"],
    properties: {
      user_id: { type: "string" },
      platform: { type: "string" },
      rtmp_url: { type: "string" },
      stream_key: { type: "string" },
      display_name: { type: "string" },
    },
  },
  GripAuthRequest: {
    type: "object",
    required: ["user_id", "session_token", "stream_key"],
    properties: {
      user_id: { type: "string" },
      session_token: { type: "string" },
      stream_key: { type: "string" },
      rtmp_url: { type: "string" },
      display_name: { type: "string" },
    },
  },
  TikTokAuthRequest: {
    type: "object",
    required: ["user_id", "session_token", "stream_key"],
    properties: {
      user_id: { type: "string" },
      session_token: { type: "string" },
      stream_key: { type: "string" },
      rtmp_url: { type: "string" },
      display_name: { type: "string" },
    },
  },
  BillingSummaryResponse: {
    type: "object",
    required: [
      "user_id",
      "period_start",
      "period_end",
      "source_minutes",
      "output_minutes_by_lang",
      "estimated_cost_usd",
    ],
    properties: {
      user_id: { type: "string" },
      period_start: { type: "integer" },
      period_end: { type: "integer" },
      source_minutes: { type: "number" },
      output_minutes_by_lang: {
        type: "object",
        additionalProperties: { type: "number" },
      },
      estimated_cost_usd: { type: "number" },
    },
  },
  SessionUsageResponse: {
    type: "object",
    required: [
      "session_id",
      "source_minutes",
      "output_minutes_by_lang",
      "estimated_cost_usd",
    ],
    properties: {
      session_id: { type: "string" },
      source_minutes: { type: "number" },
      output_minutes_by_lang: {
        type: "object",
        additionalProperties: { type: "number" },
      },
      estimated_cost_usd: { type: "number" },
    },
  },
  CompleteOnboardingRequest: {
    type: "object",
    required: ["user_id"],
    properties: {
      user_id: { type: "string" },
    },
  },
  BillingRateResponse: {
    type: "object",
    required: ["per_output_minute_usd"],
    properties: {
      per_output_minute_usd: { type: "number" },
    },
  },
  SessionQuoteBreakdownItem: {
    type: "object",
    required: ["lang", "minutes", "cost_usd"],
    properties: {
      lang: { type: "string" },
      minutes: { type: "number" },
      cost_usd: { type: "number" },
    },
  },
  SessionQuoteResponse: {
    type: "object",
    required: [
      "session_id",
      "expected_minutes",
      "output_minutes",
      "per_output_minute_usd",
      "estimated_cost_usd",
      "breakdown",
    ],
    properties: {
      session_id: { type: "string" },
      expected_minutes: { type: "number" },
      output_minutes: { type: "number" },
      per_output_minute_usd: { type: "number" },
      estimated_cost_usd: { type: "number" },
      breakdown: {
        type: "array",
        items: ref("SessionQuoteBreakdownItem"),
      },
    },
  },
  SessionSummaryResponse: {
    type: "object",
    required: [
      "session_id",
      "status",
      "is_final",
      "billing_tier",
      "source_minutes",
      "total_minutes",
      "output_by_lang",
      "total_cost_usd",
      "rate_usd",
      "billed_to",
      "updated_at",
    ],
    properties: {
      session_id: { type: "string" },
      status: { type: "string" },
      is_final: { type: "boolean" },
      billing_tier: { type: "string", enum: ["self_serve", "b2b"] },
      source_minutes: { type: "number" },
      total_minutes: { type: "number" },
      output_by_lang: {
        type: "object",
        additionalProperties: { type: "number" },
      },
      total_cost_usd: { type: "number", nullable: true },
      rate_usd: { type: "number", nullable: true },
      billed_to: { type: "string", nullable: true },
      updated_at: { type: "integer", nullable: true },
    },
  },
  InternalSessionMetricsUpdate: {
    type: "object",
    properties: {
      source_seconds: { type: "number" },
      output_seconds_by_lang: {
        type: "object",
        additionalProperties: { type: "number" },
      },
    },
  },
  UpdateSessionVoicePresetRequest: {
    type: "object",
    required: ["voice_preset"],
    properties: {
      voice_preset: { type: "string", enum: ["cloned", "female", "male"] },
    },
  },
  UpdateSessionVoicePresetResponse: {
    type: "object",
    required: ["session"],
    properties: { session: ref("Session") },
  },
  InternalSessionBundle: {
    type: "object",
    required: ["session", "streams", "voice"],
    properties: {
      session: ref("Session"),
      streams: { type: "array", items: ref("Stream") },
      voice: { allOf: [ref("Voice")], nullable: true },
    },
  },
  InternalSessionStatusUpdate: {
    type: "object",
    required: ["status"],
    properties: {
      status: { type: "string" },
      live_session_id: { type: "string", nullable: true },
    },
  },
  InternalSessionStatusResponse: {
    type: "object",
    required: ["status"],
    properties: {
      status: { type: "string", enum: ["ok"] },
    },
  },
} satisfies Record<string, SchemaObject>;

function getOperation(summary: string, responses: Record<string, unknown>, extra: OperationObject = {}) {
  return {
    summary,
    responses,
    ...extra,
  };
}

function userPaths() {
  return {
    "/api/user": {
      get: getOperation("Get or create user profile", {
        200: jsonResponse(ref("UserInfo"), "User profile"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
      }, {
        parameters: [queryParam("user_id")],
      }),
    },
    "/api/user/complete-onboarding": {
      post: getOperation("Mark first-run onboarding as complete", {
        200: jsonResponse(ref("UserInfo"), "Updated user profile"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
      }, {
        requestBody: jsonBody(ref("CompleteOnboardingRequest")),
      }),
    },
  };
}

function voicePaths() {
  return {
    "/api/voices": {
      get: getOperation("List saved voices", {
        200: jsonResponse(ref("ListVoicesResponse"), "Voice list"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
      }, {
        parameters: [queryParam("user_id")],
      }),
      post: getOperation("Create voice clone", {
        200: jsonResponse(ref("Voice"), "Created voice"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
      }, {
        requestBody: jsonBody(ref("CreateVoiceRequest")),
      }),
    },
    "/api/voices/{id}": {
      delete: getOperation("Delete voice clone", {
        200: jsonResponse(ref("StatusResponse"), "Delete status"),
        404: jsonResponse(ref("ErrorResponse"), "Voice not found"),
      }, {
        parameters: [pathParam("id", "Voice id")],
      }),
    },
  };
}

function credentialPaths() {
  return {
    "/api/credentials": {
      get: getOperation("List saved platform credentials", {
        200: jsonResponse(ref("ListCredentialsResponse"), "Credential list"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
      }, {
        parameters: [queryParam("user_id")],
      }),
      post: getOperation("Save platform credential", {
        200: jsonResponse(ref("PlatformCredential"), "Saved credential"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
      }, {
        requestBody: jsonBody(ref("SaveCredentialRequest")),
      }),
      delete: getOperation("Delete platform credential", {
        200: jsonResponse(ref("StatusResponse"), "Delete status"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
      }, {
        parameters: [queryParam("user_id"), queryParam("platform")],
      }),
    },
  };
}

function sessionPaths() {
  return {
    "/api/sessions": {
      get: getOperation("List sessions for user", {
        200: jsonResponse(ref("ListSessionsResponse"), "Session list"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
      }, {
        parameters: [queryParam("user_id")],
      }),
      post: getOperation("Create broadcast session", {
        200: jsonResponse(ref("CreateSessionResponse"), "Created session and streams"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
      }, {
        requestBody: jsonBody(ref("CreateSessionRequest")),
      }),
    },
    "/api/sessions/{id}": {
      get: getOperation("Get session with streams", {
        200: jsonResponse(ref("GetSessionResponse"), "Session detail"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
      }, {
        parameters: [pathParam("id", "Session id")],
      }),
      delete: getOperation("Delete session", {
        200: jsonResponse(ref("StatusResponse"), "Delete status"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
      }, {
        parameters: [pathParam("id", "Session id")],
      }),
    },
    "/api/sessions/{id}/voice": {
      post: getOperation("Clone voice for session host", {
        200: jsonResponse(ref("CloneSessionVoiceResponse"), "Cloned voice"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
        403: jsonResponse(ref("ErrorResponse"), "Forbidden"),
        404: jsonResponse(ref("ErrorResponse"), "Session not found"),
      }, {
        parameters: [pathParam("id", "Session id")],
        requestBody: jsonBody(ref("CloneSessionVoiceRequest")),
      }),
    },
    "/api/sessions/{id}/voice-preset": {
      patch: getOperation("Update session voice preset", {
        200: jsonResponse(ref("UpdateSessionVoicePresetResponse"), "Updated session"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
        404: jsonResponse(ref("ErrorResponse"), "Session not found"),
      }, {
        parameters: [pathParam("id", "Session id")],
        requestBody: jsonBody(ref("UpdateSessionVoicePresetRequest")),
      }),
    },
    "/api/sessions/{id}/streams": {
      post: getOperation("Add RTMP stream to session", {
        200: jsonResponse(ref("Stream"), "Created stream"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
      }, {
        parameters: [pathParam("id", "Session id")],
        requestBody: jsonBody(ref("AddStreamRequest")),
      }),
    },
    "/api/sessions/{session_id}/streams/{stream_id}": {
      delete: getOperation("Remove RTMP stream from session", {
        200: jsonResponse(ref("StatusResponse"), "Delete status"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
      }, {
        parameters: [
          pathParam("session_id", "Session id"),
          pathParam("stream_id", "Stream id"),
        ],
      }),
    },
    "/api/sessions/{id}/usage": {
      get: getOperation("Get session usage for billing", {
        200: jsonResponse(ref("SessionUsageResponse"), "Usage rollup"),
        404: jsonResponse(ref("ErrorResponse"), "Session not found"),
      }, {
        parameters: [pathParam("id", "Session id")],
      }),
    },
    "/api/sessions/{id}/quote": {
      get: getOperation("Pre-stream cost quote", {
        200: jsonResponse(ref("SessionQuoteResponse"), "Cost projection"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid expected_minutes"),
        404: jsonResponse(ref("ErrorResponse"), "Session not found"),
      }, {
        parameters: [
          pathParam("id", "Session id"),
          queryParam("expected_minutes", true, "Expected host-speaking minutes"),
        ],
      }),
    },
    "/api/sessions/{id}/summary": {
      get: getOperation("Session summary (live or final)", {
        200: jsonResponse(ref("SessionSummaryResponse"), "Summary rollup"),
        404: jsonResponse(ref("ErrorResponse"), "Session not found"),
      }, {
        parameters: [pathParam("id", "Session id")],
      }),
    },
  };
}

function authPaths() {
  return {
    "/auth/youtube": {
      get: getOperation("Redirect to YouTube OAuth (add-channel, not sign-in)", {
        302: redirectResponse("Redirect to Google OAuth"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
      }, {
        parameters: [queryParam("user_id")],
      }),
    },
    "/auth/youtube/callback": {
      get: getOperation("Handle YouTube OAuth callback", {
        302: redirectResponse("Redirect back to frontend"),
        400: jsonResponse(ref("ErrorResponse"), "Missing code or state"),
        500: jsonResponse(ref("ErrorResponse"), "OAuth exchange failed"),
      }, {
        parameters: [
          queryParam("code", false),
          queryParam("state", false),
          queryParam("error", false),
        ],
      }),
    },
    "/auth/google": {
      get: getOperation("Redirect to Google OAuth for sign-in", {
        302: redirectResponse("Redirect to Google OAuth"),
      }),
    },
    "/auth/google/callback": {
      get: getOperation("Handle Google sign-in callback", {
        302: redirectResponse("Redirect back to frontend with JWT"),
        400: jsonResponse(ref("ErrorResponse"), "Missing code"),
        500: jsonResponse(ref("ErrorResponse"), "OAuth exchange failed"),
      }, {
        parameters: [
          queryParam("code", false),
          queryParam("state", false),
          queryParam("error", false),
        ],
      }),
    },
    "/auth/grip": {
      post: getOperation(
        "Removed — Grip stream keys are one-shot per broadcast",
        {
          410: jsonResponse(
            ref("ErrorResponse"),
            "grip_creds_not_savable — paste fresh each session",
          ),
        },
        {
          requestBody: jsonBody(ref("GripAuthRequest")),
        },
      ),
    },
    "/auth/tiktok": {
      post: getOperation("Save TikTok RTMP credentials (no real OAuth)", {
        200: jsonResponse(ref("PlatformCredential"), "Saved credential"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
      }, {
        requestBody: jsonBody(ref("TikTokAuthRequest")),
      }),
    },
    "/auth/token": {
      post: getOperation("Issue short-lived JWT for media websocket", {
        200: jsonResponse(ref("AuthTokenResponse"), "Signed JWT"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
      }, {
        requestBody: jsonBody(ref("AuthTokenRequest")),
      }),
    },
  };
}

function billingPaths() {
  return {
    "/api/billing/rate": {
      get: getOperation("Get published per-output-minute USD rate", {
        200: jsonResponse(ref("BillingRateResponse"), "Current rate"),
      }),
    },
    "/api/billing/summary": {
      get: getOperation("Current-month usage + estimated cost", {
        200: jsonResponse(ref("BillingSummaryResponse"), "Billing summary"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
      }, {
        parameters: [queryParam("user_id")],
      }),
    },
    "/stripe/webhook": {
      post: getOperation("Stripe webhook receiver (scaffold, no billing logic)", {
        200: jsonResponse(ref("StatusResponse"), "Received"),
        400: jsonResponse(ref("ErrorResponse"), "Signature verification failed"),
      }),
    },
  };
}

function internalPaths() {
  return {
    "/internal/users/{id}": {
      get: getOperation("Fetch raw user row for internal services", {
        200: jsonResponse(ref("InternalUser"), "Raw user row"),
        401: { description: "Unauthorized" },
      }, {
        security: [{ InternalSecret: [] }],
        parameters: [pathParam("id", "User id")],
      }),
    },
    "/internal/voices/{id}": {
      get: getOperation("Fetch voice row for internal services", {
        200: jsonResponse({ allOf: [ref("Voice")], nullable: true }, "Voice row or null"),
        401: { description: "Unauthorized" },
      }, {
        security: [{ InternalSecret: [] }],
        parameters: [pathParam("id", "Voice id")],
      }),
    },
    "/internal/sessions/{id}": {
      get: getOperation("Fetch session bootstrap bundle for live session startup", {
        200: jsonResponse(ref("InternalSessionBundle"), "Session bundle"),
        401: { description: "Unauthorized" },
        404: jsonResponse(ref("ErrorResponse"), "Session not found"),
      }, {
        security: [{ InternalSecret: [] }],
        parameters: [pathParam("id", "Session id")],
      }),
      patch: getOperation("Update session status from media backend", {
        200: jsonResponse(ref("InternalSessionStatusResponse"), "Status updated"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
        401: { description: "Unauthorized" },
      }, {
        security: [{ InternalSecret: [] }],
        parameters: [pathParam("id", "Session id")],
        requestBody: jsonBody(ref("InternalSessionStatusUpdate")),
      }),
    },
    "/internal/sessions/{id}/metrics": {
      patch: getOperation("Merge per-session usage metrics from media backend", {
        200: jsonResponse(ref("InternalSessionStatusResponse"), "Metrics merged"),
        400: jsonResponse(ref("ErrorResponse"), "Invalid request"),
        401: { description: "Unauthorized" },
        404: jsonResponse(ref("ErrorResponse"), "Session not found"),
      }, {
        security: [{ InternalSecret: [] }],
        parameters: [pathParam("id", "Session id")],
        requestBody: jsonBody(ref("InternalSessionMetricsUpdate")),
      }),
    },
  };
}

function buildPaths() {
  return {
    "/health": {
      get: getOperation("Health check", {
        200: jsonResponse(ref("HealthResponse"), "Worker healthy"),
      }),
    },
    ...userPaths(),
    ...voicePaths(),
    ...credentialPaths(),
    ...sessionPaths(),
    ...authPaths(),
    ...billingPaths(),
    ...internalPaths(),
  };
}

export function buildOpenApiDocument() {
  return {
    openapi: OPENAPI_VERSION,
    info: {
      title: "Brivva Workers API",
      version: CONTRACT_VERSION,
      description:
        "CRUD, auth, and internal session bootstrap API for Brivva.",
    },
    servers: [{ url: "https://brivva-api.milliytechnology.workers.dev" }],
    components: {
      securitySchemes: {
        InternalSecret: {
          type: "apiKey",
          in: "header",
          name: "X-Internal-Secret",
        },
      },
      schemas,
    },
    paths: buildPaths(),
  };
}
