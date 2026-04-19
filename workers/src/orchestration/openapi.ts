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
      "created_at",
    ],
    properties: {
      id: { type: "string" },
      youtube_connected: { type: "boolean" },
      youtube_channel_name: { type: "string", nullable: true },
      youtube_channel_id: { type: "string", nullable: true },
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
      "created_at",
    ],
    properties: {
      id: { type: "string" },
      youtube_channel_id: { type: "string", nullable: true },
      youtube_channel_name: { type: "string", nullable: true },
      youtube_access_token: { type: "string", nullable: true },
      youtube_refresh_token: { type: "string", nullable: true },
      youtube_token_expires_at: { type: "integer", nullable: true },
      created_at: { type: "integer" },
    },
  },
  Voice: {
    type: "object",
    required: ["id", "user_id", "elevenlabs_voice_id", "name", "created_at"],
    properties: {
      id: { type: "string" },
      user_id: { type: "string" },
      elevenlabs_voice_id: { type: "string" },
      name: { type: "string" },
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
  };
}

function authPaths() {
  return {
    "/auth/youtube": {
      get: getOperation("Redirect to YouTube OAuth", {
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
