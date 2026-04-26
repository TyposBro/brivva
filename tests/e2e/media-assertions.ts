export type ProbeStream = {
  codec_type: string;
  codec_name?: string;
  profile?: string;
  pix_fmt?: string;
  width?: number;
  height?: number;
  has_b_frames?: number;
  avg_frame_rate?: string;
  r_frame_rate?: string;
  start_time?: string;
  extradata_size?: number;
  sample_rate?: string;
  channels?: number;
};

export const BAD_MEDIA_LOG_PATTERNS: RegExp[] = [
  /non-existing PPS/i,
  /decode_slice_header error/i,
  /\bno frame!\b/i,
  /Timestamps are unset/i,
  /Invalid data found when processing input/i,
  /Could not find codec parameters/i,
  /Error writing trailer/i,
  /Broken pipe/i,
  /Connection refused/i,
  /Server returned 4\d\d|Server returned 5\d\d/i,
];

export function assertCleanMediaLogs(logs: string, context: string): void {
  const matches = BAD_MEDIA_LOG_PATTERNS
    .map((pattern) => ({ pattern, matched: logs.match(pattern)?.[0] }))
    .filter((item) => item.matched);
  if (!matches.length) return;
  throw new Error(
    `${context} emitted media failure diagnostics: ${
      matches.map((item) => item.matched).join(", ")
    }`,
  );
}

export function assertRtmpStreams(streams: ProbeStream[], context: string): void {
  const video = streams.find((stream) => stream.codec_type === "video");
  const audio = streams.find((stream) => stream.codec_type === "audio");
  if (!video) throw new Error(`${context}: missing video track`);
  if (!audio) throw new Error(`${context}: missing audio track`);

  if (video.codec_name !== "h264") {
    throw new Error(`${context}: expected H.264 video, got ${video.codec_name ?? "unknown"}`);
  }
  if (audio.codec_name !== "aac") {
    throw new Error(`${context}: expected AAC audio, got ${audio.codec_name ?? "unknown"}`);
  }
  if (video.has_b_frames && video.has_b_frames > 0) {
    throw new Error(`${context}: RTMP video has B-frames (${video.has_b_frames})`);
  }
  if (!video.extradata_size || video.extradata_size <= 0) {
    throw new Error(`${context}: H.264 extradata/SPS/PPS is missing`);
  }
  if (audio.sample_rate !== "44100") {
    throw new Error(`${context}: expected 44.1 kHz audio, got ${audio.sample_rate ?? "unknown"}`);
  }
  if (audio.channels !== 2) {
    throw new Error(`${context}: expected stereo AAC output, got ${audio.channels ?? "unknown"} channels`);
  }
}
