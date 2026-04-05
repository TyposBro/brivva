import type { Platform, Lang } from "./types";

export const LANGS: Lang[] = [
  { code: "ko", label: "Korean", flag: "\uD83C\uDDF0\uD83C\uDDF7" },
  { code: "en", label: "English", flag: "\uD83C\uDDEC\uD83C\uDDE7" },
  { code: "ja", label: "Japanese", flag: "\uD83C\uDDEF\uD83C\uDDF5" },
  { code: "zh", label: "Chinese", flag: "\uD83C\uDDE8\uD83C\uDDF3" },
] as const;

/**
 * Platform -> default language mapping.
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

export const PLATFORMS: Platform[] = [
  // Global
  { id: "youtube", label: "YouTube", region: "Global", auto: true, defaultRtmp: "", help: "Auto-creates broadcasts via API. Connect your account above.", settingsUrl: "", keyOnly: false },
  { id: "instagram", label: "Instagram", region: "Global", auto: false, defaultRtmp: "rtmps://live-upload.instagram.com:443/rtmp/", help: "Open Instagram app \u2192 tap + \u2192 Live \u2192 tap \u2699\uFE0F \u2192 \u2018Stream with external device\u2019 \u2192 copy Stream Key.", settingsUrl: "", keyOnly: true },
  { id: "tiktok", label: "TikTok", region: "Global", auto: false, defaultRtmp: "", help: "Download TikTok LIVE Studio desktop app \u2192 Go Live \u2192 copy Server URL & Stream Key. Requires 1,000+ followers.", settingsUrl: "", keyOnly: false },
  { id: "twitch", label: "Twitch", region: "Global", auto: false, defaultRtmp: "rtmp://live.twitch.tv/app/", help: "Twitch.tv \u2192 Creator Dashboard \u2192 Settings \u2192 Stream \u2192 copy Primary Stream Key.", settingsUrl: "https://dashboard.twitch.tv/settings/stream", keyOnly: true },
  // Korea
  { id: "coupang", label: "Coupang Live", region: "Korea", auto: false, defaultRtmp: "", help: "\uCFE0\uD321 Wing \u2192 Live & Shorts \u2192 Self live \u2192 \uB77C\uC774\uBE0C \uB9CC\uB4E4\uAE30 \u2192 OBS \uC124\uC815 \u2192 copy RTMP URL & \uC2A4\uD2B8\uB9BC \uD0A4.", settingsUrl: "", keyOnly: false },
  { id: "naver", label: "Naver Shopping Live", region: "Korea", auto: false, defaultRtmp: "", help: "\uB124\uC774\uBC84 \uC2A4\uB9C8\uD2B8\uC2A4\uD1A0\uC5B4\uC13C\uD130 \u2192 \uC1FC\uD551\uB77C\uC774\uBE0C \u2192 \uB77C\uC774\uBE0C \uC608\uC57D/\uC2DC\uC791 \u2192 \uC678\uBD80 \uC1A1\uCD9C \uC124\uC815 \u2192 copy RTMP URL & \uC2A4\uD2B8\uB9BC \uD0A4.", settingsUrl: "https://sell.smartstore.naver.com/", keyOnly: false },
  // Japan
  { id: "rakuten", label: "Rakuten Live", region: "Japan", auto: false, defaultRtmp: "", help: "Rakuten RMS \u2192 \u30E9\u30A4\u30D6\u30B3\u30DE\u30FC\u30B9 \u2192 \u914D\u4FE1\u8A2D\u5B9A \u2192 copy RTMP URL & \u30B9\u30C8\u30EA\u30FC\u30E0\u30AD\u30FC.", settingsUrl: "https://rms.rakuten.co.jp/", keyOnly: false },
  // China
  { id: "douyin", label: "Douyin (\u6296\u97F3)", region: "China", auto: false, defaultRtmp: "", help: "Download \u6296\u97F3\u76F4\u64AD\u4F34\u4FA3 desktop app \u2192 login \u2192 \u5F00\u59CB\u76F4\u64AD \u2192 \u63A8\u6D41\u5730\u5740 will appear. Copy \u670D\u52A1\u5668\u5730\u5740 & \u63A8\u6D41\u7801.", settingsUrl: "", keyOnly: false },
  { id: "taobao", label: "Taobao Live (\u6DD8\u5B9D\u76F4\u64AD)", region: "China", auto: false, defaultRtmp: "", help: "\u6DD8\u5B9D\u76F4\u64AD\u4E2D\u63A7\u53F0 \u2192 \u521B\u5EFA\u76F4\u64AD \u2192 OBS\u63A8\u6D41 \u2192 copy RTMP URL & \u63A8\u6D41\u7801.", settingsUrl: "https://liveplatform.taobao.com/live/liveList.htm", keyOnly: false },
  { id: "kuaishou", label: "Kuaishou (\u5FEB\u624B)", region: "China", auto: false, defaultRtmp: "rtmp://live.kuaishou.com/live/", help: "\u5FEB\u624B\u76F4\u64AD\u4F34\u4FA3 desktop app \u2192 login \u2192 \u5F00\u64AD\u8BBE\u7F6E \u2192 copy \u63A8\u6D41\u7801 (Stream Key).", settingsUrl: "", keyOnly: true },
  { id: "xiaohongshu", label: "Xiaohongshu (\u5C0F\u7EA2\u4E66)", region: "China", auto: false, defaultRtmp: "", help: "\u5C0F\u7EA2\u4E66 App \u2192 + \u2192 \u76F4\u64AD \u2192 \u8BBE\u7F6E \u2192 \u7535\u8111\u6A21\u5F0F \u2192 copy \u6388\u6743\u7801 \u2192 \u5C0F\u7EA2\u4E66\u76F4\u64AD\u52A9\u624B desktop app \u2192 paste auth code \u2192 copy \u63A8\u6D41\u5730\u5740 & \u63A8\u6D41\u7801.", settingsUrl: "", keyOnly: false },
  { id: "bilibili", label: "Bilibili (\u54D4\u54E9\u54D4\u54E9)", region: "China", auto: false, defaultRtmp: "rtmp://live-push.bilivideo.com/live-bvc/", help: "Bilibili \u2192 \u76F4\u64AD\u4E2D\u5FC3 \u2192 \u6211\u7684\u76F4\u64AD\u95F4 \u2192 \u5F00\u59CB\u76F4\u64AD \u2192 copy \u63A8\u6D41\u7801 (Stream Key).", settingsUrl: "https://link.bilibili.com/p/center/index#/my-room/start-live", keyOnly: true },
  // Custom / Testing
  { id: "custom", label: "Custom RTMP", region: "Other", auto: false, defaultRtmp: "", help: "Enter any RTMP/RTMPS endpoint URL and stream key.", settingsUrl: "", keyOnly: false },
  { id: "local-test", label: "Local Test (MediaMTX)", region: "Other", auto: true, defaultRtmp: "rtmp://rtmp:1935/live/", help: "Auto-creates one stream per language on local MediaMTX. View with: ffplay rtmp://localhost:1935/live/{lang}", settingsUrl: "", keyOnly: false },
];

export const REGION_GROUPS = [
  { key: "Global", label: "Global", icon: "\uD83C\uDF10" },
  { key: "Korea", label: "Korean Platforms", icon: "\uD83C\uDDF0\uD83C\uDDF7" },
  { key: "Japan", label: "Japanese Platforms", icon: "\uD83C\uDDEF\uD83C\uDDF5" },
  { key: "China", label: "Chinese Platforms", icon: "\uD83C\uDDE8\uD83C\uDDF3" },
  { key: "Other", label: "Other", icon: "\u2699\uFE0F" },
] as const;
