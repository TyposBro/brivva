// YouTube Live Streaming API v3 wrapper.
//
// Keeps the three-call broadcast-creation choreography (insert broadcast,
// insert stream, bind them together) in a single file so the orchestration
// layer stays a one-liner. The OAuth client in ./google-oauth-client.ts is
// intentionally untouched — this file only consumes the access_token it
// produces + refreshes.
//
// Docs (pinned 2025-11):
//   https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/insert
//   https://developers.google.com/youtube/v3/live/docs/liveStreams/insert
//   https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/bind
//
// Scope already granted on the OAuth flow: `youtube` + `youtube.readonly`.

import type { Env } from "../../core/types";
import { refreshAccessToken } from "./google-oauth-client";

const YT_BASE = "https://www.googleapis.com/youtube/v3";

/** Shape returned to the caller after a successful broadcast-create. */
export type YouTubeBroadcastResult = {
  /** YouTube broadcast id. Becomes the watch URL video id. */
  broadcastId: string;
  /** YouTube stream id (the CDN-side object bound to the broadcast). */
  streamId: string;
  /** Primary RTMP ingest URL (from liveStreams.cdn.ingestionInfo.ingestionAddress). */
  rtmpUrl: string;
  /** Stream key (from liveStreams.cdn.ingestionInfo.streamName). */
  streamKey: string;
  /** Canonical watch URL the host can share before they start pushing audio. */
  watchUrl: string;
};

export type CreateBroadcastArgs = {
  accessToken: string;
  /** Shown on YouTube's public page + the mobile app. */
  title: string;
  /** ISO-8601 timestamp. YouTube rejects past times more than a few seconds back. */
  scheduledStartTime: string;
  /** "public" | "unlisted" | "private". Caller-supplied — we never guess. */
  privacyStatus: string;
};

type InsertBroadcastResponse = {
  id: string;
  snippet?: { title?: string };
  status?: { lifeCycleStatus?: string };
};

type InsertStreamResponse = {
  id: string;
  status?: {
    streamStatus?: string;
    healthStatus?: {
      status?: string;
      lastUpdateTimeSeconds?: string;
      configurationIssues?: Array<{
        type?: string;
        severity?: string;
        reason?: string;
        description?: string;
      }>;
    };
  };
  cdn?: {
    ingestionInfo?: {
      ingestionAddress?: string;
      rtmpsIngestionAddress?: string;
      streamName?: string;
      backupIngestionAddress?: string;
      rtmpsBackupIngestionAddress?: string;
    };
  };
};

/**
 * Create a broadcast + stream + bind, in that order.
 *
 * Auth refresh is caller's responsibility for the FIRST call. We DO retry
 * internally on a single 401 by calling `refreshAccessToken` + updating D1
 * via the supplied `onTokenRefresh` callback. That callback is optional —
 * tests pass `undefined` and get a fast 401 instead.
 */
export async function createYouTubeBroadcast(
  env: Env,
  args: CreateBroadcastArgs,
  opts: {
    refreshToken?: string | null;
    onTokenRefresh?: (newAccessToken: string, expiresAt: number) => Promise<void>;
  } = {},
): Promise<YouTubeBroadcastResult> {
  let accessToken = args.accessToken;

  const call = makeYouTubePostCaller(env, accessToken, opts, (token) => {
    accessToken = token;
  });

  // 1) liveBroadcasts.insert — the viewer-facing container.
  const broadcast = await call<InsertBroadcastResponse>(
    "/liveBroadcasts?part=snippet,status,contentDetails",
    {
      snippet: {
        title: args.title,
        scheduledStartTime: args.scheduledStartTime,
      },
      status: {
        privacyStatus: args.privacyStatus,
        selfDeclaredMadeForKids: false,
      },
      contentDetails: {
        // Enable monitor stream so the creator can preview before going live.
        // broadcastStreamDelayMs=0 since we already apply delay in Fargate.
        enableAutoStart: true,
        // Keep YouTube from finalizing the VOD on a transient RTMP disconnect.
        // Fargate has its own restart loop; Workers explicitly completes the
        // broadcast when the host ends the Brivva session.
        enableAutoStop: false,
      },
    },
  );

  // 2) liveStreams.insert — the CDN-side ingest + key.
  // `format=variable` lets YouTube auto-negotiate bitrate (up to 1080p60).
  const stream = await call<InsertStreamResponse>(
    "/liveStreams?part=snippet,cdn,contentDetails",
    {
      snippet: { title: `${args.title} — ingest` },
      cdn: {
        frameRate: "variable",
        ingestionType: "rtmp",
        resolution: "variable",
      },
      contentDetails: { isReusable: false },
    },
  );

  const rtmpUrl =
    stream.cdn?.ingestionInfo?.rtmpsIngestionAddress ??
    stream.cdn?.ingestionInfo?.ingestionAddress;
  const streamKey = stream.cdn?.ingestionInfo?.streamName;
  if (!rtmpUrl || !streamKey) {
    throw new YouTubeBroadcastError(
      `YouTube liveStreams.insert returned no ingestion info`,
      500,
    );
  }

  // 3) liveBroadcasts.bind — glue them together.
  await call<InsertBroadcastResponse>(
    `/liveBroadcasts/bind?part=id,contentDetails&id=${encodeURIComponent(broadcast.id)}&streamId=${encodeURIComponent(stream.id)}`,
    {},
  );

  return {
    broadcastId: broadcast.id,
    streamId: stream.id,
    rtmpUrl,
    streamKey,
    watchUrl: `https://www.youtube.com/watch?v=${broadcast.id}`,
  };
}

