import { useCallback } from "react";
import * as api from "../data/api-client";
import { useVoiceRecorder } from "../../../shared/audio/voice-recorder";
import type { HostAction } from "./reducer";

export const VOICE_SAMPLE_MIN_SEC = 30;
export const VOICE_SAMPLE_MAX_SEC = 180;

/** Bridges the shared voice-recorder hook to the host-session reducer.
 *  Caller stops the recording (UI gates the button by elapsed time); we then
 *  upload the sample to Workers for cloning and emit the resulting status
 *  transitions. The 30s minimum / 180s hard cap sit on the UI side; the
 *  underlying recorder enforces the cap and auto-uploads on hit. */
export function useVoiceClone(
  dispatch: (a: HostAction) => void,
  getSessionId: () => string | null,
  getUserId: () => string | null,
) {
  const upload = useCallback(
    (b64: string) => {
      dispatch({ type: "voice_cloning" });
      const sessionId = getSessionId();
      const userId = getUserId();
      if (!sessionId) {
        dispatch({ type: "error", message: "Session ID required for voice cloning" });
        dispatch({ type: "skip_voice_setup" });
        return;
      }
      if (!userId) {
        dispatch({ type: "error", message: "User ID required for voice cloning" });
        dispatch({ type: "skip_voice_setup" });
        return;
      }
      void api
        .cloneSessionVoice(sessionId, { user_id: userId, audio_base64: b64 })
        .then(() => dispatch({ type: "voice_ready" }))
        .catch((err) => {
          dispatch({
            type: "error",
            message: err instanceof Error ? err.message : "Voice clone failed",
          });
          dispatch({ type: "skip_voice_setup" });
        });
    },
    [dispatch, getSessionId, getUserId],
  );

  const recorder = useVoiceRecorder({
    minSec: VOICE_SAMPLE_MIN_SEC,
    maxSec: VOICE_SAMPLE_MAX_SEC,
    onAutoStop: upload,
  });

  const startVoiceRecording = useCallback(async () => {
    try {
      await recorder.start();
    } catch (err) {
      console.error("Voice recording failed:", err);
    }
  }, [recorder]);

  const stopVoiceRecording = useCallback(() => {
    const b64 = recorder.stop();
    if (b64 !== null) upload(b64);
  }, [recorder, upload]);

  const skipVoiceSetup = useCallback(() => {
    dispatch({ type: "skip_voice_setup" });
  }, [dispatch]);

  return {
    startVoiceRecording,
    stopVoiceRecording,
    skipVoiceSetup,
    voiceElapsedSec: recorder.elapsedSec,
    voiceIsRecording: recorder.isRecording,
    voiceMinSec: VOICE_SAMPLE_MIN_SEC,
    voiceMaxSec: VOICE_SAMPLE_MAX_SEC,
  };
}
