# Brivva End-to-End Flow

Every step from clicking "Start Broadcasting" to the viewer hearing translated audio.

Verified against the codebase as of April 4, 2026 (v15 — Progressive Chunking).

---

## Step 1: Click "Start Broadcasting"

`BroadcastPage.tsx:283` → `start()`

```
Frontend                                     Backend (lib.rs)
────────                                     ────────────────

① Filter out source lang from targets:
   available = targetLangs.filter(l => l !== sourceLang)
   e.g. sourceLang="en", targetLangs=["ja","ko"]
   → available = ["ja","ko"]

② Open WebSocket:
   ws://localhost:3000/ws
     ?sourceLang=en
     &targetLangs=ja,ko                  →  ws_handler()
     &tier=2                                 │
     &ttsModel=eleven_flash_v2_5             │
                                             ├─ parse sourceLang → Lang::En
                                             ├─ parse targetLangs → [Ja, Ko]
                                             ├─ session_id = UUID[..8] e.g. "43c1ea7f"
                                             ├─ Session::new(id, En, [Ja,Ko], tier=2)
                                             │    target_langs = [Ja, Ko]
                                             │    voice_clone_id = load_persisted_voice()
                                             │    broadcast_delay_ms = 3000
                                             │    rtmp_manager = None
                                             │    rtmp_langs = []
                                             │
                                             ├─ send SessionCreated{id} → frontend
                                             │
                                             ├─ spawn start_stt(session_id, En, audio_rx)
                                             │    STT IS NOW LISTENING — before RTMP exists
                                             │    POST /v2/live to create Gladia session
                                             │    connect WebSocket to returned URL
                                             │    params: endpointing=0.25s, max_duration=5s
                                             │
                                             ├─ spawn send_task (host_rx → ws_sink)
                                             └─ spawn recv_task (ws_stream → router)

③ Start webcam (only if RTMP URLs exist):
   getUserMedia({ video: {width:1920, height:1080,
                          frameRate:60, deviceId} })
   MediaRecorder.start(100)                  chunks every 100ms
   recorder.ondataavailable → [0x02] + buf → ws.send(tagged)

④ Send RTMP config:
   ws.send({type:"rtmp:config",          →  lib.rs
     streams: [{lang:"ja",                   │
       url:"rtmp://a.rtmp.youtube.com/..."}],│
     broadcastDelay: 3000                    │
   })                                        │
                                             ├─ session.broadcast_delay_ms = 3000
                                             ├─ RtmpManager::with_delay(3000)
                                             ├─ For each stream:
                                             │    mkfifo /tmp/brivva_audio_{stream_id}
                                             │    Spawn FFmpeg (libx264 ultrafast → FLV → RTMP)
                                             │    Spawn video drain OS thread (20ms ticks)
                                             │    Spawn audio drain OS thread (20ms ticks)
                                             └─ spawn health monitor (checks crashes every 2s)

⑤ Start audio capture:
   AudioPipeline.start(onAudio, deviceId)
     AudioContext(sampleRate=44100)
     ScriptProcessor(bufferSize=4096, 1in, 1out)
     onaudioprocess fires every ~93ms → 8192 bytes raw PCM
     tagged = [0x01] + PCM → ws.send(tagged)
     → audio_tx.send(data[1..]) → mpsc channel → start_stt

⑥ setIsLive(true)
```

---

## Step 2: You Speak — Progressive Chunk Detection

