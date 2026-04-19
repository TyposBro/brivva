// Stripe webhook signature verification — scaffolding only.
//
// We don't fulfill any billing logic yet; this exists so the Stripe dashboard
// can be pointed at /stripe/webhook ahead of real integration and we won't
// silently 500 on Stripe's ping events. Signature check uses the HMAC-SHA256
// scheme from Stripe's "Checking signatures manually" docs.
//
// When STRIPE_WEBHOOK_SECRET is unset, we skip verification and log as
// unsigned — fine for pre-launch, must be reviewed before turning billing on.

const SIGNATURE_TOLERANCE_SECONDS = 5 * 60;

export type VerifyResult =
  | { ok: true; eventType: string | null }
  | { ok: false; reason: string };

function parseSignatureHeader(header: string): { t: number; v1: string[] } | null {
  let timestamp = 0;
  const sigs: string[] = [];
  for (const part of header.split(",")) {
    const [k, v] = part.split("=", 2);
    if (!k || !v) continue;
    if (k === "t") timestamp = Number(v);
    else if (k === "v1") sigs.push(v);
  }
  if (!timestamp || sigs.length === 0) return null;
  return { t: timestamp, v1: sigs };
}

function hexFromBytes(bytes: Uint8Array): string {
  let out = "";
  for (let i = 0; i < bytes.length; i++) {
    out += bytes[i]!.toString(16).padStart(2, "0");
  }
  return out;
}

// Constant-time comparison. Returns true if any expected signature matches.
function constantTimeEqualsAny(expected: string, candidates: string[]): boolean {
  const e = new TextEncoder().encode(expected);
  let matched = false;
  for (const c of candidates) {
    const ce = new TextEncoder().encode(c);
    if (ce.length !== e.length) continue;
    let diff = 0;
    for (let i = 0; i < e.length; i++) diff |= (e[i]! ^ ce[i]!);
    if (diff === 0) matched = true;
  }
  return matched;
}

export async function verifyStripeSignature(
  payload: string,
  signatureHeader: string | null,
  secret: string,
  nowSeconds: number,
): Promise<VerifyResult> {
  if (!signatureHeader) return { ok: false, reason: "missing signature header" };
  const parsed = parseSignatureHeader(signatureHeader);
  if (!parsed) return { ok: false, reason: "malformed signature header" };
  if (Math.abs(nowSeconds - parsed.t) > SIGNATURE_TOLERANCE_SECONDS) {
    return { ok: false, reason: "signature timestamp outside tolerance" };
  }

  const signedPayload = `${parsed.t}.${payload}`;
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const sigBytes = await crypto.subtle.sign(
    "HMAC",
    key,
    new TextEncoder().encode(signedPayload),
  );
  const expected = hexFromBytes(new Uint8Array(sigBytes));
  if (!constantTimeEqualsAny(expected, parsed.v1)) {
    return { ok: false, reason: "signature mismatch" };
  }

  let eventType: string | null = null;
  try {
    const parsedEvent = JSON.parse(payload) as { type?: string };
    eventType = parsedEvent.type ?? null;
  } catch {
    // A valid signature on an invalid JSON body shouldn't happen, but if it
    // does we still let the caller return 200 — Stripe will retry.
  }
  return { ok: true, eventType };
}
