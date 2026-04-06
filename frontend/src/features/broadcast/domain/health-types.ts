export type StreamStatus = "green" | "yellow" | "red";

export type LanguageHealth = {
  lang: string;
  queueDepth: number;
  driftMs: number;
  status: StreamStatus;
};

export type PipelineHealthSnapshot = {
  sttConnected: boolean;
  languages: LanguageHealth[];
  ttsTimeouts: number;
  translateErrors: number;
  droppedChunks: number;
  e2eLatencyMs: number;
  receivedAt: number;
};

export function classifyStreamStatus(
  driftMs: number,
  queueDepth: number,
): StreamStatus {
  if (driftMs > 2000 || queueDepth > 7) return "red";
  if (driftMs > 500 || queueDepth > 3) return "yellow";
  return "green";
}

const LATENCY_WINDOW_SIZE = 12;

export function computeRollingAverage(
  samples: number[],
  newValue: number,
): { samples: number[]; average: number } {
  const next = [...samples, newValue];
  if (next.length > LATENCY_WINDOW_SIZE) next.shift();
  const average = next.reduce((a, b) => a + b, 0) / next.length;
  return { samples: next, average };
}
