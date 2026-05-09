import { useCallback, useRef } from "react";
import { encodeBtv1Frame } from "./webcodecs-frame";
import { detectWebCodecsSupport } from "./media-ingest-mode";

const DEFAULT_WIDTH = 720;
const DEFAULT_HEIGHT = 1280;
const DEFAULT_FPS = 30;
const DEFAULT_BITRATE_BPS = 2_800_000;
const KEYFRAME_INTERVAL_MS = 1000;
const MAX_WS_BUFFERED_BYTES = 4 * 1024 * 1024;
const MAX_QUEUED_VIDEO_MS = 500;

type WebCodecsSignal =
  | {
      type: "video:webcodecs_start";
      mode: "webcodecs_ws";
      codec: "vp8";
      width: number;
      height: number;
      fps: number;
      bitrate_bps: number;
      keyframe_interval_ms: number;
      timebase_us: 1;
    }
  | { type: "video:webcodecs_stop" }
  | { type: "client:media_stats"; stats: { webcodecsVideo: WebCodecsVideoStats } };

export type WebCodecsVideoStats = {
  codec: "vp8";
  capture: { width: number; height: number; fps: number };
  encoded: { width: number; height: number; fps: number };
  encodedFrames: number;
  sentFrames: number;
  droppedFrames: number;
  keyFrames: number;
  queueMs: number;
  wsBufferedBytes: number;
  encoderQueueSize?: number;
  serverAcceptedFrames?: number;
  serverLatestMediaPtsUs?: number;
  serverDrops?: number;
};

export type WebCodecsVideoDrop = {
  reason: "ws_backpressure" | "queue_overflow";
  droppedFrames: number;
  bufferedAmount: number;
  queueMs: number;
};

export type WebCodecsConnectionIssue = {
  layer: "webcodecs";
  message: string;
};

type VideoProfile = {
  width: number;
  height: number;
  fps: number;
};

type EncodedVideoChunkLike = {
  type: "key" | "delta";
  timestamp: number;
  duration?: number | null;
  byteLength: number;
  copyTo: (destination: BufferSource) => void;
};

type VideoFrameLike = {
  timestamp?: number;
  duration?: number | null;
  displayWidth?: number;
  displayHeight?: number;
  codedWidth?: number;
  codedHeight?: number;
  close: () => void;
};

type VideoEncoderLike = {
  encodeQueueSize?: number;
  configure: (config: VideoEncoderConfigLike) => void;
  encode: (frame: VideoFrameLike, options?: { keyFrame?: boolean }) => void;
  close: () => void;
};

type VideoEncoderConfigLike = {
  codec: "vp8";
  width: number;
  height: number;
  framerate: number;
  bitrate: number;
  latencyMode: "realtime";
};

type VideoEncoderCtor = {
  new (init: {
    output: (chunk: EncodedVideoChunkLike) => void;
    error: (error: Error) => void;
  }): VideoEncoderLike;
  isConfigSupported?: (
    config: VideoEncoderConfigLike,
  ) => Promise<{ supported?: boolean }>;
};

type MediaStreamTrackProcessorCtor = new (init: {
  track: MediaStreamTrack;
}) => { readable: ReadableStream<VideoFrameLike> };

type QueuedPacket = {
  payload: Uint8Array;
  keyFrame: boolean;
  sequence: number;
  captureTimeUs: number;
  durationUs: number;
};

function videoEncoderCtor(): VideoEncoderCtor | null {
  return ((globalThis as unknown as { VideoEncoder?: VideoEncoderCtor }).VideoEncoder ?? null);
}

function mediaStreamTrackProcessorCtor(): MediaStreamTrackProcessorCtor | null {
  return (
    (globalThis as unknown as { MediaStreamTrackProcessor?: MediaStreamTrackProcessorCtor })
      .MediaStreamTrackProcessor ?? null
  );
}

