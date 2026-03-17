type VideoEntry = {
  utteranceId: number;
  frames: string[]; // base64 JPEG strings
  fps: number;
  done: boolean;
};

export class VideoPlayer {
  private queue: VideoEntry[] = [];
  private receiving: VideoEntry | null = null;
  private playing = false;
  private canvas: HTMLCanvasElement | null = null;
  private animationId: number | null = null;

  attach(canvas: HTMLCanvasElement): void {
    this.canvas = canvas;
  }

  startReceiving(utteranceId: number): void {
    const entry: VideoEntry = { utteranceId, frames: [], fps: 25, done: false };
    this.receiving = entry;
    this.queue.push(entry);
  }

  addFrame(utteranceId: number, frameBase64: string): void {
    if (this.receiving?.utteranceId === utteranceId) {
      this.receiving.frames.push(frameBase64);
    }
  }

  finishReceiving(utteranceId: number): void {
    if (this.receiving?.utteranceId === utteranceId) {
      this.receiving.done = true;
      this.receiving = null;
      this.tryPlayNext();
    }
  }

  reset(): void {
    if (this.animationId !== null) {
      cancelAnimationFrame(this.animationId);
      this.animationId = null;
    }
    this.playing = false;
    this.queue = [];
    this.receiving = null;
  }

  private tryPlayNext(): void {
    if (this.playing || !this.queue.length || !this.queue[0].done) return;
    this.play(this.queue[0]);
  }

  private play(entry: VideoEntry): void {
    if (!this.canvas || entry.frames.length === 0) {
      this.queue.shift();
      this.tryPlayNext();
      return;
    }

    this.playing = true;
    const ctx = this.canvas.getContext("2d");
    if (!ctx) {
      this.advance();
      return;
    }

    const intervalMs = 1000 / entry.fps;
    let frameIndex = 0;
    let lastTime = performance.now();

    const renderFrame = (now: number) => {
      if (frameIndex >= entry.frames.length) {
        this.advance();
        return;
      }

      const elapsed = now - lastTime;
      if (elapsed >= intervalMs) {
        lastTime = now - (elapsed % intervalMs);
        const img = new Image();
        img.onload = () => {
          this.canvas!.width = img.width;
          this.canvas!.height = img.height;
          ctx.drawImage(img, 0, 0);
        };
        img.src = `data:image/jpeg;base64,${entry.frames[frameIndex]}`;
        frameIndex++;
      }

      this.animationId = requestAnimationFrame(renderFrame);
    };

    this.animationId = requestAnimationFrame(renderFrame);
  }

  private advance(): void {
    if (this.animationId !== null) {
      cancelAnimationFrame(this.animationId);
      this.animationId = null;
    }
    this.playing = false;
    this.queue.shift();
    this.tryPlayNext();
  }
}
