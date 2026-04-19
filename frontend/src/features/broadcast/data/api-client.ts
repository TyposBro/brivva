import {
  AuthTokenResponseSchema,
  CloneSessionVoiceResponseSchema,
  CreateSessionResponseSchema,
  ErrorResponseSchema,
  GetSessionResponseSchema,
  ListCredentialsResponseSchema,
  ListSessionsResponseSchema,
  ListVoicesResponseSchema,
  PlatformCredentialSchema,
  StatusResponseSchema,
  StreamSchema,
  UpdateSessionVoicePresetResponseSchema,
  UserInfoSchema,
  VoiceSchema,
  type AuthTokenResponse,
  type CloneSessionVoiceRequest,
  type CreateSessionResponse,
  type PlatformConfig,
  type PlatformCredential,
  type Session,
  type StreamRecord,
  type UserInfo,
  type Voice,
} from "@brivva/contracts/http";
import {
  DetectedPlatformSchema,
  LANGS as CONTRACT_LANGS,
  PASS_LANG_CODE,
  PLATFORM_CATALOG,
  PLATFORM_DEFAULT_LANG,
  isPassthroughLang,
  type DetectedPlatform,
  type LangEntry,
  type PlatformCatalogEntry,
} from "@brivva/contracts/platforms";
import { client } from "../../../core/contracts/workers-client";
import { appConfig } from "../../../core/config/app-config";

export type {
  AuthTokenResponse,
  CloneSessionVoiceRequest,
  CreateSessionResponse,
  PlatformConfig,
  PlatformCredential,
  Session,
  UserInfo,
  Voice,
} from "@brivva/contracts/http";

type StreamInfo = StreamRecord & {
  broadcast_id?: string;
  stream_id?: string;
  /** Runtime-only surface for sharing the YouTube link; `null`/undefined on
   *  non-YouTube destinations. */
  watch_url?: string | null;
  error?: string;
};

export type { StreamInfo };

export type Platform = PlatformCatalogEntry;

const StreamInfoSchema = StreamSchema.transform((stream): StreamInfo => ({
  ...stream,
  broadcast_id: stream.platform_broadcast_id ?? undefined,
  stream_id: stream.platform_stream_id ?? undefined,
  watch_url: stream.watch_url ?? undefined,
}));

async function parseResult<T>(
  promise: Promise<{ data?: unknown; error?: unknown; response: Response }>,
  schema: { parse(input: unknown): T },
): Promise<T> {
  const { data, error, response } = await promise;
  if (!response.ok || data === undefined) {
    const parsed = ErrorResponseSchema.safeParse(error);
    if (parsed.success) {
      throw new Error(`API ${response.status}: ${parsed.data.error}`);
    }
    throw new Error(`API ${response.status}: request failed`);
  }
  return schema.parse(data);
}

export function getUser(userId: string): Promise<UserInfo> {
  return parseResult(
    client().GET("/api/user", { params: { query: { user_id: userId } } }),
    UserInfoSchema,
  );
}

export function getAuthToken(userId: string): Promise<AuthTokenResponse> {
  return parseResult(
    client().POST("/auth/token", { body: { user_id: userId } }),
    AuthTokenResponseSchema,
  );
}

export function youtubeAuthUrl(userId: string): string {
  return `${appConfig().workersApiBase}/auth/youtube?user_id=${encodeURIComponent(userId)}`;
}

export const PLATFORMS: readonly Platform[] = PLATFORM_CATALOG;
export const PLATFORM_LANG = PLATFORM_DEFAULT_LANG;

export const LANGS: readonly LangEntry[] = CONTRACT_LANGS;
export { PASS_LANG_CODE, isPassthroughLang };
export type { LangEntry };

export function langLabel(code: string): string {
  return LANGS.find((l) => l.code === code)?.label ?? code;
}

export function langFlag(code: string): string {
  return LANGS.find((l) => l.code === code)?.flag ?? "";
}

export function createSession(body: {
  user_id: string;
  title: string;
  source_lang: string;
  target_langs: string[];
  voice_id?: string;
  platforms?: PlatformConfig[];
  privacy_status?: string;
}): Promise<CreateSessionResponse> {
  return parseResult(
    client().POST("/api/sessions", { body }),
    CreateSessionResponseSchema.transform((response) => ({
      ...response,
      streams: response.streams.map((stream) => StreamInfoSchema.parse(stream)),
    })),
  );
}

export function listSessions(userId: string): Promise<{ sessions: Session[] }> {
  return parseResult(
    client().GET("/api/sessions", { params: { query: { user_id: userId } } }),
    ListSessionsResponseSchema,
  );
}

export function getSession(id: string): Promise<{ session: Session | null; streams: StreamInfo[] }> {
  return parseResult(
    client().GET("/api/sessions/{id}", { params: { path: { id } } }),
    GetSessionResponseSchema.transform((response) => ({
      ...response,
      streams: response.streams.map((stream) => StreamInfoSchema.parse(stream)),
    })),
  );
}

