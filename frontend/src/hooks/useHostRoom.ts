import { useReducer, useRef } from "react";
import { AudioPipeline } from "../lib/AudioPipeline";
import { RoomSocket } from "../lib/RoomSocket";
import { useTimings, type UtteranceTiming } from "./useTimings";
import { hostReducer, INITIAL_STATE } from "../state/host/reducer";
import { createMessageHandler } from "../state/host/messageHandler";

export type { UtteranceTiming };
export type { HostStatus, GuestCounts, HostUtterance } from "../state/host/reducer";

export function useHostRoom() {
  const [state, dispatch] = useReducer(hostReducer, INITIAL_STATE);
  const { timings, startTimer, recordSplit, finalize, reset: resetTimings } = useTimings();

  const audio = useRef(new AudioPipeline());
  const socket = useRef(new RoomSocket());

  const handleMessage = createMessageHandler(
    dispatch,
    () => state.guestCounts,
    { startTimer, recordSplit, finalize },
  );

  // --- actions ---

  const stopRecording = () => {
    audio.current.stop();
    dispatch({ type: "recording_stopped" });
  };

  const createRoom = () => {
    dispatch({ type: "reset" });
    resetTimings();
    socket.current.connect(
      { role: "host", sourceLang: "en" },
      { onMessage: handleMessage, onClose: () => { stopRecording(); dispatch({ type: "disconnected" }); } },
    );
  };

  const startRecording = async () => {
    if (!socket.current.isOpen) return;
    const analyser = await audio.current.start((buf) => socket.current.sendAudio(buf));
    dispatch({ type: "recording_started", analyser });
  };

  const closeRoom = () => {
    socket.current.sendJson({ type: "host:end" });
    socket.current.close();
    stopRecording();
  };

  return {
    ...state, timings,
    createRoom, startRecording, stopRecording, closeRoom,
  };
}
