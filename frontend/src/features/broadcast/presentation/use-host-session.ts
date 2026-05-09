import { useReducer, useRef, useCallback } from "react";
import { appConfig } from "../../../core/config/app-config";
import { AudioPipeline } from "../../../shared/audio/audio-pipeline";
import { SessionSocket } from "../../../shared/networking/session-socket";
import { ensureFreshToken } from "../../../shared/auth/auth-store";
import {
  configureSessionLogger,
  flushSessionLogs,
  sessionLog,
} from "../../../shared/logging/session-logger";
import { useTimings, type UtteranceTiming } from "./use-timings";
import { hostReducer, INITIAL_STATE } from "./reducer";
import { createMessageHandler } from "./message-handler";
import { useWebcam, type ClientMediaStats } from "./use-webcam";
import { useVoiceClone } from "./use-voice-clone";
import {
  parseServerCapabilities,
  readStoredMediaIngestMode,
  resolveMediaIngestMode,
  webCodecsRecordBlocker,
} from "./media-ingest-mode";
import { useWebCodecsVideoIngest } from "./use-webcodecs-video";

export type { UtteranceTiming };
export type { HostStatus, HostUtterance } from "./reducer";

// PRAGMATIC: 104 lines — §3.1 wants ≤80, but this hook IS the composition
// root for the host flow. It stitches 3 child hooks (useWebcam, useVoiceClone,
// audio + socket lifecycle) into the state machine. Further splitting scatters
// the wiring across files where the dependency chain becomes harder to trace
// than the inline definition.
export function useHostSession() {
  const [state, dispatch] = useReducer(hostReducer, INITIAL_STATE);
  const {
    timings,
    startTimer,
    markInterim,
    recordStt,
    recordTranslate,
    recordTts,
    finalize,
    reset: resetTimings,
  } = useTimings();

  const audio = useRef(new AudioPipeline());
  const socket = useRef(new SessionSocket());
  const activeSessionIdRef = useRef<string | null>(null);
  const activeUserIdRef = useRef<string | null>(null);

  const {
    videoRef,
    facingMode,
    startWebcam,
    stopWebcam,
    flipCamera,
    startFrameStreaming: startFrameStreamingWebRtc,
    stopFrameStreaming: stopFrameStreamingWebRtc,
    handleWebRtcMessage,
    getVideoStream,
    getVideoProfile,
  } = useWebcam(
    (msg) => socket.current.sendJson(msg),
    () => socket.current.isOpen,
    (stats) => {
      dispatch({
        type: "media_diagnostics",
        diagnostics: toDiagnostics(
          stats,
          readStoredMediaIngestMode(),
          resolveMediaIngestMode(readStoredMediaIngestMode()),
        ),
      });
      sessionLog("debug", "frontend.media_stats", { stats });
    },
    (issue) => {
      dispatch({ type: "connection_issue", message: issue.message });
      sessionLog("warn", "frontend.webrtc_issue", { issue }, issue.message);
    },
  );

  const {
    startFrameStreaming: startFrameStreamingWebCodecs,
    stopFrameStreaming: stopFrameStreamingWebCodecs,
    handleWebCodecsMessage,
  } = useWebCodecsVideoIngest(
    (msg) => socket.current.sendJson(msg),
    (data) => socket.current.sendBinary(data),
    () => socket.current.isOpen,
    () => socket.current.bufferedAmount,
    (stats) => {
      dispatch({
        type: "media_diagnostics",
        diagnostics: {
          mediaIngest: {
            requested: readStoredMediaIngestMode(),
            resolved: "webcodecs_ws",
            active: "webcodecs_ws",
            codec: stats.codec,
          },
          webcodecs: stats,
        },
      });
      sessionLog("debug", "frontend.webcodecs_stats", { stats });
    },
    (drop) => {
      sessionLog(
        "warn",
        "frontend.webcodecs_video_drop",
        {
          reason: drop.reason,
          dropped_frames: drop.droppedFrames,
          buffered_amount: drop.bufferedAmount,
          queue_ms: drop.queueMs,
        },
        "WebCodecs video dropped frames due to local backpressure",
      );
    },
    (issue) => {
      dispatch({ type: "connection_issue", message: issue.message });
      sessionLog("warn", "frontend.webcodecs_issue", { issue }, issue.message);
    },
  );

  const voiceClone = useVoiceClone(
    dispatch,
    () => activeSessionIdRef.current,
    () => activeUserIdRef.current,
  );
  const {
    startVoiceRecording,
    stopVoiceRecording,
    skipVoiceSetup,
    voiceElapsedSec,
    voiceIsRecording,
    voiceMinSec,
    voiceMaxSec,
  } = voiceClone;

  // Latency stopwatch needs to know which targets are active so it can create
  // a slot per-lang. We stash the most recent set in a ref; the hook's caller
  // updates it when a session is started.
  const activeTargetLangsRef = useRef<string[]>([]);
  const setActiveTargetLangs = useCallback((langs: string[]) => {
    activeTargetLangsRef.current = langs;
  }, []);

  const handleMessage = createMessageHandler(
    dispatch,
    () => activeTargetLangsRef.current,
    {
      startTimer,
      markInterim,
      recordStt,
      recordTranslate,
      recordTts,
      finalize,
    },
  );

  const stopRecording = () => {
    audio.current.stop();
    stopFrameStreamingWebRtc();
    stopFrameStreamingWebCodecs();
    dispatch({ type: "recording_stopped" });
    sessionLog("info", "frontend.recording_stopped");
  };

  const fetchAuthToken = async (): Promise<string | null> => {
    try {
      return await ensureFreshToken();
    } catch (e) {
      dispatch({
        type: "error",
        message:
          e instanceof Error
            ? `Auth token fetch failed: ${e.message}`
            : "Auth failed",
      });
      return null;
    }
  };

  const connectSession = async (opts?: {
    sessionId?: string;
    sourceLang?: string;
    userId: string;
  }) => {
    if (!opts?.userId) {
      dispatch({ type: "error", message: "user_id required to open WS" });
      return;
    }
    dispatch({ type: "reset" });
    resetTimings();
    await startWebcam();
    activeSessionIdRef.current = opts.sessionId ?? null;
    activeUserIdRef.current = opts.userId;
    const requestedMediaIngestMode = readStoredMediaIngestMode();
    const resolvedMediaIngestMode = resolveMediaIngestMode(requestedMediaIngestMode);
    if (opts.sessionId) {
      configureSessionLogger({
        sessionId: opts.sessionId,
        userId: opts.userId,
      });
      sessionLog("info", "frontend.session_connect_requested", {
        source_lang: opts.sourceLang ?? "en",
      });
      sessionLog("info", "frontend.media_ingest_mode_selected", {
        requested: requestedMediaIngestMode,
        resolved: resolvedMediaIngestMode,
      });
    }

    const token = await fetchAuthToken();
    if (!token) return;

    const params: Record<string, string> = {
      sourceLang: opts.sourceLang ?? "en",
      token,
    };
    if (opts.sessionId) {
      params.sessionId = opts.sessionId;
      params.mediaIngestMode = resolvedMediaIngestMode;
    }
    socket.current.connect(params, {
      onOpen: () => {
        dispatch({ type: "connected" });
        sessionLog("info", "frontend.ws_open");
      },
      onMessage: (msg) => {
        const capabilities = parseServerCapabilities(msg);
        if (capabilities) {
          dispatch({ type: "server_capabilities", capabilities });
          sessionLog("info", "frontend.media_server_capabilities", {
            videoIngestModes: capabilities.videoIngestModes,
            webcodecsCodecs: capabilities.webcodecsCodecs,
          });
          return;
        }
        if (handleWebCodecsMessage(msg)) return;
        if (handleWebRtcMessage(msg)) return;
        handleMessage(msg);
      },
      onClose: () => {
        stopRecording();
        dispatch({ type: "disconnected" });
        sessionLog("warn", "frontend.ws_closed");
        void flushSessionLogs();
      },
    });
  };

  const startRecording = async () => {
    if (!socket.current.isOpen) return;
    const requestedMediaIngestMode = readStoredMediaIngestMode();
    const mediaIngestMode = resolveMediaIngestMode(requestedMediaIngestMode);
    const blocker = webCodecsRecordBlocker(
      requestedMediaIngestMode,
      state.mediaServerCapabilities,
    );
    if (blocker) {
      dispatch({ type: "connection_issue", message: blocker });
      sessionLog(
        "warn",
        "frontend.recording_start_blocked",
        { requested: requestedMediaIngestMode, resolved: mediaIngestMode, reason: blocker },
        blocker,
      );
      return;
    }

    let analyser: AnalyserNode;
    try {
      if (mediaIngestMode === "webcodecs_ws") {
        await startFrameStreamingWebCodecs(getVideoStream(), getVideoProfile());
      }
      analyser = await audio.current.start(
        (buf) => socket.current.sendAudio(buf),
        { timestampedAudio: appConfig().timestampedAudioEnabled },
      );
      if (mediaIngestMode === "webrtc") {
        await startFrameStreamingWebRtc();
      }
    } catch (e) {
      audio.current.stop();
      stopFrameStreamingWebRtc();
      stopFrameStreamingWebCodecs();
      const message = e instanceof Error ? e.message : "Media start failed";
      dispatch({ type: "connection_issue", message });
      sessionLog(
        "warn",
        "frontend.recording_start_failed",
        { error: message, requested: requestedMediaIngestMode, resolved: mediaIngestMode },
        message,
      );
      return;
    }
    dispatch({
      type: "recording_started",
      analyser,
      requestedMediaIngestMode,
      mediaIngestMode,
    });
    sessionLog("info", "frontend.recording_started", {
      requested_media_ingest_mode: requestedMediaIngestMode,
      media_ingest_mode: mediaIngestMode,
    });
    sessionLog("info", "frontend.video_uplink_started", { mode: mediaIngestMode });
  };

  const closeSession = () => {
    sessionLog("info", "frontend.session_close_requested");
    socket.current.sendJson({ type: "host:end" });
    socket.current.close();
    stopRecording();
    stopFrameStreamingWebRtc();
    stopFrameStreamingWebCodecs();
    stopWebcam();
    void flushSessionLogs();
    configureSessionLogger(null);
  };

  const requestedMediaIngestMode = readStoredMediaIngestMode();
  const resolvedMediaIngestMode = resolveMediaIngestMode(requestedMediaIngestMode);
  const recordDisabledReason = webCodecsRecordBlocker(
    requestedMediaIngestMode,
    state.mediaServerCapabilities,
  );

  return {
    ...state,
    timings,
    videoRef,
    facingMode,
    requestedMediaIngestMode,
    resolvedMediaIngestMode,
    recordDisabledReason,
    connectSession,
    startRecording,
    stopRecording,
    closeSession,
    flipCamera,
    startVoiceRecording,
    stopVoiceRecording,
    skipVoiceSetup,
    voiceElapsedSec,
    voiceIsRecording,
    voiceMinSec,
    voiceMaxSec,
    setActiveTargetLangs,
  };
}

function toDiagnostics(
  stats: ClientMediaStats,
  requested: ReturnType<typeof readStoredMediaIngestMode>,
  resolved: ReturnType<typeof resolveMediaIngestMode>,
) {
  const outbound = stats.outboundVideo ?? {};
  return {
    mediaIngest: {
      requested,
      resolved,
      active: resolved,
      codec: typeof outbound.codec === "string" ? outbound.codec : undefined,
    },
    source: {
      width: stats.sourceTrack?.width,
      height: stats.sourceTrack?.height,
      frameRate: stats.sourceTrack?.frameRate,
    },
    outbound: {
      frameWidth:
        typeof outbound.frameWidth === "number"
          ? outbound.frameWidth
          : undefined,
      frameHeight:
        typeof outbound.frameHeight === "number"
          ? outbound.frameHeight
          : undefined,
      framesPerSecond:
        typeof outbound.framesPerSecond === "number"
          ? outbound.framesPerSecond
          : undefined,
      framesSent:
        typeof outbound.framesSent === "number"
          ? outbound.framesSent
          : undefined,
      qualityLimitationReason: outbound.qualityLimitationReason,
      codec: outbound.codec,
      candidatePair: outbound.candidatePair,
    },
  };
}