export function cloneSessionVoice(
  sessionId: string,
  body: CloneSessionVoiceRequest,
): Promise<{ voice: Voice }> {
  return parseResult(
    client().POST("/api/sessions/{id}/voice", {
      params: { path: { id: sessionId } },
      body,
    }),
    CloneSessionVoiceResponseSchema,
  );
}

export type VoicePreset = "cloned" | "female" | "male";

export function updateSessionVoicePreset(
  sessionId: string,
  voice_preset: VoicePreset,
): Promise<{ session: Session }> {
  return parseResult(
    client().PATCH("/api/sessions/{id}/voice-preset", {
      params: { path: { id: sessionId } },
      body: { voice_preset },
    }),
    UpdateSessionVoicePresetResponseSchema,
  );
}

export function deleteSession(id: string): Promise<{ status: string }> {
  return parseResult(
    client().DELETE("/api/sessions/{id}", { params: { path: { id } } }),
    StatusResponseSchema,
  );
}

export function addStream(
  sessionId: string,
  body: { lang: string; platform: string; rtmp_url: string; stream_key: string; delay_ms?: number; host_gain?: number },
): Promise<StreamInfo> {
  return parseResult(
    client().POST("/api/sessions/{id}/streams", {
      params: { path: { id: sessionId } },
      body,
    }),
    StreamInfoSchema,
  );
}

export function removeStream(sessionId: string, streamId: string): Promise<{ status: string }> {
  return parseResult(
    client().DELETE("/api/sessions/{session_id}/streams/{stream_id}", {
      params: { path: { session_id: sessionId, stream_id: streamId } },
    }),
    StatusResponseSchema,
  );
}

export function listVoices(userId: string): Promise<{ voices: Voice[] }> {
  return parseResult(
    client().GET("/api/voices", { params: { query: { user_id: userId } } }),
    ListVoicesResponseSchema,
  );
}

export function createVoice(body: {
  user_id: string;
  name: string;
  audio_base64: string;
  source_lang?: "ko" | "en" | "ja" | "zh";
}): Promise<Voice> {
  return parseResult(client().POST("/api/voices", { body }), VoiceSchema);
}

export function deleteVoice(id: string): Promise<{ status: string }> {
  return parseResult(
    client().DELETE("/api/voices/{id}", { params: { path: { id } } }),
    StatusResponseSchema,
  );
}

export function listCredentials(userId: string): Promise<{ credentials: PlatformCredential[] }> {
  return parseResult(
    client().GET("/api/credentials", { params: { query: { user_id: userId } } }),
    ListCredentialsResponseSchema,
  );
}

export function saveCredential(body: {
  user_id: string;
  platform: string;
  rtmp_url?: string;
  stream_key?: string;
  display_name?: string;
}): Promise<PlatformCredential> {
  return parseResult(client().POST("/api/credentials", { body }), PlatformCredentialSchema);
}

export function deleteCredential(userId: string, platform: string): Promise<{ status: string }> {
  return parseResult(
    client().DELETE("/api/credentials", {
      params: { query: { user_id: userId, platform } },
    }),
    StatusResponseSchema,
  );
}

export function detectPlatform(input: string): DetectedPlatform | null {
  const trimmed = input.trim();

  const patterns: [string, string, string][] = [
    ["rtmps://live-upload.instagram.com", "instagram", "rtmps://live-upload.instagram.com:443/rtmp/"],
    ["rtmp://live.twitch.tv", "twitch", "rtmp://live.twitch.tv/app/"],
    ["rtmp://live.kuaishou.com", "kuaishou", "rtmp://live.kuaishou.com/live/"],
    ["rtmp://live-push.bilivideo.com", "bilibili", "rtmp://live-push.bilivideo.com/live-bvc/"],
    ["rtmp://a.rtmp.youtube.com", "youtube", "rtmp://a.rtmp.youtube.com/live2/"],
    ["rtmps://a.rtmps.youtube.com", "youtube", "rtmps://a.rtmps.youtube.com/live2/"],
  ];

  for (const [prefix, platform, baseUrl] of patterns) {
    if (trimmed.startsWith(prefix)) {
      const key = trimmed.startsWith(baseUrl)
        ? trimmed.slice(baseUrl.length)
        : trimmed.split("/").pop() ?? "";
      return DetectedPlatformSchema.parse({ platform, rtmpUrl: baseUrl, streamKey: key });
    }
  }

  if (trimmed.startsWith("rtmp://") || trimmed.startsWith("rtmps://")) {
    const lastSlash = trimmed.lastIndexOf("/");
    return DetectedPlatformSchema.parse({
      platform: "custom",
      rtmpUrl: trimmed.slice(0, lastSlash + 1),
      streamKey: trimmed.slice(lastSlash + 1),
    });
  }

  return null;
}
