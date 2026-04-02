import { useState, useRef, useCallback, useEffect } from "react";
import { AudioPipeline } from "../lib/AudioPipeline";

// ── Types ────────────────────────────────────────────────

type TranslationTier = 1 | 2 | 3 | 4;

type Lang = { code: string; label: string; flag: string };

const LANGS: Lang[] = [
  { code: "ko", label: "Korean", flag: "🇰🇷" },
  { code: "en", label: "English", flag: "🇬🇧" },
  { code: "ja", label: "Japanese", flag: "🇯🇵" },
  { code: "zh", label: "Chinese", flag: "🇨🇳" },
];

type TranscriptEntry = {
  id: number;
  text: string;
  translations: Record<string, string>;
};

// ── Component ────────────────────────────────────────────

export default function BroadcastPage() {
  const [sourceLang, setSourceLang] = useState("en");
  const [targetLangs, setTargetLangs] = useState<string[]>(["ja", "ko"]);
  const [tier, setTier] = useState<TranslationTier>(2);
  const [isLive, setIsLive] = useState(false);
  const [interim, setInterim] = useState("");
  const [transcripts, setTranscripts] = useState<TranscriptEntry[]>([]);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [voiceReady, setVoiceReady] = useState(false);
  const [rtmpUrls, setRtmpUrls] = useState<Record<string, string>>({});

  const wsRef = useRef<WebSocket | null>(null);
  const audioRef = useRef(new AudioPipeline());
  const scrollRef = useRef<HTMLDivElement>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const frameIntervalRef = useRef<number | null>(null);

  // Auto-scroll transcript
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [transcripts, interim]);

  const toggleLang = useCallback((code: string) => {
    setTargetLangs((prev) =>
      prev.includes(code) ? prev.filter((l) => l !== code) : [...prev, code]
    );
  }, []);

  // ── Webcam Capture ────────────────────────────────────

  const startWebcam = useCallback(async (ws: WebSocket) => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        video: { width: { ideal: 1280 }, height: { ideal: 720 } },
      });
      const video = videoRef.current!;
      video.srcObject = stream;
      await video.play();

      const canvas = canvasRef.current!;
      canvas.width = video.videoWidth;
      canvas.height = video.videoHeight;
      const ctx = canvas.getContext("2d")!;

      frameIntervalRef.current = window.setInterval(() => {
        if (ws.readyState !== WebSocket.OPEN) return;
        ctx.drawImage(video, 0, 0);
        const dataUrl = canvas.toDataURL("image/jpeg", 0.6);
        const base64 = dataUrl.split(",")[1];
        ws.send(JSON.stringify({ type: "face:frame", data: base64 }));
      }, 66); // ~15fps
    } catch (e) {
      console.error("[WEBCAM] Failed to start:", e);
    }
  }, []);

  const stopWebcam = useCallback(() => {
    if (frameIntervalRef.current) {
      clearInterval(frameIntervalRef.current);
      frameIntervalRef.current = null;
    }
    const video = videoRef.current;
    if (video?.srcObject) {
      (video.srcObject as MediaStream).getTracks().forEach((t) => t.stop());
      video.srcObject = null;
    }
  }, []);

  // ── WebSocket Messages ────────────────────────────────

  const handleWsMessage = useCallback((e: MessageEvent) => {
    if (typeof e.data !== "string") return; // Binary = TTS audio (monitoring)

    const msg = JSON.parse(e.data);
    switch (msg.type) {
      case "session:created":
        setSessionId(msg.id);
        break;
      case "interim":
        setInterim(msg.transcript);
        break;
      case "final":
        setInterim("");
        setTranscripts((prev) => [
          ...prev,
          { id: msg.utteranceId, text: msg.transcript, translations: {} },
        ]);
        break;
      case "translation":
        setTranscripts((prev) =>
          prev.map((t) =>
            t.id === msg.utteranceId
              ? { ...t, translations: { ...t.translations, [msg.lang]: msg.text } }
              : t
          )
        );
        break;
      case "voice:ready":
        setVoiceReady(true);
        break;
    }
  }, []);

  // ── Start / Stop ──────────────────────────────────────

  const start = useCallback(async () => {
    const available = targetLangs.filter((l) => l !== sourceLang);
    if (available.length === 0) return;

    const ws = new WebSocket(
      `ws://localhost:3000/ws?sourceLang=${sourceLang}&targetLangs=${available.join(",")}&tier=${tier}`
    );
    ws.binaryType = "arraybuffer";
    wsRef.current = ws;

    ws.onmessage = handleWsMessage;
    ws.onclose = () => {
      setIsLive(false);
      setSessionId(null);
    };

    await new Promise<void>((resolve) => {
      ws.onopen = () => resolve();
    });

    // Send RTMP config if any URLs are set
    const streams = Object.entries(rtmpUrls)
      .filter(([, url]) => url.trim())
      .map(([lang, url]) => ({ lang, url: url.trim() }));

    if (streams.length > 0) {
      ws.send(JSON.stringify({ type: "rtmp:config", streams }));
      // Start webcam capture for RTMP video
      await startWebcam(ws);
    }

    // Start audio capture
    await audioRef.current.start((buffer) => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(buffer);
      }
    });

    setIsLive(true);
    setTranscripts([]);
    setInterim("");
    setVoiceReady(false);
  }, [sourceLang, targetLangs, tier, rtmpUrls, handleWsMessage, startWebcam]);

  const stop = useCallback(() => {
    stopWebcam();
    audioRef.current.stop();
    wsRef.current?.close();
    wsRef.current = null;
    setIsLive(false);
    setSessionId(null);
  }, [stopWebcam]);

  const availableTargets = LANGS.filter((l) => l.code !== sourceLang);
  const hasRtmpStreams = Object.values(rtmpUrls).some((url) => url.trim());

  return (
    <div className="min-h-screen bg-background text-on-surface p-6">
      {/* Hidden canvas for webcam frame capture */}
      <canvas ref={canvasRef} className="hidden" />

      <div className="max-w-3xl mx-auto space-y-6">
        {/* Header */}
        <div className="flex items-center justify-between">
          <h1 className="font-headline text-2xl font-bold text-primary">Brivva</h1>
          {sessionId && (
            <span className="text-xs font-mono text-outline">
              Session: {sessionId}
            </span>
          )}
        </div>

        {/* Tier Selection */}
        <div className="bg-surface-container-low rounded-xl p-4 space-y-3">
          <h2 className="font-headline text-sm font-semibold text-on-surface-variant uppercase tracking-wider">
            Translation Mode
          </h2>
          <div className="grid grid-cols-2 gap-2">
            {([
              { tier: 1 as TranslationTier, label: "Subtitles Only", desc: "No voice translation", cost: "Free", ready: true },
              { tier: 2 as TranslationTier, label: "Voice + Subtitles", desc: "AI-translated voice", cost: "$5/hr", ready: true },
              { tier: 3 as TranslationTier, label: "Voice + Lipsync (Live)", desc: "Real-time lipsync", cost: "$40/hr", ready: false },
              { tier: 4 as TranslationTier, label: "Voice + Lipsync (Post)", desc: "Post-processed lipsync", cost: "$30-40/hr", ready: false },
            ]).map((opt) => (
              <button
                key={opt.tier}
                disabled={!opt.ready || isLive}
                onClick={() => setTier(opt.tier)}
                className={`text-left p-3 rounded-lg border transition-colors ${
                  tier === opt.tier
                    ? "border-primary bg-surface-container-high"
                    : "border-outline-variant bg-surface-container"
                } ${!opt.ready ? "opacity-40 cursor-not-allowed" : "hover:border-primary/60"}`}
              >
                <div className="flex items-center justify-between">
                  <span className="text-sm font-semibold">Option {opt.tier}</span>
                  <span className="text-xs text-outline">{opt.cost}</span>
                </div>
                <div className="text-sm text-on-surface mt-0.5">{opt.label}</div>
                <div className="text-xs text-outline mt-0.5">{opt.desc}</div>
                {!opt.ready && (
                  <div className="text-xs text-secondary mt-1">Coming Soon</div>
                )}
              </button>
            ))}
          </div>
        </div>

        {/* Language Config */}
        <div className="bg-surface-container-low rounded-xl p-4 space-y-4">
          {/* Source Language */}
          <div>
            <h2 className="font-headline text-sm font-semibold text-on-surface-variant uppercase tracking-wider mb-2">
              Host Language
            </h2>
            <div className="flex gap-2">
              {LANGS.map((l) => (
                <button
                  key={l.code}
                  disabled={isLive}
                  onClick={() => {
                    setSourceLang(l.code);
                    setTargetLangs((prev) => prev.filter((t) => t !== l.code));
                  }}
                  className={`px-3 py-1.5 rounded-lg text-sm transition-colors ${
                    sourceLang === l.code
                      ? "bg-primary-container text-on-primary-container font-semibold"
                      : "bg-surface-container text-on-surface-variant hover:bg-surface-container-high"
                  }`}
                >
                  {l.flag} {l.label}
                </button>
              ))}
            </div>
          </div>

          {/* Target Languages */}
          <div>
            <h2 className="font-headline text-sm font-semibold text-on-surface-variant uppercase tracking-wider mb-2">
              Translate To
            </h2>
            <div className="flex gap-2">
              {availableTargets.map((l) => (
                <button
                  key={l.code}
                  disabled={isLive}
                  onClick={() => toggleLang(l.code)}
                  className={`px-3 py-1.5 rounded-lg text-sm transition-colors ${
                    targetLangs.includes(l.code)
                      ? "bg-secondary-container text-on-secondary-container font-semibold"
                      : "bg-surface-container text-on-surface-variant hover:bg-surface-container-high"
                  }`}
                >
                  {l.flag} {l.label}
                </button>
              ))}
            </div>
            <p className="text-xs text-outline mt-2">
              {targetLangs.filter((l) => l !== sourceLang).length} language(s) selected
              {sourceLang && ` + 1 passthrough (${LANGS.find((l) => l.code === sourceLang)?.label})`}
            </p>
          </div>
        </div>

        {/* RTMP Destinations */}
        <div className="bg-surface-container-low rounded-xl p-4 space-y-3">
          <h2 className="font-headline text-sm font-semibold text-on-surface-variant uppercase tracking-wider">
            RTMP Destinations
          </h2>
          <p className="text-xs text-outline">
            Enter RTMP URL + stream key for each language. Leave blank to skip RTMP.
          </p>

          {/* Source language (passthrough) */}
          {(() => {
            const srcLang = LANGS.find((l) => l.code === sourceLang);
            return srcLang ? (
              <div className="space-y-1">
                <label className="text-sm text-on-surface-variant">
                  {srcLang.flag} {srcLang.label} (passthrough)
                </label>
                <input
                  type="text"
                  disabled={isLive}
                  placeholder="rtmp://live.example.com/stream/key"
                  value={rtmpUrls[sourceLang] || ""}
                  onChange={(e) =>
                    setRtmpUrls((prev) => ({ ...prev, [sourceLang]: e.target.value }))
                  }
                  className="w-full px-3 py-2 rounded-lg bg-surface-container border border-outline-variant text-sm text-on-surface placeholder:text-outline focus:border-primary focus:outline-none disabled:opacity-50"
                />
              </div>
            ) : null;
          })()}

          {/* Target languages */}
          {targetLangs
            .filter((l) => l !== sourceLang)
            .map((code) => {
              const lang = LANGS.find((l) => l.code === code);
              return lang ? (
                <div key={code} className="space-y-1">
                  <label className="text-sm text-on-surface-variant">
                    {lang.flag} {lang.label} (translated)
                  </label>
                  <input
                    type="text"
                    disabled={isLive}
                    placeholder="rtmp://live.example.com/stream/key"
                    value={rtmpUrls[code] || ""}
                    onChange={(e) =>
                      setRtmpUrls((prev) => ({ ...prev, [code]: e.target.value }))
                    }
                    className="w-full px-3 py-2 rounded-lg bg-surface-container border border-outline-variant text-sm text-on-surface placeholder:text-outline focus:border-primary focus:outline-none disabled:opacity-50"
                  />
                </div>
              ) : null;
            })}
        </div>

        {/* Webcam Preview (visible when streaming) */}
        <video
          ref={videoRef}
          className={isLive && hasRtmpStreams ? "w-full max-h-48 object-cover rounded-xl bg-black" : "hidden"}
          muted
          playsInline
        />

        {/* Controls */}
        <div className="flex items-center gap-4">
          {!isLive ? (
            <button
              onClick={start}
              disabled={targetLangs.filter((l) => l !== sourceLang).length === 0}
              className="monolith-gradient px-6 py-2.5 rounded-lg text-white font-semibold text-sm disabled:opacity-40 disabled:cursor-not-allowed hover:opacity-90 transition-opacity"
            >
              {hasRtmpStreams ? "Start Broadcasting" : "Start Translation"}
            </button>
          ) : (
            <button
              onClick={stop}
              className="bg-error-container text-on-error-container px-6 py-2.5 rounded-lg font-semibold text-sm hover:opacity-90 transition-opacity"
            >
              Stop
            </button>
          )}
          {isLive && (
            <div className="flex items-center gap-2">
              <span className="w-2 h-2 rounded-full bg-success animate-pulse" />
              <span className="text-sm text-success font-mono">LIVE</span>
              {hasRtmpStreams && (
                <span className="text-xs text-primary ml-2">RTMP</span>
              )}
              {voiceReady && (
                <span className="text-xs text-secondary ml-2">Voice cloned</span>
              )}
            </div>
          )}
        </div>

        {/* Live Transcript */}
        {isLive && (
          <div
            ref={scrollRef}
            className="bg-surface-container-lowest rounded-xl p-4 space-y-3 max-h-96 overflow-y-auto"
          >
            <h2 className="font-headline text-sm font-semibold text-on-surface-variant uppercase tracking-wider sticky top-0 bg-surface-container-lowest pb-2">
              Live Transcript
            </h2>

            {transcripts.map((entry) => (
              <div key={entry.id} className="space-y-1">
                <div className="text-sm text-on-surface">
                  <span className="text-outline text-xs font-mono mr-2">#{entry.id}</span>
                  {entry.text}
                </div>
                {Object.entries(entry.translations).map(([lang, text]) => (
                  <div key={lang} className="text-sm text-on-surface-variant pl-6">
                    <span className="text-xs font-mono text-secondary mr-1">
                      {LANGS.find((l) => l.code === lang)?.flag}
                    </span>
                    {text}
                  </div>
                ))}
              </div>
            ))}

            {interim && (
              <div className="text-sm text-outline italic">{interim}</div>
            )}

            {transcripts.length === 0 && !interim && (
              <div className="text-sm text-outline text-center py-4">
                Listening... speak into your microphone
              </div>
            )}
          </div>
        )}

        {/* Info */}
        <div className="text-xs text-outline space-y-1">
          <p>
            Option {tier}: {tier === 1 ? "Subtitles only — original audio passes through" : "Translated voice + subtitles — each language gets AI-generated voice audio"}
          </p>
          {hasRtmpStreams ? (
            <p>
              Webcam video + translated audio muxed via FFmpeg and pushed to RTMP destinations.
            </p>
          ) : (
            <p>
              Audio-only mode. Add RTMP destinations above to enable video streaming.
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
