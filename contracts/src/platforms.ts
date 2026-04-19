import { z } from "zod";

export const PlatformIdSchema = z.enum([
  "youtube",
  "instagram",
  "tiktok",
  "grip",
  "twitch",
  "coupang",
  "naver",
  "rakuten",
  "douyin",
  "taobao",
  "kuaishou",
  "xiaohongshu",
  "bilibili",
  "custom",
  "local-test",
]);

export const PlatformCatalogEntrySchema = z.object({
  id: PlatformIdSchema,
  label: z.string().min(1),
  region: z.string().min(1),
  auto: z.boolean(),
  defaultRtmp: z.string(),
  help: z.string(),
  settingsUrl: z.string(),
  keyOnly: z.boolean(),
});

export const DetectedPlatformSchema = z.object({
  platform: PlatformIdSchema.or(z.literal("custom")),
  rtmpUrl: z.string().min(1),
  streamKey: z.string(),
});

export const PLATFORM_CATALOG = [
  { id: "youtube", label: "YouTube", region: "Global", auto: true, defaultRtmp: "", help: "Auto-creates broadcasts via API. Connect your account above.", settingsUrl: "", keyOnly: false },
  { id: "instagram", label: "Instagram", region: "Global", auto: false, defaultRtmp: "rtmps://live-upload.instagram.com:443/rtmp/", help: "Open Instagram app -> Live -> external device -> copy stream key.", settingsUrl: "", keyOnly: true },
  { id: "tiktok", label: "TikTok", region: "Global", auto: false, defaultRtmp: "", help: "Open TikTok LIVE Studio -> Go Live -> copy server URL and stream key.", settingsUrl: "", keyOnly: false },
  { id: "grip", label: "Grip", region: "Korea", auto: true, defaultRtmp: "rtmps://live.grip.fans:443/live/", help: "Paste stream key + server URL from Grip admin. Live keys issue 1h before scheduled start, rehearsal keys 5h before (라이브 > 라이브 시작 > 스트림키 발급).", settingsUrl: "https://seller.grip.show/", keyOnly: false },
  { id: "twitch", label: "Twitch", region: "Global", auto: false, defaultRtmp: "rtmp://live.twitch.tv/app/", help: "Twitch Creator Dashboard -> Settings -> Stream -> copy stream key.", settingsUrl: "https://dashboard.twitch.tv/settings/stream", keyOnly: true },
  { id: "coupang", label: "Coupang Live", region: "Korea", auto: false, defaultRtmp: "", help: "Coupang Wing live settings -> copy RTMP URL and stream key.", settingsUrl: "", keyOnly: false },
  { id: "naver", label: "Naver Shopping Live", region: "Korea", auto: false, defaultRtmp: "", help: "Naver Smart Store Center live settings -> copy RTMP URL and stream key.", settingsUrl: "https://sell.smartstore.naver.com/", keyOnly: false },
  { id: "rakuten", label: "Rakuten Live", region: "Japan", auto: false, defaultRtmp: "", help: "Rakuten RMS live settings -> copy RTMP URL and stream key.", settingsUrl: "https://rms.rakuten.co.jp/", keyOnly: false },
  { id: "douyin", label: "Douyin", region: "China", auto: false, defaultRtmp: "", help: "Douyin live companion -> copy server URL and stream key.", settingsUrl: "", keyOnly: false },
  { id: "taobao", label: "Taobao Live", region: "China", auto: false, defaultRtmp: "", help: "Taobao Live control center -> OBS push -> copy RTMP URL and stream key.", settingsUrl: "https://liveplatform.taobao.com/live/liveList.htm", keyOnly: false },
  { id: "kuaishou", label: "Kuaishou", region: "China", auto: false, defaultRtmp: "rtmp://live.kuaishou.com/live/", help: "Kuaishou live companion -> copy stream key.", settingsUrl: "", keyOnly: true },
  { id: "xiaohongshu", label: "Xiaohongshu", region: "China", auto: false, defaultRtmp: "", help: "Xiaohongshu desktop assistant -> copy push URL and stream key.", settingsUrl: "", keyOnly: false },
  { id: "bilibili", label: "Bilibili", region: "China", auto: false, defaultRtmp: "rtmp://live-push.bilivideo.com/live-bvc/", help: "Bilibili live center -> start live -> copy stream key.", settingsUrl: "https://link.bilibili.com/p/center/index#/my-room/start-live", keyOnly: true },
  { id: "custom", label: "Custom RTMP", region: "Other", auto: false, defaultRtmp: "", help: "Enter any RTMP or RTMPS endpoint URL and stream key.", settingsUrl: "", keyOnly: false },
  { id: "local-test", label: "Local Test (MediaMTX)", region: "Other", auto: true, defaultRtmp: "rtmp://rtmp:1935/live/", help: "Auto-creates one stream per language on local MediaMTX.", settingsUrl: "", keyOnly: false },
] as const satisfies readonly z.infer<typeof PlatformCatalogEntrySchema>[];

/**
 * Language catalog for destination language selection. The trailing `pass`
 * entry is NOT a real language — it tells Fargate to bypass STT, translate,
 * and TTS for that destination and RTMP the host's raw audio/video through.
 * Kept in contracts so frontend + workers + OpenAPI stay aligned.
 */
export const LangEntrySchema = z.object({
  code: z.string().min(1),
  label: z.string().min(1),
  flag: z.string().min(1),
});

export const PASS_LANG_CODE = "pass";

export const LANGS = [
  { code: "ko", label: "Korean", flag: "\uD83C\uDDF0\uD83C\uDDF7" },
  { code: "en", label: "English", flag: "\uD83C\uDDEC\uD83C\uDDE7" },
  { code: "ja", label: "Japanese", flag: "\uD83C\uDDEF\uD83C\uDDF5" },
  { code: "zh", label: "Chinese", flag: "\uD83C\uDDE8\uD83C\uDDF3" },
  { code: PASS_LANG_CODE, label: "Passthrough (source)", flag: "\u{1F500}" },
] as const satisfies readonly z.infer<typeof LangEntrySchema>[];

export type LangEntry = (typeof LANGS)[number];

export function isPassthroughLang(code: string): boolean {
  return code === PASS_LANG_CODE;
}

export const PLATFORM_DEFAULT_LANG: Record<string, string | null> = {
  youtube: null,
  instagram: "en",
  tiktok: "en",
  grip: "ko",
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

export type PlatformId = z.infer<typeof PlatformIdSchema>;
export type PlatformCatalogEntry = z.infer<typeof PlatformCatalogEntrySchema>;
export type DetectedPlatform = z.infer<typeof DetectedPlatformSchema>;
