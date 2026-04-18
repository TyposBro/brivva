// VITE_API_URL points at the Workers CRUD API (D1-backed, global edge).
// VITE_WORKER_URL is reserved for the Fargate media WS (long-lived audio).
// Fallback to VITE_WORKER_URL for local dev when running against a single-process
// legacy backend.
const API_BASE =
  import.meta.env.VITE_API_URL ??
  import.meta.env.VITE_WORKER_URL ??
  "http://localhost:3000";

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

/**
 * Fetch a short-lived Workers-signed JWT. Required by Fargate's media WS
 * (`sub` claim must match the session owner before host:audio is accepted).
 * Cache the token for its ~15 min validity in the caller — the WS upgrade
 * is the only consumer.
 */
export function getAuthToken(userId: string): Promise<{ token: string }> {
  return request("/auth/token", {
    method: "POST",
    body: JSON.stringify({ user_id: userId }),
  });
}

// ── YouTube OAuth ───────────────────────────────────────

export function youtubeAuthUrl(userId: string): string {
  return `${API_BASE}/auth/youtube?user_id=${encodeURIComponent(userId)}`;
}

// ── Platforms ───────────────────────────────────────────

export type PlatformConfig = {
  platform: string;
  lang?: string;
  rtmp_url?: string;
  stream_key?: string;
  /** Fargate holds the delayed host media this long before pushing to RTMP,
   *  giving STT+translate+TTS a window to overlay. Target-lang streams
   *  typically 1500–3000 ms depending on expected latency. Omit for 2000. */
  delay_ms?: number;
  /** Gain applied to delayed original audio under translated TTS.
   *  Range 0–1. 1 = full (source stream), 0.2 = quiet underlay (target). */
  host_gain?: number;
};

export type Platform = {
  id: string;
  label: string;
  region: string;
  auto: boolean;
  defaultRtmp: string;
  help: string;
  settingsUrl: string;
  keyOnly: boolean;
};

export const PLATFORMS: Platform[] = [
  // Global
  { id: "youtube", label: "YouTube", region: "Global", auto: true, defaultRtmp: "", help: "Auto-creates broadcasts via API. Connect your account above.", settingsUrl: "", keyOnly: false },
  { id: "instagram", label: "Instagram", region: "Global", auto: false, defaultRtmp: "rtmps://live-upload.instagram.com:443/rtmp/", help: "Open Instagram app → tap + → Live → tap ⚙️ → 'Stream with external device' → copy Stream Key.", settingsUrl: "", keyOnly: true },
  { id: "tiktok", label: "TikTok", region: "Global", auto: false, defaultRtmp: "", help: "Download TikTok LIVE Studio desktop app → Go Live → copy Server URL & Stream Key. Requires 1,000+ followers.", settingsUrl: "", keyOnly: false },
  { id: "twitch", label: "Twitch", region: "Global", auto: false, defaultRtmp: "rtmp://live.twitch.tv/app/", help: "Twitch.tv → Creator Dashboard → Settings → Stream → copy Primary Stream Key.", settingsUrl: "https://dashboard.twitch.tv/settings/stream", keyOnly: true },
  // Korea
  { id: "coupang", label: "Coupang Live", region: "Korea", auto: false, defaultRtmp: "", help: "쿠팡 Wing → Live & Shorts → Self live → 라이브 만들기 → OBS 설정 → copy RTMP URL & 스트림 키.", settingsUrl: "", keyOnly: false },
  { id: "naver", label: "Naver Shopping Live", region: "Korea", auto: false, defaultRtmp: "", help: "네이버 스마트스토어센터 → 쇼핑라이브 → 라이브 예약/시작 → 외부 송출 설정 → copy RTMP URL & 스트림 키.", settingsUrl: "https://sell.smartstore.naver.com/", keyOnly: false },
  // Japan
  { id: "rakuten", label: "Rakuten Live", region: "Japan", auto: false, defaultRtmp: "", help: "Rakuten RMS → ライブコマース → 配信設定 → copy RTMP URL & ストリームキー.", settingsUrl: "https://rms.rakuten.co.jp/", keyOnly: false },
  // China
  { id: "douyin", label: "Douyin (抖音)", region: "China", auto: false, defaultRtmp: "", help: "Download 抖音直播伴侣 desktop app → login → 开始直播 → 推流地址 will appear. Copy 服务器地址 & 推流码.", settingsUrl: "", keyOnly: false },
  { id: "taobao", label: "Taobao Live (淘宝直播)", region: "China", auto: false, defaultRtmp: "", help: "淘宝直播中控台 → 创建直播 → OBS推流 → copy RTMP URL & 推流码.", settingsUrl: "https://liveplatform.taobao.com/live/liveList.htm", keyOnly: false },
  { id: "kuaishou", label: "Kuaishou (快手)", region: "China", auto: false, defaultRtmp: "rtmp://live.kuaishou.com/live/", help: "快手直播伴侣 desktop app → login → 开播设置 → copy 推流码 (Stream Key).", settingsUrl: "", keyOnly: true },
  { id: "xiaohongshu", label: "Xiaohongshu (小红书)", region: "China", auto: false, defaultRtmp: "", help: "小红书 App → + → 直播 → 设置 → 电脑模式 → copy 授权码 → 小红书直播助手 desktop app → paste auth code → copy 推流地址 & 推流码.", settingsUrl: "", keyOnly: false },
  { id: "bilibili", label: "Bilibili (哔哩哔哩)", region: "China", auto: false, defaultRtmp: "rtmp://live-push.bilivideo.com/live-bvc/", help: "Bilibili → 直播中心 → 我的直播间 → 开始直播 → copy 推流码 (Stream Key).", settingsUrl: "https://link.bilibili.com/p/center/index#/my-room/start-live", keyOnly: true },
  // Custom / Testing
  { id: "custom", label: "Custom RTMP", region: "Other", auto: false, defaultRtmp: "", help: "Enter any RTMP/RTMPS endpoint URL and stream key.", settingsUrl: "", keyOnly: false },
  { id: "local-test", label: "Local Test (MediaMTX)", region: "Other", auto: true, defaultRtmp: "rtmp://rtmp:1935/live/", help: "Auto-creates one stream per language on local MediaMTX. View with: ffplay rtmp://localhost:1935/live/{lang}", settingsUrl: "", keyOnly: false },
];

