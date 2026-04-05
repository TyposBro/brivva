import { request } from "./client";
import type { PlatformCredential } from "./types";

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
  return request(
    `/api/credentials?user_id=${encodeURIComponent(userId)}&platform=${encodeURIComponent(platform)}`,
    { method: "DELETE" },
  );
}
