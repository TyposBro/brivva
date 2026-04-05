Complete Failure Scenario Map

Pipeline Overview (for reference)

Host Audio → [1. STT] → [2. Chunking] → [3. Translation] → [4. TTS] → [5. StreamingPcm] → [6. Audio Drain] → [7. FFmpeg] → [8. RTMP]
[6b. Video Drain] ↗

---

Stage 1: STT (Gladia WebSocket)

┌─────┬────────────────────────────────────────┬────────────────────────────────────┬──────────────────────────────────────────┐
│ # │ Scenario │ Current Handling │ Gap │
├─────┼────────────────────────────────────────┼────────────────────────────────────┼──────────────────────────────────────────┤
│ │ │ │ During reconnect (up to 5s), all speech │
│ 1.1 │ Network drop to Gladia │ Reconnect loop, max 5 attempts, 1s │ is lost. No buffering of host audio for │
│ │ │ delay │ replay. Viewer hears nothing for that │
│ │ │ │ window. │
├─────┼────────────────────────────────────────┼────────────────────────────────────┼──────────────────────────────────────────┤
│ │ │ Logs error, sets │ Same as 1.1 — speech during reconnect │
│ 1.2 │ Gladia returns error │ ExitReason::Disconnected, breaks │ window is lost │
│ │ │ recv loop → reconnect │ │
├─────┼────────────────────────────────────────┼────────────────────────────────────┼──────────────────────────────────────────┤
│ 1.3 │ Gladia WebSocket read error │ Sets Disconnected, breaks → │ Same gap │
│ │ │ reconnect │ │
├─────┼────────────────────────────────────────┼────────────────────────────────────┼──────────────────────────────────────────┤
│ │ │ Retries up to 10 attempts │ Initial connect can block for 30s. No │
│ 1.4 │ Gladia session POST fails (HTTP level) │ (initial) or 5 (reconnect), 3s/1s │ fallback mode — broadcast runs with │
│ │ │ delay │ silence until STT connects │
├─────┼────────────────────────────────────────┼────────────────────────────────────┼──────────────────────────────────────────┤
│ │ │ derive_normalized_position matches │ If revision is too aggressive (>20% │
│ 1.5 │ Gladia revises interim transcript │ ignoring punctuation/whitespace, │ character changes), position resets to 0 │
│ │ (adds punctuation, corrects words) │ 80% threshold │ → re-emits already-translated text as a │
│ │ │ │ new chunk → duplicate audio │
├─────┼────────────────────────────────────────┼────────────────────────────────────┼──────────────────────────────────────────┤
│ │ Gladia sends no is_final for a long │ Waits indefinitely for final — │ If progressive chunking also fails to │
│ 1.6 │ time (host keeps talking without │ ProgressiveChunkDetector sends │ fire (no markers, under min_duration), │
│ │ pause) │ chunks during interims │ text accumulates with zero output │
├─────┼────────────────────────────────────────┼────────────────────────────────────┼──────────────────────────────────────────┤
│ │ Gladia │ Gladia forces a final transcript → │ Could be mid-word/mid-sentence. │
│ 1.7 │ max_duration_without_endpointing hit │ pipeline processes it │ Translation quality degrades. Next │
│ │ (5-10s) │ │ utterance starts fresh with no context. │
├─────┼────────────────────────────────────────┼────────────────────────────────────┼──────────────────────────────────────────┤
│ │ │ Connection attempt fails → │ No notification to frontend that STT is │
│ 1.8 │ STT API key expires mid-stream │ reconnect loop exhausts → STT dies │ dead. Video keeps streaming, no │
│ │ │ silently │ translations. │
├─────┼────────────────────────────────────────┼────────────────────────────────────┼──────────────────────────────────────────┤
│ 1.9 │ Adaptive reconnect triggers (speech │ Sends stop_recording, reconnects │ ~1-2s of speech lost during reconnect. │
│ │ speed classified as fast/slow) │ with new endpointing params │ Only happens once (first 5 utterances). │
└─────┴────────────────────────────────────────┴────────────────────────────────────┴──────────────────────────────────────────┘

---

Stage 2: Progressive Chunk Detection

