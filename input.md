# Brivva Live Streaming Debug Notes

Date: 2026-04-28

Context: YouTube live feed is still sometimes fast and sometimes buffering after recent FFmpeg/drain changes.

This file lists the current observed problems, likely explanations, and possible fixes. Each section includes a **User reply** area for you to respond with extra context, constraints, or corrections.

---

## 1. Main observed symptom

### Problem

YouTube output is not evenly paced:

- sometimes video appears too fast / catches up;
- sometimes YouTube buffers;
- sometimes output feels unstable even after stale-drop logic.

### Current evidence from logs

Server log still shows:

```txt
ffmpeg stderr: [in#1/s16le ...] Thread message queue blocking; consider raising the thread_queue_size option (current value: 1024)
```

This warning is from FFmpeg's **audio input queue**, not directly from our Rust queues.

### Interpretation

Rust may be writing audio continuously, but FFmpeg is not consuming/muxing it fast enough. That usually means FFmpeg is blocked waiting on video timing/frames, output/network, or encode speed.

When video resumes or FFmpeg catches up, YouTube can see bursty media timing, causing apparent fast-forward and/or buffering.

### User reply

<!-- Write your notes here. For example: when exactly does this happen, how long after going live, whether audio continues, whether the preview freezes, whether YouTube Studio reports poor connection, etc. -->

---

## 2. Raw H.264 pipe loses real timestamps

### Problem

Current server pipeline feeds FFmpeg raw Annex-B H.264 bytes:

```txt
WebRTC RTP/H264
  -> Rust depacketize to Annex-B
  -> FFmpeg stdin: -f h264 -r 30 -i pipe:0
```

Raw H.264 elementary streams do not carry container PTS/DTS timestamps.

### Why this matters

Even after Rust uses RTP timestamps to pace writes, FFmpeg receives only bytes. It does not receive the original RTP timestamps.

FFmpeg has to infer timing from:

```txt
-r 30
```

If video arrives unevenly, pauses, or bursts, FFmpeg's guessed timestamps can still produce unstable mux timing.

### Possible fixes

#### Short-term

- Keep current path but reduce source pressure and add better diagnostics.
- Possibly feed FFmpeg a timestamp-capable format instead of raw H.264.

#### Long-term

Replace raw H.264 stdin with a media graph that preserves timestamps:

```txt
WebRTC/RTP timestamps
  -> jitter buffer
  -> depay/parser/decoder
  -> CFR normalizer
  -> encoder
  -> muxer
```

Candidate technologies:

- GStreamer;
- FFmpeg with real RTP/SDP timestamped input;
- WebRTC media server/SFU/WHIP path;
- custom pipeline that maps RTP timestamp to encoder PTS.

### User reply

Replace raw H.264 stdin with a media graph that preserves timestamps:

```txt
WebRTC/RTP timestamps
  -> jitter buffer
  -> depay/parser/decoder
  -> CFR normalizer
  -> encoder
  -> muxer
```

also elaborate Candidate technologies

<!-- Write your thoughts here. Is preserving original camera timing important? Are you okay with using GStreamer? Do you want to avoid major dependency changes right now? -->

---

## 3. Browser/WebRTC video may not be constant frame rate

### Problem

Frontend asks for 30fps, but browser capture is not guaranteed to produce perfect CFR.

Current frontend settings are in:

```txt
frontend/src/features/broadcast/presentation/use-webcam.ts
```

Current constants:

```ts
const HOST_VIDEO_MAX_WIDTH = 3840;
const HOST_VIDEO_MAX_HEIGHT = 2160;
const HOST_VIDEO_FPS = 30;
const HOST_VIDEO_MAX_BITRATE_BPS = 35_000_000;
```

`getUserMedia({ frameRate: 30 })` is a request, not a hard real-time contract.

### Why this matters

Browser/WebRTC may emit:

- variable frame rate;
- dropped frames under CPU pressure;
- bursts after encode/network delay;
- sparse frames when camera/browser throttles;
- huge H.264 packets at 4K.

Audio, however, is sent continuously at 20ms ticks. This creates audio/video pressure mismatch.

### Possible fixes

#### Fast demo fix

Reduce local capture to 1080p or 720p.

Possible 1080p config:

```ts
const HOST_VIDEO_MAX_WIDTH = 1920;
const HOST_VIDEO_MAX_HEIGHT = 1080;
const HOST_VIDEO_MAX_BITRATE_BPS = 6_000_000;
```

