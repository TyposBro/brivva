# Brivva End-to-End Flow

Every step from clicking "Start Broadcasting" to the viewer hearing translated audio.

Verified against the codebase as of April 4, 2026.

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
                                             ├─ parse sourceLang → Lang::En
                                             ├─ parse targetLangs → [Ja, Ko]
                                             ├─ session_id = UUID[..8] e.g. "43c1ea7f"
                                             ├─ Session::new(id, En, [Ja,Ko], tier=2)
                                             │    target_langs = [Ja, Ko]
                                             │    voice_clone_id = None
                                             │    broadcast_delay_ms = 5000
                                             │    rtmp_manager = None
                                             │    rtmp_langs = []
                                             │
                                             ├─ send SessionCreated{id} → frontend
                                             │
                                             ├─ spawn start_stt(session_id, En, audio_rx)
                                             │    STT IS NOW LISTENING — before RTMP exists
                                             │    connects to Deepgram Nova-3 WebSocket
                                             │    wss://api.deepgram.com/v1/listen
                                             │      ?model=nova-3&encoding=linear16
                                             │      &sample_rate=44100&channels=1
                                             │      &language=en&punctuate=true
                                             │      &smart_format=true&interim_results=true
                                             │      &endpointing=400&vad_events=true
                                             │      &utterance_end_ms=1500
                                             │
                                             ├─ spawn send_task (host_rx → ws_sink)
                                             └─ spawn recv_task (ws_stream → router)

③ Start webcam (only if RTMP URLs exist):
   getUserMedia({
     video: {width:1920, height:1080,
             frameRate:60, deviceId}
   })
   video.srcObject = stream
   video.play()

   Negotiate MediaRecorder codec:
     try "video/mp4;codecs=avc1.42E01E"    (H.264 in MP4)
     else "video/webm;codecs=h264"          (H.264 in WebM)
     else "video/webm;codecs=vp8"           (VP8 fallback)

   ws.send({type:"video:codec",          →  session.video_codec = "h264"
     codec:"h264"})

   MediaRecorder.start(100)                  chunks every 100ms
   recorder.ondataavailable = (e) => {
     buf = e.data.arrayBuffer()
     tagged = [0x02] + buf                   0x02 = video tag
     ws.send(tagged)
   }

④ Send RTMP config:
   streams = Object.entries(rtmpUrls)
     .filter(([,url]) => url.trim())         only langs with URLs
     .map(([lang,url]) => ({lang, url}))

   ws.send({type:"rtmp:config",          →  lib.rs:200
     streams: [{lang:"ja",                   │
       url:"rtmp://a.rtmp.youtube.com/..."}],│
     broadcastDelay: 3000                    │
   })                                        │
                                             ├─ session.broadcast_delay_ms = 3000
                                             ├─ RtmpManager::with_delay(3000)
                                             ├─ manager.set_video_codec("h264")
                                             │
                                             ├─ For each stream in config:
                                             │    stream_id = "43c1ea7f_ja"
                                             │    manager.start_stream(id, "ja", rtmp_url)
                                             │      │
                                             │      ├─ mkfifo /tmp/brivva_audio_43c1ea7f_ja
                                             │      │
                                             │      ├─ Spawn FFmpeg:
                                             │      │    ffmpeg -y -loglevel warning
                                             │      │      -i pipe:0
                                             │      │      -f s16le -ar 44100 -ac 1
                                             │      │      -i /tmp/brivva_audio_43c1ea7f_ja
                                             │      │      -c:v libx264 -preset ultrafast
                                             │      │       -tune zerolatency -crf 23
                                             │      │       -maxrate 8000k -bufsize 16000k
                                             │      │       -pix_fmt yuv420p -g 60
                                             │      │      -c:a aac -ac:a 2 -b:a 128k
                                             │      │      -map 0:v -map 1:a
                                             │      │      -f flv rtmp://...
                                             │      │
                                             │      ├─ Spawn video drain OS thread
                                             │      │    polls every 20ms
                                             │      │    writes delayed video chunks → FFmpeg stdin
                                             │      │
                                             │      └─ Spawn audio drain OS thread
                                             │           opens FIFO (BLOCKS until FFmpeg reads)
                                             │           then runs at 20ms ticks
                                             │           writes 1764 bytes/tick → FIFO
                                             │
                                             ├─ rtmp_langs = [Ja]
                                             ├─ session.rtmp_manager = shared_mgr
                                             └─ spawn health monitor (checks crashes every 2s)

