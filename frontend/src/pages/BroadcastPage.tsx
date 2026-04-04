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

// ── Persistence ──────────────────────────────────────────

const STORAGE_KEY = "brivva_config";

function loadConfig(): {
  sourceLang: string; targetLangs: string[]; tier: TranslationTier;
  rtmpUrls: Record<string, string>; broadcastDelay: number;
  videoDeviceId: string; audioDeviceId: string;
  ttsModel: "turbo" | "flash";
} {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return { ...defaultConfig(), ...JSON.parse(raw) };
  } catch {}
  return defaultConfig();
}

function defaultConfig() {
  return {
    sourceLang: "en", targetLangs: ["ja", "ko"] as string[], tier: 2 as TranslationTier,
    rtmpUrls: {} as Record<string, string>, broadcastDelay: 5000,
    videoDeviceId: "", audioDeviceId: "", ttsModel: "turbo" as const,
  };
}

function saveConfig(cfg: ReturnType<typeof loadConfig>) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(cfg));
}

// ── Component ────────────────────────────────────────────

export default function BroadcastPage() {
  const saved = loadConfig();
  const [sourceLang, setSourceLang] = useState(saved.sourceLang);
  const [targetLangs, setTargetLangs] = useState<string[]>(saved.targetLangs);
  const [tier, setTier] = useState<TranslationTier>(saved.tier);
  const [isLive, setIsLive] = useState(false);
  const [interim, setInterim] = useState("");
  const [transcripts, setTranscripts] = useState<TranscriptEntry[]>([]);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [voiceReady, setVoiceReady] = useState(false);
  const [rtmpUrls, setRtmpUrls] = useState<Record<string, string>>(saved.rtmpUrls);
  const [broadcastDelay, setBroadcastDelay] = useState(saved.broadcastDelay);
  const [videoDeviceId, setVideoDeviceId] = useState(saved.videoDeviceId);
  const [audioDeviceId, setAudioDeviceId] = useState(saved.audioDeviceId);
  const [devices, setDevices] = useState<MediaDeviceInfo[]>([]);
  const [ttsModel, setTtsModel] = useState<"turbo" | "flash">(saved.ttsModel);
  const [showSettings, setShowSettings] = useState(false);
  const [errors, setErrors] = useState<string[]>([]);
  const [isCloning, setIsCloning] = useState(false);
  const [cloneProgress, setCloneProgress] = useState(0);

  const wsRef = useRef<WebSocket | null>(null);
  const audioRef = useRef(new AudioPipeline());
  const scrollRef = useRef<HTMLDivElement>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const recorderRef = useRef<MediaRecorder | null>(null);

  // Persist config on change
  useEffect(() => {
    saveConfig({ sourceLang, targetLangs, tier, rtmpUrls, broadcastDelay, videoDeviceId, audioDeviceId, ttsModel });
  }, [sourceLang, targetLangs, tier, rtmpUrls, broadcastDelay, videoDeviceId, audioDeviceId, ttsModel]);

  // Enumerate media devices (request permission first to reveal labels)
  useEffect(() => {
    (async () => {
      try {
        const stream = await navigator.mediaDevices.getUserMedia({ audio: true, video: true });
        stream.getTracks().forEach((t) => t.stop());
      } catch {}
      const devs = await navigator.mediaDevices.enumerateDevices();
      setDevices(devs);
    })();
  }, []);

  // Check if a persisted voice clone exists on mount
  useEffect(() => {
    fetch("http://localhost:3000/api/voice")
      .then((r) => r.json())
      .then((data) => { if (data.active) setVoiceReady(true); })
      .catch(() => {});
  }, []);

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
      const videoConstraints: MediaTrackConstraints = {
        width: { ideal: 1920 }, height: { ideal: 1080 }, frameRate: { ideal: 60 },
        ...(videoDeviceId ? { deviceId: { exact: videoDeviceId } } : {}),
      };
      const stream = await navigator.mediaDevices.getUserMedia({ video: videoConstraints });
      const video = videoRef.current!;
      video.srcObject = stream;
      await video.play();

      // Use MediaRecorder with hardware H.264 encoder — no per-frame JPEG overhead
      const mimeType = MediaRecorder.isTypeSupported("video/mp4;codecs=avc1.42E01E")
        ? "video/mp4;codecs=avc1.42E01E"
        : MediaRecorder.isTypeSupported("video/webm;codecs=h264")
          ? "video/webm;codecs=h264"
          : "video/webm;codecs=vp8";

      const isH264 = mimeType.includes("avc1") || mimeType.includes("h264");
      console.log("[WEBCAM] MediaRecorder codec:", mimeType, isH264 ? "(H.264 — passthrough)" : "(re-encode)");

      // Notify backend of video codec for FFmpeg passthrough decision
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: "video:codec", codec: isH264 ? "h264" : "vp8", mimeType }));
      }

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
  }, [videoDeviceId]);

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
    setIsCloning(true);
    setCloneProgress(0);
    setVoiceReady(false);

    const CLONE_DURATION = 30;
    const SAMPLE_RATE = 44100;
    const chunks: Int16Array[] = [];

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

    const startTime = Date.now();
    const progressInterval = window.setInterval(() => {
      const elapsed = (Date.now() - startTime) / 1000;
      setCloneProgress(Math.min(elapsed / CLONE_DURATION, 1));
    }, 200);

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

    setCloneProgress(1);

    // POST raw PCM to REST endpoint
    try {
      const resp = await fetch("http://localhost:3000/api/voice/clone", {
        method: "POST",
        headers: { "Content-Type": "application/octet-stream" },
        body: new Uint8Array(pcm.buffer),
      });
      if (resp.ok) {
        setVoiceReady(true);
      } else {
        const err = await resp.text();
        setErrors((prev) => [...prev, `Voice clone failed: ${err}`]);
      }
    } catch (e) {
      setErrors((prev) => [...prev, `Voice clone failed: ${e}`]);
    }
    setIsCloning(false);
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
      case "error":
        setErrors((prev) => [...prev, msg.message]);
        break;
    }
  }, []);

  // ── Start / Stop ──────────────────────────────────────

  const start = useCallback(async () => {
    const available = targetLangs.filter((l) => l !== sourceLang);
    if (available.length === 0) return;

    const ws = new WebSocket(
      `ws://localhost:3000/ws?sourceLang=${sourceLang}&targetLangs=${available.join(",")}&tier=${tier}&ttsModel=${ttsModel}`
    );
    ws.binaryType = "arraybuffer";
    wsRef.current = ws;

    ws.onmessage = handleWsMessage;

    // Wait for connection — show error only if connect fails
    try {
      await new Promise<void>((resolve, reject) => {
        ws.onopen = () => resolve();
        ws.onerror = () => reject();
      });
    } catch {
      setErrors((prev) => [...prev, "Cannot connect — is the backend running on localhost:3000?"]);
      wsRef.current = null;
      return;
    }

    // Connected — set up runtime handlers (no misleading "backend running?" errors)
    ws.onclose = (e) => {
      if (e.code !== 1000) {
        setErrors((prev) => [...prev, `Connection lost (code ${e.code}). Restart to reconnect.`]);
      }
      setIsLive(false);
      setSessionId(null);
    };
    ws.onerror = () => {}; // onclose handles all runtime errors

    // Send RTMP config if any URLs are set
    const streams = Object.entries(rtmpUrls)
      .filter(([, url]) => url.trim())
      .map(([lang, url]) => ({ lang, url: url.trim() }));

    if (streams.length > 0) {
      // Start webcam first — reports video:codec to backend before FFmpeg spawns
      await startWebcam(ws);
      // Then start RTMP streams (uses reported codec for passthrough decision)
      ws.send(JSON.stringify({ type: "rtmp:config", streams, broadcastDelay }));
    }

    // Start audio capture (tag byte 0x01 = audio PCM)
    await audioRef.current.start((buffer) => {
      if (ws.readyState === WebSocket.OPEN) {
        const tagged = new Uint8Array(buffer.byteLength + 1);
        tagged[0] = 0x01;
        tagged.set(new Uint8Array(buffer), 1);
        ws.send(tagged.buffer);
      }
    }, audioDeviceId || undefined);

    setIsLive(true);
    setTranscripts([]);
    setInterim("");
    setVoiceReady(false);
  }, [sourceLang, targetLangs, tier, rtmpUrls, broadcastDelay, audioDeviceId, handleWsMessage, startWebcam]);

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

        {/* Error Banners */}
        {errors.map((err, i) => (
          <div
            key={i}
            className="flex items-center justify-between bg-error-container text-on-error-container rounded-lg px-4 py-2 text-sm"
          >
            <span>{err}</span>
            <button
              onClick={() => setErrors((prev) => prev.filter((_, j) => j !== i))}
              className="ml-4 text-on-error-container/60 hover:text-on-error-container text-lg leading-none"
            >
              x
            </button>
          </div>
        ))}

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

        {/* Settings */}
        <div className="bg-surface-container-low rounded-xl overflow-hidden">
          <button
            onClick={() => setShowSettings((p) => !p)}
            className="w-full px-4 py-3 flex items-center justify-between text-sm font-semibold text-on-surface-variant uppercase tracking-wider hover:bg-surface-container transition-colors"
          >
            Settings
            <span className="text-xs text-outline">{showSettings ? "Hide" : "Show"}</span>
          </button>
          {showSettings && (
            <div className="px-4 pb-4 space-y-4 border-t border-outline-variant">
              {/* Broadcast Delay */}
              <div className="pt-3 space-y-1">
                <label className="text-sm text-on-surface-variant">
                  Broadcast Delay: {(broadcastDelay / 1000).toFixed(1)}s
                </label>
                <input
                  type="range"
                  min={1000}
                  max={10000}
                  step={500}
                  disabled={isLive}
                  value={broadcastDelay}
                  onChange={(e) => setBroadcastDelay(Number(e.target.value))}
                  className="w-full accent-primary"
                />
                <p className="text-xs text-outline">
                  Higher = more time for TTS, lower = less stream latency
                </p>
              </div>

              {/* Camera Selection */}
              <div className="space-y-1">
                <label className="text-sm text-on-surface-variant">Camera</label>
                <select
                  disabled={isLive}
                  value={videoDeviceId}
                  onChange={(e) => setVideoDeviceId(e.target.value)}
                  className="w-full px-3 py-2 rounded-lg bg-surface-container border border-outline-variant text-sm text-on-surface disabled:opacity-50"
                >
                  <option value="">Default</option>
                  {devices.filter((d) => d.kind === "videoinput").map((d) => (
                    <option key={d.deviceId} value={d.deviceId}>
                      {d.label || `Camera ${d.deviceId.slice(0, 8)}`}
                    </option>
                  ))}
                </select>
              </div>

              {/* Microphone Selection */}
              <div className="space-y-1">
                <label className="text-sm text-on-surface-variant">Microphone</label>
                <select
                  disabled={isLive}
                  value={audioDeviceId}
                  onChange={(e) => setAudioDeviceId(e.target.value)}
                  className="w-full px-3 py-2 rounded-lg bg-surface-container border border-outline-variant text-sm text-on-surface disabled:opacity-50"
                >
                  <option value="">Default</option>
                  {devices.filter((d) => d.kind === "audioinput").map((d) => (
                    <option key={d.deviceId} value={d.deviceId}>
                      {d.label || `Mic ${d.deviceId.slice(0, 8)}`}
                    </option>
                  ))}
                </select>
              </div>

              {/* TTS Model */}
              <div className="space-y-1">
                <label className="text-sm text-on-surface-variant">TTS Model</label>
                <div className="flex gap-2">
                  <button
                    disabled={isLive}
                    onClick={() => setTtsModel("turbo")}
                    className={`flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-colors disabled:opacity-50 ${
                      ttsModel === "turbo"
                        ? "bg-primary text-on-primary"
                        : "bg-surface-container border border-outline-variant text-on-surface-variant hover:bg-surface-container-high"
                    }`}
                  >
                    Expressive
                    <span className="block text-xs opacity-70">~300ms</span>
                  </button>
                  <button
                    disabled={isLive}
                    onClick={() => setTtsModel("flash")}
                    className={`flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-colors disabled:opacity-50 ${
                      ttsModel === "flash"
                        ? "bg-primary text-on-primary"
                        : "bg-surface-container border border-outline-variant text-on-surface-variant hover:bg-surface-container-high"
                    }`}
                  >
                    Fast
                    <span className="block text-xs opacity-70">~75ms</span>
                  </button>
                </div>
              </div>
            </div>
          )}
        </div>

        {/* Voice Cloning (pre-broadcast step) */}
        {!isLive && tier >= 2 && (
          <div className="bg-surface-container-low rounded-xl p-4 space-y-3">
            <div className="flex items-center justify-between">
              <h2 className="font-headline text-sm font-semibold text-on-surface-variant uppercase tracking-wider">
                Voice Cloning
              </h2>
              {voiceReady && (
                <span className="text-xs text-secondary font-medium px-2 py-0.5 bg-secondary-container rounded-full">
                  Active
                </span>
              )}
            </div>
            {isCloning ? (
              <div className="space-y-3">
                <div className="bg-surface-container rounded-lg p-3">
                  <p className="text-sm text-on-surface leading-relaxed italic">
                    "The sun went down and the sky turned orange and purple. Below, the city lights began
                    to flicker on. The busy noise of the day, cars honking and people rushing, started to
                    fade away. It was finally quiet. While most people were going home to eat dinner and
                    rest, some were just starting their work."
                  </p>
                </div>
                <div className="flex items-center gap-3">
                  <div className="flex-1 h-2 bg-surface-container-high rounded-full overflow-hidden">
                    <div
                      className="h-full bg-secondary rounded-full transition-all duration-200"
                      style={{ width: `${cloneProgress * 100}%` }}
                    />
                  </div>
                  <span className="text-sm text-on-surface-variant font-mono w-24 text-right">
                    {cloneProgress < 1 ? `${Math.round(cloneProgress * 30)}s / 30s` : "Uploading..."}
                  </span>
                </div>
                {cloneProgress < 1 && (
                  <p className="text-xs text-outline">Read the text above naturally into your microphone.</p>
                )}
              </div>
            ) : (
              <div className="space-y-2">
                <p className="text-xs text-outline">
                  {voiceReady
                    ? "Your cloned voice will be used for all translated speech."
                    : "Record 30 seconds of your voice. All translations will use your cloned voice instead of the default."}
                </p>
                <button
                  onClick={cloneVoice}
                  className="px-4 py-2 rounded-lg bg-secondary-container text-on-secondary-container text-sm font-semibold hover:opacity-90 transition-opacity"
                >
                  {voiceReady ? "Re-clone Voice" : "Start Recording (30s)"}
                </button>
              </div>
            )}
          </div>
        )}

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
                <>
                  <span className="text-xs text-primary ml-2">RTMP</span>
                  <button
                    onClick={() => wsRef.current?.send(JSON.stringify({ type: "rtmp:restart" }))}
                    className="ml-2 px-2 py-1 rounded bg-surface-container border border-outline-variant text-on-surface-variant text-xs font-medium hover:bg-surface-container-high transition-colors"
                  >
                    Restart Stream
                  </button>
                </>
              )}
              {voiceReady && (
                <span className="text-xs text-secondary ml-2">Voice cloned</span>
              )}
            </div>
          )}
        </div>

        {/* Voice Cloning (pre-broadcast only) */}

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
