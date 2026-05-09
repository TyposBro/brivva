export type Btv1Codec = "vp8" | "h264_annexb";

export type Btv1FrameMetadata = {
  codec: Btv1Codec;
  keyFrame: boolean;
  configFrame?: boolean;
  sequence: number;
  captureTimeUs: number;
  durationUs: number;
  clientSentTimeUs: number;
  width: number;
  height: number;
};

export const BTV1_HEADER_BYTES = 52;
export const BTV1_MAGIC = "BTV1";

const CODEC_IDS: Record<Btv1Codec, number> = {
  vp8: 1,
  h264_annexb: 2,
};

export function encodeBtv1Frame(
  metadata: Btv1FrameMetadata,
  payload: Uint8Array,
): ArrayBuffer {
  const out = new ArrayBuffer(BTV1_HEADER_BYTES + payload.byteLength);
  const bytes = new Uint8Array(out);
  const view = new DataView(out);
  bytes[0] = 0x42; // B
  bytes[1] = 0x54; // T
  bytes[2] = 0x56; // V
  bytes[3] = 0x31; // 1
  view.setUint8(4, 1);
  view.setUint8(5, CODEC_IDS[metadata.codec]);
  view.setUint8(6, (metadata.keyFrame ? 1 : 0) | (metadata.configFrame ? 2 : 0));
  view.setUint8(7, 0);
  view.setBigUint64(8, clampU64(metadata.sequence), true);
  view.setBigUint64(16, clampU64(metadata.captureTimeUs), true);
  view.setBigUint64(24, clampU64(metadata.durationUs), true);
  view.setBigUint64(32, clampU64(metadata.clientSentTimeUs), true);
  view.setUint32(40, clampU32(metadata.width), true);
  view.setUint32(44, clampU32(metadata.height), true);
  view.setUint32(48, payload.byteLength, true);
  bytes.set(payload, BTV1_HEADER_BYTES);
  return out;
}

function clampU64(value: number): bigint {
  if (!Number.isFinite(value) || value <= 0) return 0n;
  return BigInt(Math.min(Math.floor(value), Number.MAX_SAFE_INTEGER));
}

function clampU32(value: number): number {
  if (!Number.isFinite(value) || value <= 0) return 0;
  return Math.min(Math.floor(value), 0xffff_ffff);
}
