type SchemaObject = Record<string, unknown>;

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

const errorSchema = {
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
} satisfies SchemaObject;

const statusSchema = {
  type: "object",
  required: ["status"],
  properties: {
    status: { type: "string" },
  },
} satisfies SchemaObject;

const userInfoSchema = {
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
    youtube_channel_name: { type: ["string", "null"] },
    youtube_channel_id: { type: ["string", "null"] },
    created_at: { type: "integer" },
  },
} satisfies SchemaObject;

const voiceSchema = {
  type: "object",
  required: ["id", "user_id", "elevenlabs_voice_id", "name", "created_at"],
  properties: {
    id: { type: "string" },
    user_id: { type: "string" },
    elevenlabs_voice_id: { type: "string" },
    name: { type: "string" },
    created_at: { type: "integer" },
  },
} satisfies SchemaObject;

const streamSchema = {
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
    platform_broadcast_id: { type: ["string", "null"] },
    platform_stream_id: { type: ["string", "null"] },
    stream_key: { type: ["string", "null"] },
    rtmp_url: { type: ["string", "null"] },
    status: { type: "string" },
    delay_ms: { type: "integer" },
    host_gain: { type: "number" },
    created_at: { type: "integer" },
  },
} satisfies SchemaObject;

const sessionSchema = {
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
    voice_id: { type: ["string", "null"] },
    title: { type: "string" },
    source_lang: { type: "string" },
    target_langs: { type: "string" },
    status: { type: "string" },
    live_session_id: { type: ["string", "null"] },
    created_at: { type: "integer" },
  },
} satisfies SchemaObject;

const platformCredentialSchema = {
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
    rtmp_url: { type: ["string", "null"] },
    stream_key: { type: ["string", "null"] },
    display_name: { type: ["string", "null"] },
    created_at: { type: "integer" },
    updated_at: { type: "integer" },
  },
} satisfies SchemaObject;

