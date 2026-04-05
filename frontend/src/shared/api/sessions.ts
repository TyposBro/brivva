import { request } from "./client";
import type { Session, StreamInfo, CreateSessionResponse, PlatformConfig } from "./types";

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

export function listSessions(userId: string): Promise<{ sessions: Session[] }> {
  return request(`/api/sessions?user_id=${encodeURIComponent(userId)}`);
}

export function getSession(id: string): Promise<{ session: Session | null; streams: StreamInfo[] }> {
  return request(`/api/sessions/${encodeURIComponent(id)}`);
}

export function deleteSession(id: string): Promise<{ status: string }> {
  return request(`/api/sessions/${encodeURIComponent(id)}`, { method: "DELETE" });
}

export function addStream(
  sessionId: string,
  body: { lang: string; platform: string; rtmp_url: string; stream_key: string },
): Promise<StreamInfo> {
  return request(`/api/sessions/${encodeURIComponent(sessionId)}/streams`, {
    method: "POST",
    body: JSON.stringify(body),
  });
}

export function removeStream(sessionId: string, streamId: string): Promise<{ status: string }> {
  return request(
    `/api/sessions/${encodeURIComponent(sessionId)}/streams/${encodeURIComponent(streamId)}`,
    { method: "DELETE" },
  );
}