⑤ Start audio capture:
   AudioPipeline.start(onAudio, deviceId)
     getUserMedia({audio: deviceId || true})
     AudioContext(sampleRate=44100)
     ScriptProcessor(bufferSize=4096, 1in, 1out)

     onaudioprocess fires every ~93ms:       4096 / 44100 = 92.9ms
       float32[4096] → int16[4096]           toInt16(): clamp * 32768
       = 8192 bytes raw PCM
       tagged = [0x01] + PCM                 0x01 = audio tag
       ws.send(tagged)                    →  lib.rs:140
                                             audio_tx.send(data[1..])
                                             → mpsc channel → start_stt

⑥ setIsLive(true)
```

**Key detail:** STT starts at step ② BEFORE RTMP starts at step ④. Audio chunks sent in step ⑤ flow to Deepgram immediately. If the user speaks before RTMP is configured, STT processes it but TTS audio has nowhere to go (rtmp_manager is None, streaming slot creation is skipped).

---

## Step 2: Voice Cloning (overlaps with live streaming)

Voice cloning happens WHILE the user is already live. TTS uses the default Cartesia voice (`694f9389`) until cloning completes, then switches to the cloned voice.

`BroadcastPage.tsx:176` → `cloneVoice()`

```
Frontend                                     Backend
────────                                     ───────

① getUserMedia({audio: true})                SEPARATE mic stream from AudioPipeline
   AudioContext(sampleRate=44100)
   ScriptProcessor(4096, 1, 1)

   onaudioprocess:
     float32 → int16
     push to chunks[]

   ┌── Records for 30 seconds ───────────────────────────┐
   │   Progress bar updates every 200ms                   │
   │   User speaks naturally into mic                     │
   │                                                      │
   │   Meanwhile: AudioPipeline is ALSO capturing         │
   │   to a different AudioContext → STT is running       │
   │   → translations use default voice                   │
   └──────────────────────────────────────────────────────┘

② Combine chunks:
   totalSamples = sum(chunk.length)
   pcm = new Int16Array(totalSamples)
   copy each chunk at offset

③ Encode as base64:
   bytes = new Uint8Array(pcm.buffer)
   binary = String.fromCharCode for each byte
   base64 = btoa(binary)

④ ws.send({type:"voice:sample",          →  lib.rs:167
     audio: base64})                          base64 decode → Vec<u8> raw PCM
                                              30s × 44100 × 2 = 2,646,000 bytes
                                              │
                                              spawn clone_voice()

                                         pipeline.rs:756
                                         ⑤ pcm_to_wav(pcm):
                                              44-byte WAV header:
                                                RIFF, WAVE, fmt (PCM, 44100Hz,
                                                mono, 16-bit), data
                                              + raw PCM bytes

                                         ⑥ POST https://api.cartesia.ai/voices/clone
                                              multipart/form-data:
                                                name: "brivva-43c1ea"
                                                language: "en"
                                                enhance: "false"
                                                clip: voice_sample.wav (audio/wav)
                                              Headers:
                                                X-API-Key: TTS_API_KEY
                                                Cartesia-Version: 2025-04-16

                                         ⑦ Response: {id: "8dee54b7-..."}
                                              session.voice_clone_id = Some("8dee54b7-...")

handleWsMessage:                         ⑧ send ServerMsg::VoiceReady
  msg.type === "voice:ready"            ←     {voiceId: "8dee54b7-..."}
  setVoiceReady(true)
  setIsCloning(false)
  voice clone UI disappears

All subsequent TTS calls use voice_id = "8dee54b7-..." instead of default.
```

---

## Step 3: You Speak — Audio to STT

```
Microphone → MediaStream → AudioContext(44100Hz) → ScriptProcessor(4096)

Every ~93ms:
  float32[4096] → int16[4096] = 8192 bytes
  [0x01] + PCM → WebSocket

lib.rs:140 → audio_tx.send(data[1..])
                │
                ↓ mpsc::unbounded_channel
                │
pipeline.rs:213-234 (STT send_task)
  audio_rx.recv() → data
    │
    ├─ audio_acc.push(data.clone())          accumulates for passthrough
    │                                         drained on each FINAL
    │
    └─ deepgram_ws.send(Binary(data))        streams to Deepgram
```

### Deepgram processes and returns events

```
pipeline.rs:250-400 (STT recv_task)

Deepgram WebSocket → JSON text messages:

