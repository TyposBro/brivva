import { useRef, useEffect } from "react";

type RecorderState = "idle" | "recording" | "processing";

interface AudioRecorderProps {
  state: RecorderState;
  analyser: AnalyserNode | null;
  onStart: () => void;
  onStop: () => void;
}

export function AudioRecorder({
  state,
  analyser,
  onStart,
  onStop,
}: AudioRecorderProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const animRef = useRef<number>(0);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    if (!analyser) return clearCanvas(canvas);

    startDrawLoop(canvas, analyser, animRef);
    return () => cancelAnimationFrame(animRef.current);
  }, [analyser]);

  const isRecording = state === "recording";
  const isProcessing = state === "processing";

  return (
    <div className="recorder">
      <canvas
        ref={canvasRef}
        width={600}
        height={80}
        className={`waveform ${isRecording ? "active" : ""}`}
      />
      <button
        className={`record-btn ${isRecording ? "recording" : ""}`}
        onClick={isRecording ? onStop : onStart}
        disabled={isProcessing}
      >
        <ButtonLabel state={state} />
      </button>
    </div>
  );
}

// --- waveform rendering ---

const BACKGROUND_COLOR = "#111118";
const BAR_WIDTH_SCALE = 2.5;
const BAR_GAP = 1;
const BAR_HEIGHT_SCALE = 0.9;
const MAX_BYTE_VALUE = 255;
const HUE_START = 240; // blue
const HUE_RANGE = 60; // blue → purple
const SATURATION = 80;
const LIGHTNESS = 60;

function clearCanvas(canvas: HTMLCanvasElement) {
  canvas.getContext("2d")!.clearRect(0, 0, canvas.width, canvas.height);
}

function startDrawLoop(
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

function drawBars(
  ctx: CanvasRenderingContext2D,
  canvas: HTMLCanvasElement,
  data: Uint8Array,
  length: number
) {
  ctx.fillStyle = BACKGROUND_COLOR;
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

// --- button label ---

function ButtonLabel({ state }: { state: RecorderState }) {
  if (state === "processing") return <span className="spinner" />;
  if (state === "recording")
    return (
      <>
        <span className="dot" /> Stop
      </>
    );
  return (
    <>
      <span className="mic-icon">🎙</span> Record
    </>
  );
}
