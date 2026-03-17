import { useReducer, useRef, useEffect, useCallback } from "react";
import { RoomSocket } from "../lib/RoomSocket";
import { TtsPlayer } from "../lib/TtsPlayer";
import { VideoPlayer } from "../lib/VideoPlayer";
import { guestReducer, INITIAL_STATE } from "../state/guest/reducer";
import { createGuestMessageHandler } from "../state/guest/messageHandler";

export type { GuestStatus, GuestUtterance } from "../state/guest/reducer";

export function useGuestRoom(roomId: string, lang: string | null) {
  const [state, dispatch] = useReducer(guestReducer, INITIAL_STATE);

  const socket = useRef(new RoomSocket());
  const player = useRef(new TtsPlayer());
  const videoPlayer = useRef(new VideoPlayer());

  const attachCanvas = useCallback((canvas: HTMLCanvasElement | null) => {
    if (canvas) videoPlayer.current.attach(canvas);
  }, []);

  useEffect(() => {
    if (!lang || !roomId) return;

    dispatch({ type: "reset" });
    player.current.reset();
    videoPlayer.current.reset();

    const handleMessage = createGuestMessageHandler(
      dispatch, player.current, videoPlayer.current, socket.current,
    );

    socket.current.connect(
      { role: "guest", roomId, lang },
      {
        onMessage: handleMessage,
        onBinary: (data) => player.current.addChunk(data),
        onClose: () => dispatch({ type: "closed" }),
      },
    );

    return () => socket.current.close();
  }, [roomId, lang]);

  return { ...state, attachCanvas };
}
