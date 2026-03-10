import { useRef, useEffect } from "react";
import type { RecorderState } from "../hooks/useAudioRecorder";

interface AudioRecorderProps {
  state: RecorderState;
  analyser: AnalyserNode | null;
  onStart: () => void;
  onStop: () => void;
}

export function AudioRecorder({ state, analyser, onStart, onStop }: AudioRecorderProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const animRef = useRef<number>(0);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d")!;

    if (!analyser) {
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      return;
    }

    const bufferLength = analyser.frequencyBinCount;
    const dataArray = new Uint8Array(bufferLength);

    const draw = () => {
      animRef.current = requestAnimationFrame(draw);
      analyser.getByteFrequencyData(dataArray);

      ctx.fillStyle = "#111118";
      ctx.fillRect(0, 0, canvas.width, canvas.height);

      const barWidth = (canvas.width / bufferLength) * 2.5;
      let x = 0;
      for (let i = 0; i < bufferLength; i++) {
        const barHeight = (dataArray[i] / 255) * canvas.height * 0.9;
        const hue = 240 + (i / bufferLength) * 60; // blue → purple
        ctx.fillStyle = `hsl(${hue}, 80%, 60%)`;
        ctx.fillRect(x, canvas.height - barHeight, barWidth, barHeight);
        x += barWidth + 1;
      }
    };

    draw();
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
        {isProcessing ? (
          <span className="spinner" />
        ) : isRecording ? (
          <>
            <span className="dot" /> Stop
          </>
        ) : (
          <>
            <span className="mic-icon">🎙</span> Record
          </>
        )}
      </button>
    </div>
  );
}
