import { AudioCapture } from "./capture/audio-capture";
import { SessionClock } from "./capture/session-clock";
import { VideoCapture } from "./capture/video-capture";
import { WsSender } from "./capture/ws-sender";
import { ChunkKind } from "./protocol/types";

const startBtn = document.querySelector<HTMLButtonElement>("#start")!;
const stopBtn = document.querySelector<HTMLButtonElement>("#stop")!;
const preview = document.querySelector<HTMLVideoElement>("#preview")!;
const logEl = document.querySelector<HTMLPreElement>("#log")!;
const wsBaseEl = document.querySelector<HTMLInputElement>("#ws-base")!;
const outputUrlEl = document.querySelector<HTMLInputElement>("#output-url")!;
const delayMsEl = document.querySelector<HTMLInputElement>("#delay-ms")!;

let sender: WsSender | null = null;
let audioCapture: AudioCapture | null = null;
let videoCapture: VideoCapture | null = null;

function log(line: string): void {
  logEl.textContent = `${new Date().toISOString()} ${line}\n${logEl.textContent ?? ""}`.slice(0, 4000);
}

startBtn.onclick = async () => {
  if (sender) {
    log("session already running");
    return;
  }

  const outputUrl = outputUrlEl.value.trim();
  if (!outputUrl) {
    log("missing output url");
    return;
  }

  const delayMs = Number(delayMsEl.value || "1000");
  const wsBase = wsBaseEl.value.trim();
  const wsUrl = `${wsBase}?output_url=${encodeURIComponent(outputUrl)}&delay_ms=${encodeURIComponent(String(delayMs))}`;

  const clock = new SessionClock();
  sender = new WsSender();
  audioCapture = new AudioCapture(clock);
  videoCapture = new VideoCapture(clock);

  try {
    await sender.connect(wsUrl, JSON.stringify({
      video_codec: "vp8",
      video_container: "webm",
      audio_codec: "pcm_s16le",
      source_lang: "en",
    }));
    await audioCapture.start((frame) => {
      sender?.sendAudioFrame(frame);
    });
    await videoCapture.start(preview, (chunk) => {
      sender?.sendVideoChunk({
        ...chunk,
        chunkKind: chunk.chunkKind === 0 ? ChunkKind.Init : ChunkKind.Media,
      });
    });
    log("stream started");
  } catch (error) {
    log(`start failed: ${String(error)}`);
    audioCapture?.stop();
    videoCapture?.stop();
    sender?.close();
    sender = null;
    audioCapture = null;
    videoCapture = null;
  }
};

stopBtn.onclick = () => {
  audioCapture?.stop();
  videoCapture?.stop();
  sender?.close();
  sender = null;
  audioCapture = null;
  videoCapture = null;
  preview.srcObject = null;
  log("stream stopped");
};
