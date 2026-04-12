# Brivva — Apr 16 Demo Plan

**Date:** April 13, 2026
**Demo:** April 16 (Wednesday), 1-4pm with Korean show hosts + videographer
**Goal:** Flawless live demo → finalize partnership terms

---

## Pre-Demo Build (Apr 13-15)

### Must Ship

| # | Task | Details | Deadline |
|---|---|---|---|
| 1 | **Extended voice recording** | 30s minimum, up to 3 min. Frontend: timer continues past 30s, "minimum reached" at 30s, stop button always visible. Backend: longer WAV → ElevenLabs clone API. | Apr 14 (Mon) |
| 2 | **ElevenLabs V2 model** | Add `eleven_multilingual_v2` as TTS model for cloned voices. Higher quality, ~150-200ms TTFB vs 75ms. Worth the tradeoff for demo quality. Keep Flash as fallback. | Apr 14 (Mon) |
| 3 | **30-min endurance test** | Run continuous Korean→Japanese stream. Monitor: crashes, drift, FIFO starvation, audio pile-up. Fix anything that breaks. | Apr 15 (Tue) |
| 4 | **Korean voice clone test** | Record or find 2-3 min clean Korean speech sample. Clone it. Evaluate quality — must sound human, not robotic. | Apr 15 (Tue) |
| 5 | **Backup demo recording** | Screen-record a flawless 5-min session. Insurance for Wednesday. | Apr 15 (Tue) |

### Do NOT Build

- AWS deployment (desktop works)
- Frontend polish (D1-D4 from todo.md)
- Resilience tasks (B3, C1-C4)
- Skip-ahead logic
- Tier 3-4 lipsync
- Subtitle overlay
- Any new feature not on the list above

---

## Demo Day Script (Apr 16, 1-4pm)

### Setup (1:00-1:15pm)
- Launch app on laptop
- Connect to venue wifi, verify RTMP push works
- Have backup screen recording ready on phone
- Have terms document on phone/printed

### Demo Phase (1:15-3:00pm)
1. **Intro** (5 min): "Let me show you what the product does now."
2. **Voice clone** (5 min): Record Korean host for 2-3 minutes → clone → show it works
3. **Live stream test** (30-45 min):
   - Korean host speaks naturally (live commerce style)
   - Show translated streams: Japanese, English (or Chinese)
   - Source-language passthrough on Korean stream
   - Monitor quality, sync, stability
4. **Q&A from hosts/videographer** (15-30 min): Let them react, ask questions, try speaking themselves

### Terms Phase (3:00-4:00pm)
- Wait for natural break after demo
- "Now that you've seen what this does — let's talk about how we work together."
- Present two options:
  - **Option A:** ₩100M salary, no equity
  - **Option B:** 30% profit share, ₩5M/mo minimum guarantee
- Non-negotiables: Aziz owns code, Brivva pays API costs, written contract
- Don't fill silence. Let them respond.
- If they push back: "What would work for you?" Then negotiate from there.

---

## Demo Checklist

Before leaving for the meeting:

- [ ] App launches without crash
- [ ] Korean voice clone sounds human (2-3 min sample used)
- [ ] V2 model active for cloned voice
- [ ] RTMP stream starts within 10 seconds
- [ ] Translation appears within 3-5 seconds of speech
- [ ] A/V stays synced for 30+ minutes
- [ ] No audio pile-up or gaps
- [ ] Source-language passthrough works (host's own voice on Korean stream)
- [ ] Backup recording on phone
- [ ] Terms document accessible
- [ ] Laptop fully charged + charger packed
- [ ] Test venue wifi before demo (or tether from phone as backup)

---

## Post-Demo (depends on outcome)

**If they say yes (either option):**
- Get terms in writing within 1 week
- Start full-time after F-2-7 (Jun-Aug)
- First paid project: China test stream in May

**If they want to think about it:**
- Don't chase. Demo speaks for itself.
- Follow up once in 3 days: "Let me know when you're ready to move forward."

**If they say no:**
- You have a portfolio piece, Brivva streaming engine on your resume
- Focus on Spiko + Triptych + own company after F-2-7
- Offer Brivva tech as B2B service to other live commerce companies

---

## File Changes for Pre-Demo Build

### Frontend (voice recording extension)
```
frontend/src/features/broadcast/presentation/components/voice-clone-card.tsx
- Recording timer: continues past 30s
- Progress indicator: "Minimum reached ✓" at 30s
- Stop button: always visible after 30s
- Max recording: 3 minutes
- Save longer WAV to backend
```

### Backend (V2 model + longer clone)
```
server-rs/src/shared/tts/config.rs
- Add eleven_multilingual_v2 as model option
- Default to V2 for cloned voices, Flash for defaults

server-rs/src/shared/voice_clone/
- Accept WAV up to 3 min (currently caps at 30s?)
- Verify ElevenLabs API accepts longer samples

server-rs/src/features/broadcast/data/voice_api.rs
- Pass tts_model selection from session config
```

### Testing
```
- 30-min continuous stream test (Korean→Japanese)
- Voice clone quality comparison: 30s sample vs 2-3 min sample
- V2 vs Flash quality comparison on same clone
- Record backup demo video
```
