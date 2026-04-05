type Props = {
  tier: number;
  hasRtmpStreams: boolean;
};

export function InfoFooter({ tier, hasRtmpStreams }: Props) {
  const tierDescription = tier === 1
    ? "Subtitles only — original audio passes through"
    : "Translated voice + subtitles — each language gets AI-generated voice audio";

  const streamDescription = hasRtmpStreams
    ? "Webcam video + translated audio muxed via FFmpeg and pushed to RTMP destinations."
    : "Audio-only mode. Add RTMP destinations above to enable video streaming.";

  return (
    <div className="text-xs text-outline space-y-1">
      <p>Option {tier}: {tierDescription}</p>
      <p>{streamDescription}</p>
    </div>
  );
}
