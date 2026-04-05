import { useReducer, useRef, useCallback } from "react";
import { AudioPipeline } from "../lib/AudioPipeline";
import { RoomSocket } from "../lib/RoomSocket";
import { useTimings, type UtteranceTiming } from "./useTimings";
import { hostReducer, INITIAL_STATE } from "../state/host/reducer";
import { createMessageHandler } from "../state/host/messageHandler";
import { useWebcamCapture } from "../features/host/hooks/useWebcamCapture";
import { useVoiceRecording } from "../features/host/hooks/useVoiceRecording";

export type { UtteranceTiming };
export type { HostStatus, GuestCounts, HostUtterance } from "../state/host/reducer";

export function useHostRoom() {
  const [state, dispatch] = useReducer(hostReducer, INITIAL_STATE);
  const { timings, startTimer, markInterim, recordStt, recordTranslate, recordTts, reset: resetTimings } = useTimings();

  const audio = useRef(new AudioPipeline());
  const socket = useRef(new RoomSocket());

  const isSocketOpen = useCallback(() => socket.current.isOpen, []);
  const sendJson = useCallback((msg: object) => socket.current.sendJson(msg), []);

  const { videoRef, startWebcam, stopWebcam, startFrameStreaming, stopFrameStreaming } = useWebcamCapture({ isSocketOpen, sendJson });
  const { startVoiceRecording, stopVoiceRecording, skipVoiceSetup } = useVoiceRecording({ dispatch, sendJson });

  const handleMessage = createMessageHandler(
    dispatch,
    () => state.guestCounts,
    { startTimer, markInterim, recordStt, recordTranslate, recordTts },
  );

  const stopRecording = useCallback(() => {
    audio.current.stop();
    stopFrameStreaming();
    dispatch({ type: "recording_stopped" });
  }, [stopFrameStreaming]);

  const createRoom = useCallback((opts?: { sessionId?: string; sourceLang?: string }) => {
    dispatch({ type: "reset" });
    resetTimings();
    startWebcam();
    const params: Record<string, string> = { role: "host", sourceLang: opts?.sourceLang ?? "en" };
    if (opts?.sessionId) params.sessionId = opts.sessionId;
    socket.current.connect(
      params,
      {
        onMessage: (msg) => { handleMessage(msg); },
        onClose: () => { stopRecording(); dispatch({ type: "disconnected" }); },
      },
    );
  }, [handleMessage, resetTimings, startWebcam, stopRecording]);

  const startRecording = useCallback(async () => {
    if (!socket.current.isOpen) return;
    const analyser = await audio.current.start((buf) => socket.current.sendAudio(buf));
    dispatch({ type: "recording_started", analyser });
    startFrameStreaming();
  }, [startFrameStreaming]);

  const closeRoom = useCallback(() => {
    sendJson({ type: "host:end" });
    socket.current.close();
    stopRecording();
    stopFrameStreaming();
    stopWebcam();
  }, [sendJson, stopRecording, stopFrameStreaming, stopWebcam]);

  return {
    ...state, timings, videoRef,
    createRoom, startRecording, stopRecording, closeRoom,
    startVoiceRecording, stopVoiceRecording, skipVoiceSetup,
  };
}
