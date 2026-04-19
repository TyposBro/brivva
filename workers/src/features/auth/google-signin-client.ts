// Google OAuth for application sign-in (scope: openid email profile).
//
// Distinct from features/youtube/google-oauth-client.ts, which uses YouTube
// scopes + a separate redirect URI tied to the add-channel flow. This module
// exists so a user can sign in with Google without granting access to their
// YouTube account, and can later connect YouTube as a separate step.

export type GoogleSigninEnv = {
  GOOGLE_CLIENT_ID: string;
  GOOGLE_CLIENT_SECRET: string;
  GOOGLE_SIGNIN_REDIRECT_URI: string;
};

const SCOPES = "openid email profile";

export function authorizeUrl(env: GoogleSigninEnv, state: string): string {
  const params = new URLSearchParams({
    client_id: env.GOOGLE_CLIENT_ID,
    redirect_uri: env.GOOGLE_SIGNIN_REDIRECT_URI,
    response_type: "code",
    scope: SCOPES,
    access_type: "online",
    prompt: "select_account",
    state,
  });
  return `https://accounts.google.com/o/oauth2/v2/auth?${params}`;
}

type TokenResponse = {
  access_token: string;
  id_token?: string;
  expires_in: number;
  token_type: string;
  scope: string;
};

export async function exchangeCode(
  env: GoogleSigninEnv,
  code: string,
): Promise<TokenResponse> {
  const body = new URLSearchParams({
    code,
    client_id: env.GOOGLE_CLIENT_ID,
    client_secret: env.GOOGLE_CLIENT_SECRET,
    redirect_uri: env.GOOGLE_SIGNIN_REDIRECT_URI,
    grant_type: "authorization_code",
  });
  const resp = await fetch("https://oauth2.googleapis.com/token", {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body,
  });
  if (!resp.ok) {
    throw new Error(`OAuth token exchange failed: ${resp.status} ${await resp.text()}`);
  }
  return await resp.json<TokenResponse>();
}

export type GoogleUserInfo = {
  sub: string;
  email: string | null;
  name: string | null;
  picture: string | null;
};

export async function fetchUserInfo(accessToken: string): Promise<GoogleUserInfo> {
  const resp = await fetch("https://openidconnect.googleapis.com/v1/userinfo", {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  if (!resp.ok) {
    throw new Error(`userinfo failed: ${resp.status} ${await resp.text()}`);
  }
  const json = await resp.json<{
    sub: string;
    email?: string;
    name?: string;
    picture?: string;
  }>();
  if (!json.sub) throw new Error("userinfo missing sub");
  return {
    sub: json.sub,
    email: json.email ?? null,
    name: json.name ?? null,
    picture: json.picture ?? null,
  };
}
