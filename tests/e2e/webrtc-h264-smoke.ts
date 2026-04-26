import { spawnSync } from "node:child_process";
import { chromium, type Browser, type Page } from "playwright";
import { assertRtmpStreams, type ProbeStream } from "./media-assertions";

const SERVER_URL = process.env.SERVER_URL ?? "http://localhost:3000";
const WS_URL = process.env.WS_URL ?? "ws://localhost:3000/api/session";
const RTMP_URL = process.env.RTMP_URL ?? "rtmp://localhost:1935/live/smoke-ja";
const JWT_SECRET = process.env.JWT_SECRET ?? "smoke-jwt-secret";
const SESSION_ID = process.env.SESSION_ID ?? "SMOKE001";
const USER_ID = process.env.USER_ID ?? "smoke-user";
const STREAM_DURATION_MS = Number(process.env.STREAM_DURATION_MS ?? 35_000);
const PROBE_AT_MS = Number(process.env.PROBE_AT_MS ?? 18_000);

const SAMPLE_RATE = 44_100;
const FRAME_MS = 20;
const SAMPLES_PER_FRAME = (SAMPLE_RATE * FRAME_MS) / 1000;

type WsSignals = {
  sawTranslation: boolean;
  sawTtsEnd: boolean;
  probedStreams: ProbeStream[];
  probeError: string | null;
  whipError: string | null;
  peerStats: PeerStats | null;
};

type PeerStats = {
  connectionState: RTCPeerConnectionState;
  iceConnectionState: RTCIceConnectionState;
  iceGatheringState: RTCIceGatheringState;
  signalingState: RTCSignalingState;
  selectedCandidatePair?: {
    state?: string;
    nominated?: boolean;
    localCandidateId?: string;
    remoteCandidateId?: string;
    bytesSent?: number;
    packetsSent?: number;
  };
  outboundVideo?: {
    bytesSent?: number;
    framesEncoded?: number;
    packetsSent?: number;
    framesPerSecond?: number;
    qualityLimitationReason?: string;
  };
};

function b64url(input: string | Uint8Array): string {
  const bytes = typeof input === "string" ? new TextEncoder().encode(input) : input;
  return Buffer.from(bytes)
    .toString("base64")
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

async function mintJwt(sub: string, secret: string): Promise<string> {
  const header = { alg: "HS256", typ: "JWT" };
  const payload = {
    sub,
    iss: "brivva-api",
    aud: "brivva-fargate",
    exp: Math.floor(Date.now() / 1000) + 900,
  };
  const signingInput = `${b64url(JSON.stringify(header))}.${b64url(JSON.stringify(payload))}`;
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const sig = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(signingInput));
  return `${signingInput}.${b64url(new Uint8Array(sig))}`;
}

function generateSineFrame(freqHz: number, phase: number): { pcm: Buffer; nextPhase: number } {
  const buf = Buffer.alloc(SAMPLES_PER_FRAME * 2);
  const step = (2 * Math.PI * freqHz) / SAMPLE_RATE;
  let p = phase;
  for (let i = 0; i < SAMPLES_PER_FRAME; i++) {
    const sample = Math.round(Math.sin(p) * 12_000);
    buf.writeInt16LE(sample, i * 2);
    p += step;
  }
  return { pcm: buf, nextPhase: p % (2 * Math.PI) };
}

async function waitForHealth(url: string, timeoutMs = 60_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const resp = await fetch(`${url}/health`);
      if (resp.ok) return;
    } catch {
      // retry until deadline
    }
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(`server /health not ready after ${timeoutMs}ms`);
}

function probeRtmp(url: string): ProbeStream[] {
  const result = spawnSync("ffprobe", [
    "-v",
    "error",
    "-rw_timeout",
    "10000000",
    "-print_format",
    "json",
    "-show_streams",
    url,
  ], { encoding: "utf-8", timeout: 15_000 });
  if (result.status !== 0) {
    throw new Error(`ffprobe failed: ${result.stderr}`);
  }
  return (JSON.parse(result.stdout) as { streams?: ProbeStream[] }).streams ?? [];
}