export type YouTubeStreamHealth = {
  streamId: string;
  streamStatus: string | null;
  healthStatus: string | null;
  configurationIssues: Array<{
    type?: string;
    severity?: string;
    reason?: string;
    description?: string;
  }>;
  providerConfirmedLive: boolean;
};

export async function getYouTubeStreamHealth(
  env: Env,
  args: { accessToken: string; streamId: string },
  opts: {
    refreshToken?: string | null;
    onTokenRefresh?: (newAccessToken: string, expiresAt: number) => Promise<void>;
  } = {},
): Promise<YouTubeStreamHealth> {
  let accessToken = args.accessToken;
  const call = makeYouTubeGetCaller(env, accessToken, opts, (token) => {
    accessToken = token;
  });
  const response = await call<{ items?: InsertStreamResponse[] }>(
    `/liveStreams?part=id,status&id=${encodeURIComponent(args.streamId)}`,
  );
  const item = response.items?.[0];
  const streamStatus = item?.status?.streamStatus ?? null;
  const healthStatus = item?.status?.healthStatus?.status ?? null;
  return {
    streamId: args.streamId,
    streamStatus,
    healthStatus,
    configurationIssues: item?.status?.healthStatus?.configurationIssues ?? [],
    providerConfirmedLive: streamStatus === "active",
  };
}

export async function completeYouTubeBroadcast(
  env: Env,
  args: { accessToken: string; broadcastId: string },
  opts: {
    refreshToken?: string | null;
    onTokenRefresh?: (newAccessToken: string, expiresAt: number) => Promise<void>;
  } = {},
): Promise<void> {
  let accessToken = args.accessToken;
  const call = makeYouTubePostCaller(env, accessToken, opts, (token) => {
    accessToken = token;
  });
  await call<InsertBroadcastResponse>(
    `/liveBroadcasts/transition?broadcastStatus=complete&id=${encodeURIComponent(args.broadcastId)}&part=status`,
    {},
  );
}

function makeYouTubePostCaller(
  env: Env,
  initialAccessToken: string,
  opts: {
    refreshToken?: string | null;
    onTokenRefresh?: (newAccessToken: string, expiresAt: number) => Promise<void>;
  },
  onAccessToken: (token: string) => void,
) {
  return makeYouTubeCaller(env, initialAccessToken, opts, onAccessToken, "POST");
}

function makeYouTubeGetCaller(
  env: Env,
  initialAccessToken: string,
  opts: {
    refreshToken?: string | null;
    onTokenRefresh?: (newAccessToken: string, expiresAt: number) => Promise<void>;
  },
  onAccessToken: (token: string) => void,
) {
  return makeYouTubeCaller(env, initialAccessToken, opts, onAccessToken, "GET");
}

function makeYouTubeCaller(
  env: Env,
  initialAccessToken: string,
  opts: {
    refreshToken?: string | null;
    onTokenRefresh?: (newAccessToken: string, expiresAt: number) => Promise<void>;
  },
  onAccessToken: (token: string) => void,
  method: "GET" | "POST",
) {
  let accessToken = initialAccessToken;
  return async <T>(path: string, body?: unknown): Promise<T> => {
    const run = async (token: string): Promise<Response> =>
      await fetch(`${YT_BASE}${path}`, {
        method,
        headers: {
          Authorization: `Bearer ${token}`,
          ...(method === "POST" ? { "Content-Type": "application/json" } : {}),
        },
        ...(method === "POST" ? { body: JSON.stringify(body ?? {}) } : {}),
      });

    let resp = await run(accessToken);
    // One-shot refresh on 401. A 403 (quota, permission, invalid transition)
    // is NOT transient — surface it as a YouTubeBroadcastError.
    if (resp.status === 401 && opts.refreshToken && opts.onTokenRefresh) {
      const refreshed = await refreshAccessToken(env, opts.refreshToken);
      accessToken = refreshed.access_token;
      onAccessToken(accessToken);
      const expiresAt = Math.floor(Date.now() / 1000) + refreshed.expires_in;
      await opts.onTokenRefresh(refreshed.access_token, expiresAt);
      resp = await run(accessToken);
    }
    if (!resp.ok) {
      const text = await resp.text();
      throw new YouTubeBroadcastError(
        `YouTube ${path} failed: ${resp.status} ${text}`,
        resp.status,
      );
    }
    return (await resp.json()) as T;
  };
}

export class YouTubeBroadcastError extends Error {
  readonly status: number;
  constructor(message: string, status: number) {
    super(message);
    this.name = "YouTubeBroadcastError";
    this.status = status;
  }
}
