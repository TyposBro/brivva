export function displayUrl(url: string): string {
  return url
    .replace("rtmp://rtmp:", "rtmp://localhost:")
    .replace("rtmp://server:", "rtmp://localhost:");
}

export function streamPath(rtmpUrl: string): string {
  return rtmpUrl.replace(/^rtmps?:\/\/[^/]+/, "");
}
