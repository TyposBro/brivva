import type { Ref } from "react";
import { cn } from "../../../lib/cn";

export function CameraPreview({
  videoRef,
  visible,
}: {
  videoRef: Ref<HTMLVideoElement | null>;
  visible: boolean;
}) {
  return (
    <div className={cn("flex flex-col items-center gap-2", visible ? "block" : "hidden")}>
      <video
        ref={videoRef}
        autoPlay
        muted
        playsInline
        className="w-64 h-64 object-cover -scale-x-100 rounded-xl border-2 border-surface-container-highest"
      />
      <span className="text-on-surface-variant text-xs font-label">
        Your camera (mirrored)
      </span>
    </div>
  );
}
