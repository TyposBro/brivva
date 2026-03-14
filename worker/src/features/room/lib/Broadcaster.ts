export type Lang = "en" | "ja" | "zh";
export const LANGS: Lang[] = ["en", "ja", "zh"];

export class Broadcaster {
  private hostWs: WebSocket | null = null;
  private guests = new Map<string, { ws: WebSocket; lang: Lang }>();
  private langGroups = new Map<Lang, Set<string>>(LANGS.map((l) => [l, new Set()]));

  setHost(ws: WebSocket) {
    this.hostWs = ws;
  }

  clearHost() {
    this.hostWs = null;
  }

  get hasHost(): boolean {
    return this.hostWs !== null;
  }

  addGuest(id: string, ws: WebSocket, lang: Lang) {
    this.guests.set(id, { ws, lang });
    this.langGroups.get(lang)!.add(id);
  }

  removeGuest(id: string) {
    const guest = this.guests.get(id);
    if (!guest) return;
    this.langGroups.get(guest.lang)?.delete(id);
    this.guests.delete(id);
  }

  activeLangs(): Lang[] {
    return LANGS.filter((lang) => (this.langGroups.get(lang)?.size ?? 0) > 0);
  }

  sendToHost(data: string) {
    if (this.hostWs) this.send(this.hostWs, data);
  }

  sendToLang(lang: Lang, data: string | Uint8Array) {
    for (const id of this.langGroups.get(lang) ?? []) {
      const guest = this.guests.get(id);
      if (guest) this.send(guest.ws, data);
    }
  }

  sendToAllGuests(data: string) {
    for (const [, guest] of this.guests) this.send(guest.ws, data);
  }

  sendToEveryone(data: string) {
    this.sendToHost(data);
    this.sendToAllGuests(data);
  }

  pushGuestCount() {
    const counts: Record<string, number> = {};
    for (const [lang, ids] of this.langGroups) counts[lang] = ids.size;
    this.sendToHost(JSON.stringify({ type: "room:guest_count", counts }));
  }

  private send(ws: WebSocket, data: string | Uint8Array) {
    try { ws.send(data); } catch {}
  }
}