export function useWebCodecsVideoIngest(
  sendSignalJson: (msg: WebCodecsSignal) => void,
  sendBinary: (data: ArrayBuffer) => void,
  isSocketOpen: () => boolean,
  getBufferedAmount: () => number,
  onStats?: (stats: WebCodecsVideoStats) => void,
  onDrop?: (drop: WebCodecsVideoDrop) => void,
  onConnectionIssue?: (issue: WebCodecsConnectionIssue) => void,
) {
  const runningRef = useRef(false);
  const readerRef = useRef<ReadableStreamDefaultReader<VideoFrameLike> | null>(null);
  const encoderRef = useRef<VideoEncoderLike | null>(null);
  const queueRef = useRef<QueuedPacket[]>([]);
  const statsIntervalRef = useRef<number | null>(null);
  const pendingStartRef = useRef<{
    resolve: () => void;
    reject: (error: Error) => void;
    timeout: number;
  } | null>(null);
  const forceNextKeyframeRef = useRef(true);
  const lastKeyframeAtUsRef = useRef(0);
  const sequenceRef = useRef(0);
  const statsRef = useRef<WebCodecsVideoStats>(emptyStats());

  const emitStats = useCallback(() => {
    statsRef.current = {
      ...statsRef.current,
      queueMs: queueDurationMs(queueRef.current, statsRef.current.capture.fps),
      wsBufferedBytes: getBufferedAmount(),
      encoderQueueSize: encoderRef.current?.encodeQueueSize,
    };
    onStats?.(statsRef.current);
    if (isSocketOpen()) {
      sendSignalJson({
        type: "client:media_stats",
        stats: { webcodecsVideo: statsRef.current },
      });
    }
  }, [getBufferedAmount, isSocketOpen, onStats, sendSignalJson]);

  const stopFrameStreaming = useCallback(() => {
    const wasActive =
      runningRef.current ||
      pendingStartRef.current !== null ||
      encoderRef.current !== null ||
      readerRef.current !== null;
    runningRef.current = false;
    if (statsIntervalRef.current !== null) window.clearInterval(statsIntervalRef.current);
    statsIntervalRef.current = null;
    readerRef.current?.cancel().catch(() => undefined);
    readerRef.current = null;
    encoderRef.current?.close();
    encoderRef.current = null;
    queueRef.current = [];
    forceNextKeyframeRef.current = true;
    if (pendingStartRef.current) {
      window.clearTimeout(pendingStartRef.current.timeout);
      pendingStartRef.current.reject(new Error("WebCodecs ingest stopped before server ready"));
      pendingStartRef.current = null;
    }
    if (wasActive && isSocketOpen()) sendSignalJson({ type: "video:webcodecs_stop" });
  }, [isSocketOpen, sendSignalJson]);

  const flushQueue = useCallback(() => {
    if (!isSocketOpen()) return;
    applyQueuePolicy({
      queue: queueRef.current,
      stats: statsRef.current,
      bufferedAmount: getBufferedAmount(),
      onDrop,
      forceNextKeyframeRef,
    });
    while (queueRef.current.length > 0 && getBufferedAmount() <= MAX_WS_BUFFERED_BYTES) {
      const packet = queueRef.current.shift()!;
      const frame = encodeBtv1Frame(
        {
          codec: "vp8",
          keyFrame: packet.keyFrame,
          sequence: packet.sequence,
          captureTimeUs: packet.captureTimeUs,
          durationUs: packet.durationUs,
          clientSentTimeUs: nowUs(),
          width: statsRef.current.encoded.width,
          height: statsRef.current.encoded.height,
        },
        packet.payload,
      );
      sendBinary(frame);
      statsRef.current.sentFrames += 1;
      statsRef.current.wsBufferedBytes = getBufferedAmount();
    }
  }, [getBufferedAmount, isSocketOpen, onDrop, sendBinary]);

  const enqueueChunk = useCallback(
    (chunk: EncodedVideoChunkLike) => {
      const payload = new Uint8Array(chunk.byteLength);
      chunk.copyTo(payload);
      const keyFrame = chunk.type === "key";
      queueRef.current.push({
        payload,
        keyFrame,
        sequence: sequenceRef.current++,
        captureTimeUs: safeUs(chunk.timestamp),
        durationUs: safeUs(chunk.duration ?? 0),
      });
      statsRef.current.encodedFrames += 1;
      if (keyFrame) statsRef.current.keyFrames += 1;
      flushQueue();
    },
    [flushQueue],
  );

  const startFrameStreaming = useCallback(
    async (stream: MediaStream | null, profile?: VideoProfile) => {
      if (runningRef.current) return;
      if (!isSocketOpen()) throw new Error("Media socket is not open");
      const support = detectWebCodecsSupport();
      if (!support.available) {
        throw new Error(
          "WebCodecs ingest is not available in this browser. Use WebRTC or switch to Chrome/Brave.",
        );
      }
      const track = stream?.getVideoTracks()[0];
      if (!track) throw new Error("Camera video track is not available");
      const Encoder = videoEncoderCtor();
      const Processor = mediaStreamTrackProcessorCtor();
      if (!Encoder || !Processor) throw new Error("Required WebCodecs APIs are unavailable");

      const videoProfile = normalizeProfile(profile ?? readTrackProfile(track));
      const config: VideoEncoderConfigLike = {
        codec: "vp8",
        width: videoProfile.width,
        height: videoProfile.height,
        framerate: videoProfile.fps,
        bitrate: DEFAULT_BITRATE_BPS,
        latencyMode: "realtime",
      };
      const supported = await Encoder.isConfigSupported?.(config);
      if (supported && supported.supported === false) {
        throw new Error("VP8 WebCodecs encoder is not supported by this browser");
      }

      await waitForServerReady(sendSignalJson, videoProfile);
      statsRef.current = emptyStats(videoProfile);
      sequenceRef.current = 0;
      lastKeyframeAtUsRef.current = 0;
      forceNextKeyframeRef.current = true;
      runningRef.current = true;

      const encoder = new Encoder({
        output: enqueueChunk,
        error: (error) => {
          onConnectionIssue?.({ layer: "webcodecs", message: `WebCodecs encoder failed: ${error.message}` });
          stopFrameStreaming();
        },
      });
      encoder.configure(config);
      encoderRef.current = encoder;

      const processor = new Processor({ track });
      const reader = processor.readable.getReader();
      readerRef.current = reader;
      statsIntervalRef.current = window.setInterval(() => {
        flushQueue();
        emitStats();
      }, 1000);

      void pumpFrames({
        reader,
        encoder,
        runningRef,
        forceNextKeyframeRef,
        lastKeyframeAtUsRef,
        onConnectionIssue,
      });
    },
    [emitStats, enqueueChunk, flushQueue, isSocketOpen, onConnectionIssue, sendSignalJson, stopFrameStreaming],
  );

  const handleWebCodecsMessage = useCallback(
    (msg: unknown): boolean => {
      if (typeof msg !== "object" || msg === null) return false;
      const typed = msg as { type?: unknown; accepted?: unknown; message?: unknown };
      if (typed.type === "video:webcodecs_ready") {
        const pending = pendingStartRef.current;
        if (pending) {
          window.clearTimeout(pending.timeout);
          pendingStartRef.current = null;
          pending.resolve();
        }
        return true;
      }
      if (typed.type === "video:webcodecs_error") {
        const message = typeof typed.message === "string" ? typed.message : "WebCodecs video ingest rejected";
        const pending = pendingStartRef.current;
        if (pending) {
          window.clearTimeout(pending.timeout);
          pendingStartRef.current = null;
          pending.reject(new Error(message));
        }
        onConnectionIssue?.({ layer: "webcodecs", message });
        return true;
      }
      if (typed.type === "video:webcodecs_stats") {
        const stats = msg as {
          frames_received?: unknown;
          latest_media_pts_us?: unknown;
          drops?: unknown;
        };
        statsRef.current = {
          ...statsRef.current,
          serverAcceptedFrames: numberOrUndefined(stats.frames_received),
          serverLatestMediaPtsUs: numberOrUndefined(stats.latest_media_pts_us),
          serverDrops: numberOrUndefined(stats.drops),
        };
        onStats?.(statsRef.current);
        return true;
      }
      return false;
    },
    [onConnectionIssue, onStats],
  );

  function waitForServerReady(
    send: (msg: WebCodecsSignal) => void,
    videoProfile: VideoProfile,
  ): Promise<void> {
    if (pendingStartRef.current) {
      return Promise.reject(new Error("WebCodecs start already pending"));
    }
    return new Promise((resolve, reject) => {
      const timeout = window.setTimeout(() => {
        pendingStartRef.current = null;
        reject(new Error("Timed out waiting for WebCodecs ingest readiness"));
      }, 5000);
      pendingStartRef.current = { resolve, reject, timeout };
      send({
        type: "video:webcodecs_start",
        mode: "webcodecs_ws",
        codec: "vp8",
        width: videoProfile.width,
        height: videoProfile.height,
        fps: videoProfile.fps,
        bitrate_bps: DEFAULT_BITRATE_BPS,
        keyframe_interval_ms: KEYFRAME_INTERVAL_MS,
        timebase_us: 1,
      });
    });
  }

  return {
    startFrameStreaming,
    stopFrameStreaming,
    handleWebCodecsMessage,
  };
}

