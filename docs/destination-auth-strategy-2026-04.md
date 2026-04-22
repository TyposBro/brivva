# Brivva — Destination Authentication Strategy

**Date:** 2026-04-22
**Prepared by:** Aziz (tech lead)
**Purpose:** Decide how non-technical sellers attach live-streaming destinations (YouTube, Grip, TikTok, Instagram, etc.) to Brivva sessions with minimum friction, and sequence the work around the May 10 launch.
**Scope:** Destination authentication + credential capture only. Does not cover the streaming pipeline itself.

---

## Executive Summary

- **YouTube + Grip** already ship with fully automated OAuth / API key provisioning. Zero paste.
- **TikTok + Instagram** cannot ship the same auto-flow before May 10. Both require platform partner approvals (TikTok LIVE Partner program, Meta App Review) measured in weeks, not days, and both require Brivva Tech to be a formed entity with business verification.
- **Decision:** For May 10, TikTok and Instagram ship as **raw-RTMP paste-creds** destinations (same pattern as TikTok today). Polish paste UX, not add OAuth code that cannot be activated.
- **Post-launch:** file Meta App Review and TikTok LIVE Partner applications in parallel. When approvals land, swap the paste UI for auto-flow without changing session contracts.
- **Mobile app (Expo):** scoped as Q3 2026 work. Unlocks Share Extension handoff (the "one-tap import from TikTok/IG" UX), native go-live-from-phone, and push-notification handoff. Out of scope for May 10.

---

## 1. Problem Statement

Non-technical Korean sellers (primary ICP) struggle to attach live-streaming destinations because RTMP server URLs and stream keys are:

- **Long** (~40 characters, random-looking strings).
- **Phone-first** for most platforms (TikTok, Instagram). The streaming app is the desktop browser or OBS. The key source is the phone. Cross-device copy-paste is the friction point.
- **Time-limited** (Grip 1h, TikTok ~24h, IG ~24h, YouTube tied to broadcast lifetime). Users repaste frequently.
- **Invisible to error** until mid-broadcast. A typo or expired key fails silently at go-live, wasting the stream window.

The goal is not to eliminate paste globally — it is to eliminate paste **wherever the platform offers a programmatic alternative** and to minimize paste friction everywhere else.

---

## 2. Current Destination State (2026-04-22)

| Platform | Flow | Auto-provisioning | Implementation |
|---|---|---|---|
| YouTube | OAuth + auto | Yes | `workers/src/features/youtube/google-oauth-client.ts`, `broadcast-api.ts` (liveBroadcasts.insert + liveStreams.insert + bind). Scopes: `youtube` + `youtube.readonly`. D1: `users.youtube_*`. |
| Grip | Seller API + auto | Yes | `workers/src/orchestration/app.ts:351-463`. One-shot per-broadcast keys, 1h TTL. |
| TikTok | Paste-creds | No | `POST /auth/tiktok` at `workers/src/orchestration/app.ts:1051-1063`. Schema `TikTokAuthRequest` at `contracts/src/http.ts:237`. UI: `dashboard-destination-card.tsx:256`. |
| Instagram | Catalog entry only | No | `contracts/src/platforms.ts:40` (keyOnly=true, default RTMPS URL). No endpoint, no dedicated UI card. Users today route through "Custom RTMP". |
| Twitch / Coupang / Naver / Rakuten / Douyin / Taobao / Kuaishou / Xiaohongshu / Bilibili | Paste-creds | No | Covered by the generic Custom-RTMP flow. Each has a help string in the platform catalog. |

---

## 3. Why OAuth Auto-Flow Is Not Feasible for TikTok/Instagram Pre-May-10

### 3.1 TikTok

