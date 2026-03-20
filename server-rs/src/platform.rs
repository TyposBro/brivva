//! Platform provider abstraction for automated stream creation.
//!
//! YouTube is already implemented. Twitch, Instagram, etc. will follow.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtmpDetails {
    pub rtmp_url: String,
    pub stream_key: String,
    pub broadcast_id: Option<String>,
    pub stream_id: Option<String>,
}

/// Known RTMP base URLs for platforms where the ingest URL is static.
/// The user only needs to provide a stream key.
pub fn default_rtmp_url(platform: &str) -> Option<&'static str> {
    match platform {
        "instagram" => Some("rtmps://live-upload.instagram.com:443/rtmp/"),
        "twitch" => Some("rtmp://live.twitch.tv/app/"),
        "kuaishou" => Some("rtmp://live.kuaishou.com/live/"),
        "bilibili" => Some("rtmp://live-push.bilivideo.com/live-bvc/"),
        _ => None,
    }
}

/// Try to detect which platform an RTMP URL belongs to.
/// Returns (platform_id, rtmp_base_url, stream_key) if detected.
pub fn detect_platform(input: &str) -> Option<(&'static str, String, String)> {
    let input = input.trim();

    // Check for known RTMP URL patterns
    let patterns: &[(&str, &str, &str)] = &[
        (
            "rtmps://live-upload.instagram.com",
            "instagram",
            "rtmps://live-upload.instagram.com:443/rtmp/",
        ),
        (
            "rtmp://live.twitch.tv",
            "twitch",
            "rtmp://live.twitch.tv/app/",
        ),
        (
            "rtmp://live.kuaishou.com",
            "kuaishou",
            "rtmp://live.kuaishou.com/live/",
        ),
        (
            "rtmp://live-push.bilivideo.com",
            "bilibili",
            "rtmp://live-push.bilivideo.com/live-bvc/",
        ),
        (
            "rtmp://a.rtmp.youtube.com",
            "youtube",
            "rtmp://a.rtmp.youtube.com/live2/",
        ),
        (
            "rtmps://a.rtmps.youtube.com",
            "youtube",
            "rtmps://a.rtmps.youtube.com/live2/",
        ),
    ];

    for &(prefix, platform, base_url) in patterns {
        if input.starts_with(prefix) {
            // Extract the stream key (everything after the base URL)
            let key = input
                .strip_prefix(base_url)
                .or_else(|| {
                    // Try to find the last path segment as the key
                    input.rsplit('/').next()
                })
                .unwrap_or("")
                .to_string();
            return Some((platform, base_url.to_string(), key));
        }
    }

    // If it starts with rtmp:// or rtmps:// but doesn't match known patterns,
    // it's a custom RTMP URL. Try to split into base + key.
    if input.starts_with("rtmp://") || input.starts_with("rtmps://") {
        // Split at the last '/' to separate base URL from stream key
        if let Some(last_slash) = input.rfind('/') {
            let base = &input[..=last_slash];
            let key = &input[last_slash + 1..];
            if !key.is_empty() {
                return Some(("custom", base.to_string(), key.to_string()));
            }
        }
        return Some(("custom", input.to_string(), String::new()));
    }

    None
}

/// Deep link URLs for platform streaming settings pages.
/// Only includes URLs that actually work (require login but land on the right page).
pub fn settings_url(platform: &str) -> Option<&'static str> {
    match platform {
        "twitch" => Some("https://dashboard.twitch.tv/settings/stream"),
        "coupang" => Some("https://wing.coupang.com/vendor/live-commerce/lives"),
        "naver" => Some("https://sell.smartstore.naver.com/"),
        "rakuten" => Some("https://rms.rakuten.co.jp/"),
        "taobao" => Some("https://liveplatform.taobao.com/live/liveList.htm"),
        "bilibili" => Some("https://link.bilibili.com/p/center/index#/my-room/start-live"),
        // instagram, tiktok, douyin, kuaishou, xiaohongshu — app-only or no web deep link
        _ => None,
    }
}
