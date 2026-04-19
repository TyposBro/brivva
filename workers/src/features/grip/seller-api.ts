// Grip Cloud — Seller API wrapper.
//
// Grip's official Seller API was discovered 2026-04-19 and supersedes the
// reverse-engineered paste-the-stream-key flow documented in
// `docs/grip-integration-notes.md`. Sellers authenticate with an
// `AccessKey` + `SecretKey` pair issued from the Grip Seller Center
// (https://seller.grip.show/), not an OAuth2 flow.
//
// Docs: https://docs.gripcloud.show/ (pinned 2026-04-19)
//
// !!! STATUS: scaffolded, exact endpoint path NOT YET VERIFIED against
// the live Grip Seller API. See the TODO inside `provisionBroadcast`.
// The wrapper keeps the same surface as the YouTube wrapper so the
// orchestration layer treats Grip as "just another auto-fill platform";
// when the endpoint/response shape is pinned down we only change the
// internals of this file.
//
// Caller contract:
//   provisionBroadcast({ accessKey, secretKey, productId }) ->
//     { rtmpUrl, streamKey, broadcastId }
//
// Failure mode:
//   Throws `GripSellerApiError` with the HTTP status code. Orchestration
//   layer catches it, falls back to manual paste-credentials path (Task B),
//   and surfaces the error to the FE.
//
// Auth note:
//   Grip's scheme is `AccessKey <key>, Signature <sig>` with HMAC-SHA256
//   over a canonical request string. Since the exact signature payload
//   isn't documented in-repo yet, we send the keys as plain headers for
//   now and leave the HMAC TODO below. Workers' Web Crypto API makes the
//   HMAC step a ~10 LOC addition once the signing string is pinned.

import type { Env } from "../../core/types";

/** Shape returned to the caller on successful broadcast provision. */
export type GripBroadcastResult = {
  /** RTMPS ingest URL — includes the `:443` port Grip mandates. */
  rtmpUrl: string;
  /** Per-session stream key issued by Grip. Short TTL — do not cache. */
  streamKey: string;
  /** Grip-side broadcast / session id, echoed back for logging. */
  broadcastId: string;
};

export type ProvisionBroadcastArgs = {
  accessKey: string;
  secretKey: string;
  /** Grip product id the seller wants to go live against. */
  productId: string;
  /** Optional seller-facing title. Grip defaults this when omitted. */
  title?: string;
};

// Grip API base. The seller docs reference `api.grip.show` but the
// tenant-specific tenant may differ — override via env if needed.
const GRIP_API_BASE = "https://api.grip.show";

export class GripSellerApiError extends Error {
  readonly status: number;
  constructor(message: string, status: number) {
    super(message);
    this.name = "GripSellerApiError";
    this.status = status;
  }
}

/**
 * Provision a Grip live broadcast and return fresh RTMP credentials.
 *
 * TODO(grip-api, 2026-04-19):
 *   Confirm the exact endpoint path + request/response shape against
 *   https://docs.gripcloud.show/. The best-guess endpoint is a POST to
 *   something like `/v1/broadcasts` returning
 *   `{ id, ingest: { url, stream_key } }` — adjust the `fetch` call +
 *   response parsing below once the live docs are open.
 *
 *   Also replace the plain `X-Access-Key` / `X-Secret-Key` headers with
 *   the documented `AccessKey` + signed `Signature` header once the
 *   canonical-request format is confirmed. Use SubtleCrypto.HMAC-SHA256
 *   — available out of the box on Cloudflare Workers, no extra deps.
 */
export async function provisionBroadcast(
  _env: Env,
  args: ProvisionBroadcastArgs,
): Promise<GripBroadcastResult> {
  if (!args.accessKey || !args.secretKey) {
    throw new GripSellerApiError("missing Grip AccessKey or SecretKey", 401);
  }

  // TODO(grip-api): swap for the real endpoint path from docs.gripcloud.show.
  const endpoint = `${GRIP_API_BASE}/v1/broadcasts`;

  const resp = await fetch(endpoint, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      // TODO(grip-api): replace with signed Authorization header.
      "X-Access-Key": args.accessKey,
      "X-Secret-Key": args.secretKey,
    },
    body: JSON.stringify({
      product_id: args.productId,
      title: args.title,
    }),
  });

  if (!resp.ok) {
    const text = await resp.text();
    throw new GripSellerApiError(
      `Grip Seller API ${endpoint} failed: ${resp.status} ${text}`,
      resp.status,
    );
  }

  // TODO(grip-api): adjust to the real response shape.
  const data = (await resp.json()) as {
    id?: string;
    broadcast_id?: string;
    ingest?: { url?: string; stream_key?: string };
    rtmp_url?: string;
    stream_key?: string;
  };

  const rtmpUrl = data.ingest?.url ?? data.rtmp_url;
  const streamKey = data.ingest?.stream_key ?? data.stream_key;
  const broadcastId = data.id ?? data.broadcast_id;

  if (!rtmpUrl || !streamKey || !broadcastId) {
    throw new GripSellerApiError(
      `Grip Seller API returned incomplete payload: ${JSON.stringify(data)}`,
      500,
    );
  }

  return { rtmpUrl, streamKey, broadcastId };
}
