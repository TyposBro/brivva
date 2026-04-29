import { useCallback, useRef } from "react";

const HOST_VIDEO_MIN_WIDTH = 1920;
const HOST_VIDEO_MIN_HEIGHT = 1080;
const HOST_VIDEO_IDEAL_WIDTH = 3840;
const HOST_VIDEO_IDEAL_HEIGHT = 2160;
const HOST_VIDEO_MIN_FPS = 30;
const HOST_VIDEO_IDEAL_FPS = 60;
const HOST_VIDEO_MAX_BITRATE_BPS = 35_000_000;

type VideoProfile = {
  width: number;
  height: number;
  fps: number;
};

type WebRtcSignal =
  | { type: "webrtc:offer"; sdp: string; videoProfile: VideoProfile }
  | { type: "client:media_stats"; stats: ClientMediaStats };

type WebRtcAnswer = { type: "webrtc:answer"; sdp: string };

export type ClientMediaStats = {
  userAgent: string;
  sourceTrack?: MediaTrackSettings;
  uplinkTrack?: MediaTrackSettings;
  cfr?: { framesDrawn: number; framesPerSecond: number };
  outboundVideo?: Record<string, unknown>;
};

export type WebRtcConnectionIssue = {
  layer: "webrtc";
  state: RTCPeerConnectionState | RTCIceConnectionState;
  message: string;
};

/** Webcam preview + WebRTC video uplink. Audio still uses the PCM WS path. */
export function useWebcam(
  sendSignalJson: (msg: WebRtcSignal) => void,
  isSocketOpen: () => boolean,
  onMediaStats?: (stats: ClientMediaStats) => void,
  onConnectionIssue?: (issue: WebRtcConnectionIssue) => void,
) {
  const streamRef = useRef<MediaStream | null>(null);
  const videoElRef = useRef<HTMLVideoElement | null>(null);
  const peerRef = useRef<RTCPeerConnection | null>(null);
  const cfrUplinkRef = useRef<CfrUplink | null>(null);
  const statsIntervalRef = useRef<number | null>(null);
  const pendingLocalOfferRef = useRef(false);

  const videoRef = useCallback((el: HTMLVideoElement | null) => {
    videoElRef.current = el;
    if (el && streamRef.current) {
      el.srcObject = streamRef.current;
    }
  }, []);

  const startWebcam = useCallback(async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        video: {
          width: { min: HOST_VIDEO_MIN_WIDTH, ideal: HOST_VIDEO_IDEAL_WIDTH },
          height: { min: HOST_VIDEO_MIN_HEIGHT, ideal: HOST_VIDEO_IDEAL_HEIGHT },
          frameRate: { min: HOST_VIDEO_MIN_FPS, ideal: HOST_VIDEO_IDEAL_FPS },
          facingMode: "user",
        },
      });
      streamRef.current = stream;
      if (videoElRef.current) {
        videoElRef.current.srcObject = stream;
      }
    } catch (err) {
      console.error("Webcam access failed:", err);
    }
  }, []);

  const stopWebcam = useCallback(() => {
    stopPeer(peerRef);
    stopMediaStats(statsIntervalRef);
    stopCfrUplink(cfrUplinkRef);
    streamRef.current?.getTracks().forEach((t) => t.stop());
    streamRef.current = null;
  }, []);

  const startFrameStreaming = useCallback(async () => {
    if (!isSocketOpen() || !streamRef.current || peerRef.current) return;

    const peer = new RTCPeerConnection({
      iceServers: [{ urls: "stun:stun.l.google.com:19302" }],
    });
    peerRef.current = peer;
    attachConnectionDiagnostics(peer, onConnectionIssue);

    // Use the camera track directly for production-quality browser WebRTC.
    // Canvas capture is timer-driven and Chromium throttles timers when the
    // Brivva tab is backgrounded (e.g. while watching YouTube Studio), which
    // collapsed the uplink to ~1fps in real tests. Source getSettings() shows
    // the camera can provide 30fps, so preserve that native track here.
    stopCfrUplink(cfrUplinkRef);
    cfrUplinkRef.current = null;
    const uplinkStream = streamRef.current;

    for (const track of uplinkStream.getVideoTracks()) {
      track.contentHint = "motion";
      const sender = peer.addTrack(track, uplinkStream);
      await preferHighQuality(sender);
      preferH264(peer);
    }

    const offer = await peer.createOffer();
    await peer.setLocalDescription(offer);
    pendingLocalOfferRef.current = true;
    await waitForIceGatheringComplete(peer);
    if (!isSocketOpen() || !peer.localDescription) return;
    sendSignalJson({
      type: "webrtc:offer",
      sdp: peer.localDescription.sdp,
      videoProfile: readVideoProfile(streamRef.current),
    });
    startMediaStats({
      peer,
      sourceStream: streamRef.current,
      cfrUplink: cfrUplinkRef.current,
      sendSignalJson,
      intervalRef: statsIntervalRef,
      isSocketOpen,
      onMediaStats,
    });
  }, [isSocketOpen, onConnectionIssue, onMediaStats, sendSignalJson]);

  const stopFrameStreaming = useCallback(() => {
    pendingLocalOfferRef.current = false;
    stopPeer(peerRef);
    stopMediaStats(statsIntervalRef);
    stopCfrUplink(cfrUplinkRef);
  }, []);

  const handleWebRtcMessage = useCallback((msg: unknown): boolean => {
    if (!isWebRtcAnswer(msg)) return false;
    const peer = peerRef.current;
    if (!peer || !pendingLocalOfferRef.current) return true;
    peer
      .setRemoteDescription({ type: "answer", sdp: msg.sdp })
      .catch((err) => console.error("WebRTC answer failed:", err));
    pendingLocalOfferRef.current = false;
    return true;
  }, []);

  return {
    videoRef,
    startWebcam,
    stopWebcam,
    startFrameStreaming,
    stopFrameStreaming,
    handleWebRtcMessage,
  };
}