async function startWhip(page: Page, token: string): Promise<void> {
  await page.goto(`${SERVER_URL}/health`);
  await page.evaluate(
    async ({ serverUrl, token, sessionId }) => {
      const canvas = document.createElement("canvas");
      canvas.width = 1280;
      canvas.height = 720;
      document.body.appendChild(canvas);
      const ctx = canvas.getContext("2d");
      if (!ctx) throw new Error("2d canvas unavailable");

      let frame = 0;
      const draw = () => {
        const hue = frame % 360;
        ctx.fillStyle = `hsl(${hue}, 70%, 35%)`;
        ctx.fillRect(0, 0, canvas.width, canvas.height);
        ctx.fillStyle = "white";
        ctx.font = "64px sans-serif";
        ctx.fillText(`BRIVVA WHIP ${frame}`, 80, 160);
        ctx.fillStyle = "rgba(255,255,255,0.35)";
        ctx.fillRect((frame * 17) % canvas.width, 300, 220, 140);
        frame += 1;
      };
      draw();
      const timer = setInterval(draw, 33);

      const stream = canvas.captureStream(30);
      const pc = new RTCPeerConnection();
      const [track] = stream.getVideoTracks();
      if (!track) throw new Error("canvas capture produced no video track");
      const sender = pc.addTrack(track, stream);

      const caps = RTCRtpSender.getCapabilities?.("video");
      const h264 = caps?.codecs.filter((codec) => codec.mimeType.toLowerCase() === "video/h264") ?? [];
      if (!h264.length) throw new Error("Chromium did not expose H.264 WebRTC encoding");
      const transceiver = pc.getTransceivers().find((item) => item.sender.track?.kind === "video");
      transceiver?.setCodecPreferences?.(h264);

      const params = sender.getParameters();
      params.encodings = params.encodings?.length ? params.encodings : [{}];
      params.encodings[0] = {
        ...params.encodings[0],
        maxBitrate: 4_000_000,
        maxFramerate: 30,
      };
      await sender.setParameters(params);

      pc.addEventListener("iceconnectionstatechange", () => {
        console.log(`[webrtc-h264-page] ice=${pc.iceConnectionState}`);
      });
      pc.addEventListener("connectionstatechange", () => {
        console.log(`[webrtc-h264-page] connection=${pc.connectionState}`);
      });

      const offer = await pc.createOffer({ offerToReceiveAudio: false, offerToReceiveVideo: false });
      await pc.setLocalDescription(offer);
      if (pc.iceGatheringState !== "complete") {
        await new Promise<void>((resolve) => {
          const done = () => {
            if (pc.iceGatheringState === "complete") {
              pc.removeEventListener("icegatheringstatechange", done);
              resolve();
            }
          };
          pc.addEventListener("icegatheringstatechange", done);
          setTimeout(resolve, 3000);
        });
      }

      const local = pc.localDescription;
      if (!local?.sdp) throw new Error("WebRTC offer SDP missing");
      const url = new URL(`${serverUrl}/whip/session`);
      url.searchParams.set("token", token);
      url.searchParams.set("sourceLang", "en");
      url.searchParams.set("videoMode", "webrtc-h264");
      url.searchParams.set("sessionId", sessionId);

      const resp = await fetch(url, {
        method: "POST",
        headers: { "content-type": "application/sdp" },
        body: local.sdp,
      });
      if (!resp.ok) throw new Error(`WHIP ${resp.status}: ${await resp.text()}`);
      await pc.setRemoteDescription({ type: "answer", sdp: await resp.text() });

      (window as unknown as {
        __brivvaWhip?: { pc: RTCPeerConnection; stream: MediaStream; timer: number };
      }).__brivvaWhip = { pc, stream, timer };
    },
    { serverUrl: SERVER_URL, token, sessionId: SESSION_ID },
  );
}

async function readPeerStats(page: Page): Promise<PeerStats | null> {
  return await page.evaluate(async () => {
    const state = (window as unknown as {
      __brivvaWhip?: { pc: RTCPeerConnection; stream: MediaStream; timer: number };
    }).__brivvaWhip;
    if (!state) return null;
    const stats = await state.pc.getStats();
    const result: PeerStats = {
      connectionState: state.pc.connectionState,
      iceConnectionState: state.pc.iceConnectionState,
      iceGatheringState: state.pc.iceGatheringState,
      signalingState: state.pc.signalingState,
    };
    for (const item of stats.values()) {
      if (item.type === "candidate-pair" && (item as RTCIceCandidatePairStats).selected) {
        const pair = item as RTCIceCandidatePairStats;
        result.selectedCandidatePair = {
          state: pair.state,
          nominated: pair.nominated,
          localCandidateId: pair.localCandidateId,
          remoteCandidateId: pair.remoteCandidateId,
          bytesSent: pair.bytesSent,
          packetsSent: pair.packetsSent,
        };
      }
      if (item.type === "outbound-rtp" && (item as RTCOutboundRtpStreamStats).kind === "video") {
        const outbound = item as RTCOutboundRtpStreamStats;
        result.outboundVideo = {
          bytesSent: outbound.bytesSent,
          framesEncoded: outbound.framesEncoded,
          packetsSent: outbound.packetsSent,
          framesPerSecond: outbound.framesPerSecond,
          qualityLimitationReason: outbound.qualityLimitationReason,
        };
      }
    }
    return result;
  }).catch(() => null);
}

async function stopWhip(page: Page): Promise<void> {
  await page.evaluate(() => {
    const state = (window as unknown as {
      __brivvaWhip?: { pc: RTCPeerConnection; stream: MediaStream; timer: number };
    }).__brivvaWhip;
    if (!state) return;
    clearInterval(state.timer);
    state.stream.getTracks().forEach((track) => track.stop());
    state.pc.close();
  }).catch(() => {});
}

