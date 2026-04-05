import { type Dispatch } from "react";
import { type RoomMessage, type RoomSocket } from "../../../lib/RoomSocket";
import { type TtsPlayer } from "../../../lib/TtsPlayer";
import { type VideoPlayer } from "../../../lib/VideoPlayer";
import { type GuestAction } from "./reducer";

export function createGuestMessageHandler(
  dispatch: Dispatch<GuestAction>,
  player: TtsPlayer,
  videoPlayer: VideoPlayer,
  socket: RoomSocket,
) {
  return (msg: RoomMessage) => {
    switch (msg.type) {
      case "room:joined":
        dispatch({ type: "joined" });
        break;

      case "interim":
        dispatch({ type: "interim", transcript: (msg.transcript as string) ?? "" });
        break;

      case "final":
        dispatch({ type: "final", id: msg.utteranceId as number, original: (msg.transcript as string) ?? "" });
        break;

      case "translation":
        dispatch({ type: "translation", id: msg.utteranceId as number, text: (msg.text as string) ?? "" });
        break;

      case "tts_start":
        player.startReceiving(msg.utteranceId as number);
        break;

      case "tts_end":
        player.finishReceiving(msg.utteranceId as number);
        break;

      // Live host video frames
      case "face:frame":
        videoPlayer.renderDirect(msg.data as string);
        break;

      case "room:closed":
        dispatch({ type: "closed" });
        socket.close();
        break;

      case "error":
        dispatch({ type: "error", message: (msg.message as string) ?? "Unknown error" });
        break;
    }
  };
}