- **TikTok Login Kit** is self-serve and returns user identity. It does **not** grant live-streaming permissions.
- Programmatic RTMP URL + stream-key creation requires access to the **TikTok LIVE Studio API** or the **TikTok LIVE Streaming API**. Both are partner-gated via the **TikTok LIVE Partner program** or **TikTok for Business**.
- Approval requires: formed business entity, business verification, use-case review, and (historically) favorable regional tier. Korean seller tier is not pre-approved; typical turnaround is weeks to months.
- Brivva Tech entity is not yet formed (see `memory/project_brivva_interview.md`). Cannot file the application yet.
- **No self-serve bypass exists.** Every competitor we benchmarked (Restream, StreamYard, OneStream) ships TikTok as paste-creds. None have the partner API.

### 3.2 Instagram (Meta)

- Meta Graph API now supports `POST /{ig-user-id}/live_media`, which returns `rtmp_url` + `stream_key`.
- Requires:
  - Instagram Business or Creator account linked to a Facebook Page.
  - Facebook App with `instagram_live_streaming` permission (newly added in 2024).
  - Facebook App Review with business verification.
- Typical Meta App Review turnaround: 2-4 weeks after submission. Requires Brivva Tech entity formed.
- **Self-serve-ish** — once the app is approved, any user can OAuth their IG Business account and auto-provision RTMP. But the first approval is gating.

### 3.3 Timeline Reality

| Task | Blocker | ETA |
|---|---|---|
| TikTok LIVE Partner application | Entity formation + business verification | Post-May-10; approval unpredictable |
| Meta App Review (Instagram) | Entity formation + business verification | Post-May-10; approval 2-4 weeks |
| Raw-RTMP paste for TikTok | Code only | Already shipped |
| Raw-RTMP paste for Instagram | Code only | ~1 day |

Conclusion: no OAuth auto-flow for either platform ships by May 10. The only question is how good the paste UX is.

---

## 4. Paste-UX Improvement Options (Ranked)

All target non-technical sellers. Effort estimates assume existing codebase familiarity.

### 4.1 Single-Paste URL to Auto-Split + Platform Detection (highest leverage)

**Effort:** ~3 hours.

User copies one string from the source app, e.g.:

- `rtmps://live-upload.instagram.com:443/rtmp/ABC123XYZ` (Instagram, URL includes key as path segment)
- `rtmp://va.tiktokv.com/live/xyz?code=abc&flow=...` (TikTok, URL includes key as query param)
- `rtmps://xxxx.global-contribute.live-video.net:443/app/<key>` (Grip, AWS IVS pattern)

Brivva frontend:

1. Hostname-regex-detect → auto-select the correct platform card (no dropdown click).
2. Split URL into `rtmp_url` + `stream_key` per platform's known pattern.
3. Validate that IG paste uses RTMPS (Meta rejects plain RTMP on this ingest).
4. Single textarea replaces the current two-field pattern (URL + key) plus dropdown selection.

Before: pick platform → paste URL → paste key = 3 steps, easy to misalign the fields.
After: paste once → done.

### 4.2 Guided Walkthrough Modal with Korean Screenshots

**Effort:** ~1 day.

For each paste-creds platform (Grip, TikTok, IG, and the four Korean/Chinese platforms), a modal shows 3-4 annotated screenshots of the exact tap sequence:

- TikTok: open app → `+` → LIVE → swipe to "Mobile" or "PC" tab → tap server/key to copy.
- Instagram: open app → `+` → Live → scroll to "Live producing" → tap "Use streaming software" → copy.
- Grip: open Seller Center → Broadcast Management → PC Broadcast → create broadcast → copy URL + key.

Animated GIFs where a static screenshot is ambiguous. Korean labels. Parity with Restream's walkthrough UX (current industry best).

### 4.3 Connection Test Button

**Effort:** ~4 hours.

"Test stream key" button sends a 2-second dummy RTMP push from `server-rs`. Surfaces platform-specific errors before the user goes live:

- `auth rejected` → wrong key
- `connection refused` → wrong URL
- `key expired` → TikTok/Grip-typical, prompt to refresh
- `handshake timeout` → network/firewall issue