async function pumpFrames({
  reader,
  encoder,
  runningRef,
  forceNextKeyframeRef,
  lastKeyframeAtUsRef,
  onConnectionIssue,
}: {
  reader: ReadableStreamDefaultReader<VideoFrameLike>;
  encoder: VideoEncoderLike;
  runningRef: React.MutableRefObject<boolean>;
  forceNextKeyframeRef: React.MutableRefObject<boolean>;
  lastKeyframeAtUsRef: React.MutableRefObject<number>;
  onConnectionIssue?: (issue: WebCodecsConnectionIssue) => void;
}) {
  try {
    while (runningRef.current) {
      const { done, value } = await reader.read();
      if (done || !value) break;
      const timestampUs = safeUs(value.timestamp ?? nowUs());
      const keyFrame =
        forceNextKeyframeRef.current ||
        timestampUs - lastKeyframeAtUsRef.current >= KEYFRAME_INTERVAL_MS * 1000;
      if (keyFrame) {
        forceNextKeyframeRef.current = false;
        lastKeyframeAtUsRef.current = timestampUs;
      }
      encoder.encode(value, { keyFrame });
      value.close();
    }
  } catch (error) {
    if (runningRef.current) {
      onConnectionIssue?.({
        layer: "webcodecs",
        message: error instanceof Error ? error.message : "WebCodecs camera frame loop failed",
      });
    }
  }
}