export function buildOpenApiDocument() {
  return {
    openapi: "3.0.0",
    info: {
      title: "Brivva Workers API",
      version: "0.1.0",
      description:
        "CRUD and auth API for Brivva sessions, voices, stream destinations, and media bootstrap.",
    },
    servers: [{ url: "https://brivva-api.milliytechnology.workers.dev" }],
    components: {
      schemas: {
        ErrorResponse: errorSchema,
        StatusResponse: statusSchema,
        UserInfo: userInfoSchema,
        Voice: voiceSchema,
        Stream: streamSchema,
        Session: sessionSchema,
        PlatformCredential: platformCredentialSchema,
      },
    },
    paths: {
      "/health": {
        get: {
          summary: "Health check",
          responses: {
            200: jsonResponse(
              {
                type: "object",
                required: ["ok"],
                properties: { ok: { type: "boolean", enum: [true] } },
              },
              "Worker healthy",
            ),
          },
        },
      },
      "/api/user": {
        get: {
          summary: "Get or create user profile",
          parameters: [
            { in: "query", name: "user_id", required: true, schema: { type: "string" } },
          ],
          responses: {
            200: jsonResponse(userInfoSchema, "User profile"),
            400: jsonResponse(errorSchema, "Invalid request"),
          },
        },
      },
      "/auth/token": {
        post: {
          summary: "Issue short-lived JWT for media websocket",
          requestBody: {
            required: true,
            content: {
              "application/json": {
                schema: {
                  type: "object",
                  required: ["user_id"],
                  properties: { user_id: { type: "string" } },
                },
              },
            },
          },
          responses: {
            200: jsonResponse(
              {
                type: "object",
                required: ["token"],
                properties: { token: { type: "string" } },
              },
              "Signed JWT",
            ),
            400: jsonResponse(errorSchema, "Invalid request"),
          },
        },
      },
      "/api/voices": {
        get: {
          summary: "List saved voices for user",
          parameters: [
            { in: "query", name: "user_id", required: true, schema: { type: "string" } },
          ],
          responses: {
            200: jsonResponse(
              {
                type: "object",
                required: ["voices"],
                properties: { voices: { type: "array", items: voiceSchema } },
              },
              "Voice list",
            ),
            400: jsonResponse(errorSchema, "Invalid request"),
          },
        },
        post: {
          summary: "Create voice clone",
          requestBody: {
            required: true,
            content: {
              "application/json": {
                schema: {
                  type: "object",
                  required: ["user_id", "name", "audio_base64"],
                  properties: {
                    user_id: { type: "string" },
                    name: { type: "string" },
                    audio_base64: { type: "string" },
                  },
                },
              },
            },
          },
          responses: {
            200: jsonResponse(voiceSchema, "Created voice"),
            400: jsonResponse(errorSchema, "Invalid request"),
          },
        },
      },
      "/api/voices/{id}": {
        delete: {
          summary: "Delete voice clone",
          parameters: [
            { in: "path", name: "id", required: true, schema: { type: "string" } },
          ],
          responses: {
            200: jsonResponse(statusSchema, "Delete status"),
            404: jsonResponse(errorSchema, "Voice not found"),
          },
        },
      },
      "/api/credentials": {
        get: {
          summary: "List saved platform credentials",
          parameters: [
            { in: "query", name: "user_id", required: true, schema: { type: "string" } },
          ],
          responses: {
            200: jsonResponse(
              {
                type: "object",
                required: ["credentials"],
                properties: {
                  credentials: { type: "array", items: platformCredentialSchema },
                },
              },
              "Credential list",
            ),
            400: jsonResponse(errorSchema, "Invalid request"),
          },
        },
        post: {
          summary: "Save platform credential",
          requestBody: {
            required: true,
            content: {
              "application/json": {
                schema: {
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
              },
            },
          },
          responses: {
            200: jsonResponse(platformCredentialSchema, "Saved credential"),
            400: jsonResponse(errorSchema, "Invalid request"),
          },
        },
        delete: {
          summary: "Delete platform credential",
          parameters: [
            { in: "query", name: "user_id", required: true, schema: { type: "string" } },
            { in: "query", name: "platform", required: true, schema: { type: "string" } },
          ],
          responses: {
            200: jsonResponse(statusSchema, "Delete status"),
            400: jsonResponse(errorSchema, "Invalid request"),
          },
        },
      },
      "/api/sessions": {
        get: {
          summary: "List sessions for user",
          parameters: [
            { in: "query", name: "user_id", required: true, schema: { type: "string" } },
          ],
          responses: {
            200: jsonResponse(
              {
                type: "object",
                required: ["sessions"],
                properties: { sessions: { type: "array", items: sessionSchema } },
              },
              "Session list",
            ),
            400: jsonResponse(errorSchema, "Invalid request"),
          },
        },
        post: {
          summary: "Create broadcast session",
          requestBody: {
            required: true,
            content: {
              "application/json": {
                schema: {
                  type: "object",
                  required: ["user_id", "title", "source_lang", "target_langs"],
                  properties: {
                    user_id: { type: "string" },
                    title: { type: "string" },
                    source_lang: { type: "string" },
                    target_langs: { type: "array", items: { type: "string" }, minItems: 1 },
                    voice_id: { type: "string" },
                    privacy_status: { type: "string" },
                    platforms: {
                      type: "array",
                      items: {
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
                    },
                  },
                },
              },
            },
          },
          responses: {
            200: jsonResponse(
              {
                type: "object",
                required: ["session", "streams"],
                properties: {
                  session: sessionSchema,
                  streams: { type: "array", items: streamSchema },
                  errors: { type: "array", items: { type: "string" } },
                },
              },
              "Created session and streams",
            ),
            400: jsonResponse(errorSchema, "Invalid request"),
          },
        },
      },
      "/api/sessions/{id}": {
        get: {
          summary: "Get session with streams",
          parameters: [
            { in: "path", name: "id", required: true, schema: { type: "string" } },
          ],
          responses: {
            200: jsonResponse(
              {
                type: "object",
                required: ["session", "streams"],
                properties: {
                  session: { anyOf: [sessionSchema, { type: "null" }] },
                  streams: { type: "array", items: streamSchema },
                },
              },
              "Session detail",
            ),
            400: jsonResponse(errorSchema, "Invalid request"),
          },
        },
        delete: {
          summary: "Delete session",
          parameters: [
            { in: "path", name: "id", required: true, schema: { type: "string" } },
          ],
          responses: {
            200: jsonResponse(statusSchema, "Delete status"),
            400: jsonResponse(errorSchema, "Invalid request"),
          },
        },
      },
      "/api/sessions/{id}/voice": {
        post: {
          summary: "Clone voice for session host",
          parameters: [
            { in: "path", name: "id", required: true, schema: { type: "string" } },
          ],
          requestBody: {
            required: true,
            content: {
              "application/json": {
                schema: {
                  type: "object",
                  required: ["user_id", "audio_base64"],
                  properties: {
                    user_id: { type: "string" },
                    audio_base64: { type: "string" },
                    name: { type: "string" },
                  },
                },
              },
            },
          },
          responses: {
            200: jsonResponse(
              {
                type: "object",
                required: ["voice"],
                properties: { voice: voiceSchema },
              },
              "Cloned voice",
            ),
            400: jsonResponse(errorSchema, "Invalid request"),
            403: jsonResponse(errorSchema, "Forbidden"),
            404: jsonResponse(errorSchema, "Session not found"),
          },
        },
      },
      "/api/sessions/{id}/streams": {
        post: {
          summary: "Add RTMP stream to session",
          parameters: [
            { in: "path", name: "id", required: true, schema: { type: "string" } },
          ],
          requestBody: {
            required: true,
            content: {
              "application/json": {
                schema: {
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
              },
            },
          },
          responses: {
            200: jsonResponse(streamSchema, "Created stream"),
            400: jsonResponse(errorSchema, "Invalid request"),
          },
        },
      },
      "/api/sessions/{session_id}/streams/{stream_id}": {
        delete: {
          summary: "Remove RTMP stream from session",
          parameters: [
            { in: "path", name: "session_id", required: true, schema: { type: "string" } },
            { in: "path", name: "stream_id", required: true, schema: { type: "string" } },
          ],
          responses: {
            200: jsonResponse(statusSchema, "Delete status"),
            400: jsonResponse(errorSchema, "Invalid request"),
          },
        },
      },
    },
  };
}