┌─────┬────────────────────────────────┬───────────────────────────────────┬───────────────────────────────────────────────────┐
│ # │ Scenario │ Current Handling │ Gap │
├─────┼────────────────────────────────┼───────────────────────────────────┼───────────────────────────────────────────────────┤
│ │ Host rambles without clause │ Force-splits at max_duration (2s) │ Translation of arbitrarily-split text produces │
│ 2.1 │ markers (no commas, "and", │ regardless of grammar │ garbage, especially for SOV languages (JA/KO). No │
│ │ "but", etc.) │ │ quality signal back to viewer. │
├─────┼────────────────────────────────┼───────────────────────────────────┼───────────────────────────────────────────────────┤
│ │ Very short utterance │ No progressive chunks emitted. │ Legacy path has no streaming TTS — waits for full │
│ 2.2 │ (<min_duration, 800-1000ms) │ Falls through to legacy │ utterance, translates, then calls do_tts (REST, │
│ │ │ full-utterance path on final │ not WS streaming). Audio arrives late. │
├─────┼────────────────────────────────┼───────────────────────────────────┼───────────────────────────────────────────────────┤
│ │ Rapid-fire short phrases │ Each becomes a separate legacy │ Each triggers a separate pipeline with TTS │
│ 2.3 │ ("yes", "ok", "right") │ utterance │ connection overhead. Could overwhelm ElevenLabs │
│ │ │ │ with concurrent WebSocket connections. │
├─────┼────────────────────────────────┼───────────────────────────────────┼───────────────────────────────────────────────────┤
│ 2.4 │ chunk_pipeline_tx channel full │ try_send fails, chunk is dropped │ Dropped chunk = untranslated segment. Viewer │
│ │ (capacity=6) │ with error log │ hears a gap. No retry, no fallback. │
├─────┼────────────────────────────────┼───────────────────────────────────┼───────────────────────────────────────────────────┤
│ 2.5 │ Force-split at max_duration │ Splits at character boundary, not │ Broken words → broken translation. "I was walk-" │
│ │ cuts mid-word │ word boundary │ → translates partial word. │
├─────┼────────────────────────────────┼───────────────────────────────────┼───────────────────────────────────────────────────┤
│ │ Host code-switches (mixes │ Gladia has code_switching: true │ Mixed-language chunk gets translated as │
│ 2.6 │ languages mid-sentence) │ but chunks are translated from │ source_lang → garbled output for the switched │
│ │ │ single source_lang │ segment. │
└─────┴────────────────────────────────┴───────────────────────────────────┴───────────────────────────────────────────────────┘

---

Stage 3: Translation (Google Cloud Translation API v2)

┌─────┬────────────────────────────┬─────────────────────────────────┬─────────────────────────────────────────────────────────┐
│ # │ Scenario │ Current Handling │ Gap │
├─────┼────────────────────────────┼─────────────────────────────────┼─────────────────────────────────────────────────────────┤
│ │ │ │ CRITICAL: A hung translation request blocks the │
│ 3.1 │ No timeout on translation │ reqwest::Client default timeout │ pipeline forever. The chunk never completes, TTS never │
│ │ request │ (none unless configured) │ fires, audio drain writes silence indefinitely for that │
│ │ │ │ utterance. │
├─────┼────────────────────────────┼─────────────────────────────────┼─────────────────────────────────────────────────────────┤
│ │ │ Returns Err, logs error, │ No retry. Chunk lost. If rate-limited, ALL concurrent │
│ 3.2 │ Google rate limit (429) │ translate_chunk returns None → │ language translations for that chunk fail. │
│ │ │ chunk skipped │ │
├─────┼────────────────────────────┼─────────────────────────────────┼─────────────────────────────────────────────────────────┤
│ │ Google API key │ HTTP error → Err → every │ No notification to user. Broadcast continues with no │
│ 3.3 │ invalid/expired │ translation fails │ translations. Host sees nothing in UI indicating │
│ │ │ │ failure. │
├─────┼────────────────────────────┼─────────────────────────────────┼─────────────────────────────────────────────────────────┤
│ 3.4 │ Google API intermittent │ Same as 3.2 — error logged, │ No retry with backoff. Transient errors cause permanent │
│ │ errors (500, 503) │ chunk skipped │ gaps. │
├─────┼────────────────────────────┼─────────────────────────────────┼─────────────────────────────────────────────────────────┤
│ │ │ │ If Google translates or removes the separator, the │
│ 3.5 │ Context separator ||| │ strip_context_prefix looks for │ entire context+chunk is returned as one string. │
│ │ mangled by Google │ ||| in output │ Previous chunk's translation leaks into current chunk's │
│ │ │ │ audio. │
├─────┼────────────────────────────┼─────────────────────────────────┼─────────────────────────────────────────────────────────┤
│ 3.6 │ Translation returns empty │ Passed to TTS → TTS generates │ No guard against empty translation. ElevenLabs may │
│ │ string │ nothing or errors │ error or produce silence. │
├─────┼────────────────────────────┼─────────────────────────────────┼─────────────────────────────────────────────────────────┤
│ │ Translation much longer │ TTS generates proportionally │ Audio for one chunk overruns the time window, pushes │
│ 3.7 │ than source (e.g., JA→EN │ more audio │ subsequent chunks' audio later → cascading delay. │
│ │ can expand 2-3x) │ │ │
├─────┼────────────────────────────┼─────────────────────────────────┼─────────────────────────────────────────────────────────┤
│ │ │ Each lang logs error │ StreamingPcm is created but never gets data → complete │
│ 3.8 │ All translations for an │ independently. Pipeline │ never set to true → audio drain waits forever for this │
│ │ utterance fail │ completes with no output. │ utterance, writing silence while waiting → blocks all │
│ │ │ │ subsequent audio for that language │
└─────┴────────────────────────────┴─────────────────────────────────┴─────────────────────────────────────────────────────────┘

