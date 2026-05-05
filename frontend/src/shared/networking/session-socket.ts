import { appConfig } from "../../core/config/app-config";

export type SessionSocketCallbacks = {
  onOpen?: () => void;
  onMessage: (msg: unknown) => void;
  onBinary?: (data: ArrayBuffer) => void;
  onClose: () => void;
};

export class SessionSocket {
  private ws: WebSocket | null = null;

  connect(
    params: Record<string, string>,
    callbacks: SessionSocketCallbacks
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
    const url = new URL(`${appConfig().mediaWsBase}/api/session`);
    for (const [key, value] of Object.entries(params))
      url.searchParams.set(key, value);
    return url.toString();
  }

  private wireHandlers({
    onOpen,
    onMessage,
    onBinary,
    onClose,
  }: SessionSocketCallbacks): void {
    if (onOpen) this.ws!.onopen = () => onOpen();
    this.ws!.onmessage = (e) => this.routeMessage(e, onMessage, onBinary);
    this.ws!.onclose = onClose;
    this.ws!.onerror = () => this.ws?.close();
  }

  private routeMessage(
    e: MessageEvent,
    onMessage: SessionSocketCallbacks["onMessage"],
    onBinary?: SessionSocketCallbacks["onBinary"]
  ): void {
    if (typeof e.data === "string") {
      try {
        onMessage(JSON.parse(e.data));
      } catch {
        onMessage({
          type: "error",
          message:
            "Server sent a malformed live message. End this run and create a fresh session if it repeats.",
        });
      }
    } else if (e.data instanceof ArrayBuffer && onBinary) onBinary(e.data);
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
