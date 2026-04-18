const WS_BASE = (
  import.meta.env.VITE_WORKER_URL ?? "http://localhost:8787"
).replace(/^http/, "ws");

export type RoomMessage = { type: string; [k: string]: unknown };

export type RoomSocketCallbacks = {
  onOpen?: () => void;
  onMessage: (msg: RoomMessage) => void;
  onBinary?: (data: ArrayBuffer) => void;
  onClose: () => void;
};

export class RoomSocket {
  private ws: WebSocket | null = null;

  connect(
    params: Record<string, string>,
    callbacks: RoomSocketCallbacks
  ): void {
    // Close any existing connection to prevent duplicate pipelines
    if (this.ws) {
      this.ws.onclose = null; // prevent onClose callback from firing
      this.ws.close();
      this.ws = null;
    }
    const url = this.buildUrl(params);
    this.ws = new WebSocket(url);
    this.ws.binaryType = "arraybuffer";
    this.wireHandlers(callbacks);
  }

  private buildUrl(params: Record<string, string>): string {
    const url = new URL(`${WS_BASE}/api/session`);
    for (const [key, value] of Object.entries(params))
      url.searchParams.set(key, value);
    return url.toString();
  }

  private wireHandlers({
    onOpen,
    onMessage,
    onBinary,
    onClose,
  }: RoomSocketCallbacks): void {
    if (onOpen) this.ws!.onopen = () => onOpen();
    this.ws!.onmessage = (e) => this.routeMessage(e, onMessage, onBinary);
    this.ws!.onclose = onClose;
    this.ws!.onerror = () => this.ws?.close();
  }

  private routeMessage(
    e: MessageEvent,
    onMessage: RoomSocketCallbacks["onMessage"],
    onBinary?: RoomSocketCallbacks["onBinary"]
  ): void {
    if (typeof e.data === "string") onMessage(JSON.parse(e.data));
    else if (e.data instanceof ArrayBuffer && onBinary) onBinary(e.data);
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
