import { Broadcaster, LANGS, type Lang } from "./Broadcaster";

export class GuestManager {
  constructor(private broadcaster: Broadcaster) {}

  accept(ws: WebSocket, lang: string | null, roomId: string): string | null {
    if (!this.broadcaster.hasHost) return "Room not found";
    if (!this.isValidLang(lang)) return "Invalid language";

    const guestId = crypto.randomUUID();
    this.broadcaster.addGuest(guestId, ws, lang);
    ws.send(JSON.stringify({ type: "room:joined", roomId, lang }));
    this.broadcaster.pushGuestCount();

    ws.addEventListener("close", () => this.remove(guestId));
    return null;
  }

  // --- internals ---

  private remove(guestId: string) {
    this.broadcaster.removeGuest(guestId);
    this.broadcaster.pushGuestCount();
  }

  private isValidLang(lang: string | null): lang is Lang {
    return !!lang && LANGS.includes(lang as Lang);
  }
}
