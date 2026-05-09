import { describe, expect, it } from "vitest";
import { BTV1_HEADER_BYTES, encodeBtv1Frame } from "./webcodecs-frame";

describe("encodeBtv1Frame", () => {
  it("writes the BTV1 binary header in little-endian order", () => {
    const payload = new Uint8Array([9, 8, 7]);
    const frame = encodeBtv1Frame(
      {
        codec: "vp8",
        keyFrame: true,
        sequence: 42,
        captureTimeUs: 123_456,
        durationUs: 33_333,
        clientSentTimeUs: 124_000,
        width: 720,
        height: 1280,
      },
      payload,
    );
    const bytes = new Uint8Array(frame);
    const view = new DataView(frame);

    expect(String.fromCharCode(...bytes.slice(0, 4))).toBe("BTV1");
    expect(view.getUint8(4)).toBe(1);
    expect(view.getUint8(5)).toBe(1);
    expect(view.getUint8(6)).toBe(1);
    expect(view.getBigUint64(8, true)).toBe(42n);
    expect(view.getBigUint64(16, true)).toBe(123_456n);
    expect(view.getBigUint64(24, true)).toBe(33_333n);
    expect(view.getBigUint64(32, true)).toBe(124_000n);
    expect(view.getUint32(40, true)).toBe(720);
    expect(view.getUint32(44, true)).toBe(1280);
    expect(view.getUint32(48, true)).toBe(payload.byteLength);
    expect([...bytes.slice(BTV1_HEADER_BYTES)]).toEqual([9, 8, 7]);
  });
});