Kills a whole class of "nothing is showing up" support tickets where users discover the problem 30 seconds into a live show.

### 4.4 Expiration Warnings

**Effort:** ~2 hours.

Store `pasted_at` timestamp column in `platform_credentials` D1 table. Dashboard card shows age-based warnings:

- Grip: warn at 45 min, block at 60 min (vendor TTL).
- TikTok: warn at 20 h, block at 24 h.
- Instagram: warn at 20 h, block at 24 h.

Complements the test button — users know to refresh before going live.

### 4.5 QR Handoff Desktop-to-Mobile (web-only version)

**Effort:** ~1 day.

See §5 for full mechanic. Phone becomes a "paste remote" for the desktop session. Eliminates typing 40-char keys across devices, which is the single biggest pain point for platforms whose key lives on the phone (TikTok, Instagram).

Web-only version requires no mobile app — user scans QR with phone camera → Safari/Chrome opens a paste page → pastes from phone clipboard → desktop updates via WebSocket push.

### 4.6 Meta App Review Submission (parallel, not code)

**Effort:** ~2-4 hours to prepare + file; ~2-4 weeks Meta turnaround.

Simon-driven. Requires Brivva Tech entity formed + business verification (tax ID, website, privacy policy page, demo video of the integration).

When approved: Instagram becomes full auto-flow like YouTube. Code to consume it = ~1 day.

### 4.7 TikTok LIVE Partner Application (parallel, not code)

**Effort:** file and forget.

Politically gated. Korean seller tier not pre-approved. Submit; do not block on it.

---

## 5. QR Handoff — Full Mechanic

### 5.1 The Problem It Solves

For TikTok and Instagram, the stream key lives on the user's phone (their app is phone-first). The stream itself originates from the user's desktop (camera, OBS, Brivva web studio). Moving the key from phone to desktop is manual: retype, email to self, AirDrop, cross-device clipboard sync — all break for non-tech users.

QR handoff makes the phone a paste remote: the desktop dashboard shows a QR code, phone scans, phone becomes the paste input, desktop receives the result in real time.

### 5.2 Web-Only Version (no mobile app)

```
[Desktop: Brivva dashboard]            [Phone: user]
    |                                       |
    | show QR encoding                      |
    | https://brivva.com/paste/<tok>        |
    |                                       | scan QR with camera app
    |                                       | Safari/Chrome opens paste page
    |                                       |
    |                                       | switch to TikTok app, copy key
    |                                       | switch back to Safari
    |                                       | paste into textarea, submit
    |                                       |
    | <-- WebSocket push: key arrived ------|
    | dashboard card updates green          |
```

**Token mechanics:**

- `<tok>` is a single-use 16-char random token, signed by workers, 5-minute TTL.
- Token is scoped to one `(user_id, session_id, destination_slot)` tuple.
- No account login required on the phone — the token itself authorizes the one-time paste.
- Server validates token on receipt, writes credentials into the session's destination slot, invalidates the token.

**Friction removed:** cross-device typing, clipboard sync, email-to-self detour.
**Friction remaining:** user still switches between TikTok app and Safari on the phone, still manually copies and pastes.

### 5.3 Mobile App Version (Expo)

A native mobile app unlocks two additional levels of seamlessness.

#### Level 1 — QR Opens Brivva App Directly (Universal Links / App Links)

```
[Desktop]                              [Phone: Brivva app installed]
    |                                       |
    | QR -> brivva.com/paste/<tok>          | scan QR
    |                                       | iOS/Android intercepts universal link
    |                                       | opens Brivva app, not browser
    |                                       | app screen: "Paste TikTok key for Session X"
    |                                       |
    |                                       | user switches to TikTok, copies key
    |                                       | switches back to Brivva app, pastes
    | <-- native API call ------------------|
```