```
Microphone → MediaStream → AudioContext(44100Hz) → ScriptProcessor(4096)

Every ~93ms:
  float32[4096] → int16[4096] = 8192 bytes
  [0x01] + PCM → WebSocket → audio_tx → start_stt

  audio_acc.push(data.clone())          accumulates for passthrough
  gladia_ws.send(Binary(data))          streams to Gladia

Gladia processes and returns events:

─── Interim (partial) event ───────────────────────────────

  transcript = Gladia utterance text
  utterance_start = Instant::now() (first interim)

  send Interim{transcript} → frontend

  ┌─ ProgressiveChunkDetector.check(transcript) ──────────┐
  │                                                         │
  │  derive_position():                                     │
  │    find where previously emitted text ends               │
  │    in the current transcript (handles Gladia revisions)  │
  │    uses normalized character matching (strip punctuation) │
  │                                                         │
  │  if since_last_chunk ≥ max_duration (2000ms):           │
  │    force split at current position                       │
  │                                                         │
  │  if since_last_chunk ≥ min_duration:                    │
  │    search for clause boundary markers in new text        │
  │    EN: ", and ", ", but ", "; ", ". So ", etc.           │
  │    JA: "けど、", "ので、", "ですね、", etc.               │
  │    KO: "는데요 ", "지만 ", " 그리고 ", etc.               │
  │    ZH: "，但是", "，所以", "，然后", etc.                  │
  │                                                         │
  │  if boundary found:                                     │
  │    return ChunkBoundary{split_pos, chunk_text}          │
  │    emitted_text = transcript[..split_pos]               │
  └─────────────────────────────────────────────────────────┘

  if chunk boundary detected:
    if first chunk for this utterance:
      uc += 1
      spawn_chunked_pipeline(utterance_id, source_lang, ...)
        creates mpsc channel (capacity 6)
        spawns consumer task

    send ChunkEvent{text, chunk_index, context, ...}
      via try_send to pipeline channel
    chunk_index += 1

─── Final event ───────────────────────────────────────────

  host_audio = audio_acc.drain()         grab ALL accumulated PCM
  prosody → emotion → StyleParams

  if chunked pipeline active:
    progressive.flush(transcript)         emit remaining text as last chunk
    send ChunkEvent{is_utterance_final: true}
    drop channel sender                   signals pipeline completion
    queue source-language passthrough (full host audio)
  else:
    emit_final() → run_pipeline()         legacy full-sentence path

  progressive.reset()
  chunk_index = 0

─── Adaptive endpointing (after 5 FINALs) ────────────────

  Track WPM across 5 utterances
  avg_wpm ≥ 180 → "fast":   endpointing=0.20s, max_dur=5s
  avg_wpm < 120 → "slow":   endpointing=0.35s, max_dur=10s
  else          → "normal":  keep defaults

  If not "normal": send stop_recording, reconnect Gladia with new params
```

---

## Step 3: Chunked Pipeline — Translate + TTS

```
spawn_chunked_pipeline() consumer task:

  Per-language state: HashMap<String, StreamingPcm>

  while let Some(chunk) = chunk_rx.recv().await:

    For EACH target lang (parallel tokio tasks):

    ─── Source lang → SKIP ── (passthrough handled at FINAL) ───

    ─── Target lang ─── TRANSLATE + TTS ───

      if chunk_index == 0:
        StreamingPcm = rtmp_manager.queue_streaming_audio(lang, utterance_start)
        send TtsStart → frontend

      ① Context-aware translation:
         query = "{prev_chunk} ||| {chunk_text}"  (or just chunk_text if first)
         POST Google Translate API v2
         strip context prefix from response (after "|||")
         ~130-376ms

      ② Send ChunkTranslation → frontend (progressive display)

      ③ TTS (tier 2+ only):
         do_tts_ws(translated_text, voice_id, lang, ...)
           ┌─── ElevenLabs Flash v2.5 WebSocket ─────────────┐
           │  BOS: {text:" ", xi_api_key, voice_settings,     │
           │        generation_config:{chunk_length_schedule:[50]}}│
           │  Text: {text: translated, flush: true}            │
           │  EOS: {text: ""}                                  │
           │                                                   │
           │  IncrementalMp3Decoder:                           │
           │    pre-drain stdout (prevent deadlock)             │
           │    for each MP3 chunk from ElevenLabs:            │
           │      decoder.feed(mp3) → PCM bytes                │
           │      streaming.append_with_limit(pcm, max_bytes)  │
           │      → audio drain reads IMMEDIATELY              │
           │    decoder.finish() → remaining PCM               │
           │                                                   │
           │  TTFB: ~75ms (first PCM available)                │
           │  Full generation: continues streaming             │
           └───────────────────────────────────────────────────┘

      Backpressure: if pipeline_elapsed > broadcast_delay, skip chunk

    Wait for all languages to finish this chunk
    Process next chunk

  After channel closes (sender dropped):
    streaming.finish() for all languages
    send TtsEnd → frontend
```

---

## Step 4: Video Path — Frontend to FFmpeg

```
Frontend (every 100ms)                    Backend
──────────────────────                    ───────

MediaRecorder.ondataavailable:
  e.data → arrayBuffer → buf
  tagged = [0x02] + buf               →  rtmp_manager.push_video_chunk()
  ws.send(tagged)                         video_chunks.push_back((Instant::now(), data))

                                       Video drain OS thread:
                                         polls every 20ms
                                         deadline = Instant::now() - broadcast_delay
                                         for each chunk where timestamp ≤ deadline:
                                           stdin.write_all(chunk_data) → FFmpeg pipe:0
```

---

## Step 5: Audio Path — Streaming Buffer to FFmpeg

