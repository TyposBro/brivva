/**
 * Renders live host video frames on a canvas.
 * Receives base64 JPEG frames from the server and draws them immediately.
 */
export class VideoPlayer {
  private canvas: HTMLCanvasElement | null = null;

  attach(canvas: HTMLCanvasElement): void {
    this.canvas = canvas;
  }

  /** Draw a single frame on the canvas (live host video). */
  renderDirect(frameBase64: string): void {
    if (!this.canvas) return;
    const ctx = this.canvas.getContext("2d");
    if (!ctx) return;
    const img = new Image();
    img.onload = () => {
      this.canvas!.width = img.width;
      this.canvas!.height = img.height;
      ctx.drawImage(img, 0, 0);
    };
    img.src = `data:image/jpeg;base64,${frameBase64}`;
  }

  reset(): void {
    this.canvas = null;
  }
}
