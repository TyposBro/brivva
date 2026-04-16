export const enum MessageType {
  SessionInit = 0x01,
  AudioFrame = 0x02,
  VideoChunk = 0x03,
  StreamEnd = 0x04,
  Ping = 0x05,
}

export const enum ChunkKind {
  Init = 0,
  Media = 1,
}

export type SessionInit = {
  type: MessageType.SessionInit;
  version: number;
  flags: number;
  audioSampleRate: number;
  audioChannels: number;
  audioFrameDurationMs: number;
  videoTimescale: number;
  sessionStartUnixMs: bigint;
  metadataJson: string;
};

export type AudioFrame = {
  type: MessageType.AudioFrame;
  seq: bigint;
  captureTsMs: bigint;
  durationMs: number;
  payload: ArrayBuffer;
};

export type VideoChunk = {
  type: MessageType.VideoChunk;
  seq: bigint;
  captureTsMs: bigint;
  durationMs: number;
  isKeyframe: boolean;
  chunkKind: ChunkKind;
  payload: ArrayBuffer;
};

export type StreamEnd = {
  type: MessageType.StreamEnd;
  reasonCode: number;
};

export type Ping = {
  type: MessageType.Ping;
  clientTimeMs: bigint;
};

export type ProtocolMessage =
  | SessionInit
  | AudioFrame
  | VideoChunk
  | StreamEnd
  | Ping;
