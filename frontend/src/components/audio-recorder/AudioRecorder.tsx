import { useRef, useEffect } from "react";
import { Mic, Square } from "lucide-react";
import { cn } from "../../lib/cn";
import { CANVAS_WIDTH, CANVAS_HEIGHT } from "./constants";
import { clearCanvas, startDrawLoop } from "./waveform";

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
        width={CANVAS_WIDTH}
        height={CANVAS_HEIGHT}
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