Possible safer 720p config:

```ts
const HOST_VIDEO_MAX_WIDTH = 1280;
const HOST_VIDEO_MAX_HEIGHT = 720;
const HOST_VIDEO_MAX_BITRATE_BPS = 3_000_000;
```

#### Better browser-side timing fix

Use a canvas CFR uplink:

```txt
camera video
  -> hidden video element
  -> canvas draw loop at 30fps
  -> canvas.captureStream(30)
  -> WebRTC
```

This can force repeated frames and smoother frame cadence before WebRTC.

### User reply

okay lets go 1080p and
Use a canvas CFR uplink:

camera video
-> hidden video element
-> canvas draw loop at 30fps
-> canvas.captureStream(30)
-> WebRTC
This can force repeated frames and smoother frame cadence before WebRTC.

<!-- Write your thoughts here. Are you testing with webcam or screen share? Do you require 4K for the demo? Would 1080p/720p be acceptable temporarily? -->

---

## 4. Local machine / dev stack may be under pressure

### Problem

Workers logs show some local internal requests taking about 5 seconds:

```txt
GET /internal/sessions/... 200 OK (5030ms)
GET /internal/sessions/... 200 OK (10038ms)
```

Server logs also show repeated active voice refresh fetch failures:

```txt
active-voice refresh: bundle fetch failed — retaining cached voice
```

Workers health is currently OK, but local Wrangler/workerd occasionally stalls.

### Why this matters

Even if Workers is not directly responsible for video mux timing, local CPU/event-loop pressure can affect:

- browser capture;
- WebRTC encoding;
- Rust server;
- FFmpeg x264 encode/downscale;
- local Workers/DB calls;
- TTS request handling.

The 5s local stalls suggest the dev environment is not fully smooth under live load.

### Possible fixes

- Reduce video capture to 1080p/720p for local testing.
- Run Workers separately or reduce polling/refresh calls during live.
- Disable active-voice refresh during demo if not needed.
- Profile CPU while live:
  - browser process;
  - FFmpeg;
  - server-rs;
  - workerd/wrangler.

### User reply

why do we need polling/refresh during live? Disable active-voice refresh during demo if not needed.

<!-- Write your notes here. What machine are you testing on? Is CPU high? Are other apps open? Does Chrome show camera/WebRTC CPU spikes? -->

---

## 5. Current FFmpeg encode may still fall behind realtime

### Current FFmpeg direction

We changed FFmpeg from H.264 copy to re-encode for stable output:

```txt
-c:v libx264
-preset ultrafast
-tune zerolatency
-r 30
-g 60
-b:v 3500k
-maxrate 4500k
-bufsize 9000k
-vf fps=30,scale=max 1080p
```

### Problem

Even with `ultrafast`, FFmpeg may still fall behind if input is 4K, bursty, or if local CPU is loaded.

The warning:

```txt
Thread message queue blocking ... s16le
```

can indicate FFmpeg cannot consume audio input in real time because it is busy waiting on or processing video.

### Possible diagnostics

Change FFmpeg logging to expose stats like:

```txt
fps=...
speed=...
dup=...
drop=...
time=...
bitrate=...
```

Key signal:

```txt
speed < 1.0x
```

means FFmpeg is encoding/muxing slower than real time.

### Possible fixes

- Lower capture resolution.
- Lower output resolution to 720p for demo.
- Lower output bitrate.
- Use hardware encoder where available:
  - NVENC;
  - VAAPI;
  - VideoToolbox on macOS.
- Encode video once and fan out to multiple audio muxers long-term.

### User reply

DO these:

1. Change FFmpeg logging to expose stats like:

```txt
fps=...
speed=...
dup=...
drop=...
time=...
bitrate=...
```

2. Encode video once and fan out to multiple audio muxers long-term.
<!-- Write your notes here. Is 720p okay for demo? Do you have NVIDIA GPU / hardware encoder available? Are you okay with FFmpeg stats being noisier in logs? -->

---

## 6. Rust stale-drop helps before FFmpeg, but not inside FFmpeg

### Current Rust fixes

Video stale-drop:

```txt
VIDEO_MAX_LAG = 250ms
drain_next_h264_live(...)
```

Audio stale-drop:

```txt
AUDIO_MAX_LAG = 250ms
drain_aged_host_audio(...)
cap_ready_audio_to_live(...)
```

### Problem

These only drop media before it enters FFmpeg.

