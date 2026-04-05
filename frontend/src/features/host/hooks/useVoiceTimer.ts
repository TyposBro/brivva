import { useState, useRef, useCallback } from "react";
import { VOICE_TIMER_SECONDS, TIMER_INTERVAL_MS } from "../constants";

export function useVoiceTimer(startVoiceRecording: () => void) {
  const [voiceTimer, setVoiceTimer] = useState(0);
  const [isVoiceRecording, setIsVoiceRecording] = useState(false);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const handleStartVoice = useCallback(() => {
    setVoiceTimer(VOICE_TIMER_SECONDS);
    setIsVoiceRecording(true);
    startVoiceRecording();

    intervalRef.current = setInterval(() => {
      setVoiceTimer((t) => {
        if (t <= 1) {
          clearInterval(intervalRef.current!);
          setIsVoiceRecording(false);
          return 0;
        }
        return t - 1;
      });
    }, TIMER_INTERVAL_MS);
  }, [startVoiceRecording]);

  return { voiceTimer, isVoiceRecording, handleStartVoice };
}
