type DetectionResult = { platform: string; rtmpUrl: string; streamKey: string };

const KNOWN_PREFIXES: [prefix: string, platform: string, baseUrl: string][] = [
  ["rtmps://live-upload.instagram.com", "instagram", "rtmps://live-upload.instagram.com:443/rtmp/"],
  ["rtmp://live.twitch.tv", "twitch", "rtmp://live.twitch.tv/app/"],
  ["rtmp://live.kuaishou.com", "kuaishou", "rtmp://live.kuaishou.com/live/"],
  ["rtmp://live-push.bilivideo.com", "bilibili", "rtmp://live-push.bilivideo.com/live-bvc/"],
  ["rtmp://a.rtmp.youtube.com", "youtube", "rtmp://a.rtmp.youtube.com/live2/"],
  ["rtmps://a.rtmps.youtube.com", "youtube", "rtmps://a.rtmps.youtube.com/live2/"],
];

export function detectPlatform(input: string): DetectionResult | null {
  const trimmed = input.trim();

  const matched = matchKnownPlatform(trimmed);
  if (matched) return matched;

  return parseGenericRtmp(trimmed);
}

function matchKnownPlatform(url: string): DetectionResult | null {
  for (const [prefix, platform, baseUrl] of KNOWN_PREFIXES) {
    if (!url.startsWith(prefix)) continue;

    const key = url.startsWith(baseUrl)
      ? url.slice(baseUrl.length)
      : url.split("/").pop() ?? "";

    return { platform, rtmpUrl: baseUrl, streamKey: key };
  }
  return null;
}

function parseGenericRtmp(url: string): DetectionResult | null {
  if (!url.startsWith("rtmp://") && !url.startsWith("rtmps://")) return null;

  const lastSlash = url.lastIndexOf("/");
  return {
    platform: "custom",
    rtmpUrl: url.slice(0, lastSlash + 1),
    streamKey: url.slice(lastSlash + 1),
  };
}
