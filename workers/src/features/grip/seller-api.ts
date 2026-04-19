// Grip Cloud — Seller API wrapper.
//
// Grip issues `AccessKey` + `SecretKey` from the Seller Center
// (https://seller.grip.show/) but publishes ZERO developer-facing REST
// documentation — docs.gripcloud.show, gripcloud.show, and the FAQ are
// all operator/admin-panel guides only. Reviewed 2026-04-19.
//
// !!! STATUS: BLOCKED on Grip support. The real REST endpoint + auth
// scheme (suspected HMAC-SHA256 over a canonical request) is private.
// Email request sent to cloud.bd@gripcorp.co / cloud_csm@gripcorp.co;
// rewire the internals of `provisionBroadcast` once the spec lands.
//
// Until then the production path is paste-creds: the user copies
// stream key + URL from Grip admin (issued 1h before scheduled start)
// and saves via POST /auth/grip. Orchestration never calls this file
// unless `product_id` is supplied, which the FE does not send today.
//
// When Grip replies:
//   1. Replace the GRIP_API_BASE + endpoint path below.
//   2. Replace the plain `X-Access-Key` / `X-Secret-Key` headers with
//      the documented signed `Authorization` header (SubtleCrypto
//      HMAC-SHA256, available on Workers without extra deps).
//   3. Adjust response parsing to the real payload shape.
//
// The wrapper keeps the same surface as the YouTube wrapper so the
// orchestration layer treats Grip as "just another auto-fill platform"
// the moment the internals are real.
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
 * TODO(grip-api, blocked-on-grip-support):
 *   Grip has no public REST docs — the Seller API spec is gated behind
 *   cloud.bd@gripcorp.co / cloud_csm@gripcorp.co. Awaiting their reply
 *   with endpoint paths, auth (likely HMAC-SHA256 signed Authorization
 *   header), and response shape. Until then this function is a stub:
 *   the guessed `POST /v1/broadcasts` + plain-header auth below will
 *   404 or 401 in prod. Orchestration therefore falls back to the
 *   paste-creds path, which is the current production flow.
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
