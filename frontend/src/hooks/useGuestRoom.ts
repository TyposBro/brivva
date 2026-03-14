import { useReducer, useRef, useEffect } from "react";
import { RoomSocket } from "../lib/RoomSocket";
import { TtsPlayer } from "../lib/TtsPlayer";
import { guestReducer, INITIAL_STATE } from "../state/guest/reducer";
import { createGuestMessageHandler } from "../state/guest/messageHandler";

export type { Lang, GuestStatus, GuestUtterance } from "../state/guest/reducer";

export function useGuestRoom(roomId: string, lang: string | null) {
  const [state, dispatch] = useReducer(guestReducer, INITIAL_STATE);

  const socket = useRef(new RoomSocket());
  const player = useRef(new TtsPlayer());

  useEffect(() => {
    if (!lang || !roomId) return;

    dispatch({ type: "reset" });
    player.current.reset();

    const handleMessage = createGuestMessageHandler(dispatch, player.current, socket.current);

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

  return state;
}