```
Audio drain OS thread (ffmpeg.rs)

  Runs at 20ms ticks (50 ticks/sec)
  Writes exactly 1764 bytes per tick = 44100Hz × 2B × 1ch × 0.02s

  Each tick:
    ┌── Timing ─────────────────────────────────────────┐
    │  sleep until next_tick                             │
    │  jitter check (>500ms → reset anchor)             │
    │  next_tick += 20ms                                │
    │  target_ts = now() - broadcast_delay              │
    └────────────────────────────────────────────────────┘

    ┌── Pick up queued audio ───────────────────────────┐
    │  if no active utterance:                           │
    │    peek front of audio queue                       │
    │    if queue.front().play_at ≤ target_ts:           │
    │      pop → active_audio                            │
    └────────────────────────────────────────────────────┘

    ┌── Write 1764 bytes ──────────────────────────────┐
    │                                                    │
    │  IF active_audio exists:                           │
    │    lock StreamingPcm mutex (~1μs)                  │
    │    available = pcm.len() - offset                  │
    │                                                    │
    │    available ≥ 1764:                               │
    │      copy & write to FIFO                          │
    │                                                    │
    │    available < 1764 AND complete=true:              │
    │      last chunk — pad silence, mark done           │
    │                                                    │
    │    available < 1764 AND complete=false:             │
    │      TTS still streaming — write SILENCE           │
    │      (inter-chunk gap: ~75ms = 3-4 ticks)          │
    │      next tick will check again                    │
    │                                                    │
    │  IF no active_audio:                               │
    │    write SILENCE                                   │
    │                                                    │
    │  Drift check every 250 ticks (~5s)                 │
    └────────────────────────────────────────────────────┘

  NOTE: Audio drain needed ZERO changes for progressive chunking.
  Multiple TTS chunks feed the same StreamingPcm buffer.
  The drain sees no difference between one TTS and five sequential ones.
```

---

## Step 6: FFmpeg → RTMP → Viewer

```
FFmpeg process (one per language with RTMP URL):

  pipe:0 (stdin)          ← video drain thread writes delayed video chunks
  /tmp/brivva_audio_*     ← audio drain thread writes PCM at 20ms ticks (FIFO)

  Encoding:
    Video: libx264 ultrafast zerolatency, crf=23, maxrate=8Mbps
    Audio: AAC stereo 128kbps
    Container: FLV → RTMP

  Health monitor (tokio task, every 2s):
    for each FFmpeg process: try_wait()
    if crashed: restart (up to 50 retries, 2s delay, preserve audio queue)

  RTMP Ingest → CDN → Viewer
    Platform adds ~5-15 seconds latency
    Total: broadcast_delay + platform_delay = 3s + 5-15s = 8-18s
```

---

## Timing Budget (v15 — Progressive Chunking)

```
Host speaks ──────────────────────┐
  Audio streams to Gladia          │ ~2-3s of speech
  Interims arrive during speech    │
  ProgressiveChunkDetector runs    │
                                   │
  Chunk boundary detected (~1-2s)  │
  ├─ Google Translate              │  130-376ms (context-aware)
  ├─ TTS WebSocket connect         │  ~0ms (reuse) / ~200ms (cold)
  ├─ TTS TTFB                      │  ~75ms (first PCM via IncrementalMp3Decoder)
  ├─ PCM → StreamingPcm            │  immediate
  │                                │
  Another chunk detected (~2-4s)   │
  ├─ Translate + TTS again         │  feeds same StreamingPcm
  │                                │
Host stops ────────────────────────┘
  Endpointing delay                  250-350ms
  FINAL → flush remaining chunk
  Translate + TTS for last chunk

Audio drain picks up at: utterance_start + broadcast_delay (3s)

Per-chunk latency (warm): ~200ms translate + ~75ms TTFB = ~275ms
Per-chunk latency (cold): ~376ms translate + ~275ms TTS = ~651ms

Example: 8-second utterance, 3 chunks:
  Before (v14): 8s speech + 250ms endpointing + 200ms translate + 1500ms TTS batch
                = ~10s silence, then audio plays at 10s + 5s delay = 15s
  After (v15):  Chunk 1 at ~2s + 275ms = audio at 2.3s + 3s delay = 5.3s
                Chunk 2 at ~4.5s, Chunk 3 at ~7s
                ~7.7 second improvement
```

---

## What the Viewer Receives

For source language (EN) with RTMP:
- Video: host camera, delayed by broadcast_delay
- Audio: host's actual voice, delayed by broadcast_delay
- No TTS, no translation cost

For target language (JA) with RTMP:
- Video: same host camera, delayed by broadcast_delay
- Audio: ElevenLabs TTS in host's cloned voice speaking Japanese
- Progressive chunks fill StreamingPcm as they arrive
- Inter-chunk silence (~75ms) is imperceptible
- Synced because both video and audio use the same delay anchor
