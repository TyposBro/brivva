type TtsEntry = {
  utteranceId: number;
  chunks: ArrayBuffer[];
  done: boolean;
};

export class TtsPlayer {
  private queue: TtsEntry[] = [];
  private receiving: TtsEntry | null = null;
  private playing = false;
  private audio: HTMLAudioElement | null = null;

  startReceiving(utteranceId: number): void {
    const entry: TtsEntry = { utteranceId, chunks: [], done: false };
    this.receiving = entry;
    this.queue.push(entry);
  }

  addChunk(data: ArrayBuffer): void {
    this.receiving?.chunks.push(data);
  }

  finishReceiving(utteranceId: number): void {
    if (this.receiving?.utteranceId === utteranceId) {
      this.receiving.done = true;
      this.receiving = null;
      this.tryPlayNext();
    }
  }

  reset(): void {
    if (this.audio) {
      this.audio.pause();
      this.audio.src = "";
      this.audio = null;
    }
    this.playing = false;
    this.queue = [];
    this.receiving = null;
  }

  private tryPlayNext(): void {
    if (this.playing || !this.queue.length || !this.queue[0].done) return;
    this.play(this.queue[0]);
  }

  private play(entry: TtsEntry): void {
    this.playing = true;
    const url = this.createBlobUrl(entry.chunks);
    this.audio = new Audio(url);
    this.registerPlaybackHandlers(url);
    this.audio.play().catch((err) => {
      console.error("TTS play() rejected:", err);
      this.advance(url);
    });
  }

  private createBlobUrl(chunks: ArrayBuffer[]): string {
    return URL.createObjectURL(new Blob(chunks, { type: "audio/mpeg" }));
  }

  private registerPlaybackHandlers(url: string): void {
    this.audio!.onended = () => this.advance(url);
    this.audio!.onerror = () => {
      console.error("TTS playback error");
      this.advance(url);
    };
  }

  private advance(url: string): void {
    URL.revokeObjectURL(url);
    this.audio = null;
    this.playing = false;
    this.queue.shift();
    this.tryPlayNext();
  }
}
