const WS_BASE = (import.meta.env.VITE_WORKER_URL ?? "http://localhost:8787").replace(/^http/, "ws");

export type RoomMessage = { type: string; [k: string]: unknown };

export type RoomSocketCallbacks = {
  onMessage: (msg: RoomMessage) => void;
  onBinary?: (data: ArrayBuffer) => void;
  onClose: () => void;
};

export class RoomSocket {
  private ws: WebSocket | null = null;

  connect(params: Record<string, string>, { onMessage, onBinary, onClose }: RoomSocketCallbacks): void {
    const url = new URL(`${WS_BASE}/api/room`);
    for (const [key, value] of Object.entries(params)) url.searchParams.set(key, value);

    this.ws = new WebSocket(url.toString());
    this.ws.binaryType = "arraybuffer";
    this.ws.onmessage = (e) => {
      if (typeof e.data === "string") onMessage(JSON.parse(e.data));
      else if (e.data instanceof ArrayBuffer && onBinary) onBinary(e.data);
    };
    this.ws.onclose = onClose;
    this.ws.onerror = () => this.ws?.close();
  }

  sendAudio(buffer: ArrayBuffer): void {
    if (this.isOpen) this.ws!.send(buffer);
  }

  sendJson(msg: object): void {
    if (this.isOpen) this.ws!.send(JSON.stringify(msg));
  }

  get isOpen(): boolean {
    return this.ws?.readyState === WebSocket.OPEN;
  }

  close(): void {
    this.ws?.close();
    this.ws = null;
  }
}
