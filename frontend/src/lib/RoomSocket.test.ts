import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { RoomSocket } from "./RoomSocket";

class FakeWebSocket {
  static OPEN = 1;
  static instances: FakeWebSocket[] = [];
  readyState = 0;
  binaryType = "";
  url: string;
  onopen: (() => void) | null = null;
  onmessage: ((e: MessageEvent) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  sent: unknown[] = [];
  closed = false;

  constructor(url: string) {
    this.url = url;
    FakeWebSocket.instances.push(this);
  }
  open() {
    this.readyState = FakeWebSocket.OPEN;
    this.onopen?.();
  }
  fireMessage(data: string | ArrayBuffer) {
    this.onmessage?.({ data } as MessageEvent);
  }
  fireError() {
    this.onerror?.();
  }
  close() {
    this.closed = true;
    this.readyState = 3;
    this.onclose?.();
  }
  send(data: unknown) {
    this.sent.push(data);
  }
}

describe("RoomSocket", () => {
  let OrigWS: typeof WebSocket;

  beforeEach(() => {
    FakeWebSocket.instances = [];
    OrigWS = globalThis.WebSocket;
    // @ts-expect-error test shim
    globalThis.WebSocket = FakeWebSocket;
    // @ts-expect-error static constant used by isOpen check
    globalThis.WebSocket.OPEN = 1;
  });

  afterEach(() => {
    globalThis.WebSocket = OrigWS;
  });

  it("connect builds URL w/ params", () => {
    const s = new RoomSocket();
    s.connect(
      { sourceLang: "en", token: "t1", sessionId: "s1" },
      { onMessage: () => {}, onClose: () => {} },
    );
    const url = FakeWebSocket.instances[0].url;
    expect(url).toContain("/api/room");
    expect(url).toContain("sourceLang=en");
    expect(url).toContain("token=t1");
    expect(url).toContain("sessionId=s1");
  });

  it("onOpen fires after socket opens (happy)", () => {
    const s = new RoomSocket();
    const onOpen = vi.fn();
    s.connect({}, { onOpen, onMessage: () => {}, onClose: () => {} });
    FakeWebSocket.instances[0].open();
    expect(onOpen).toHaveBeenCalledOnce();
  });

  it("routes JSON strings to onMessage as parsed objects", () => {
    const s = new RoomSocket();
    const onMessage = vi.fn();
    s.connect({}, { onMessage, onClose: () => {} });
    FakeWebSocket.instances[0].fireMessage(JSON.stringify({ type: "final", utteranceId: 1 }));
    expect(onMessage).toHaveBeenCalledWith({ type: "final", utteranceId: 1 });
  });

  it("routes ArrayBuffer to onBinary", () => {
    const s = new RoomSocket();
    const onBinary = vi.fn();
    s.connect({}, { onMessage: () => {}, onBinary, onClose: () => {} });
    const buf = new ArrayBuffer(8);
    FakeWebSocket.instances[0].fireMessage(buf);
    expect(onBinary).toHaveBeenCalledWith(buf);
  });

  it("ignores binary if no onBinary callback (sad)", () => {
    const s = new RoomSocket();
    const onMessage = vi.fn();
    s.connect({}, { onMessage, onClose: () => {} });
    FakeWebSocket.instances[0].fireMessage(new ArrayBuffer(4));
    expect(onMessage).not.toHaveBeenCalled();
  });

  it("sendJson writes stringified when open", () => {
    const s = new RoomSocket();
    s.connect({}, { onMessage: () => {}, onClose: () => {} });
    FakeWebSocket.instances[0].open();
    s.sendJson({ type: "host:end" });
    expect(FakeWebSocket.instances[0].sent).toEqual(['{"type":"host:end"}']);
  });

  it("sendJson drops when not open (sad)", () => {
    const s = new RoomSocket();
    s.connect({}, { onMessage: () => {}, onClose: () => {} });
    s.sendJson({ type: "x" });
    expect(FakeWebSocket.instances[0].sent).toEqual([]);
  });

  it("sendAudio writes buffer when open", () => {
    const s = new RoomSocket();
    s.connect({}, { onMessage: () => {}, onClose: () => {} });
    FakeWebSocket.instances[0].open();
    const buf = new ArrayBuffer(16);
    s.sendAudio(buf);
    expect(FakeWebSocket.instances[0].sent).toEqual([buf]);
  });

  it("isOpen reflects readyState", () => {
    const s = new RoomSocket();
    expect(s.isOpen).toBe(false);
    s.connect({}, { onMessage: () => {}, onClose: () => {} });
    expect(s.isOpen).toBe(false);
    FakeWebSocket.instances[0].open();
    expect(s.isOpen).toBe(true);
  });

  it("double connect closes previous w/o firing onClose", () => {
    const s = new RoomSocket();
    const onClose = vi.fn();
    s.connect({}, { onMessage: () => {}, onClose });
    const first = FakeWebSocket.instances[0];
    s.connect({}, { onMessage: () => {}, onClose });
    expect(first.closed).toBe(true);
    expect(onClose).not.toHaveBeenCalled();
    expect(FakeWebSocket.instances).toHaveLength(2);
  });

  it("onclose fires user callback (sad: server drop)", () => {
    const s = new RoomSocket();
    const onClose = vi.fn();
    s.connect({}, { onMessage: () => {}, onClose });
    FakeWebSocket.instances[0].close();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("onerror triggers close", () => {
    const s = new RoomSocket();
    const onClose = vi.fn();
    s.connect({}, { onMessage: () => {}, onClose });
    FakeWebSocket.instances[0].fireError();
    expect(FakeWebSocket.instances[0].closed).toBe(true);
  });

  it("close() on already-null socket is safe", () => {
    const s = new RoomSocket();
    expect(() => s.close()).not.toThrow();
  });

  it("binaryType set to arraybuffer", () => {
    const s = new RoomSocket();
    s.connect({}, { onMessage: () => {}, onClose: () => {} });
    expect(FakeWebSocket.instances[0].binaryType).toBe("arraybuffer");
  });
});
