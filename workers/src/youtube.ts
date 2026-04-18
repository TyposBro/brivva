// Google OAuth for YouTube Live Streaming + Data API.
// Ported from server-rs/src/youtube.rs. Keeps scope + redirect URI aligned.

import type { Env } from "./types";

const SCOPES =
  "https://www.googleapis.com/auth/youtube https://www.googleapis.com/auth/youtube.readonly";

export function authorizeUrl(env: Env, state: string): string {
  const params = new URLSearchParams({
    client_id: env.GOOGLE_CLIENT_ID,
    redirect_uri: env.OAUTH_REDIRECT_URI,
    response_type: "code",
    scope: SCOPES,
    access_type: "offline",
    prompt: "consent",
    state,
  });
  return `https://accounts.google.com/o/oauth2/v2/auth?${params}`;
}

type TokenResponse = {
  access_token: string;
  refresh_token?: string;
  expires_in: number;
  token_type: string;
  scope: string;
};

export async function exchangeCode(env: Env, code: string): Promise<TokenResponse> {
  const body = new URLSearchParams({
    code,
    client_id: env.GOOGLE_CLIENT_ID,
    client_secret: env.GOOGLE_CLIENT_SECRET,
    redirect_uri: env.OAUTH_REDIRECT_URI,
    grant_type: "authorization_code",
  });
  const resp = await fetch("https://oauth2.googleapis.com/token", {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body,
  });
  if (!resp.ok) throw new Error(`OAuth token exchange failed: ${resp.status} ${await resp.text()}`);
  return await resp.json<TokenResponse>();
}

export async function refreshAccessToken(
  env: Env,
  refreshToken: string,
): Promise<{ access_token: string; expires_in: number }> {
  const body = new URLSearchParams({
    refresh_token: refreshToken,
    client_id: env.GOOGLE_CLIENT_ID,
    client_secret: env.GOOGLE_CLIENT_SECRET,
    grant_type: "refresh_token",
  });
  const resp = await fetch("https://oauth2.googleapis.com/token", {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body,
  });
  if (!resp.ok) throw new Error(`OAuth refresh failed: ${resp.status} ${await resp.text()}`);
  return await resp.json<{ access_token: string; expires_in: number }>();
}

type ChannelInfo = { id: string; title: string };

export async function getChannelInfo(accessToken: string): Promise<ChannelInfo> {
  const resp = await fetch(
    "https://www.googleapis.com/youtube/v3/channels?part=snippet&mine=true",
    { headers: { Authorization: `Bearer ${accessToken}` } },
  );
  if (!resp.ok) throw new Error(`Channel lookup failed: ${resp.status} ${await resp.text()}`);
  const json = await resp.json<{ items?: { id: string; snippet: { title: string } }[] }>();
  const item = json.items?.[0];
  if (!item) throw new Error("No channel found for this Google account");
  return { id: item.id, title: item.snippet.title };
}
