import { useCallback, useRef } from "react";

const HOST_VIDEO_MAX_WIDTH = 3840;
const HOST_VIDEO_MAX_HEIGHT = 2160;
const HOST_VIDEO_FPS = 30;
const HOST_VIDEO_MAX_BITRATE_BPS = 35_000_000;

type WebRtcAnswer = { type: "webrtc:answer"; sdp: string };

/** Webcam preview + WebRTC video uplink. Audio still uses the PCM WS path. */
export function useWebcam(
  sendSignalJson: (msg: { type: "webrtc:offer"; sdp: string }) => void,
  isSocketOpen: () => boolean,
) {
  const streamRef = useRef<MediaStream | null>(null);
  const videoElRef = useRef<HTMLVideoElement | null>(null);
  const peerRef = useRef<RTCPeerConnection | null>(null);
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
          width: { ideal: HOST_VIDEO_MAX_WIDTH, max: HOST_VIDEO_MAX_WIDTH },
          height: { ideal: HOST_VIDEO_MAX_HEIGHT, max: HOST_VIDEO_MAX_HEIGHT },
          frameRate: { ideal: HOST_VIDEO_FPS, max: HOST_VIDEO_FPS },
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
    streamRef.current?.getTracks().forEach((t) => t.stop());
    streamRef.current = null;
  }, []);

  const startFrameStreaming = useCallback(async () => {
    if (!isSocketOpen() || !streamRef.current || peerRef.current) return;

    const peer = new RTCPeerConnection();
    peerRef.current = peer;

    for (const track of streamRef.current.getVideoTracks()) {
      track.contentHint = "detail";
      const sender = peer.addTrack(track, streamRef.current);
      await preferHighQuality(sender);
      preferH264(peer);
    }

    const offer = await peer.createOffer();
    await peer.setLocalDescription(offer);
    pendingLocalOfferRef.current = true;
    await waitForIceGatheringComplete(peer);
    if (!isSocketOpen() || !peer.localDescription) return;
    sendSignalJson({ type: "webrtc:offer", sdp: peer.localDescription.sdp });
  }, [isSocketOpen, sendSignalJson]);

  const stopFrameStreaming = useCallback(() => {
    pendingLocalOfferRef.current = false;
    stopPeer(peerRef);
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

function stopPeer(ref: React.MutableRefObject<RTCPeerConnection | null>) {
  ref.current?.close();
  ref.current = null;
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
      maxFramerate: HOST_VIDEO_FPS,
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
    (c) => c.mimeType.toLowerCase() === "video/h264",
  );
  if (h264.length > 0) transceiver.setCodecPreferences(h264);
}
