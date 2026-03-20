const API_BASE = import.meta.env.VITE_WORKER_URL ?? "http://localhost:3000";

async function request<T>(path: string, opts?: RequestInit): Promise<T> {
  const resp = await fetch(`${API_BASE}${path}`, {
    headers: { "Content-Type": "application/json" },
    ...opts,
  });
  if (!resp.ok) {
    const body = await resp.text().catch(() => "");
    throw new Error(`API ${resp.status}: ${body}`);
  }
  return resp.json();
}

// ── User ────────────────────────────────────────────────

export type UserInfo = {
  id: string;
  youtube_connected: boolean;
  youtube_channel_name: string | null;
  youtube_channel_id: string | null;
  created_at: number;
};

export function getUser(userId: string): Promise<UserInfo> {
  return request(`/api/user?user_id=${encodeURIComponent(userId)}`);
}

// ── YouTube OAuth ───────────────────────────────────────

export function youtubeAuthUrl(userId: string): string {
  return `${API_BASE}/auth/youtube?user_id=${encodeURIComponent(userId)}`;
}

// ── Platforms ───────────────────────────────────────────

export type PlatformConfig = {
  platform: string;
  rtmp_url?: string;
  stream_key?: string;
};

export const PLATFORMS = [
  { id: "youtube", label: "YouTube", auto: true },
  { id: "instagram", label: "Instagram", auto: false, defaultRtmp: "rtmps://live-upload.instagram.com:443/rtmp/" },
  { id: "coupang", label: "Coupang Live", auto: false, defaultRtmp: "" },
  { id: "tiktok", label: "TikTok", auto: false, defaultRtmp: "" },
  { id: "twitch", label: "Twitch", auto: false, defaultRtmp: "rtmp://live.twitch.tv/app/" },
  { id: "custom", label: "Custom RTMP", auto: false, defaultRtmp: "" },
] as const;

// ── Sessions ────────────────────────────────────────────

export type Session = {
  id: string;
  user_id: string;
  voice_id: string | null;
  title: string;
  source_lang: string;
  target_langs: string;
  status: string;
  room_id: string | null;
  created_at: number;
};

export type StreamInfo = {
  id: string;
  lang: string;
  platform?: string;
  broadcast_id?: string;
  stream_id?: string;
  rtmp_url?: string;
  stream_key?: string;
  status?: string;
  error?: string;
};

export type CreateSessionResponse = {
  session: Session;
  streams: StreamInfo[];
  errors?: string[];
};

export function createSession(body: {
  user_id: string;
  title: string;
  source_lang: string;
  target_langs: string[];
  voice_id?: string;
  platforms?: PlatformConfig[];
}): Promise<CreateSessionResponse> {
  return request("/api/sessions", {
    method: "POST",
    body: JSON.stringify(body),
  });
}

export function listSessions(
  userId: string
): Promise<{ sessions: Session[] }> {
  return request(`/api/sessions?user_id=${encodeURIComponent(userId)}`);
}

export function getSession(
  id: string
): Promise<{ session: Session | null; streams: StreamInfo[] }> {
  return request(`/api/sessions/${encodeURIComponent(id)}`);
}

export function deleteSession(
  id: string
): Promise<{ status: string }> {
  return request(`/api/sessions/${encodeURIComponent(id)}`, {
    method: "DELETE",
  });
}

// ── Stream management ───────────────────────────────────

export function addStream(
  sessionId: string,
  body: { lang: string; platform: string; rtmp_url: string; stream_key: string }
): Promise<StreamInfo> {
  return request(`/api/sessions/${encodeURIComponent(sessionId)}/streams`, {
    method: "POST",
    body: JSON.stringify(body),
  });
}

export function removeStream(
  sessionId: string,
  streamId: string
): Promise<{ status: string }> {
  return request(
    `/api/sessions/${encodeURIComponent(sessionId)}/streams/${encodeURIComponent(streamId)}`,
    { method: "DELETE" }
  );
}

// ── Voices ──────────────────────────────────────────────

export type Voice = {
  id: string;
  user_id: string;
  elevenlabs_voice_id: string;
  name: string;
  created_at: number;
};

export function listVoices(
  userId: string
): Promise<{ voices: Voice[] }> {
  return request(`/api/voices?user_id=${encodeURIComponent(userId)}`);
}

export function createVoice(body: {
  user_id: string;
  name: string;
  audio_base64: string;
}): Promise<Voice> {
  return request("/api/voices", {
    method: "POST",
    body: JSON.stringify(body),
  });
}

export function deleteVoice(id: string): Promise<{ status: string }> {
  return request(`/api/voices/${encodeURIComponent(id)}`, {
    method: "DELETE",
  });
}
