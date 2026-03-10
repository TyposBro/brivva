import { useState, useEffect } from "react";

// BCP-47 tags for SpeechSynthesis
// TODO: swap this for Replica.com TTS API call (set VITE_REPLICA_API_KEY)
const LANG_BCP47: Record<string, string> = {
  en: "en-US",
  ko: "ko-KR",
  ja: "ja-JP",
  zh: "zh-CN",
  es: "es-ES",
  fr: "fr-FR",
  de: "de-DE",
  pt: "pt-BR",
  ar: "ar-SA",
  ru: "ru-RU",
};

interface AudioPlayerProps {
  text: string;
  lang: string;
}

export function AudioPlayer({ text, lang }: AudioPlayerProps) {
  const [isPlaying, setIsPlaying] = useState(false);

  useEffect(() => {
    setIsPlaying(false);
    window.speechSynthesis.cancel();
  }, [text]);

  const handlePlay = () => {
    if (isPlaying) {
      window.speechSynthesis.cancel();
      setIsPlaying(false);
      return;
    }

    const utterance = new SpeechSynthesisUtterance(text);
    utterance.lang = LANG_BCP47[lang] ?? "en-US";
    utterance.rate = 0.95;
    utterance.onend = () => setIsPlaying(false);
    utterance.onerror = () => setIsPlaying(false);

    window.speechSynthesis.speak(utterance);
    setIsPlaying(true);
  };

  return (
    <div className="audio-player">
      <button className={`play-btn ${isPlaying ? "playing" : ""}`} onClick={handlePlay}>
        {isPlaying ? "⏹ Stop" : "▶ Play Translation"}
      </button>
    </div>
  );
}