function readVideoProfile(stream: MediaStream | null): VideoProfile {
  const track = stream?.getVideoTracks()[0];
  const settings = typeof track?.getSettings === "function" ? track.getSettings() : undefined;
  const width = Math.round(Number(settings?.width) || HOST_VIDEO_MIN_WIDTH);
  const height = Math.round(Number(settings?.height) || HOST_VIDEO_MIN_HEIGHT);
  const fps = Math.round(Number(settings?.frameRate) || HOST_VIDEO_MIN_FPS);
  return { width, height, fps };
}

type CfrUplink = {
  stream: MediaStream;
  getStats: () => { framesDrawn: number; framesPerSecond: number };
  stop: () => void;
};

function stopPeer(ref: React.MutableRefObject<RTCPeerConnection | null>) {
  ref.current?.close();
  ref.current = null;
}

function stopCfrUplink(ref: React.MutableRefObject<CfrUplink | null>) {
  ref.current?.stop();
  ref.current = null;
}

function stopMediaStats(ref: React.MutableRefObject<number | null>) {
  if (ref.current !== null) window.clearInterval(ref.current);
  ref.current = null;
}

type StartMediaStatsArgs = {
  peer: RTCPeerConnection;
  sourceStream: MediaStream | null;
  cfrUplink: CfrUplink | null;
  sendSignalJson: (msg: WebRtcSignal) => void;
  intervalRef: React.MutableRefObject<number | null>;
  isSocketOpen: () => boolean;
  onMediaStats?: (stats: ClientMediaStats) => void;
};

function startMediaStats(args: StartMediaStatsArgs) {
  stopMediaStats(args.intervalRef);
  args.intervalRef.current = window.setInterval(() => {
    void collectMediaStats(args).then((stats) => {
      args.onMediaStats?.(stats);
      if (args.isSocketOpen()) {
        args.sendSignalJson({ type: "client:media_stats", stats });
      }
      console.info("brivva media stats", stats);
    });
  }, 2000);
}