Incremental win: no Safari detour, branded UX, auth can be richer (face ID, notifications). User still manually switches apps.

#### Level 2 — iOS Share Extension / Android Share Target (the holy grail)

```
[Phone: user inside TikTok app, stream-key screen]
    |
    | tap TikTok's "Share" button on the server URL
    | iOS Share Sheet pops up
    |   -> shows: Messages, Mail, AirDrop, ..., [Brivva]
    | user taps Brivva
    |
[Brivva app receives share intent]
    | app reads shared text
    | parses as RTMP URL + key
    | prompts: "which session?" or auto-picks the active one
    | POST to backend
    |
[Desktop: Brivva dashboard]
    | <-- WebSocket push: destination connected
    | card updates green
```

**One tap. Zero paste.** This is how Spotify / Pocket / Notion accept data from other apps. Implementation on mobile:

- **iOS:** register a share extension (`NSExtensionActivationSupportsWebURLWithMaxCount`) that matches RTMP/RTMPS URL patterns. App declares it accepts `public.url` and `public.plain-text`.
- **Android:** register an `ACTION_SEND` intent filter with `mimeType="text/plain"` matching RTMP patterns.

User experience: Brivva appears in the native iOS/Android share sheet of every app. User in TikTok taps Share on the key → Brivva → done.

#### Level 3 — Push-Notification Handoff (bonus)

```
[Desktop]                              [Phone: Brivva app, logged in]
    | user clicks "Add TikTok destination"  |
    | backend sends APNs/FCM push ---->     |
    |                                       | notification: "Tap to paste TikTok key for Session X"
    |                                       | user taps, app opens in paste mode
    |                                       | user switches to TikTok, copies, comes back, pastes
    | <-- API call ------------------------|
```

No QR scan at all. Requires phone logged into Brivva account. Nice-to-have.

### 5.4 Why QR Handoff + Mobile App Compose Well

- QR handoff is the **cold-start** path (new user, first session, no app installed yet). Works immediately.
- Share Extension is the **power-user** path (app installed, recurring sessions). Feels native.
- Both coexist. They target different moments in the user journey.
- The backend token/credential-save code is the **same** for both paths. Only the client-side trigger differs. Implementing QR first does not waste work when the mobile app lands.

---

## 6. Mobile App Strategy

### 6.1 Scope (when we do it)

The mobile app is not just a destination-key paste helper. It unlocks several categories at once:

1. **Paste remote** (QR handoff Level 1 + Share Extension Level 2).
2. **Go-live-from-phone** — KR influencer primary streaming mode is phone, not desktop. Currently they must use OBS on desktop or Brivva web. Native app unlocks the fastest-growing live-commerce host segment.
3. **Real-time caption viewer** — hosts watch their own translated captions to verify quality mid-show.
4. **Push notifications** — session-start, billing events, new-booking alerts.
5. **In-app voice clone recording** — native mic captures better audio than browser MediaRecorder; improves clone quality.

### 6.2 Technology Choice

| Option | Effort (full scope above) | Pros | Cons |
|---|---|---|---|
| Native iOS (Swift) + Android (Kotlin) | ~6-8 weeks | Best performance, full platform access | Two codebases, no frontend reuse |
| React Native | ~3-4 weeks | Shared TS, ok performance | Some native modules still required |
| **Expo (React Native + managed services)** | **~2-3 weeks** | Reuse existing frontend devs + `contracts` package, TypeScript end-to-end, hot reload, EAS Build handles app-store submission | Ejecting required for custom native modules (share extensions eventually need config-plugin work) |
| PWA only | ~1 week | No app store | No share extension, no push on iOS until recently, no native go-live camera |

**Recommendation: Expo.** Brivva's team is TypeScript-fluent (frontend + workers). `contracts/` package already shares types across boundaries — same package works in Expo. EAS Build handles App Store Connect + Play Console submission without local Xcode tooling. Share extensions require a config-plugin but Expo-compatible plugins exist.