---

Stage 4: TTS (ElevenLabs WebSocket)

#: 4.1
Scenario: TTS timeout (2.5s for 3s broadcast delay)
Current Handling: tokio::time::timeout → logs TIMEOUT
Gap: StreamingPcm may have partial data. finish() is NOT called on timeout → audio drain never sees complete=true → drains whatever
PCM was produced, then writes silence forever, blocking subsequent utterances
────────────────────────────────────────
#: 4.2
Scenario: ElevenLabs rate limit / concurrent connection limit
Current Handling: WS connect fails → Err → logs error
Gap: No retry. That chunk gets no audio. If this happens to chunk 0, StreamingPcm is never created at all.
────────────────────────────────────────
#: 4.3
Scenario: ElevenLabs returns 0 audio bytes
Current Handling: Warns in log
Gap: StreamingPcm has empty buffer but gets finish() called → audio drain plays 0 bytes of audio (gap in stream)
────────────────────────────────────────
#: 4.4
Scenario: Voice clone ID deleted/invalid
Current Handling: Falls back to default_voice
Gap: Silent fallback — host doesn't know their cloned voice isn't being used
────────────────────────────────────────
#: 4.5
Scenario: IncrementalMp3Decoder fails
Current Handling: Returns Err → logged
Gap: Partial PCM may have been appended to StreamingPcm before failure. Truncated audio plays.
────────────────────────────────────────
#: 4.6
Scenario: ElevenLabs WebSocket closes unexpectedly
Current Handling: receive_loop ends, drain_remaining runs
Gap: If close happens mid-stream, whatever was decoded so far is kept. But finish() may not be called properly.
────────────────────────────────────────
#: 4.7
Scenario: Multiple concurrent TTS connections per utterance (one per target language)
Current Handling: All spawned in parallel as tokio tasks
Gap: 4 languages = 4 concurrent ElevenLabs WS connections. ElevenLabs has connection limits per API key. Under load, some
connections
may fail.
────────────────────────────────────────
#: 4.8
Scenario: TTS generates audio longer than the utterance
Current Handling: append_with_limit truncates at max_bytes (broadcast_delay + 5s padding = ~8s of audio)
Gap: 8s of audio for a 2s chunk is generous, but if translation expanded significantly AND speech is slow, truncation causes abrupt
cut-off

---

Stage 5: StreamingPcm + Audio Queue

