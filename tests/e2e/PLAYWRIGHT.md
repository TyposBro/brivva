# Real-stack Playwright e2e — combination playbook

The single test at `frontend/e2e/full-stack-live.e2e.ts` is fully
parameterized. The shell wrapper at `scripts/test-e2e-real.sh` parses
CLI flags, validates env vars, then exports the matrix to Playwright.

```bash
./scripts/test-e2e-real.sh --help
```

**Different from the smoke test in `README.md`:** that one runs the
server-rs stack inside docker-compose with stubbed Soniox + ElevenLabs.
This one drives the **real local dev stack** (frontend + workers +
server-rs + real Soniox + real ElevenLabs + your live Grip /
custom-RTMP destinations) through the actual UI. Use the smoke test
in CI; use this one for manual matrix testing before a release.

## Flag reference

| Flag | Values | Purpose |
|------|--------|---------|
| `--no-clone` | (boolean) | Skip ElevenLabs clone, use default-male library voice. Faster, no clone credits. |
| `--source <lang>` | `en` `ko` `ja` `zh` | Source spoken language (default `ko`). Voice clone enrolls in this language. |
| `--grip <lang>` | `en` `ko` `ja` `zh` | Add a Grip destination at `<lang>`. Auto-passthrough when `<lang>` == `--source`. |
| `--youtube <lang>` | `en` `ko` `ja` `zh` | Add a Custom-RTMP destination using `YOUTUBE_RTMP_URL/KEY` from `.env`. |
| `--rtmp <lang>` | `en` `ko` `ja` `zh` | Add a second Custom-RTMP destination using `RTMP_URL/KEY`. |
| `--rtmp2 <lang>` | `en` `ko` `ja` `zh` | Add a third Custom-RTMP destination using `RTMP2_URL/KEY`. |
| `--duration <sec>` | integer | How long to record after going live (default `90`). |
| `--headed` | (boolean) | Open the browser visibly so you can watch the run. |

**Auto-passthrough rule.** When a destination's lang equals the session
source lang, the test selects `pass` (raw passthrough) on that
destination instead of the lang code. So `--source ko --rtmp2 ko` gives
you a fourth lane that re-broadcasts the host's untouched Korean
audio — useful for verifying the no-translation path under load while
the other lanes exercise STT/translate/TTS.

## Recommended combinations

### Smoke (~2 min) — confirm the pipeline is breathing

```bash
./scripts/test-e2e-real.sh --no-clone --source ko --grip zh
```

| Source | Grip | YouTube | RTMP | RTMP2 | Streams |
|--------|------|---------|------|-------|---------|
| ko | zh | — | — | — | 1 translation |

### Two-lane (~3 min) — translation + passthrough

```bash
./scripts/test-e2e-real.sh --no-clone --source ko --grip zh --youtube ko
```

| Source | Grip | YouTube | RTMP | RTMP2 | Streams |
|--------|------|---------|------|-------|---------|
| ko | zh | **ko (pass)** | — | — | 1 translation + 1 raw |

### Cloned-voice three-lane (~5 min) — full STT/clone/TTS chain

```bash
./scripts/test-e2e-real.sh --source ko --grip en --youtube zh --rtmp ja
```

| Source | Grip | YouTube | RTMP | RTMP2 | Streams |
|--------|------|---------|------|-------|---------|
| ko | en | zh | ja | — | 3 translations w/ cloned voice |

### Maximum pressure (~5–7 min) — 1 source → 3 translations + 1 passthrough

```bash
./scripts/test-e2e-real.sh --source ko --grip en --youtube zh --rtmp ja --rtmp2 ko
```

| Source | Grip | YouTube | RTMP | RTMP2 | Streams |
|--------|------|---------|------|-------|---------|
| ko | en | zh | ja | **ko (pass)** | 3 translations + 1 raw, 4 ffmpeg pushes |

### Inverse (English source) — coverage of `en` STT path

```bash
./scripts/test-e2e-real.sh --no-clone --source en --grip ko --youtube ja --rtmp zh --rtmp2 en
```

| Source | Grip | YouTube | RTMP | RTMP2 | Streams |
|--------|------|---------|------|-------|---------|
| en | ko | ja | zh | **en (pass)** | 3 translations + 1 raw |

### Single-lane regression — fastest possible run

```bash
./scripts/test-e2e-real.sh --no-clone --source ko --rtmp zh --duration 30
```

Runs in ~90 s. Use for quick smoke after touching the TTS worker or
ffmpeg pipeline.

## Source-lang × destination-lang matrix

When **dest == source**, that destination is automatically passthrough.
All other cells exercise the full STT → translate → ElevenLabs TTS →
ffmpeg → RTMP chain.

| source ↓ \ dest → | en | ko | ja | zh |
|---|---|---|---|---|
| **en** | passthrough | translate | translate | translate |
| **ko** | translate | passthrough | translate | translate |
| **ja** | translate | translate | passthrough | translate |
| **zh** | translate | translate | translate | passthrough |

## Verifying the run

While the test runs, tail the server-rs log in another terminal:

```bash
tail -f .dev-logs/server-rs.log | \
  grep -E "tts worker (started|processing|exited|outer-timeout)|tts complete|flushing utterance|Stray %|tts dispatch produced"
```

Healthy run shows:
- One `tts worker started` per non-passthrough destination lang.
- `tts dispatch starting` → `tts complete` pairs for each utterance.
- Zero `tts worker outer-timeout fired`.
- Zero `Stray %` (drawtext caption escapes are now applied).
- Zero `tts dispatch produced no audio`.

After the test stops, confirm audio actually reached each platform by
opening the live preview on Grip Seller Center / YouTube Studio /
your custom RTMP viewer.

## Per-platform credentials

Edit `tests/e2e/.env` (gitignored). Only the credential blocks for
destinations you actually pass via `--grip|--youtube|--rtmp|--rtmp2`
are required — the script fail-fasts if a flag is set but its
matching `*_URL` / `*_KEY` is missing or still a placeholder.

| Flag | Env vars |
|------|----------|
| `--grip` | `GRIP_RTMP_URL`, `GRIP_RTMP_KEY` |
| `--youtube` | `YOUTUBE_RTMP_URL`, `YOUTUBE_RTMP_KEY` |
| `--rtmp` | `RTMP_URL`, `RTMP_KEY` |
| `--rtmp2` | `RTMP2_URL`, `RTMP2_KEY` |
