import type { Bindings } from "../../../core/types";
import { Broadcaster } from "./Broadcaster";
import { NovaStt } from "./NovaStt";
import { TranslationPipeline } from "./TranslationPipeline";

export class HostSession {
  private nova: NovaStt | null = null;
  private pipeline: TranslationPipeline | null = null;

  constructor(
    private env: Bindings,
    private broadcaster: Broadcaster,
  ) {}

  async accept(ws: WebSocket, roomId: string, sourceLang: string): Promise<string | null> {
    if (this.broadcaster.hasHost) return "Room already has a host";

    try {
      await this.startPipeline(roomId, sourceLang, ws);
      ws.send(JSON.stringify({ type: "room:created", roomId }));
    } catch (err) {
      this.stop();
      return String(err);
    }

    this.wireEvents(ws);
    return null;
  }

  // --- internals ---

  private async startPipeline(roomId: string, sourceLang: string, ws: WebSocket) {
    this.broadcaster.setHost(ws);
    this.pipeline = new TranslationPipeline(this.env, roomId, this.broadcaster, sourceLang);
    this.nova = new NovaStt(this.env, roomId, {
      onInterim: (transcript) => {
        this.broadcaster.sendToEveryone(JSON.stringify({ type: "interim", transcript }));
      },
      onFinal: (transcript, utteranceId) => {
        this.broadcaster.sendToEveryone(JSON.stringify({ type: "final", transcript, utteranceId }));
        this.pipeline!.process(transcript, utteranceId).catch(console.error);
      },
    });

    await this.nova.connect(sourceLang);
  }

  private wireEvents(ws: WebSocket) {
    ws.addEventListener("message", (e) => {
      if (e.data instanceof ArrayBuffer) this.nova?.sendAudio(e.data);
      else this.handleText(e.data as string);
    });
    ws.addEventListener("close", () => this.stop());
  }

  private handleText(data: string) {
    try {
      const msg = JSON.parse(data);
      if (msg.type === "host:end") this.stop();
    } catch {}
  }

  private stop() {
    this.broadcaster.sendToAllGuests(JSON.stringify({ type: "room:closed" }));
    this.nova?.close();
    this.broadcaster.clearHost();
    this.nova = null;
    this.pipeline = null;
  }
}
