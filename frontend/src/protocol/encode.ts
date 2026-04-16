import { ChunkKind, MessageType, type AudioFrame, type Ping, type SessionInit, type StreamEnd, type VideoChunk } from "./types";

const TEXT_ENCODER = new TextEncoder();

function writeType(view: DataView, type: MessageType): void {
  view.setUint8(0, type);
}

function copyPayload(target: ArrayBuffer, offset: number, payload: ArrayBuffer): void {
  new Uint8Array(target, offset).set(new Uint8Array(payload));
}

export function encodeSessionInit(message: SessionInit): ArrayBuffer {
  const metadata = TEXT_ENCODER.encode(message.metadataJson);
  const buffer = new ArrayBuffer(1 + 24 + metadata.byteLength);
  const view = new DataView(buffer);

  writeType(view, MessageType.SessionInit);
  view.setUint16(1, message.version, true);
  view.setUint16(3, message.flags, true);
  view.setUint32(5, message.audioSampleRate, true);
  view.setUint16(9, message.audioChannels, true);
  view.setUint16(11, message.audioFrameDurationMs, true);
  view.setUint16(13, message.videoTimescale, true);
  view.setUint16(15, 0, true);
  view.setBigUint64(17, message.sessionStartUnixMs, true);
  new Uint8Array(buffer, 25).set(metadata);

  return buffer;
}

export function encodeAudioFrame(message: AudioFrame): ArrayBuffer {
  const payloadSize = message.payload.byteLength;
  const buffer = new ArrayBuffer(1 + 24 + payloadSize);
  const view = new DataView(buffer);

  writeType(view, MessageType.AudioFrame);
  view.setBigUint64(1, message.seq, true);
  view.setBigUint64(9, message.captureTsMs, true);
  view.setUint32(17, message.durationMs, true);
  view.setUint32(21, payloadSize, true);
  copyPayload(buffer, 25, message.payload);

  return buffer;
}

export function encodeVideoChunk(message: VideoChunk): ArrayBuffer {
  const payloadSize = message.payload.byteLength;
  const buffer = new ArrayBuffer(1 + 28 + payloadSize);
  const view = new DataView(buffer);

  writeType(view, MessageType.VideoChunk);
  view.setBigUint64(1, message.seq, true);
  view.setBigUint64(9, message.captureTsMs, true);
  view.setUint32(17, message.durationMs, true);
  view.setUint8(21, message.isKeyframe ? 1 : 0);
  view.setUint8(22, message.chunkKind);
  view.setUint16(23, 0, true);
  view.setUint32(25, payloadSize, true);
  copyPayload(buffer, 29, message.payload);

  return buffer;
}

export function encodeStreamEnd(message: StreamEnd): ArrayBuffer {
  const buffer = new ArrayBuffer(1 + 8);
  const view = new DataView(buffer);

  writeType(view, MessageType.StreamEnd);
  view.setUint32(1, message.reasonCode, true);
  view.setUint32(5, 0, true);

  return buffer;
}

export function encodePing(message: Ping): ArrayBuffer {
  const buffer = new ArrayBuffer(1 + 8);
  const view = new DataView(buffer);

  writeType(view, MessageType.Ping);
  view.setBigUint64(1, message.clientTimeMs, true);

  return buffer;
}

export function encodeProtocolMessage(
  message: SessionInit | AudioFrame | VideoChunk | StreamEnd | Ping,
): ArrayBuffer {
  switch (message.type) {
    case MessageType.SessionInit:
      return encodeSessionInit(message);
    case MessageType.AudioFrame:
      return encodeAudioFrame(message);
    case MessageType.VideoChunk:
      return encodeVideoChunk(message);
    case MessageType.StreamEnd:
      return encodeStreamEnd(message);
    case MessageType.Ping:
      return encodePing(message);
    default: {
      const exhaustive: never = message;
      throw new Error(`Unknown protocol message: ${String(exhaustive)}`);
    }
  }
}

export { ChunkKind, MessageType };