### 6.3 Timeline

Q3 2026. Not before. Rationale:

- May 10 launch is the immediate priority. Web frontend is production-ready; mobile is additive.
- ~60 pipeline contracts need delivery in May/June. Operational focus dominates.
- Real user friction data from the first month of paid usage tells us exactly which mobile flow matters most. Don't guess pre-launch.
- Q3 aligns with Brivva Tech entity formation + partner applications ripening.

---

## 7. Staged Rollout

| Stage | Scope | Effort | Target Ship |
|---|---|---|---|
| 0 | Single-paste URL to auto-split + platform detect (web) | ~3 hrs | May 10 |
| 1a | Guided walkthrough modal (KR screenshots) for Grip, TikTok, IG | ~1 day | May 10 |
| 1b | Connection test button | ~4 hrs | May 10 |
| 1c | Expiration warnings | ~2 hrs | May 10 |
| 2 | Instagram raw-RTMP paste endpoint + dashboard card (parity with TikTok) | ~1 day | May 10 |
| 3 | e2e matrix flags `--tiktok`/`--instagram` in `scripts/test-e2e-real.sh` | ~1 hr | May 10 |
| 4 | Web QR handoff (no app) | ~1 day | May 10 stretch, else +1 wk |
| 5 | File Meta App Review for Instagram | paperwork | Week of 2026-04-28 (post-entity) |
| 6 | File TikTok LIVE Partner application | paperwork | Week of 2026-04-28 (post-entity) |
| 7 | Instagram OAuth auto-flow (activates on Meta approval) | ~1 day code, ~2-4 wk wait | Late May / early June |
| 8 | TikTok OAuth auto-flow (activates on partner approval) | ~1-2 days code, unpredictable wait | Whenever approval lands |
| 9 | Expo mobile app shell + universal-link paste-target | ~1 week | Q3 2026 |
| 10 | iOS share extension + Android share target | ~3 days | Q3 2026 (on top of 9) |
| 11 | Push-notification handoff | ~2 days | Q3 2026 (on top of 10) |
| 12 | Mobile go-live-from-phone + caption viewer + in-app clone recording | ~2 weeks | Q3 2026 (additive) |

---

## 8. Decision Log

**2026-04-22 — Go with raw-RTMP paste-creds for TikTok and Instagram for May 10.**

- No OAuth auto-flow code for either platform before launch.
- Instagram gets the same paste-creds pattern TikTok already has (`POST /auth/instagram`, dedicated dashboard card, default RTMPS URL prefill).
- Focus UX effort on stages 0-4 (single-paste, walkthrough, test button, expiration warnings, optional QR handoff).
- File Meta App Review and TikTok LIVE Partner application in parallel once Brivva Tech entity is formed.
- Re-evaluate mobile app start date at end of May based on post-launch friction data.

---

## 9. References

- `contracts/src/platforms.ts` — platform catalog source of truth.
- `contracts/src/http.ts:237` — `TikTokAuthRequestSchema` (template for InstagramAuthRequestSchema).
- `workers/src/orchestration/app.ts:1051-1063` — TikTok paste endpoint (template for Instagram).
- `workers/src/features/youtube/` — YouTube OAuth reference (template for future IG/TikTok OAuth when approvals land).
- `frontend/src/features/broadcast/presentation/dashboard-destination-card.tsx:256` — UI wiring for TikTok paste-creds.
- `scripts/test-e2e-real.sh` — e2e matrix; needs `--tiktok` / `--instagram` flags added in stage 3.
- `docs/may10-agent-tasks.md` — pre-launch task queue; append stages 0-4 here.
- `docs/grip-integration-notes.md` — companion doc for the other paste-creds platform.
- Meta Graph API `live_media` endpoint (for future reference): requires `instagram_live_streaming` permission.
- TikTok LIVE Streaming API (for future reference): partner program only, no public docs.