─── "Results" with is_final=false (INTERIM) ───────────────────

  transcript = dg.channel.alternatives[0].transcript
  if transcript is empty → skip

  if utterance_start is None:
    utterance_start = Instant::now()             first interim sets the clock

  send Interim{transcript} → frontend           [INTERIM] "The sun had begun"

  chunk_detector.check(transcript):
    MarkerDetector for English checks:
      min_duration elapsed? (1500ms)
      clause boundary found? (", and ", "; ", etc.)
      enough chars after marker? (≥3)
    if yes → send Finalize to Deepgram           forces early FINAL

─── "Results" with speech_final=true (FINAL) ──────────────────

  utterance_counter += 1
  uid = utterance_counter                        [FINAL #7] "descent, painting the horizon"

  utterance_start = take() or now()              grab the timestamp
  host_audio = audio_acc.drain(..).flatten()     grab ALL accumulated PCM since last FINAL

  ┌─ Prosody extraction (stt.rs:238-329) ──────────────────────────┐
  │  PCM bytes → float32 samples (÷ 32768)                         │
  │                                                                 │
  │  energy_rms = sqrt(mean(s²))                                    │
  │                                                                 │
  │  pitch via autocorrelation:                                     │
  │    30ms frames, 10ms hop                                        │
  │    for each frame with enough energy:                           │
  │      find best lag in [sample_rate/500 .. sample_rate/50]       │
  │      pitch_hz = sample_rate / best_lag                          │
  │      keep if 50Hz < pitch < 500Hz and correlation > 0.3         │
  │    pitch_mean = mean(pitches)                                   │
  │    pitch_std = std(pitches)                                     │
  │                                                                 │
  │  pause_density:                                                 │
  │    frame energies → sorted → median                             │
  │    threshold = median × 0.1                                     │
  │    pause_density = count(energy < threshold) / total            │
  │                                                                 │
  │  speaking_rate_wpm = word_count / duration_s × 60               │
  └─────────────────────────────────────────────────────────────────┘

  ┌─ Emotion classification (stt.rs:341-363) ──────────────────────┐
  │  is_loud      = energy > 0.035                                  │
  │  is_quiet     = energy < 0.018                                  │
  │  is_expressive = pitch_std > 85                                 │
  │  is_monotone  = pitch_std < 55                                  │
  │  is_high_pitch = pitch_mean > 200                               │
  │  is_hesitant  = pause_density > 0.4                             │
  │                                                                 │
  │  loud + expressive + high_pitch → "excited"                     │
  │  loud + expressive              → "angry"                       │
  │  loud + !monotone               → "happy"                       │
  │  quiet + monotone + hesitant    → "sad"                         │
  │  quiet + (hesitant | monotone)  → "sad"                         │
  │  !loud + !quiet + monotone      → "serious"                     │
  │  loud                           → "happy"                       │
  │  expressive                     → "happy"                       │
  │  else                           → "neutral"                     │
  └─────────────────────────────────────────────────────────────────┘

  map_style(emotion) → (stability, similarity, style, speed)
    "excited" → speed=1.20    "happy" → speed=1.10
    "angry"   → speed=1.05    "sad"   → speed=0.85
    "serious" → speed=0.95    neutral → speed=1.00

  emit_final(sessions, session_id, transcript, uid,
             source_lang, StyleParams{speed, emotion},
             utterance_start, host_audio)

─── Adaptive endpointing (after 5 FINALs) ────────────────────

  Track WPM across 5 utterances
  avg_wpm ≥ 180 → "fast":   endpointing=300, utterance_end_ms=800
  avg_wpm < 120 → "slow":   endpointing=500, utterance_end_ms=2000
  else          → "normal":  keep defaults

  If not "normal": send CloseStream to Deepgram, reconnect with new params
```

---

## Step 4: emit_final → run_pipeline

```
pipeline.rs:73-115  emit_final()

  send Final{transcript, utterance_id} → frontend

  active_langs = session.active_langs():
    starts with target_langs [Ja, Ko]          from WS query
    adds any rtmp_langs not already present     from rtmp:config
    e.g. if En has RTMP URL → [Ja, Ko, En]
    e.g. if only Ja has RTMP → [Ja, Ko]        Ko has no RTMP

  tokio::spawn → run_pipeline(
    transcript, uid, source_lang=En, active_langs,
    sessions, session_id, style_params, tier=2,
    utterance_start, utterance_end, host_audio
  )
```

### run_pipeline — parallel per language

```
pipeline.rs:469-619

  voice_clone_id = session.voice_clone_id
    Some("8dee54b7") after cloning, None before

  For EACH lang in active_langs (spawned as parallel tokio tasks):

  ─── IF lang == source_lang (e.g. En == En) ── PASSTHROUGH ───

    ONLY happens if source lang is in active_langs,
    which ONLY happens if source lang has an RTMP URL.
    If you didn't enter an RTMP URL for English → no passthrough.

    rtmp_manager.queue_audio("en", host_audio, utterance_start)
      wraps PCM in Arc<Mutex<Vec>> with complete=true
      pushes QueuedAudio to "en" stream's audio queue
    [PASSTHROUGH] Queued host audio for en (150KB)

  ─── IF lang != source_lang (e.g. Ja != En) ── TRANSLATE+TTS ─

    ① Google Translate:
       POST https://translation.googleapis.com/language/translate/v2
         ?key=TRANSLATE_API_KEY
       Body: {q: transcript, source: "en", target: "ja", format: "text"}
       Response: {data: {translations: [{translatedText: "太陽は..."}]}}
       ~130-376ms

    ② Send translation to frontend:
       ServerMsg::Translation{lang:"ja", text:"太陽は...", utteranceId, translateMs}

    ③ TTS (only if tier ≥ 2):
       do_tts(client, translated_text, utterance_id, Ja,
              sessions, session_id, voice_clone_id, style_params,
              utterance_start, utterance_end)
```

---

## Step 5: TTS — Cartesia WebSocket Streaming

```
pipeline.rs:645-760  do_tts()

  broadcast_delay_ms = session.broadcast_delay_ms        3000
  tts_deadline = min(3000 - 500, 10000) = 2500ms        hard timeout

  voice_id = voice_clone_id ("8dee54b7") or default ("694f9389")
  cartesia_emotion = "serious" → "calm", else pass through

  max_bytes = (utterance_dur + 2s) × 88200              overflow limit

  ┌─── QUEUE STREAMING SLOT TO RTMP ── BEFORE TTS STARTS ─────────┐
  │                                                                 │
  │  rtmp_manager.queue_streaming_audio("ja", utterance_start)      │
  │    → creates StreamingPcm:                                      │
  │        pcm:      Arc<Mutex<Vec<u8>>>  (empty, will grow)        │
  │        complete:  AtomicBool = false                             │
  │    → pushes QueuedAudio{play_at: utterance_start, pcm, complete}│
  │      to the "ja" stream's audio queue                           │
  │    → returns StreamingPcm handle to TTS                         │
  │                                                                 │
  │  Audio drain thread will pick this up when:                     │
  │    now() - broadcast_delay ≥ utterance_start                    │
  │  i.e. 3 seconds after the host started speaking                 │
  └─────────────────────────────────────────────────────────────────┘

  send TtsStart{lang:"ja", utteranceId} → frontend

  ┌─── do_tts_ws() with connection pool ───────────────────────────┐
  │                                                                 │
  │  get_pooled_ws():                                               │
  │    pool has connection? → reuse (0ms overhead)                  │
  │    pool empty? → connect_cartesia_ws():                         │
  │      wss://api.cartesia.ai/tts/websocket                       │
  │        ?api_key=TTS_API_KEY                                     │
  │        &cartesia_version=2025-04-16                             │
  │      ~300ms (DNS + TCP + TLS + WS upgrade)                      │
  │                                                                 │
  │  do_tts_on_ws(ws, ...):                                         │
  │    ws.send({                                                    │
  │      context_id: UUID,                                          │
  │      model_id: "sonic-3",                                       │
  │      transcript: "太陽は傾き始めた。",                            │
  │      voice: {mode:"id", id:"8dee54b7"},                         │
  │      output_format: {container:"raw",                           │
  │        encoding:"pcm_s16le", sample_rate:44100},                │
  │      language: "ja",                                            │
  │      generation_config: {speed:1.10, emotion:"happy"}           │
  │    })                                                           │
  │                                                                 │
  │    Read response chunks in a loop:                              │
  │                                                                 │
  │    ◆ {done:false, data:"base64..."}  chunk #1                   │
  │      base64 decode → 12288 bytes raw PCM                        │
  │      [TTS:ja] TTFB 72ms (12288B first chunk)                    │
  │      streaming.append_with_limit(chunk, max_bytes)              │
  │        → pcm.lock().extend_from_slice(chunk)                    │
  │        → audio drain can read this IMMEDIATELY                  │
  │                                                                 │
  │    ◆ {done:false, data:"base64..."}  chunk #2..N                │
  │      more PCM → append to growing buffer                        │
  │      if total ≥ max_bytes → truncate + fadeout, set complete    │
  │                                                                 │
  │    ◆ {done:true}                                                │
  │      break loop                                                 │
  │      [TTS:ja] done: 45 chunks, 540KB in 1200ms                  │
  │                                                                 │
  │  If WS fails: retry with fresh connection                       │
  │  If retry fails: fall back to do_tts_rest() (POST /tts/bytes)   │
  │  Return connection to pool for reuse                            │
  └─────────────────────────────────────────────────────────────────┘

  streaming.finish()                     sets complete = true
  (also set on any error/timeout path)

  send TtsEnd{lang:"ja", utteranceId, ttsMs} → frontend
```

---

## Step 6: Video Path — Frontend to FFmpeg

```
Frontend (every 100ms)                    Backend
──────────────────────                    ───────

MediaRecorder.ondataavailable:
  e.data → arrayBuffer → buf
  tagged = [0x02] + buf               →  lib.rs:146
  ws.send(tagged)                         tag 0x02 = video

                                          rtmp_manager.push_video_chunk(&data[1..])
                                            video_chunks.push_back((Instant::now(), data))
                                            cap at 600 items (~60s buffer)

                                       Video drain OS thread (ffmpeg.rs:540-601):
                                         polls every 20ms
                                         deadline = Instant::now() - broadcast_delay

                                         for each chunk where timestamp ≤ deadline:
                                           stdin.write_all(chunk_data)
                                           → FFmpeg pipe:0 input
                                           chunks_written += 1

                                         Video plays broadcast_delay seconds after capture.
```

---

## Step 7: Audio Path — Streaming Buffer to FFmpeg

```
Audio drain OS thread (ffmpeg.rs:660+)

  Runs at 20ms ticks (50 ticks/sec)
  Writes exactly 1764 bytes per tick
    = 44100Hz × 2 bytes/sample × 1 channel × 0.02s

  Each tick:
    ┌── Timing ─────────────────────────────────────────┐
    │  sleep until next_tick                             │
    │  jitter = actual_time - next_tick                  │
    │                                                    │
    │  if jitter > 500ms:                                │
    │    RECOVERY: reset anchor, skip ticks              │
    │    prevents cascade of late writes after a stall   │
    │                                                    │
    │  if jitter > 100ms:                                │
    │    rate-limited warning (every 25th)               │
    │                                                    │
    │  next_tick += 20ms                                 │
    │  target_ts = now() - broadcast_delay               │
    └────────────────────────────────────────────────────┘

    ┌── Pick up queued audio ───────────────────────────┐
    │  if no active utterance:                           │
    │    peek front of audio queue                       │
    │    if queue.front().play_at ≤ target_ts:           │
    │      pop it → active_audio                         │
    │      active = {pcm (shared), complete, offset=0}   │
    └────────────────────────────────────────────────────┘

    ┌── Write 1764 bytes ──────────────────────────────┐
    │                                                    │
    │  IF active_audio exists:                           │
    │    lock pcm mutex (held ~1μs for 1764B copy)       │
    │    available = pcm.len() - offset                  │
    │                                                    │
    │    available ≥ 1764:                               │
    │      copy pcm[offset..offset+1764]                 │
    │      offset += 1764                                │
    │      write to FIFO                                 │
    │                                                    │
    │    available < 1764 AND complete=true:              │
    │      last chunk — pad with silence                 │
    │      write to FIFO                                 │
    │      active_audio = None (utterance done)          │
    │                                                    │
    │    available < 1764 AND complete=false:             │
    │      TTS still streaming, not enough yet           │
    │      write SILENCE to FIFO (keep active)           │
    │      next tick will check again                    │
    │                                                    │
    │  IF no active_audio:                               │
    │    write SILENCE to FIFO                           │
    │                                                    │
    │  total_bytes_written += 1764                       │
    │                                                    │
    │  Every 250 ticks (~5s): drift check                │
    │    expected = elapsed_seconds × 88200              │
    │    drift = |total_bytes_written - expected|         │
    │    if drift > 50ms worth: warn                     │
    └────────────────────────────────────────────────────┘
```

---

## Step 8: FFmpeg → RTMP → Viewer

```
FFmpeg process (one per language with RTMP URL):

  pipe:0 (stdin)          ← video drain thread writes delayed video chunks
  /tmp/brivva_audio_*     ← audio drain thread writes PCM at 20ms ticks (FIFO)

  Encoding:
    Video: libx264 ultrafast zerolatency, crf=23, maxrate=8Mbps, keyframe every 60 frames
    Audio: AAC stereo 128kbps (mono PCM input upixed to stereo for RTMP compat)
    Container: FLV
    Output: rtmp://...

  Health monitor (tokio task, every 2s):
    for each FFmpeg process: try_wait()
    if crashed (exited unexpectedly):
      if restart_count < 3:
        wait 2s → restart FFmpeg
        reuse audio queue (pending audio preserved)
      else: give up

         │
         ↓

  RTMP Ingest (YouTube / Coupang / Rakuten)
    accepts FLV over RTMP
    transcodes to HLS/DASH
    adds ~5-15 seconds platform latency
    CDN distribution

         │
         ↓

  Viewer's browser/app
    Video: host on camera
    Audio: translated speech in host's cloned voice
    Total latency: broadcast_delay + platform_delay
                   3s             + 5-15s = 8-18s
```

---

## Step 9: Session Cleanup

When the user clicks "Stop" or closes the WebSocket:

```
Frontend                                     Backend (lib.rs:275-294)
────────                                     ───────

stopWebcam()
  MediaRecorder.stop()
  video tracks.stop()

AudioPipeline.stop()
  ScriptProcessor.disconnect()
  mic tracks.stop()
  AudioContext.close()

ws.close()                                →  recv_task ends
                                             │
                                             ├─ session.rtmp_stop = true
                                             │    health monitor sees this → stops
                                             │
                                             ├─ rtmp_manager.stop_all():
                                             │    for each stream:
                                             │      stop_flag = true
                                             │      kill FFmpeg process
                                             │      join video drain thread (3s timeout)
                                             │      join audio drain thread (3s timeout)
                                             │      remove FIFO
                                             │
                                             ├─ if voice_clone_id exists:
                                             │    spawn delete_cloned_voice(voice_id)
                                             │      DELETE https://api.cartesia.ai/voices/{id}
                                             │
                                             └─ sessions.remove(session_id)
```

---

## Timing Budget for One Utterance

With broadcast_delay=3000ms, the entire pipeline must complete within 3s:

```
Host speaks ──────────────────────┐
  Audio streams to Deepgram       │ ~2-3s of speech
  Interims arrive during speech   │
Host stops ───────────────────────┘
  │
  ├─ Endpointing delay               400ms (default) / 500ms (slow speaker)
  │
  ├─ FINAL arrives from Deepgram
  │  Prosody + emotion                ~2ms
  │  emit_final → spawn pipeline
  │
  ├─ Google Translate                 130-376ms
  │
  ├─ queue_streaming_audio            ~0ms (just creates empty slot)
  │
  ├─ TTS WebSocket:
  │    Connection (pooled)            ~0ms (reuse) / ~300ms (cold)
  │    TTFB (first chunk)             ~70ms (pooled) / ~370ms (cold)
  │    Full generation                1200-2400ms (continues streaming)
  │
  └─ Audio drain picks up at:        utterance_start + broadcast_delay
     Starts playing from streaming buffer.
     TTS generation is ~4x realtime, so buffer fills
     faster than drain consumes.

Total pipeline time (warm):   400 + 2 + 200 + 0 + 70 = ~672ms
Remaining buffer:             3000 - 672 = 2328ms ✓ comfortable

Total pipeline time (cold):   400 + 2 + 376 + 370 = ~1148ms
Remaining buffer:             3000 - 1148 = 1852ms ✓ still ok

Total pipeline time (cloned voice, cold):
                              400 + 2 + 376 + 1714 = ~2492ms
Remaining buffer:             3000 - 2492 = 508ms ⚠ tight
                              With 2500ms deadline → TIMEOUT
```

This explains the `[TTS] TIMEOUT: utterance 6 for ja exceeded 2500ms` in the logs. Cloned voices on cold WebSocket connections can exceed the deadline.

---

## What the Viewer Receives

For source language (EN) with RTMP:
- Video: host camera, delayed by broadcast_delay
- Audio: host's actual voice, delayed by broadcast_delay
- No TTS, no translation cost

For target language (JA) with RTMP:
- Video: same host camera, delayed by broadcast_delay
- Audio: Cartesia TTS in host's cloned voice speaking Japanese
- Synced because both video and audio use the same delay anchor (utterance_start + broadcast_delay)
