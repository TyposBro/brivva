// Re-export the Drizzle-inferred row types so existing imports (`./types`)
// keep working without each call site having to pick between `./types` and
// `./schema`. schema.ts stays the single source of truth.

export type {
  PlatformCredential,
  Session,
  StreamRecord,
  User,
  Voice,
} from "./schema";

import type { User } from "./schema";

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
