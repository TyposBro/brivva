import { useRef, useEffect } from "react";
import { Mic, Square } from "lucide-react";
import { cn } from "../../../core/cn";

interface AudioRecorderProps {
  isRecording: boolean;
  analyser: AnalyserNode | null;
  onStart: () => void;
  onStop: () => void;
}

export function AudioRecorder({
  isRecording,
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

  return (
    <div className="flex flex-col items-center gap-4">
      <canvas
        ref={canvasRef}
        width={600}
        height={80}
        className={cn(
          "w-full max-w-[600px] h-20 rounded-xl transition-opacity",
          isRecording ? "opacity-100" : "opacity-40"
        )}
      />
      <button
        className={cn(
          "flex items-center gap-2 px-8 py-3 rounded-xl font-headline font-bold transition-all",
          isRecording
            ? "bg-error-container text-on-error-container hover:opacity-90"
            : "monolith-gradient text-white hover:scale-[0.98] shadow-xl"
        )}
        onClick={isRecording ? onStop : onStart}
      >
        {isRecording ? (
          <>
            <Square className="w-4 h-4" /> Stop
          </>
        ) : (
          <>
            <Mic className="w-4 h-4" /> Record
          </>
        )}
      </button>
    </div>
  );
}

// --- waveform rendering ---

const BAR_WIDTH_SCALE = 2.5;
const BAR_GAP = 1;
const BAR_HEIGHT_SCALE = 0.9;
const MAX_BYTE_VALUE = 255;
const HUE_START = 240;
const HUE_RANGE = 60;
const SATURATION = 80;
const LIGHTNESS = 60;

function clearCanvas(canvas: HTMLCanvasElement) {
  const ctx = canvas.getContext("2d")!;
  ctx.fillStyle = "#131313";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
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
    drawBars({ ctx, canvas, data: dataArray, length: bufferLength });
  };

  draw();
}

interface DrawBarsArgs {
  ctx: CanvasRenderingContext2D;
  canvas: HTMLCanvasElement;
  data: Uint8Array;
  length: number;
}

function drawBars({ ctx, canvas, data, length }: DrawBarsArgs) {
  ctx.fillStyle = "#131313";
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
