# Real-Time Translated Live Commerce Broadcasting — Latency Optimization

## What We're Building

A desktop app that enables a single live commerce host to broadcast simultaneously in multiple languages. The host speaks naturally in one language. The app translates their speech into N target languages using their cloned voice, so viewers on each platform hear the same person speaking their language. Each platform (YouTube, Coupang, Rakuten) receives a synced video+audio stream in one language.

The goal is to feel as close to a human simultaneous interpreter as possible — but with the host's own cloned voice instead of an interpreter's voice.

## Current Pipeline (v15 — Progressive Chunking)

```
Host speaks → Gladia STT (real-time transcription)
           → ProgressiveChunkDetector (clause-boundary splitting during speech)
           → Google Cloud Translation API v2 (~130-376ms, context-aware with ||| separator)
           → ElevenLabs Flash v2.5 TTS (voice-cloned, incremental MP3→PCM streaming)
           → Single StreamingPcm accumulator per (utterance, language)
           → FFmpeg muxes translated audio + delayed video
           → RTMP push to platform
```

## The Latency Problem

The bottleneck is **NOT** any single API call — it's **sentence chunking**. STT must wait for the speaker to finish a thought before translating, because partial sentences produce bad translations.

**Real speech patterns:**

- Short phrase: "This is 50% off" → 2 seconds → fast
- Long sentence: "This product was shipped from Colombia and sold out within one month of launch, and right now we're offering buy one get one free" → 8-10 seconds → viewer waits the entire time before hearing anything

A human simultaneous interpreter starts translating after ~3-4 words (1-2 seconds), working with incomplete context and correcting as they go. Our v14 system waited for a complete utterance (sentence boundary detected by STT), then translated the whole thing at once. This created **2-10 second gaps**.

## What We Built (v15)

### Three-pronged attack on latency:

**1. True Streaming TTS Decode** — Previously, `do_tts_ws` accumulated ALL MP3 chunks from ElevenLabs into a buffer, waited for `isFinal`, then batch-decoded to PCM. Now `IncrementalMp3Decoder` (long-lived FFmpeg subprocess) decodes each MP3 chunk as it arrives and appends to `StreamingPcm` immediately. Audio drain gets real PCM within ~75ms (TTFB) instead of waiting for full generation. **Saves 500-1500ms per utterance.**

**2. Progressive Clause Chunking** — `ProgressiveChunkDetector` splits long utterances at clause boundaries during interim transcripts. Each chunk is translated and TTS'd independently. Language-adaptive thresholds: EN 1000ms/2000ms, JA/KO 800ms/2000ms. Context-aware translation (previous chunk prepended with `|||` separator). Handles Gladia's transcript revisions via normalized character matching (`derive_position`).

**3. Single StreamingPcm Accumulator** — Multiple TTS chunks feed the same `StreamingPcm` buffer per (utterance, language). The audio drain loop in `ffmpeg.rs` needed **zero changes**. Inter-chunk silence (~75ms TTFB = 3-4 ticks) is imperceptible.

### Expected improvement:

- **Before** (8s utterance): ~13s total silence (8s speech + 5s delay)
- **After** (same, 3 chunks): first audio at ~5.3s (2.3s chunk + 3s delay). **~7.7s improvement.**

## What Makes This Hard

1. **Translation quality vs speed tradeoff** — Translating partial sentences produces grammatically broken output (especially SOV languages). Mitigated by context-aware translation.

2. **Voice cloning constraint** — TTS prosody depends on sentence context. Short chunks may sound less natural. Mitigated by using clause-level (not word-level) boundaries.

3. **A/V sync** — Fixed-delay jitter buffer (now 3s default, reduced from 5s). Multiple TTS chunks feed single StreamingPcm; audio drain writes silence during inter-chunk gaps.

4. **Language asymmetry** — SOV languages (JA/KO) get slightly longer min_duration (800ms) than SVO (EN 1000ms). Clause markers include verb-final patterns.

