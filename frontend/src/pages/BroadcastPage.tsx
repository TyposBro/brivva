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
  const [isCloning, setIsCloning] = useState(false);
  const [cloneProgress, setCloneProgress] = useState(0);

  const wsRef = useRef<WebSocket | null>(null);
  const audioRef = useRef(new AudioPipeline());
  const scrollRef = useRef<HTMLDivElement>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const recorderRef = useRef<MediaRecorder | null>(null);

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
        video: { width: { ideal: 1920 }, height: { ideal: 1080 }, frameRate: { ideal: 60 } },
      });
      const video = videoRef.current!;
      video.srcObject = stream;
      await video.play();

      // Use MediaRecorder with hardware H.264 encoder — no per-frame JPEG overhead
      const mimeType = MediaRecorder.isTypeSupported("video/mp4;codecs=avc1.42E01E")
        ? "video/mp4;codecs=avc1.42E01E"
        : MediaRecorder.isTypeSupported("video/webm;codecs=h264")
          ? "video/webm;codecs=h264"
          : "video/webm;codecs=vp8";

      console.log("[WEBCAM] MediaRecorder codec:", mimeType);

      const recorder = new MediaRecorder(stream, {
        mimeType,
        videoBitsPerSecond: 8_000_000, // 8 Mbps
      });

      recorder.ondataavailable = (e) => {
        if (e.data.size > 0 && ws.readyState === WebSocket.OPEN) {
          // Tag byte 0x02 = video chunk (0x01 = audio PCM)
          e.data.arrayBuffer().then((buf) => {
            const tagged = new Uint8Array(buf.byteLength + 1);
            tagged[0] = 0x02;
            tagged.set(new Uint8Array(buf), 1);
            ws.send(tagged.buffer);
          });
        }
      };

      recorder.start(100); // chunk every 100ms
      recorderRef.current = recorder;
    } catch (e) {
      console.error("[WEBCAM] Failed to start:", e);
    }
  }, []);

  const stopWebcam = useCallback(() => {
    if (recorderRef.current && recorderRef.current.state !== "inactive") {
      recorderRef.current.stop();
      recorderRef.current = null;
    }
    const video = videoRef.current;
    if (video?.srcObject) {
      (video.srcObject as MediaStream).getTracks().forEach((t) => t.stop());
      video.srcObject = null;
    }
  }, []);

  // ── Voice Cloning ──────────────────────────────────────

  const cloneVoice = useCallback(async () => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;

    setIsCloning(true);
    setCloneProgress(0);

    const CLONE_DURATION = 30; // seconds
    const SAMPLE_RATE = 44100;
    const chunks: Int16Array[] = [];

    // Capture mic audio for voice sample
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    const ctx = new AudioContext({ sampleRate: SAMPLE_RATE });
    const source = ctx.createMediaStreamSource(stream);
    const processor = ctx.createScriptProcessor(4096, 1, 1);

    processor.onaudioprocess = (e) => {
      const float32 = e.inputBuffer.getChannelData(0);
      const int16 = new Int16Array(float32.length);
      for (let i = 0; i < float32.length; i++) {
        int16[i] = Math.max(-32768, Math.min(32767, float32[i] * 32768));
      }
      chunks.push(int16);
    };

    source.connect(processor);
    processor.connect(ctx.destination);

    // Progress timer
    const startTime = Date.now();
    const progressInterval = window.setInterval(() => {
      const elapsed = (Date.now() - startTime) / 1000;
      setCloneProgress(Math.min(elapsed / CLONE_DURATION, 1));
    }, 200);

    // Wait for recording duration
    await new Promise((resolve) => setTimeout(resolve, CLONE_DURATION * 1000));

    clearInterval(progressInterval);
    processor.disconnect();
    stream.getTracks().forEach((t) => t.stop());
    ctx.close();

    // Combine chunks into single PCM buffer
    const totalSamples = chunks.reduce((sum, c) => sum + c.length, 0);
    const pcm = new Int16Array(totalSamples);
    let offset = 0;
    for (const chunk of chunks) {
      pcm.set(chunk, offset);
      offset += chunk.length;
    }

    // Convert to base64 and send
    const bytes = new Uint8Array(pcm.buffer);
    let binary = "";
    for (let i = 0; i < bytes.length; i++) {
      binary += String.fromCharCode(bytes[i]);
    }
    const base64 = btoa(binary);

    ws.send(JSON.stringify({ type: "voice:sample", audio: base64 }));
    setCloneProgress(1);
    // isCloning stays true until voice:ready arrives
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
        setIsCloning(false);
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

    // Start audio capture (tag byte 0x01 = audio PCM)
    await audioRef.current.start((buffer) => {
      if (ws.readyState === WebSocket.OPEN) {
        const tagged = new Uint8Array(buffer.byteLength + 1);
        tagged[0] = 0x01;
        tagged.set(new Uint8Array(buffer), 1);
        ws.send(tagged.buffer);
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
              {voiceReady ? (
                <span className="text-xs text-secondary ml-2">Voice cloned</span>
              ) : !isCloning ? (
                <button
                  onClick={cloneVoice}
                  className="ml-2 px-3 py-1 rounded-lg bg-secondary-container text-on-secondary-container text-xs font-semibold hover:opacity-90 transition-opacity"
                >
                  Clone Voice
                </button>
              ) : (
                <div className="ml-2 flex items-center gap-2">
                  <div className="w-24 h-1.5 bg-surface-container-high rounded-full overflow-hidden">
                    <div
                      className="h-full bg-secondary rounded-full transition-all duration-200"
                      style={{ width: `${cloneProgress * 100}%` }}
                    />
                  </div>
                  <span className="text-xs text-outline">
                    {cloneProgress < 1 ? `${Math.round(cloneProgress * 30)}s / 30s` : "Cloning..."}
                  </span>
                </div>
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