/**
 * Platform → default language mapping.
 * null = user picks (YouTube, Custom, Local Test).
 * Regional platforms auto-assign their audience's language.
 */
export const PLATFORM_LANG: Record<string, string | null> = {
  youtube: null,
  instagram: "en",
  tiktok: "en",
  twitch: "en",
  coupang: "ko",
  naver: "ko",
  rakuten: "ja",
  douyin: "zh",
  taobao: "zh",
  kuaishou: "zh",
  xiaohongshu: "zh",
  bilibili: "zh",
  custom: null,
  "local-test": null,
};

export const LANGS = [
  { code: "ko", label: "Korean", flag: "\uD83C\uDDF0\uD83C\uDDF7" },
  { code: "en", label: "English", flag: "\uD83C\uDDEC\uD83C\uDDE7" },
  { code: "ja", label: "Japanese", flag: "\uD83C\uDDEF\uD83C\uDDF5" },
  { code: "zh", label: "Chinese", flag: "\uD83C\uDDE8\uD83C\uDDF3" },
] as const;

export function langLabel(code: string): string {
  return LANGS.find((l) => l.code === code)?.label ?? code;
}

export function langFlag(code: string): string {
  return LANGS.find((l) => l.code === code)?.flag ?? "";
}

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
  privacy_status?: string;
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

// ── Platform Credentials (Vault) ────────────────────────

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

export function listCredentials(userId: string): Promise<{ credentials: PlatformCredential[] }> {
  return request(`/api/credentials?user_id=${encodeURIComponent(userId)}`);
}

export function saveCredential(body: {
  user_id: string;
  platform: string;
  rtmp_url?: string;
  stream_key?: string;
  display_name?: string;
}): Promise<PlatformCredential> {
  return request("/api/credentials", {
    method: "POST",
    body: JSON.stringify(body),
  });
}

export function deleteCredential(userId: string, platform: string): Promise<{ status: string }> {
  return request(`/api/credentials?user_id=${encodeURIComponent(userId)}&platform=${encodeURIComponent(platform)}`, {
    method: "DELETE",
  });
}

// ── Magic Paste Detection ───────────────────────────────

/** Auto-detect platform from a pasted RTMP URL string */
export function detectPlatform(input: string): { platform: string; rtmpUrl: string; streamKey: string } | null {
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
      return { platform, rtmpUrl: baseUrl, streamKey: key };
    }
  }

  if (trimmed.startsWith("rtmp://") || trimmed.startsWith("rtmps://")) {
    const lastSlash = trimmed.lastIndexOf("/");
    return {
      platform: "custom",
      rtmpUrl: trimmed.slice(0, lastSlash + 1),
      streamKey: trimmed.slice(lastSlash + 1),
    };
  }

  return null;
}