5. **STT transcript instability** — Gladia revises interim text (adds punctuation, corrects words). `ProgressiveChunkDetector` uses `derive_position` with normalized character matching to handle revisions.

## Strategic Pivot — Build vs Buy the Pipeline

The industry is waking up to this exact problem. Several tools now handle semantic chunking and live translation natively:

- **Alibaba Qwen3-LiveTranslate-Flash-Realtime** — unified WebSocket API with semantic unit prediction for SOV languages. Could replace Gladia + Google Translate.
- **OpenAI Realtime API (`semantic_vad`)** — semantic endpointing that understands grammar, not just silence.
- **Soniox v4** — STT with native semantic endpointing.
- **Palabra AI / Pinch API** — end-to-end speech-to-speech translation in a single WebSocket.

**Key insight: the translation pipeline is a commodity input, not the moat.** Brivva's value is the live commerce broadcasting experience — multi-platform RTMP muxing, A/V sync engineering, voice clone persistence, seller UX.

### Why Palabra/Pinch don't replace Brivva

Evaluated 2026-04-04. Palabra pricing makes it unviable for live commerce:

- Broadcaster costs 60-80 credits/hour depending on plan
- Business plan ($2917/mo) gives ~58 hours — a host streaming 160hr/month needs ~$8,750/mo **per language**
- 4 languages = ~$35,000/month per host vs ~$500/month with own pipeline (Gladia + Google Translate + ElevenLabs)
- Palabra handles translation but NOT: multi-platform RTMP muxing, A/V sync, desktop app UX, voice clone persistence

### Google DeepMind End-to-End S2ST (Nov 2025, blog published)

Evaluated 2026-04-04. Google shipped a real-time end-to-end speech-to-speech translation model in Meet and Pixel 10:

- **2-second fixed delay** — same "fixed-delay jitter buffer" concept as our `ffmpeg.rs` broadcast delay. Validates the architectural approach.
- **End-to-end audio-to-audio** — no cascade (STT→Translate→TTS). A streaming encoder/decoder with RVQ audio tokens predicts translated audio directly. Eliminates the chunking problem entirely — the model learns _when_ to start translating from time-synchronized training data.
- **Voice preservation built-in** — custom TTS preserves speaker voice characteristics without a separate cloning step.
- **5 Latin language pairs only**: EN ↔ ES/DE/FR/IT/PT. No CJK support. They acknowledge "languages with word orders significantly different from English" need longer lookahead and are future work. Hindi "promising" but not shipped.
- **Not available as an API** — embedded in Meet (server-side) and Pixel 10 (on-device). Cannot be purchased or integrated.

**Implications for Brivva:**

- Our 3s delay with a cascade pipeline is competitive with Google's 2s end-to-end model
- Our JA/KO/ZH support (SOV clause chunking) is a differentiator — Google doesn't cover these yet
- When/if this ships as a Cloud API, it could replace our entire STT→Translate→TTS cascade
- Confirms the strategic read: pipeline is commodity, RTMP muxing + A/V sync + multi-platform UX is the moat

**Next step:** Evaluate Soniox v4 or Qwen3 as STT/chunking upgrades (semantic endpointing would eliminate `ProgressiveChunkDetector`). Monitor Google Cloud for S2ST API availability. Keep `ffmpeg.rs` (RTMP muxing, jitter buffer) intact — that's the real engineering no translation API touches.

## Constraints

- Desktop app (Tauri + Rust), runs locally, no cloud GPU
- ElevenLabs Flash v2.5 for TTS (current — may be replaced by end-to-end API)
- Must maintain voice cloning quality (host's voice, not generic TTS voice)
- Target: **<3 second average end-to-end latency** (speech → translated audio on stream)
- Must handle 4+ languages simultaneously
- Default broadcast delay reduced from 5000ms to 3000ms
