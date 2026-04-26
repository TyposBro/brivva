import { useReducer, useRef, useCallback } from "react";
import { AudioPipeline } from "../../../shared/audio/audio-pipeline";
import { SessionSocket } from "../../../shared/networking/session-socket";
import { ensureFreshToken } from "../../../shared/auth/auth-store";
import { useTimings, type UtteranceTiming } from "./use-timings";
import { hostReducer, INITIAL_STATE } from "./reducer";
import { createMessageHandler } from "./message-handler";
import { useWebcam } from "./use-webcam";
import { useVoiceClone } from "./use-voice-clone";
import { appConfig } from "../../../core/config/app-config";
import { WebRtcVideoIngest } from "./webrtc-video-ingest";

export type { UtteranceTiming };
export type { HostStatus, HostUtterance } from "./reducer";

// PRAGMATIC: 104 lines — §3.1 wants ≤80, but this hook IS the composition
// root for the host flow. It stitches 3 child hooks (useWebcam, useVoiceClone,
// audio + socket lifecycle) into the state machine. Further splitting scatters
// the wiring across files where the dependency chain becomes harder to trace
// than the inline definition.
export function useHostSession() {
  const [state, dispatch] = useReducer(hostReducer, INITIAL_STATE);
  const { timings, startTimer, markInterim, recordStt, recordTranslate, recordTts, finalize, reset: resetTimings } = useTimings();

  const audio = useRef(new AudioPipeline());
  const socket = useRef(new SessionSocket());
  const webRtcVideo = useRef(new WebRtcVideoIngest());
  const activeSessionIdRef = useRef<string | null>(null);
  const activeUserIdRef = useRef<string | null>(null);
  const activeSourceLangRef = useRef<string>("en");
  const activeTokenRef = useRef<string | null>(null);

  const { videoRef, startWebcam, stopWebcam, startFrameStreaming, stopFrameStreaming, getStream } = useWebcam(
    (msg) => socket.current.sendJson(msg),
    () => socket.current.isOpen,
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
    { startTimer, markInterim, recordStt, recordTranslate, recordTts, finalize },
  );

  const stopRecording = () => {
    audio.current.stop();
    webRtcVideo.current.stop();
    stopFrameStreaming();
    dispatch({ type: "recording_stopped" });
  };

  const fetchAuthToken = async (): Promise<string | null> => {
    try {
      return await ensureFreshToken();
    } catch (e) {
      dispatch({
        type: "error",
        message: e instanceof Error ? `Auth token fetch failed: ${e.message}` : "Auth failed",
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
    startWebcam();
    activeSessionIdRef.current = opts.sessionId ?? null;
    activeUserIdRef.current = opts.userId;
    activeSourceLangRef.current = opts.sourceLang ?? "en";

    const token = await fetchAuthToken();
    if (!token) return;
    activeTokenRef.current = token;

    const params: Record<string, string> = { sourceLang: opts.sourceLang ?? "en", token };
    if (opts.sessionId) params.sessionId = opts.sessionId;
    if (appConfig().videoIngest === "webrtc") params.videoMode = "webrtc-h264";
    socket.current.connect(params, {
      onOpen: () => dispatch({ type: "connected" }),
      onMessage: (msg) => handleMessage(msg),
      onClose: () => {
        stopRecording();
        dispatch({ type: "disconnected" });
      },
    });
  };

  const startRecording = async () => {
    if (!socket.current.isOpen) return;
    const analyser = await audio.current.start((buf) => socket.current.sendAudio(buf));
    if (appConfig().videoIngest === "webrtc") {
      const stream = getStream();
      const token = activeTokenRef.current;
      if (!stream || !token) {
        dispatch({ type: "error", message: "WebRTC video ingest could not start" });
        audio.current.stop();
        return;
      }
      try {
        await webRtcVideo.current.start({
          stream,
          token,
          sessionId: activeSessionIdRef.current ?? undefined,
          sourceLang: activeSourceLangRef.current,
        });
      } catch (e) {
        dispatch({
          type: "error",
          message: e instanceof Error ? `WebRTC video failed: ${e.message}` : "WebRTC video failed",
        });
        audio.current.stop();
        return;
      }
    }
    dispatch({ type: "recording_started", analyser });
    if (appConfig().videoIngest !== "webrtc") startFrameStreaming();
  };

  const closeSession = () => {
    socket.current.sendJson({ type: "host:end" });
    socket.current.close();
    webRtcVideo.current.stop();
    stopRecording();
    stopFrameStreaming();
    stopWebcam();
  };

  return {
    ...state, timings, videoRef,
    connectSession, startRecording, stopRecording, closeSession,
    startVoiceRecording, stopVoiceRecording, skipVoiceSetup,
    voiceElapsedSec, voiceIsRecording, voiceMinSec, voiceMaxSec,
    setActiveTargetLangs,
  };
}
