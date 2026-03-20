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
  settingsUrl: string;
  keyOnly: boolean;
};

export const PLATFORMS: Platform[] = [
  // Global
  { id: "youtube", label: "YouTube", region: "Global", auto: true, defaultRtmp: "", help: "Auto-creates broadcasts via API. Connect your account above.", settingsUrl: "", keyOnly: false },
  { id: "instagram", label: "Instagram", region: "Global", auto: false, defaultRtmp: "rtmps://live-upload.instagram.com:443/rtmp/", help: "Instagram.com → Create → Live → copy URL & Stream Key. Requires Professional account.", settingsUrl: "https://www.instagram.com/live/producer/", keyOnly: true },
  { id: "tiktok", label: "TikTok", region: "Global", auto: false, defaultRtmp: "", help: "TikTok LIVE Studio → copy Server URL & Stream Key. Requires 1,000+ followers.", settingsUrl: "https://www.tiktok.com/studio/live", keyOnly: false },
  { id: "twitch", label: "Twitch", region: "Global", auto: false, defaultRtmp: "rtmp://live.twitch.tv/app/", help: "Twitch Dashboard → Settings → Stream → copy Stream Key.", settingsUrl: "https://dashboard.twitch.tv/u/_/settings/stream", keyOnly: true },
  // Korea
  { id: "coupang", label: "Coupang Live", region: "Korea", auto: false, defaultRtmp: "", help: "Coupang Seller Portal → Live → copy RTMP URL & Key.", settingsUrl: "https://wing.coupang.com/", keyOnly: false },
  { id: "naver", label: "Naver Shopping Live", region: "Korea", auto: false, defaultRtmp: "", help: "Naver Shopping Live Studio → copy RTMP URL & Stream Key.", settingsUrl: "https://shoppinglive.naver.com/studio", keyOnly: false },
  // Japan
  { id: "rakuten", label: "Rakuten Live", region: "Japan", auto: false, defaultRtmp: "", help: "Rakuten Live Commerce dashboard → copy RTMP URL & Key.", settingsUrl: "https://live.rakuten.co.jp/", keyOnly: false },
  // China
  { id: "douyin", label: "Douyin (抖音)", region: "China", auto: false, defaultRtmp: "", help: "Douyin Live Companion → 推流地址 will appear. Copy Server URL & Stream Key.", settingsUrl: "https://live.douyin.com/", keyOnly: false },
  { id: "taobao", label: "Taobao Live (淘宝直播)", region: "China", auto: false, defaultRtmp: "", help: "Taobao Live Studio → OBS推流 → copy RTMP URL & 推流码.", settingsUrl: "https://liveplatform.taobao.com/", keyOnly: false },
  { id: "kuaishou", label: "Kuaishou (快手)", region: "China", auto: false, defaultRtmp: "rtmp://live.kuaishou.com/live/", help: "Kuaishou Live Center → 直播设置 → copy 推流码 (Stream Key).", settingsUrl: "https://studio.kuaishou.com/", keyOnly: true },
  { id: "xiaohongshu", label: "Xiaohongshu (小红书)", region: "China", auto: false, defaultRtmp: "", help: "小红书 App → + → Live → Settings → Computer mode → copy auth code → get URL & Key from web.", settingsUrl: "https://www.xiaohongshu.com/", keyOnly: false },
  { id: "bilibili", label: "Bilibili (哔哩哔哩)", region: "China", auto: false, defaultRtmp: "rtmp://live-push.bilivideo.com/live-bvc/", help: "Bilibili Live Center → 开始直播 → copy 推流地址 & 推流码.", settingsUrl: "https://link.bilibili.com/p/center/index", keyOnly: true },
  // Custom
  { id: "custom", label: "Custom RTMP", region: "Other", auto: false, defaultRtmp: "", help: "Enter any RTMP/RTMPS endpoint URL and stream key.", settingsUrl: "", keyOnly: false },
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