async function collectMediaStats(args: StartMediaStatsArgs): Promise<ClientMediaStats> {
  const sourceTrack = args.sourceStream?.getVideoTracks()[0]?.getSettings();
  const uplinkTrack = args.cfrUplink?.stream.getVideoTracks()[0]?.getSettings();
  const outboundVideo = await collectOutboundVideoStats(args.peer);
  return {
    userAgent: navigator.userAgent,
    sourceTrack,
    uplinkTrack,
    cfr: args.cfrUplink?.getStats(),
    outboundVideo,
  };
}

async function collectOutboundVideoStats(
  peer: RTCPeerConnection,
): Promise<Record<string, unknown> | undefined> {
  const stats = await peer.getStats?.();
  if (!stats) return undefined;
  for (const report of stats.values()) {
    if (report.type === "outbound-rtp" && report.kind === "video") {
      return {
        framesEncoded: report.framesEncoded,
        framesPerSecond: report.framesPerSecond,
        framesSent: report.framesSent,
        frameWidth: report.frameWidth,
        frameHeight: report.frameHeight,
        qualityLimitationReason: report.qualityLimitationReason,
        qualityLimitationDurations: report.qualityLimitationDurations,
        encoderImplementation: report.encoderImplementation,
        targetBitrate: report.targetBitrate,
        totalEncodeTime: report.totalEncodeTime,
      };
    }
  }
  return undefined;
}


function attachConnectionDiagnostics(
  peer: RTCPeerConnection,
  onConnectionIssue?: (issue: WebRtcConnectionIssue) => void,
) {
  const report = (state: RTCPeerConnectionState | RTCIceConnectionState) => {
    if (!["disconnected", "failed", "closed"].includes(state)) return;
    onConnectionIssue?.({
      layer: "webrtc",
      state,
      message:
        state === "disconnected"
          ? "WebRTC media temporarily disconnected. Keep the tab visible and network stable."
          : "WebRTC media connection failed. End this stream and create a fresh session before going live again.",
    });
  };
  peer.addEventListener("iceconnectionstatechange", () =>
    report(peer.iceConnectionState),
  );
  peer.addEventListener("connectionstatechange", () =>
    report(peer.connectionState),
  );
}

function waitForIceGatheringComplete(peer: RTCPeerConnection): Promise<void> {
  if (peer.iceGatheringState === "complete") return Promise.resolve();
  return new Promise((resolve) => {
    const onStateChange = () => {
      if (peer.iceGatheringState !== "complete") return;
      peer.removeEventListener("icegatheringstatechange", onStateChange);
      resolve();
    };
    peer.addEventListener("icegatheringstatechange", onStateChange);
  });
}

function isWebRtcAnswer(msg: unknown): msg is WebRtcAnswer {
  return (
    typeof msg === "object" &&
    msg !== null &&
    (msg as { type?: unknown }).type === "webrtc:answer" &&
    typeof (msg as { sdp?: unknown }).sdp === "string"
  );
}

async function preferHighQuality(sender: RTCRtpSender) {
  if (!sender.setParameters) return;
  const params = sender.getParameters();
  params.encodings = [
    {
      ...(params.encodings?.[0] ?? {}),
      maxBitrate: HOST_VIDEO_MAX_BITRATE_BPS,
      maxFramerate: HOST_VIDEO_IDEAL_FPS,
      scaleResolutionDownBy: 1,
    },
  ];
  await sender
    .setParameters(params)
    .catch((err) => console.warn("WebRTC video quality params failed:", err));
}

function preferH264(peer: RTCPeerConnection | null) {
  if (!peer) return;
  const transceiver = peer
    .getTransceivers()
    .find((t) => t.sender.track?.kind === "video");
  const capabilities = RTCRtpSender.getCapabilities?.("video");
  if (!transceiver || !capabilities?.codecs || !transceiver.setCodecPreferences)
    return;
  const h264 = capabilities.codecs.filter(
    (c) => c.mimeType.toLowerCase() === "video/h264" && !/packetization-mode=0/i.test(c.sdpFmtpLine ?? ""),
  );
  if (h264.length > 0) transceiver.setCodecPreferences(h264);
}
