// Re-export the Drizzle-inferred row types so existing imports (`./types`)
// keep working without each call site having to pick between `./types` and
// `./schema`. schema.ts stays the single source of truth.

export type {
  PlatformCredential,
  Session,
  SessionMetrics,
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
  email: string | null;
  name: string | null;
  picture: string | null;
  onboarding_completed_at: number | null;
  active_voice_id: string | null;
  billing_tier: string;
  bills_to: string | null;
  created_at: number;
};

export function toUserInfo(u: User): UserInfo {
  return {
    id: u.id,
    youtube_connected: Boolean(u.youtube_refresh_token),
    youtube_channel_name: u.youtube_channel_name,
    youtube_channel_id: u.youtube_channel_id,
    email: u.email,
    name: u.name,
    picture: u.picture,
    onboarding_completed_at: u.onboarding_completed_at,
    active_voice_id: u.active_voice_id,
    billing_tier: u.billing_tier,
    bills_to: u.bills_to,
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
  OAUTH_REDIRECT_URI: string; // YouTube add-channel redirect — matches Google Console
  GOOGLE_SIGNIN_REDIRECT_URI: string; // Sign-in redirect — separate Google Console entry
  FRONTEND_URL: string;
  INTERNAL_SECRET: string; // shared between Workers and Fargate (server→worker calls)
  // Grip Cloud Seller API credentials (set 2026-04-19). When both are
  // present and the session destination includes `platform=grip`, Workers
  // calls the Seller API to provision fresh RTMP creds per session. When
  // absent, orchestration falls back to the paste-creds row saved via
  // POST /auth/grip.
  GRIP_ACCESS_KEY?: string;
  GRIP_SECRET_KEY?: string;
  // Optional — unset during early dev, required before turning billing on.
  STRIPE_WEBHOOK_SECRET?: string;
};
