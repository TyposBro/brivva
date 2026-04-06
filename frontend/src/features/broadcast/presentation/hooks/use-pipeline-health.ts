import { useState, useCallback, useRef } from "react";
import type { PipelineHealthMsg } from "../../data/dtos";
import type { PipelineHealthSnapshot, LanguageHealth } from "../../domain/health-types";
import { classifyStreamStatus, computeRollingAverage } from "../../domain/health-types";

export function usePipelineHealth() {
  const [health, setHealth] = useState<PipelineHealthSnapshot | null>(null);
  const latencySamplesRef = useRef<number[]>([]);

  const handleHealthMessage = useCallback((msg: PipelineHealthMsg) => {
    const langs = Object.keys(msg.queueDepth);
    const driftValues = Object.values(msg.driftMs);
    const maxDrift = driftValues.length > 0
      ? Math.max(...driftValues)
      : 0;

    const { samples, average } = computeRollingAverage(
      latencySamplesRef.current,
      maxDrift,
    );
    latencySamplesRef.current = samples;

    const languages: LanguageHealth[] = langs.map((lang) => ({
      lang,
      queueDepth: msg.queueDepth[lang] ?? 0,
      driftMs: msg.driftMs[lang] ?? 0,
      status: classifyStreamStatus(
        msg.driftMs[lang] ?? 0,
        msg.queueDepth[lang] ?? 0,
      ),
    }));

    setHealth({
      sttConnected: msg.sttConnected,
      languages,
      ttsTimeouts: msg.ttsTimeouts,
      translateErrors: msg.translateErrors,
      droppedChunks: msg.droppedChunks,
      e2eLatencyMs: Math.round(average),
      receivedAt: Date.now(),
    });
  }, []);

  const resetHealth = useCallback(() => {
    setHealth(null);
    latencySamplesRef.current = [];
  }, []);

  return { health, handleHealthMessage, resetHealth };
}
