import { appConfig } from "../../../core/config/app-config";

const WEBRTC_VIDEO_BITRATE_BPS = 8_000_000;

export class WebRtcVideoIngest {
  private pc: RTCPeerConnection | null = null;

  async start(args: {
    stream: MediaStream;
    token: string;
    sessionId?: string;
    sourceLang?: string;
  }): Promise<void> {
    this.stop();
    const track = args.stream.getVideoTracks()[0];
    if (!track) throw new Error("No webcam video track available");

    const pc = new RTCPeerConnection();
    this.pc = pc;
    const sender = pc.addTrack(track, args.stream);
    if (!preferH264Codec(pc)) {
      throw new Error("Browser did not expose H.264 WebRTC encoding");
    }
    await preferH264(sender);

    const offer = await pc.createOffer({
      offerToReceiveAudio: false,
      offerToReceiveVideo: false,
    });
    await pc.setLocalDescription(offer);
    await waitForIceGathering(pc);

    const local = pc.localDescription;
    if (!local?.sdp) throw new Error("WebRTC offer SDP missing");

    const url = new URL(`${appConfig().mediaHttpBase}/whip/session`);
    url.searchParams.set("token", args.token);
    url.searchParams.set("sourceLang", args.sourceLang ?? "en");
    url.searchParams.set("videoMode", "webrtc-h264");
    if (args.sessionId) url.searchParams.set("sessionId", args.sessionId);

    const resp = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/sdp" },
      body: local.sdp,
    });
    if (!resp.ok) throw new Error(`WHIP ${resp.status}: ${await resp.text()}`);
    const answer = await resp.text();
    await pc.setRemoteDescription({ type: "answer", sdp: answer });
  }

  stop(): void {
    this.pc?.close();
    this.pc = null;
  }
}

function preferH264Codec(pc: RTCPeerConnection): boolean {
  const caps = RTCRtpSender.getCapabilities?.("video");
  const h264 = caps?.codecs.filter((codec) => codec.mimeType.toLowerCase() === "video/h264") ?? [];
  if (!h264.length) return false;
  const transceiver = pc.getTransceivers().find((item) => item.sender.track?.kind === "video");
  transceiver?.setCodecPreferences?.(h264);
  return true;
}

async function preferH264(sender: RTCRtpSender): Promise<void> {
  const params = sender.getParameters();
  params.encodings = params.encodings?.length ? params.encodings : [{}];
  params.encodings[0] = {
    ...params.encodings[0],
    maxBitrate: WEBRTC_VIDEO_BITRATE_BPS,
    maxFramerate: 30,
  };
  await sender.setParameters(params);
}

function waitForIceGathering(pc: RTCPeerConnection): Promise<void> {
  if (pc.iceGatheringState === "complete") return Promise.resolve();
  return new Promise((resolve) => {
    const done = () => {
      if (pc.iceGatheringState === "complete") {
        pc.removeEventListener("icegatheringstatechange", done);
        resolve();
      }
    };
    pc.addEventListener("icegatheringstatechange", done);
    setTimeout(resolve, 3000);
  });
}