function applyQueuePolicy({
  queue,
  stats,
  bufferedAmount,
  onDrop,
  forceNextKeyframeRef,
}: {
  queue: QueuedPacket[];
  stats: WebCodecsVideoStats;
  bufferedAmount: number;
  onDrop?: (drop: WebCodecsVideoDrop) => void;
  forceNextKeyframeRef: React.MutableRefObject<boolean>;
}) {
  if (bufferedAmount <= MAX_WS_BUFFERED_BYTES && queueDurationMs(queue, stats.capture.fps) <= MAX_QUEUED_VIDEO_MS) {
    return;
  }
  const before = queue.length;
  while (queueDurationMs(queue, stats.capture.fps) > MAX_QUEUED_VIDEO_MS) {
    const index = queue.findIndex((packet) => !packet.keyFrame);
    if (index < 0) break;
    queue.splice(index, 1);
  }
  if (bufferedAmount > MAX_WS_BUFFERED_BYTES) {
    while (queue.length > 1 && !queue[0].keyFrame) queue.shift();
  }
  const dropped = before - queue.length;
  if (dropped <= 0) return;
  stats.droppedFrames += dropped;
  forceNextKeyframeRef.current = true;
  onDrop?.({
    reason: bufferedAmount > MAX_WS_BUFFERED_BYTES ? "ws_backpressure" : "queue_overflow",
    droppedFrames: dropped,
    bufferedAmount,
    queueMs: queueDurationMs(queue, stats.capture.fps),
  });
}

function queueDurationMs(queue: QueuedPacket[], fps: number): number {
  if (queue.length === 0) return 0;
  const first = queue[0];
  const last = queue[queue.length - 1];
  if (last.captureTimeUs > first.captureTimeUs) {
    return (last.captureTimeUs - first.captureTimeUs) / 1000;
  }
  return Math.round((queue.length * 1000) / Math.max(1, fps));
}

function emptyStats(profile?: VideoProfile): WebCodecsVideoStats {
  const normalized = normalizeProfile(profile ?? { width: DEFAULT_WIDTH, height: DEFAULT_HEIGHT, fps: DEFAULT_FPS });
  return {
    codec: "vp8",
    capture: normalized,
    encoded: normalized,
    encodedFrames: 0,
    sentFrames: 0,
    droppedFrames: 0,
    keyFrames: 0,
    queueMs: 0,
    wsBufferedBytes: 0,
  };
}

function normalizeProfile(profile: VideoProfile): VideoProfile {
  return {
    width: Math.round(profile.width || DEFAULT_WIDTH),
    height: Math.round(profile.height || DEFAULT_HEIGHT),
    fps: Math.round(profile.fps || DEFAULT_FPS),
  };
}

function readTrackProfile(track: MediaStreamTrack): VideoProfile {
  const settings = typeof track.getSettings === "function" ? track.getSettings() : {};
  return {
    width: Number(settings.width) || DEFAULT_WIDTH,
    height: Number(settings.height) || DEFAULT_HEIGHT,
    fps: Number(settings.frameRate) || DEFAULT_FPS,
  };
}

function safeUs(value: number | null | undefined): number {
  if (!Number.isFinite(value ?? NaN) || Number(value) < 0) return 0;
  return Math.round(Number(value));
}

function nowUs(): number {
  return Math.round(performance.now() * 1000);
}

function numberOrUndefined(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}
