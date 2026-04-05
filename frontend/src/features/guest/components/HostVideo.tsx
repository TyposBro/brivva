type Props = {
  attachCanvas: (canvas: HTMLCanvasElement | null) => void;
};

export function HostVideo({ attachCanvas }: Props) {
  return (
    <div className="host-video">
      <canvas
        ref={attachCanvas}
        width={256}
        height={256}
        style={{
          width: "256px",
          height: "256px",
          borderRadius: "12px",
          border: "2px solid #333",
          background: "#1a1a1a",
        }}
      />
      <span className="host-video-label">Host (live)</span>
    </div>
  );
}