The current warning suggests backlog is inside FFmpeg:

```txt
Rust -> FFmpeg internal input queue -> encoder/muxer -> YouTube
```

Once audio is in FFmpeg's internal queue, Rust cannot drop it anymore.

### Possible fixes

- Reduce FFmpeg internal queue size instead of increasing it, so backpressure shows earlier.
- Use a single clocked media graph where audio/video are synchronized before FFmpeg queues grow.
- Move audio/video muxing into a pipeline that can drop late frames/samples intentionally.
- Add instrumentation around FFmpeg pipe/FIFO write stalls and FFmpeg stats.

### User reply

1. Move audio/video muxing into a pipeline that can drop late frames/samples intentionally. if we move audio/video muxing into rust code, will ffmpeg be redundant? why do we need ffmpeg in the first place? if we are using 1% of its capabilities and that can be done with our rust code, why not drop external dependency all together??
<!-- Write your notes here. Are you okay with dropping audio/video to stay live, or do you prefer preserving all content even if delayed? -->

---

## 7. H.264 stale dropping may cause corruption until next keyframe

### Problem

Current video stale-drop can drop arbitrary H.264 chunks.

H.264 frames depend on prior frames. Dropping non-keyframe data can cause visual corruption until next IDR/keyframe.

Current GOP target:

```txt
-g 60
-keyint_min 60
```

At 30fps, next keyframe may be up to about 2 seconds away.

### Possible fixes

- Make stale dropping keyframe-aware.
- When stale, drop until next IDR frame rather than arbitrary chunk.
- Force more frequent keyframes for demo, e.g. GOP 30 = 1 second.
- Better: decode/normalize/re-encode so frame dropping happens on decoded frames, not compressed H.264 packets.

### User reply

<!-- Write your notes here. Have you seen blocky/corrupt frames after buffering? Is a 1-second GOP acceptable? -->

---

## 8. Audio behavior in translated-only stream

### Current config observed

Korean-only translated stream had:

```txt
stream_count=1
langs=[Ko]
passthrough=false
is_source=false
host_gain=0.03
```

### Problem

Original host audio is intentionally almost muted in the translated stream. If TTS is delayed or times out, early output can sound silent.

Logs showed one TTS timeout:

```txt
tts elevenlabs request timed out
tts dispatch produced no audio — utterance dropped
```

### Why this matters

This does not directly explain video speed/buffering, but it affects perceived live quality.

### Possible fixes

- For demo, increase host bed audio from `0.03` to maybe `0.10` or `0.15`.
- Add fallback behavior when TTS times out.
- Show UI indication that translated stream will be quiet until first TTS arrives.

### User reply

- For demo, increase host bed audio from `0.03` to maybe `0.10` or `0.15`. users have already control on frontend to change it. no need to worry

<!-- Write your notes here. Do you want Korean-only output to keep low original audio, or should original speech remain more audible during TTS gaps? -->

---

## 9. Possible architecture: reliable demo fallback

### Option

Use a fixed-cadence frame path for local demo:

```txt
browser canvas at 30fps
  -> send JPEG/RGBA frames
  -> FFmpeg image2pipe/rawvideo
  -> x264
  -> RTMP
```

### Pros

- Server controls cadence.
- Much easier to make smooth.
- Avoids raw H.264 RTP timestamp loss.
- Good for proving product demo.

### Cons

- Less efficient.
- Poor fit for multiple 4K streams.
- More bandwidth/CPU if implemented naively.
- Not ideal production architecture.

### User reply

Brivva needs enterprise level software that scales. I need to make it to the best of my knowledge. DEMO is not demo but launch. we cant flop it no matter what. Quality >> Deadline/Speed

<!-- Write your notes here. Would you accept a demo-only fallback path if it makes YouTube stable, even if production later uses WebRTC/GStreamer? No DEMO==LAUNCH==PRODUCTION==ENTERPRISE GRADE READINESS-->

---

## 10. Possible architecture: production media graph

### Recommended long-term architecture

```txt
Browser WebRTC A/V ingest
  -> media server preserving RTP timestamps
  -> jitter buffer
  -> decode once
  -> normalize video to CFR
  -> encode video once per output profile
  -> fan out encoded video
  -> mux per-language audio
  -> publish to YouTube/TikTok/etc.
```

For multiple languages/platforms:

```txt
shared encoded video
  ├─ Korean audio -> mux -> YouTube Korean
  ├─ Japanese audio -> mux -> YouTube Japanese
  ├─ English/source audio -> mux -> passthrough
  └─ other platform outputs
```

