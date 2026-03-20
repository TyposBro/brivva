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

export type Platform = {
  id: string;
  label: string;
  region: string;
  auto: boolean;
  defaultRtmp: string;
  help: string;
};

export const PLATFORMS: Platform[] = [
  // Global
  { id: "youtube", label: "YouTube", region: "Global", auto: true, defaultRtmp: "", help: "Auto-creates broadcasts via API. Connect your account above." },
  { id: "instagram", label: "Instagram", region: "Global", auto: false, defaultRtmp: "rtmps://live-upload.instagram.com:443/rtmp/", help: "Instagram.com → Create → Live → copy URL & Stream Key. Requires Professional account." },
  { id: "tiktok", label: "TikTok", region: "Global", auto: false, defaultRtmp: "", help: "TikTok LIVE Studio → copy Server URL & Stream Key. Requires 1,000+ followers." },
  { id: "twitch", label: "Twitch", region: "Global", auto: false, defaultRtmp: "rtmp://live.twitch.tv/app/", help: "Twitch Dashboard → Settings → Stream → copy Stream Key." },
  // Korea
  { id: "coupang", label: "Coupang Live", region: "Korea", auto: false, defaultRtmp: "", help: "Coupang Seller Portal → Live → copy RTMP URL & Key." },
  { id: "naver", label: "Naver Shopping Live", region: "Korea", auto: false, defaultRtmp: "", help: "Naver Shopping Live Studio → copy RTMP URL & Stream Key." },
  // Japan
  { id: "rakuten", label: "Rakuten Live", region: "Japan", auto: false, defaultRtmp: "", help: "Rakuten Live Commerce dashboard → copy RTMP URL & Key." },
  // China
  { id: "douyin", label: "Douyin (抖音)", region: "China", auto: false, defaultRtmp: "", help: "Douyin Live Companion → 推流地址 will appear. Copy Server URL & Stream Key." },
  { id: "taobao", label: "Taobao Live (淘宝直播)", region: "China", auto: false, defaultRtmp: "", help: "Taobao Live Studio → OBS推流 → copy RTMP URL & 推流码." },
  { id: "kuaishou", label: "Kuaishou (快手)", region: "China", auto: false, defaultRtmp: "rtmp://live.kuaishou.com/live/", help: "Kuaishou Live Center → 直播设置 → copy 推流码 (Stream Key)." },
  { id: "xiaohongshu", label: "Xiaohongshu (小红书)", region: "China", auto: false, defaultRtmp: "", help: "小红书 App → + → Live → Settings → Computer mode → copy auth code → get URL & Key from web." },
  { id: "bilibili", label: "Bilibili (哔哩哔哩)", region: "China", auto: false, defaultRtmp: "rtmp://live-push.bilivideo.com/live-bvc/", help: "Bilibili Live Center → 开始直播 → copy 推流地址 & 推流码." },
  // Custom
  { id: "custom", label: "Custom RTMP", region: "Other", auto: false, defaultRtmp: "", help: "Enter any RTMP/RTMPS endpoint URL and stream key." },
];

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