┌─────┬───────────────────────────────┬───────────────────────────────────┬─────────────────────────────────────────────────────┐
│ # │ Scenario │ Current Handling │ Gap │
├─────┼───────────────────────────────┼───────────────────────────────────┼─────────────────────────────────────────────────────┤
│ │ StreamingPcm never marked │ │ CRITICAL: Audio drain will drain whatever PCM │
│ 5.1 │ complete (TTS error/timeout │ Audio drain checks complete flag │ exists, then write silence INDEFINITELY while │
│ │ without calling finish()) │ before clearing utterance │ waiting for more data. Blocks ALL subsequent │
│ │ │ │ utterances for that language stream. │
├─────┼───────────────────────────────┼───────────────────────────────────┼─────────────────────────────────────────────────────┤
│ │ │ │ If utterances arrive faster than they can be played │
│ 5.2 │ Audio queue depth grows │ No limit on VecDeque<QueuedAudio> │ (host talks fast, translations pile up), memory │
│ │ unbounded │ size │ grows. More importantly, audio falls further and │
│ │ │ │ further behind video. │
├─────┼───────────────────────────────┼───────────────────────────────────┼─────────────────────────────────────────────────────┤
│ │ │ │ If translation took 5s but play_at is │
│ 5.3 │ Utterances queued with stale │ Audio drain pops when target_ts │ utterance_start (5s ago), the audio immediately │
│ │ play_at timestamps │ >= front.play_at │ becomes eligible → plays without proper delay │
│ │ │ │ alignment. │
├─────┼───────────────────────────────┼───────────────────────────────────┼─────────────────────────────────────────────────────┤
│ │ Multiple StreamingPcm for │ Chunk 0 creates the StreamingPcm. │ If chunk 0's translation fails but chunk 1 │
│ 5.4 │ same utterance (chunk 0 │ Chunks 1+ clone and append to │ succeeds, the StreamingPcm was never created → │
│ │ creates slot, what about │ same buffer. │ chunk 1's task.streaming is None → audio silently │
│ │ chunk 1+?) │ │ lost. │
├─────┼───────────────────────────────┼───────────────────────────────────┼─────────────────────────────────────────────────────┤
│ 5.5 │ No stream found for language │ queue_streaming_audio warns, │ TTS writes to orphan buffer that nothing reads. │
│ │ │ returns orphan StreamingPcm │ Wasted compute + memory. │
└─────┴───────────────────────────────┴───────────────────────────────────┴─────────────────────────────────────────────────────┘

---

Stage 6: Audio Drain Thread