### Key principle

Do not re-encode video once per language unless necessary. Video should be shared; audio should vary per output.

### Candidate implementation paths

#### GStreamer

Conceptual pipeline:

```txt
rtpjitterbuffer
  ! rtph264depay
  ! h264parse
  ! avdec_h264
  ! videorate
  ! video/x-raw,framerate=30/1
  ! videoscale
  ! x264enc tune=zerolatency speed-preset=ultrafast key-int-max=60
  ! flvmux
  ! rtmpsink
```

#### FFmpeg with timestamped RTP/SDP input

Let FFmpeg consume RTP with timestamps instead of raw h264 stdin.

#### Dedicated WebRTC media server / SFU / WHIP

Use a tool/library designed to preserve WebRTC timing.

### User reply

```txt
Browser WebRTC A/V ingest
  -> media server preserving RTP timestamps
  -> jitter buffer
  -> decode once
  -> normalize video to CFR
  -> shared encoded video
    ├─ Korean audio -> mux
    ├─ Japanese audio -> mux
    └─ English/source audio -> mux
  -> publish to YouTube/TikTok/etc.
```

also give me pros and cons of each candidate.

GStreamer vs FFmpeg with timestamped RTP/SDP input vs Dedicated WebRTC media server / SFU / WHIP
for context each streamer/session will get a fargate instance and each session will have between 2 to 8 languages. the lowest we can go right now is 1080p and no subtitles so video can be shared

```txt

```

<!-- Write your notes here. Preferred architecture? I choose cleanest production direction -->

---

## 11. Instrumentation we should add next

### Current gap

Logs show symptoms but not enough internal timing.

### Suggested metrics/logs

Log once per second per stream:

- video chunks received;
- RTP timestamp delta;
- wall-clock delta;
- video buffer length;
- stale video chunks dropped;
- audio chunks received;
- audio buffer length;
- ready audio bytes dropped;
- FIFO write WouldBlock count;
- FFmpeg stats line including fps/speed/drop/dup;
- RTMP output errors.

### Why

This will distinguish:

- browser sending uneven frames;
- Rust buffering/backpressure;
- FFmpeg encoding slower than realtime;
- RTMP/YouTube network backpressure;
- audio/video mux waiting.

### User reply

<!-- Write your notes here. Are you okay with noisy logs during debugging? yes. just logs enough. i wont be reading them anyway, you will be -->

---

## 12. My proposed next code changes

### Priority A: immediate demo stability

1. Lower frontend video capture to 1080p/6Mbps or 720p/3Mbps.
2. Enable FFmpeg stats logging.
3. Add per-second video/audio drain metrics.
4. Retest YouTube live.

### Priority B: if still uneven

5. Implement browser canvas CFR WebRTC track.
6. Optionally lower GOP from 60 to 30 for faster recovery after drops.
7. Make H.264 stale dropping IDR/keyframe-aware.

### Priority C: production direction

8. Replace raw H.264 pipe with timestamp-preserving media graph.
9. Encode shared video once.
10. Mux per-language audio outputs separately.

### User reply

<!-- Write your preferred priority order here. What should I implement first? What is unacceptable for demo or production? -->

---

## 13. Open questions for you

Please answer whichever ones matter:

1. Are you using webcam, screen share, OBS virtual camera, or something else? desktop webcame
2. Is 4K required for demo, or is 1080p/720p acceptable? 1080p is the lowest I can go
3. Does YouTube Studio say anything like “poor connection” or “not receiving enough video”? no, it says excellent
4. When it speeds up, does audio also speed up, or only video? I think video because I cant hear original audio
5. Does the local preview look smooth while YouTube is buffering? Yes. my frontend is smooth
6. Are you okay with dropping frames/audio to stay live? yes, but I would like to avoid it as much as possible
7. Are you okay with a demo-only stable fallback if production architecture differs? no. not acceptable at all
8. Are you open to GStreamer or do you want FFmpeg-only? choose the right tool for the job, I dont care about the tool itself. if we can, I would prefer hand written rust code so its more testable and we can change it however we want whenever we want
9. Do you have hardware encoding available on the deployment target? my deployment target is AWS Fargate, you can see it on terraform files
10. Should translated streams keep original host audio very low, or should it be more audible during TTS gaps? it can be decided by streamer, not us

### User reply

<!-- Answer here. -->
