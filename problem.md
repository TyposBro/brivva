# Real-Time Translated Live Commerce Broadcasting — Latency Optimization

## What We're Building

A desktop app that enables a single live commerce host to broadcast simultaneously in multiple languages. The host speaks naturally in one language. The app translates their speech into N target languages using their cloned voice, so viewers on each platform hear the same person speaking their language. Each platform (YouTube, Coupang, Rakuten) receives a synced video+audio stream in one language.

The goal is to feel as close to a human simultaneous interpreter as possible — but with the host's own cloned voice instead of an interpreter's voice.

## Current Pipeline

```
Host speaks → Gladia STT (real-time transcription)
           → Google Cloud Translation API v2 (~40ms)
           → ElevenLabs Flash v2.5 TTS (voice-cloned audio, ~75ms TTFB)
           → FFmpeg muxes translated audio + delayed video
           → RTMP push to platform
```

## The Latency Problem

The bottleneck is **NOT** any single API call — it's **sentence chunking**. STT must wait for the speaker to finish a thought before translating, because partial sentences produce bad translations.

**Real speech patterns:**
- Short phrase: "This is 50% off" → 2 seconds → fast
- Long sentence: "This product was shipped from Colombia and sold out within one month of launch, and right now we're offering buy one get one free" → 8-10 seconds → viewer waits the entire time before hearing anything

A human simultaneous interpreter starts translating after ~3-4 words (1-2 seconds), working with incomplete context and correcting as they go. Our system waits for a complete utterance (sentence boundary detected by STT), then translates the whole thing at once. This creates **2-10 second gaps** where the viewer sees the host speaking but hears nothing in their language.

## What Makes This Hard

1. **Translation quality vs speed tradeoff** — Translating partial sentences produces grammatically broken or wrong output (especially for structurally different languages like Japanese/Korean where the verb comes last). But waiting for full sentences creates unacceptable lag.

2. **Voice cloning constraint** — TTS generates audio for the complete translated text. You can't easily "stream" cloned voice word-by-word because prosody and intonation depend on the full sentence.

3. **A/V sync** — Video is delayed by a fixed D seconds (jitter buffer). If translation takes longer than D, the audio either gets dropped (silence) or arrives late (sync slip). Variable-length utterances make D hard to tune.

4. **Language asymmetry** — Japanese/Korean sentences can't be translated incrementally the way Spanish/French can from English, because the grammatical structure is fundamentally different (SOV vs SVO).

5. **Live commerce context** — The host is demonstrating products, pointing at things, reacting to chat. Long delays make the translated stream feel disconnected from the visual action.

## Current STT Setup

- Gladia (recently switched from Deepgram for faster transcription)
- Adaptive endpointing: measures host WPM, adjusts utterance boundary detection
- But endpointing is inherently a tradeoff: faster boundaries = more fragments = worse translation

## Constraints

- Desktop app (Tauri + Rust), runs locally, no cloud GPU
- ElevenLabs Flash v2.5 for TTS (staying — best quality+speed balance, Cartesia Sonic 3 tested and rejected)
- Google Cloud Translation API v2 for translation (~40ms, not the bottleneck)
- Must maintain voice cloning quality (host's voice, not generic TTS voice)
- Target: **<3 second average end-to-end latency** (speech → translated audio on stream)
- Must handle 4+ languages simultaneously

## Open Questions

- Strategies for reducing perceived latency without destroying translation quality
- Whether incremental/streaming translation is viable for Korean/Japanese target languages
- How human simultaneous interpreters handle this and what we can borrow
- Whether "chunked progressive translation" (translate partial → refine → re-translate) is worth the complexity
- Best practices for adaptive endpointing tuning