async function streamAudioAndWhip(wsUrl: string, rtmpUrl: string, page: Page, token: string): Promise<WsSignals> {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(wsUrl);
    let phase = 0;
    let sawTranslation = false;
    let sawTtsEnd = false;
    let probedStreams: ProbeStream[] = [];
    let probeError: string | null = null;
    let whipError: string | null = null;
    let peerStats: PeerStats | null = null;
    let audioTimer: ReturnType<typeof setInterval> | null = null;
    let stopTimer: ReturnType<typeof setTimeout> | null = null;
    let probeTimer: ReturnType<typeof setTimeout> | null = null;
    let whipTimer: ReturnType<typeof setTimeout> | null = null;

    const cleanup = () => {
      if (audioTimer) clearInterval(audioTimer);
      if (stopTimer) clearTimeout(stopTimer);
      if (probeTimer) clearTimeout(probeTimer);
      if (whipTimer) clearTimeout(whipTimer);
    };

    ws.addEventListener("open", () => {
      console.log("[webrtc-h264] WS open, streaming for", STREAM_DURATION_MS, "ms");
      audioTimer = setInterval(() => {
        if (ws.readyState !== WebSocket.OPEN) return;
        const { pcm, nextPhase } = generateSineFrame(440, phase);
        phase = nextPhase;
        ws.send(pcm);
      }, FRAME_MS);

      whipTimer = setTimeout(() => {
        startWhip(page, token).catch((err) => {
          whipError = err instanceof Error ? err.message : String(err);
          try {
            ws.close();
          } catch {
            // ignore
          }
        });
      }, 1500);

      probeTimer = setTimeout(() => {
        console.log(`[webrtc-h264] probing RTMP mid-stream: ${rtmpUrl}`);
        try {
          probedStreams = probeRtmp(rtmpUrl);
        } catch (err) {
          probeError = err instanceof Error ? err.message : String(err);
        }
        readPeerStats(page).then((stats) => {
          peerStats = stats;
          console.log("[webrtc-h264] peer stats:", JSON.stringify(stats));
        });
      }, PROBE_AT_MS);

      stopTimer = setTimeout(() => {
        try {
          ws.send(JSON.stringify({ type: "host:end" }));
        } catch {
          // ignore
        }
        setTimeout(() => ws.close(), 500);
      }, STREAM_DURATION_MS);
    });

    ws.addEventListener("message", (event) => {
      if (typeof event.data !== "string") return;
      try {
        const msg = JSON.parse(event.data) as { type?: string; targetLang?: string };
        if (msg.type === "translation" && msg.targetLang === "ja") sawTranslation = true;
        if (msg.type === "tts_end" && msg.targetLang === "ja") sawTtsEnd = true;
      } catch {
        // ignore non-json frames
      }
    });

    ws.addEventListener("close", () => {
      cleanup();
      resolve({ sawTranslation, sawTtsEnd, probedStreams, probeError, whipError, peerStats });
    });

    ws.addEventListener("error", (err) => {
      cleanup();
      reject(err);
    });
  });
}

async function main() {
  console.log("[webrtc-h264] wait for server /health ...");
  await waitForHealth(SERVER_URL);
  const token = await mintJwt(USER_ID, JWT_SECRET);
  const wsUrl =
    `${WS_URL}?token=${encodeURIComponent(token)}&sourceLang=en&videoMode=webrtc-h264&sessionId=${SESSION_ID}`;

  let browser: Browser | null = null;
  try {
    browser = await chromium.launch({ headless: true });
    const page = await browser.newPage();
    const signals = await streamAudioAndWhip(wsUrl, RTMP_URL, page, token);
    const finalPeerStats = await readPeerStats(page);
    await stopWhip(page);

    if (signals.whipError) throw new Error(`WHIP failed: ${signals.whipError}`);
    if (!signals.sawTranslation) throw new Error("smoke FAIL - no translation event observed for ja");
    if (!signals.sawTtsEnd) throw new Error("smoke FAIL - no tts_end event observed for ja");
    if (signals.probeError) {
      throw new Error(
        `smoke FAIL - RTMP ffprobe error: ${signals.probeError}; peerStats=${
          JSON.stringify(finalPeerStats ?? signals.peerStats)
        }`,
      );
    }

    assertRtmpStreams(signals.probedStreams, "webrtc h264 smoke");
    const audio = signals.probedStreams.find((stream) => stream.codec_type === "audio");
    const video = signals.probedStreams.find((stream) => stream.codec_type === "video");
    console.log(`[webrtc-h264] OK audio=${audio?.codec_name} video=${video?.codec_name}`);
  } finally {
    await browser?.close();
  }
}

main().catch((err) => {
  console.error("[webrtc-h264] ERROR", err);
  process.exit(1);
});
