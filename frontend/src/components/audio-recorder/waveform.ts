import {
  CANVAS_BG, BAR_WIDTH_SCALE, BAR_GAP,
  BAR_HEIGHT_SCALE, MAX_BYTE_VALUE,
  HUE_START, HUE_RANGE, SATURATION, LIGHTNESS,
} from "./constants";

export function clearCanvas(canvas: HTMLCanvasElement) {
  const ctx = canvas.getContext("2d")!;
  ctx.fillStyle = CANVAS_BG;
  ctx.fillRect(0, 0, canvas.width, canvas.height);
}

export function startDrawLoop(
  canvas: HTMLCanvasElement,
  analyser: AnalyserNode,
  animRef: React.RefObject<number>
) {
  const ctx = canvas.getContext("2d")!;
  const bufferLength = analyser.frequencyBinCount;
  const dataArray = new Uint8Array(bufferLength);

  const draw = () => {
    animRef.current = requestAnimationFrame(draw);
    analyser.getByteFrequencyData(dataArray);
    drawBars(ctx, canvas, dataArray, bufferLength);
  };

  draw();
}

export function drawBars(
  ctx: CanvasRenderingContext2D,
  canvas: HTMLCanvasElement,
  data: Uint8Array,
  length: number
) {
  ctx.fillStyle = CANVAS_BG;
  ctx.fillRect(0, 0, canvas.width, canvas.height);

  const barWidth = (canvas.width / length) * BAR_WIDTH_SCALE;
  let x = 0;
  for (let i = 0; i < length; i++) {
    const barHeight =
      (data[i] / MAX_BYTE_VALUE) * canvas.height * BAR_HEIGHT_SCALE;
    const hue = HUE_START + (i / length) * HUE_RANGE;
    ctx.fillStyle = `hsl(${hue}, ${SATURATION}%, ${LIGHTNESS}%)`;
    ctx.fillRect(x, canvas.height - barHeight, barWidth, barHeight);
    x += barWidth + BAR_GAP;
  }
}
