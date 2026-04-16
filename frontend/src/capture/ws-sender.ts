import { encodeAudioFrame, encodeProtocolMessage, encodeVideoChunk } from "../protocol/encode";
import { ChunkKind, MessageType } from "../protocol/types";
import { AUDIO_CHANNELS, AUDIO_FRAME_DURATION_MS, SAMPLE_RATE } from "../core/audio/pcm";

export class WsSender {
  private ws: WebSocket | null = null;
  private audioSeq = 0n;
  private videoSeq = 0n;

  async connect(url: string, metadataJson: string): Promise<void> {
    const ws = new WebSocket(url);
    ws.binaryType = "arraybuffer";
    await new Promise<void>((resolve, reject) => {
      ws.onopen = () => resolve();
      ws.onerror = () => reject(new Error("websocket open failed"));
    });
    this.ws = ws;
    this.ws.send(
      encodeProtocolMessage({
        type: MessageType.SessionInit,
        version: 1,
        flags: 0b11,
        audioSampleRate: SAMPLE_RATE,
        audioChannels: AUDIO_CHANNELS,
        audioFrameDurationMs: AUDIO_FRAME_DURATION_MS,
        videoTimescale: 1000,
        sessionStartUnixMs: BigInt(Date.now()),
        metadataJson,
      }),
    );
  }

  sendAudioFrame(frame: { captureTsMs: bigint; durationMs: number; payload: ArrayBuffer }): void {
    this.ensureOpen();
    this.ws!.send(
      encodeAudioFrame({
        type: MessageType.AudioFrame,
        seq: this.audioSeq++,
        captureTsMs: frame.captureTsMs,
        durationMs: frame.durationMs,
        payload: frame.payload,
      }),
    );
  }

  sendVideoChunk(chunk: {
    captureTsMs: bigint;
    durationMs: number;
    isKeyframe: boolean;
    chunkKind: ChunkKind;
    payload: ArrayBuffer;
  }): void {
    this.ensureOpen();
    this.ws!.send(
      encodeVideoChunk({
        type: MessageType.VideoChunk,
        seq: this.videoSeq++,
        captureTsMs: chunk.captureTsMs,
        durationMs: chunk.durationMs,
        isKeyframe: chunk.isKeyframe,
        chunkKind: chunk.chunkKind,
        payload: chunk.payload,
      }),
    );
  }

  close(): void {
    this.ws?.close();
    this.ws = null;
  }

  private ensureOpen(): void {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      throw new Error("websocket not open");
    }
  }
}
