# Real-Response Fixtures (§0.5.1)

Every external API used by workers must have JSON fixtures captured from
real live responses — both happy-path AND error shapes. Tests deserialize
these fixtures instead of inventing shapes inline.

Why: the April 2026 triad (Soniox integer `error_code`, WS double
`live_session`, `SQLITE_BUSY` from metrics loop) slipped past 689 tests
because we validated assumptions, not reality.

## Directory layout

```
workers/tests/fixtures/
├── elevenlabs/     # voice clone, TTS synth, error shapes
├── google-oauth/   # token exchange, refresh, userinfo
├── grip/           # Seller API provision, IVS ingest responses
├── stripe/         # webhook events: checkout.session.completed, invoice.paid
└── youtube/        # Live API: broadcast.insert, stream.insert, bind
```

## Per-vendor README

Each subdirectory has its own README marking which files are CAPTURED (from
real API) vs HAND_CRAFTED_PENDING_REAL_CAPTURE (placeholder until Aziz runs
a live capture). Only CAPTURED fixtures satisfy §0.5.1.

## Loading a fixture in a test

```ts
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const fixture = (vendor: string, file: string) =>
  JSON.parse(readFileSync(resolve(__dirname, "../fixtures", vendor, file), "utf8"));

// Usage:
const event = fixture("stripe", "checkout_session_completed.json");
```

## Capture checklist

Before May 10 launch, capture at least one HAPPY + one ERROR fixture per
vendor and replace HAND_CRAFTED entries. Mark captured files in the README
with date captured + Soniox-style provenance note.
