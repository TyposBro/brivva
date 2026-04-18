// Shared row types — field names mirror D1 columns so we can pass rows
// straight through JSON without renaming. Keep in sync with migrations/.

export type User = {
  id: string;
  youtube_channel_id: string | null;
  youtube_channel_name: string | null;
  youtube_access_token: string | null;
  youtube_refresh_token: string | null;
  youtube_token_expires_at: number | null;
  created_at: number;
};

export type Voice = {
  id: string;
  user_id: string;
  elevenlabs_voice_id: string;
  name: string;
  created_at: number;
};

export type Session = {
  id: string;
  user_id: string;
  voice_id: string | null;
  title: string;
  source_lang: string;
  target_langs: string; // JSON-encoded array
  status: string; // 'setup' | 'live' | 'ended'
  live_session_id: string | null;
  created_at: number;
};

export type StreamRecord = {
  id: string;
  session_id: string;
  lang: string;
  platform: string;
  platform_broadcast_id: string | null;
  platform_stream_id: string | null;
  stream_key: string | null;
  rtmp_url: string | null;
  status: string;
  delay_ms: number;
  host_gain: number;
  created_at: number;
};

export type PlatformCredential = {
  id: string;
  user_id: string;
  platform: string;
  rtmp_url: string | null;
  stream_key: string | null;
  display_name: string | null;
  created_at: number;
  updated_at: number;
};

// Shape the Fargate WS handler and FE already expect.
export type UserInfo = {
  id: string;
  youtube_connected: boolean;
  youtube_channel_name: string | null;
  youtube_channel_id: string | null;
  created_at: number;
};

export function toUserInfo(u: User): UserInfo {
  return {
    id: u.id,
    youtube_connected: Boolean(u.youtube_refresh_token),
    youtube_channel_name: u.youtube_channel_name,
    youtube_channel_id: u.youtube_channel_id,
    created_at: u.created_at,
  };
}

// Worker env bindings. Declared in wrangler.toml.
export type Env = {
  DB: D1Database;
  ELEVENLABS_API_KEY: string;
  GOOGLE_CLIENT_ID: string;
  GOOGLE_CLIENT_SECRET: string;
  JWT_SECRET: string;
  OAUTH_REDIRECT_URI: string; // full URL — must match Google Console
  FRONTEND_URL: string;
  INTERNAL_SECRET: string; // shared between Workers and Fargate (server→worker calls)
};
