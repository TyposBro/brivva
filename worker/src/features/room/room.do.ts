import type { Bindings } from "../../core/types";
import { Broadcaster } from "./lib/Broadcaster";
import { HostSession } from "./lib/HostSession";
import { GuestManager } from "./lib/GuestManager";

export class RoomDO {
  private host: HostSession;
  private guests: GuestManager;
  private roomId = "";

  constructor(_state: DurableObjectState, env: Bindings) {
    const broadcaster = new Broadcaster();
    this.host = new HostSession(env, broadcaster);
    this.guests = new GuestManager(broadcaster);
  }

  async fetch(request: Request): Promise<Response> {
    if (!this.isWebSocketUpgrade(request)) {
      return new Response("WebSocket upgrade required", { status: 426 });
    }

    const { client, server } = this.createSocketPair();
    const url = new URL(request.url);

    const error = await this.route(server, url);
    if (error) this.reject(server, error);

    return new Response(null, { status: 101, webSocket: client } as ResponseInit);
  }

  // --- internals ---

  private async route(ws: WebSocket, url: URL): Promise<string | null> {
    const params = url.searchParams;
    const role = params.get("role");
    const lang = params.get("lang");
    const sourceLang = params.get("sourceLang") ?? "en";
    this.roomId = params.get("roomId") ?? this.roomId;

    if (role === "host") return this.host.accept(ws, this.roomId, sourceLang);
    if (role === "guest") return this.guests.accept(ws, lang, this.roomId);

    ws.close();
    return null;
  }

  private isWebSocketUpgrade(request: Request): boolean {
    return request.headers.get("Upgrade") === "websocket";
  }

  private createSocketPair(): { client: WebSocket; server: WebSocket } {
    const { 0: client, 1: server } = new WebSocketPair();
    server.accept();
    return { client, server };
  }

  private reject(ws: WebSocket, message: string) {
    ws.send(JSON.stringify({ type: "error", message }));
    ws.close();
  }
}