┌─────┬───────────────────────────┬──────────────────────────┬─────────────────────────────────────────────────────────────────┐
│ # │ Scenario │ Current Handling │ Gap │
├─────┼───────────────────────────┼──────────────────────────┼─────────────────────────────────────────────────────────────────┤
│ 6.1 │ FIFO write error (FFmpeg │ Drain thread exits │ No mechanism to signal this back. Health monitor detects FFmpeg │
│ │ died, pipe broken) │ │ crash separately. Audio queue keeps growing. │
├─────┼───────────────────────────┼──────────────────────────┼─────────────────────────────────────────────────────────────────┤
│ │ Jitter > 500ms (OS │ Recovery mode: writes │ During recovery, drains active audio fast → may produce audible │
│ 6.2 │ scheduler, CPU spike) │ catch-up buffer (up to │ artifacts (sped-up-sounding audio). Beyond 2s of jitter, ticks │
│ │ │ 2s), resets tick anchor │ are skipped → gap in audio stream. │
├─────┼───────────────────────────┼──────────────────────────┼─────────────────────────────────────────────────────────────────┤
│ │ Drift accumulates over │ Drift check every 250 │ Warning only. No correction mechanism. Over a 4-hour stream, │
│ 6.3 │ long session │ ticks (~5s), warns at │ drift could reach hundreds of ms. │
│ │ │ >50ms │ │
├─────┼───────────────────────────┼──────────────────────────┼─────────────────────────────────────────────────────────────────┤
│ │ Audio drain plays silence │ │ This IS the problem the user describes. When host rambles, │
│ 6.4 │ for extended period (no │ Writes 0-bytes at 20ms │ video plays fine (it's just delayed frames), but audio is │
│ │ audio queued) │ ticks │ silence because translation pipeline hasn't produced output │
│ │ │ │ yet. │
└─────┴───────────────────────────┴──────────────────────────┴─────────────────────────────────────────────────────────────────┘

---

Stage 7: Video Drain + FFmpeg

┌─────┬────────────────────────────────────┬────────────────────────────┬──────────────────────────────────────────────────────┐
│ # │ Scenario │ Current Handling │ Gap │
├─────┼────────────────────────────────────┼────────────────────────────┼──────────────────────────────────────────────────────┤
│ │ │ Health monitor detects │ 2s detection + 2s restart delay = 4s gap. RTMP │
│ 7.1 │ FFmpeg process crash │ every 2s, restarts (max 50 │ connection to platform drops → viewers see "stream │
│ │ │ attempts) │ offline" briefly. │
├─────┼────────────────────────────────────┼────────────────────────────┼──────────────────────────────────────────────────────┤
│ 7.2 │ Video buffer overflow (>600 │ Drops oldest chunks │ Dropped video frames = visible stutter or jump for │
│ │ chunks) │ │ viewers │
├─────┼────────────────────────────────────┼────────────────────────────┼──────────────────────────────────────────────────────┤
│ │ │ │ Same restart path as 7.1. Platform-specific issues │
│ 7.3 │ RTMP server rejects/disconnects │ FFmpeg exits with error │ (bad stream key, account suspension) cause permanent │
│ │ │ │ restart loop. │
├─────┼────────────────────────────────────┼────────────────────────────┼──────────────────────────────────────────────────────┤
│ │ FFmpeg stdin pipe breaks (video │ Video drain thread exits │ If video drain dies but audio drain is alive (or │
│ 7.4 │ drain writes to dead process) │ on write error │ vice versa), FFmpeg gets only one input → corrupted │
│ │ │ │ stream │
└─────┴────────────────────────────────────┴────────────────────────────┴──────────────────────────────────────────────────────┘

---

Stage 8: Frontend ↔ Backend Communication

┌─────┬─────────────────────────┬───────────────────────────────────────────┬──────────────────────────────────────────────────┐
│ # │ Scenario │ Current Handling │ Gap │
├─────┼─────────────────────────┼───────────────────────────────────────────┼──────────────────────────────────────────────────┤
│ │ Host WebSocket │ Session removed from DashMap → │ In-flight TTS/translation tasks continue running │
│ 8.1 │ disconnect │ STT/pipelines check sessions.contains_key │ until they finish (wasted resources). No │
│ │ │ and stop │ graceful cancellation. │
├─────┼─────────────────────────┼───────────────────────────────────────────┼──────────────────────────────────────────────────┤
│ 8.2 │ Frontend doesn't │ Sent via send_to_host │ If WS buffer full or host disconnected, messages │
│ │ receive TtsStart/TtsEnd │ │ silently dropped. UI state goes stale. │
└─────┴─────────────────────────┴───────────────────────────────────────────┴──────────────────────────────────────────────────┘

---

Cross-Cutting: The Rambling / Drift Problem

This is the root cause of what you're experiencing:

Timeline (host rambles for 10s straight):

0s Host starts talking
Video: being captured, buffered with 3s delay
Audio: STT receiving, waiting for chunk boundary

1.0s Progressive detector min_duration not yet reached
Video: chunk at t=0 plays on platform (delayed by 3s, so plays at t=3)
Audio: silence playing on platform

2.0s max_duration hit → force-split chunk 0 ("first 2s of speech")
chunk 0 → translate (~200ms) → TTS WS connect (~100ms) → TTFB (~75ms) → streaming
Video: playing fine
Audio: STILL SILENCE (chunk 0 not yet produced)

2.4s Chunk 0 first PCM arrives in StreamingPcm
But play_at = utterance_start (t=0), and audio drain's target_ts = now - 3s delay = t=-0.6s
So target_ts < play_at → NOT YET ELIGIBLE → more silence

3.0s target_ts = 0s = play_at → audio drain starts playing chunk 0
Meanwhile host is at second 3 of their speech
Video shows host at t=0 (3s delay) — IN SYNC with chunk 0 audio ✓

4.0s Another force-split → chunk 1 → translate → TTS...
Chunk 1's play_at = utterance_start (still t=0, same utterance!)

        Wait — this is where it breaks. ALL chunks share the same utterance_start
        and the same StreamingPcm buffer. Chunks append sequentially.
        Audio drain plays buffer linearly.

        So chunk 1's audio appends AFTER chunk 0's audio in the buffer.
        It plays right after chunk 0 finishes — correct ordering.

        BUT: chunk 1 took 2.4s to produce (2s chunk + 0.4s pipeline).
        If chunk 0 audio was 2s long, it finishes playing at t=5s.
        Chunk 1 audio starts at t=5s.
        Video at t=5s shows host at t=2s.
        Chunk 1 audio is for speech at t=2-4s → audio at t=5s for video at t=2s.
        That's 3s of audio covering speech from t=2-4s playing alongside video from t=2s.
        STILL OK because broadcast delay absorbs it.

THE PROBLEM: when pipeline latency > broadcast delay

6.0s Chunk 2 arrives but... chunks 0+1 audio is still playing (4s of audio)
Audio drain is at offset ~3s into the buffer
Chunk 2 appends at the end. Will play at ~7s.
Video at 7s shows host at t=4s. Chunk 2 covers t=4-6s.
→ audio is now 2s BEHIND the matching video frame

8-10s The gap keeps growing. Each chunk adds ~0.4s of pipeline overhead.
After 5 chunks: 5 × 0.4s = 2s of accumulated lag.
With 3s delay, this is still marginal.
But if translation is slow (376ms) or TTS is slow, it exceeds the buffer.

Key insight from the code: When audio falls behind, there is no catch-up mechanism. The audio drain just keeps playing at real-time
speed. It can't:

- Skip stale audio
- Speed up playback
- Drop old utterances
- Signal the video drain to slow down

---

The 7 Unhandled Critical Gaps

1. Translation has no timeout (pipeline_chunk.rs:255 — raw translate() call with no tokio::time::timeout). A hung Google API request
   stalls the entire chunk pipeline forever.
2. TTS timeout doesn't call finish() (pipeline_chunk.rs:323-324). After timeout, StreamingPcm.complete stays false → audio drain
   writes silence forever for that language, blocking all subsequent utterances.
3. No audio queue depth limit or staleness eviction. If audio falls behind, old utterances pile up. There's no mechanism to say
   "this utterance is too old, skip it."
4. No drift detection between audio and video. Audio drain tracks byte-level drift, but there's no concept of "audio is N seconds
   behind the video timeline." No signal to the frontend or the pipeline.
5. No backpressure from audio to STT. When audio is behind, the system keeps accepting new speech, translating, and queueing —
   making the problem worse.
6. No graceful degradation. When TTS can't keep up, there's no fallback to subtitle-only mode, no audio speed-up, no utterance
   skipping.
7. Legacy pipeline path has no streaming TTS. Short utterances (<1s) go through pipeline_full.rs which calls do_tts (REST, not WS
   streaming) — slower first-byte time, no incremental decode.

---

Want me to enter plan mode and design solutions for these gaps? I'd prioritize:

1. Translation timeout (quick fix)
2. finish() on TTS timeout/error (quick fix)
3. Audio queue staleness eviction (medium)
4. Audio-video drift monitor with skip logic (the real fix for rambling)

✻ Churned for 4m 52s

⏺ Agent "Explore frontend broadcast state" completed

⏺ Agent "Explore broadcast pipeline code" completed

⏺ Agent "Explore error handling and edge cases" completed

⏺ All three agents completed. Let me add the frontend-side failure scenarios from the exploration:

---

Stage 9: Frontend (from agent findings)

┌─────┬────────────────────────┬────────────────────────────────┬──────────────────────────────────────────────────────────────┐
│ # │ Scenario │ Current Handling │ Gap │
├─────┼────────────────────────┼────────────────────────────────┼──────────────────────────────────────────────────────────────┤
│ │ │ onclose shows "Connection lost │ No auto-reconnect. User must manually restart the entire │
│ 9.1 │ Host WebSocket drops │ (code X). Restart to │ broadcast. All RTMP streams die. │
│ │ │ reconnect." │ │
├─────┼────────────────────────┼────────────────────────────────┼──────────────────────────────────────────────────────────────┤
│ 9.2 │ Audio chunks sent to │ ws.readyState === OPEN check │ If WS closes between check and send → silent failure. Audio │
│ │ closed WebSocket │ before send │ frames lost. │
├─────┼────────────────────────┼────────────────────────────────┼──────────────────────────────────────────────────────────────┤
│ 9.3 │ Video MediaRecorder │ Same readyState check, no │ No buffering. If WS briefly disconnects, video frames are │
│ │ chunks dropped │ queue │ permanently lost. │
├─────┼────────────────────────┼────────────────────────────────┼──────────────────────────────────────────────────────────────┤
│ │ No pipeline health │ Frontend shows interim/final │ No latency metrics, no "audio behind" indicator, no TTS │
│ 9.4 │ visibility │ transcripts and errors │ failure count, no queue depth. Host has no idea translations │
│ │ │ │ are failing or falling behind. │
├─────┼────────────────────────┼────────────────────────────────┼──────────────────────────────────────────────────────────────┤
│ │ Error banner │ Errors pushed to array, │ Repeated errors (e.g., TTS timeout every utterance) spam │
│ 9.5 │ dismissed, same error │ dismissible │ error list. No deduplication or rate limiting. │
│ │ recurs │ │ │
├─────┼────────────────────────┼────────────────────────────────┼──────────────────────────────────────────────────────────────┤
│ 9.6 │ No graceful │ Either broadcasting or not │ No "subtitles-only fallback" button, no "audio is behind by │
│ │ degradation UI │ │ Xs" warning, no "STT disconnected" status indicator │
└─────┴────────────────────────┴────────────────────────────────┴──────────────────────────────────────────────────────────────┘

---

Additional gaps from agent findings

#: A.1
Gap: TTS REST fallback in orchestrator.rs exists but is NOT used by chunked pipeline
Details: pipeline_chunk.rs calls do_tts_ws directly (line 316). If WS fails, there's no REST fallback for the streaming path — only
the legacy full-utterance path has it.
────────────────────────────────────────
#: A.2
Gap: finish() not called on translation failure
Details: In pipeline_chunk.rs:234-247, if translate_chunk returns None, the function returns early. If this was the only chunk for
that utterance, complete_pipeline still runs and calls finish(). But if chunk 0 fails, the StreamingPcm slot was already created
(line 166) → subsequent chunks may work, so this is OK. However, if ALL chunks fail for a language, the StreamingPcm gets
finish()
called with 0 bytes → audio drain plays silence briefly then moves on. This is actually handled correctly.
────────────────────────────────────────
#: A.3
Gap: IncrementalMp3Decoder has internal timeouts
Details: finish() has 500ms timeout, per-read is 5ms loop. If FFmpeg subprocess hangs, decoder can deadlock for up to 500ms. Under
TTS timeout budget this is fine, but it eats into the 2500ms budget.

---

Revised Priority List

Based on everything — here are the fixes ranked by impact on the rambling/desync problem:

P0 — Causes data loss or infinite hangs

1. Translation timeout — wrap translate() in tokio::time::timeout. A hung Google API request blocks the chunk pipeline forever.
2. finish() on TTS timeout — after timeout at pipeline_chunk.rs:323, call streaming.finish() so audio drain doesn't wait forever.

P1 — Causes unbounded audio drift (the rambling problem)

3. Audio queue staleness eviction — if an utterance's play_at is older than now - 2\*broadcast_delay, drop it. Prevents unbounded
   queue growth.
4. Audio-video drift monitor — track queue_depth \* avg_utterance_duration vs real time. When drift exceeds threshold, skip oldest
   utterances.
5. Utterance overlap handling — when a new utterance arrives but old one is still playing, decide: finish current? truncate?
   crossfade?

P2 — Improves resilience

6. Translation retry with backoff — retry once on 5xx/timeout before giving up.
7. Frontend pipeline health dashboard — expose queue_depth, drift_ms, tts_failure_count, stt_connected to frontend via new ServerMsg
   types.
8. Graceful degradation — auto-switch to subtitle-only when TTS can't keep up (drift > threshold).

P3 — Nice to have

9. Frontend auto-reconnect — reconnect WebSocket on transient disconnect.
10. TTS REST fallback for chunked pipeline — fall back to REST when WS fails.
11. Chunk channel drop recovery — retry dropped chunks or notify frontend.
