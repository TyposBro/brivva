import { z } from "zod";

export const LangCodeSchema = z.enum(["ko", "en", "ja", "zh"]);

export const UserInfoSchema = z.object({
  id: z.string().min(1),
  youtube_connected: z.boolean(),
  youtube_channel_name: z.string().nullable(),
  youtube_channel_id: z.string().nullable(),
  created_at: z.number().int(),
});

export const VoiceSchema = z.object({
  id: z.string().min(1),
  user_id: z.string().min(1),
  elevenlabs_voice_id: z.string().min(1),
  name: z.string().min(1),
  created_at: z.number().int(),
});

export const SessionSchema = z.object({
  id: z.string().min(1),
  user_id: z.string().min(1),
  voice_id: z.string().nullable(),
  title: z.string().min(1),
  source_lang: z.string().min(1),
  target_langs: z.string().min(1),
  status: z.string().min(1),
  live_session_id: z.string().nullable(),
  created_at: z.number().int(),
});

export const StreamSchema = z.object({
  id: z.string().min(1),
  session_id: z.string().min(1),
  lang: z.string().min(1),
  platform: z.string().min(1),
  platform_broadcast_id: z.string().nullable(),
  platform_stream_id: z.string().nullable(),
  stream_key: z.string().nullable(),
  rtmp_url: z.string().nullable(),
  status: z.string().min(1),
  delay_ms: z.number().int(),
  host_gain: z.number(),
  created_at: z.number().int(),
});

export const PlatformCredentialSchema = z.object({
  id: z.string().min(1),
  user_id: z.string().min(1),
  platform: z.string().min(1),
  rtmp_url: z.string().nullable(),
  stream_key: z.string().nullable(),
  display_name: z.string().nullable(),
  created_at: z.number().int(),
  updated_at: z.number().int(),
});

export const ErrorResponseSchema = z.object({
  error: z.string().min(1),
  issues: z
    .array(
      z.object({
        path: z.string(),
        message: z.string(),
      }),
    )
    .optional(),
});

export const StatusResponseSchema = z.object({
  status: z.string().min(1),
});

export const UserQuerySchema = z.object({
  user_id: z.string().min(1),
});

export const PlatformQuerySchema = z.object({
  user_id: z.string().min(1),
  platform: z.string().min(1),
});

export const SessionIdParamsSchema = z.object({
  id: z.string().min(1),
});

export const SessionStreamParamsSchema = z.object({
  session_id: z.string().min(1),
  stream_id: z.string().min(1),
});

export const AuthTokenRequestSchema = z.object({
  user_id: z.string().min(1),
});

export const AuthTokenResponseSchema = z.object({
  token: z.string().min(1),
});

export const PlatformConfigSchema = z.object({
  platform: z.string().min(1),
  lang: z.string().min(1).optional(),
  rtmp_url: z.string().min(1).optional(),
  stream_key: z.string().min(1).optional(),
  delay_ms: z.number().int().optional(),
  host_gain: z.number().optional(),
});

export const CreateSessionRequestSchema = z.object({
  user_id: z.string().min(1),
  title: z.string().min(1),
  source_lang: z.string().min(1),
  target_langs: z.array(z.string().min(1)).min(1),
  voice_id: z.string().min(1).optional(),
  platforms: z.array(PlatformConfigSchema).optional(),
  privacy_status: z.string().min(1).optional(),
});

export const CreateSessionResponseSchema = z.object({
  session: SessionSchema,
  streams: z.array(StreamSchema),
  errors: z.array(z.string()).optional(),
});

export const ListSessionsResponseSchema = z.object({
  sessions: z.array(SessionSchema),
});

export const GetSessionResponseSchema = z.object({
  session: SessionSchema.nullable(),
  streams: z.array(StreamSchema),
});

export const CloneSessionVoiceRequestSchema = z.object({
  user_id: z.string().min(1),
  audio_base64: z.string().min(1),
  name: z.string().min(1).optional(),
});

export const CloneSessionVoiceResponseSchema = z.object({
  voice: VoiceSchema,
});

export const AddStreamRequestSchema = z.object({
  lang: z.string().min(1),
  platform: z.string().min(1),
  rtmp_url: z.string().min(1),
  stream_key: z.string().min(1),
  delay_ms: z.number().int().optional(),
  host_gain: z.number().optional(),
});

export const ListVoicesResponseSchema = z.object({
  voices: z.array(VoiceSchema),
});

export const CreateVoiceRequestSchema = z.object({
  user_id: z.string().min(1),
  name: z.string().min(1),
  audio_base64: z.string().min(1),
});

export const ListCredentialsResponseSchema = z.object({
  credentials: z.array(PlatformCredentialSchema),
});

export const SaveCredentialRequestSchema = z.object({
  user_id: z.string().min(1),
  platform: z.string().min(1),
  rtmp_url: z.string().optional(),
  stream_key: z.string().optional(),
  display_name: z.string().optional(),
});

export const InternalSessionBundleSchema = z.object({
  session: SessionSchema,
  streams: z.array(StreamSchema),
  voice: VoiceSchema.nullable(),
});

export const InternalSessionStatusUpdateSchema = z.object({
  status: z.string().min(1),
  live_session_id: z.string().nullable().optional(),
});

export type UserInfo = z.infer<typeof UserInfoSchema>;
export type Voice = z.infer<typeof VoiceSchema>;
export type Session = z.infer<typeof SessionSchema>;
export type StreamRecord = z.infer<typeof StreamSchema>;
export type PlatformCredential = z.infer<typeof PlatformCredentialSchema>;
export type AuthTokenRequest = z.infer<typeof AuthTokenRequestSchema>;
export type AuthTokenResponse = z.infer<typeof AuthTokenResponseSchema>;
export type PlatformConfig = z.infer<typeof PlatformConfigSchema>;
export type CreateSessionRequest = z.infer<typeof CreateSessionRequestSchema>;
export type CreateSessionResponse = z.infer<typeof CreateSessionResponseSchema>;
export type ListSessionsResponse = z.infer<typeof ListSessionsResponseSchema>;
export type GetSessionResponse = z.infer<typeof GetSessionResponseSchema>;
export type CloneSessionVoiceRequest = z.infer<typeof CloneSessionVoiceRequestSchema>;
export type CloneSessionVoiceResponse = z.infer<typeof CloneSessionVoiceResponseSchema>;
export type AddStreamRequest = z.infer<typeof AddStreamRequestSchema>;
export type CreateVoiceRequest = z.infer<typeof CreateVoiceRequestSchema>;
export type SaveCredentialRequest = z.infer<typeof SaveCredentialRequestSchema>;
