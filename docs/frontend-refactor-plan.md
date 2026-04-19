# Frontend refactor plan — apply CLAUDE.md §2.2 layered layout

Status: **planned, not started**
Owner: self, single uninterrupted pass per the user instruction ("don't
stop until 100% done").
CI gate: each commit must land with `bun run --cwd frontend typecheck`,
`bun run --cwd frontend test`, and `bun run --cwd frontend build` all
green. Per-commit green = reversible history, cheap revert if anything
surprises us mid-way.

## Why this refactor

The frontend currently ships a classic React folder layout
(`components/`, `hooks/`, `lib/`, `pages/`, `state/`) that predates the
architectural rules in CLAUDE.md §1–§2. The code inside is decent, the
layering isn't. The app has outgrown "flat folders by kind" and the
lib/ bucket in particular is mixing layer-1 utilities, layer-2 shared
networking, and layer-3 feature DTOs in one pile.

Target layout (CLAUDE.md §2.2):

```
frontend/src/
├── core/                    # Layer 1 — pure utilities, generated types
├── shared/                  # Layer 2 — cross-feature capabilities
│   ├── audio/
│   ├── media/
│   └── networking/
├── features/                # Layer 3 — vertical slices
│   ├── broadcast/
│   │   ├── domain/          # types, constants
│   │   ├── data/            # api client, mappers
│   │   └── presentation/    # pages, hooks, components, reducer
│   └── public/              # marketing + legal pages (home / privacy / terms)
│       └── presentation/
├── orchestration/           # router, providers, app bootstrap
├── main.tsx
├── globals.css
└── vite-env.d.ts
```

## Current inventory — file-by-file target

### Layer 1 — core/

| From | To | Kebab-case | Notes |
|---|---|---|---|
| `src/lib/cn.ts` | `src/core/cn.ts` | yes | clsx utility, zero deps |
| `src/lib/cn.test.ts` | `src/core/cn.test.ts` | yes | co-located |
| `src/lib/generated/client.ts` | `src/core/contracts/workers-client.ts` | yes | generated TS client |
| `src/lib/generated/workers-api.d.ts` | `src/core/contracts/workers-api.d.ts` | n/a | generated types |
| `src/types.ts` | delete | — | superseded by `@brivva/contracts` — `Lang` and `LANGS` already live there, the local `LANG_LABELS` moves to `features/broadcast/domain/langs.ts` |
| `src/vite-env.d.ts` | unchanged | n/a | required at src root by Vite |

**Generator change:** `frontend/openapi.config.ts` (or wherever the
codegen target path lives) must be updated to emit into
`src/core/contracts/` instead of `src/lib/generated/`.

### Layer 2 — shared/

| From | To | Notes |
|---|---|---|
| `src/lib/AudioPipeline.ts` | `src/shared/audio/audio-pipeline.ts` | Mic capture + PCM streaming. Cross-feature. |
| `src/lib/AudioPipeline.test.ts` | `src/shared/audio/audio-pipeline.test.ts` | |
| `src/lib/TtsPlayer.ts` | `src/shared/audio/tts-player.ts` | |
| `src/lib/VideoPlayer.ts` | `src/shared/media/video-player.ts` | |
| `src/lib/SessionSocket.ts` | `src/shared/networking/session-socket.ts` | WS transport, auth-agnostic |
| `src/lib/SessionSocket.test.ts` | `src/shared/networking/session-socket.test.ts` | |

No sibling imports among `shared/audio`, `shared/media`,
`shared/networking` — per CLAUDE.md §1.3. If one calls another today,
that's a fix-up in this refactor.

### Layer 3 — features/broadcast/

| From | To | Notes |
|---|---|---|
| `src/lib/api.ts` | `src/features/broadcast/data/api-client.ts` | HTTP calls to Workers API |
| `src/lib/api.test.ts` | `src/features/broadcast/data/api-client.test.ts` | |
| `src/components/AudioRecorder.tsx` | `src/features/broadcast/presentation/audio-recorder.tsx` | |
| `src/components/LatencyDashboard.tsx` | `src/features/broadcast/presentation/latency-dashboard.tsx` | |
| `src/components/PipelineAnalysis.tsx` | `src/features/broadcast/presentation/pipeline-analysis.tsx` | |
| `src/hooks/useHostSession.ts` | `src/features/broadcast/presentation/use-host-session.ts` | |
| `src/hooks/useHostSession.test.tsx` | `src/features/broadcast/presentation/use-host-session.test.tsx` | |
| `src/hooks/useTimings.ts` | `src/features/broadcast/presentation/use-timings.ts` | |
| `src/hooks/useTimings.test.ts` | `src/features/broadcast/presentation/use-timings.test.ts` | |
| `src/state/host/messageHandler.ts` | `src/features/broadcast/presentation/message-handler.ts` | |
| `src/state/host/messageHandler.test.ts` | `src/features/broadcast/presentation/message-handler.test.ts` | |
| `src/state/host/reducer.ts` | `src/features/broadcast/presentation/reducer.ts` | |
| `src/state/host/reducer.test.ts` | `src/features/broadcast/presentation/reducer.test.ts` | |
| `src/pages/HostPage.tsx` | `src/features/broadcast/presentation/host-page.tsx` | |
| `src/pages/DashboardPage.tsx` | `src/features/broadcast/presentation/dashboard-page.tsx` | Session list + creation |
| `src/pages/SessionPage.tsx` | `src/features/broadcast/presentation/session-page.tsx` | Session start / management |
| _new_ | `src/features/broadcast/domain/langs.ts` | `Lang`, `LANGS`, `LANG_LABELS` |
| _new_ | `src/features/broadcast/domain/types.ts` | Feature-local UI state types lifted out of reducer |

### Layer 3 — features/public/

| From | To | Notes |
|---|---|---|
| `src/pages/HomePage.tsx` | `src/features/public/presentation/home-page.tsx` | OAuth callback + landing |
| `src/pages/HomePage.test.tsx` | `src/features/public/presentation/home-page.test.tsx` | |
| `src/pages/PrivacyPage.tsx` | `src/features/public/presentation/privacy-page.tsx` | |
| `src/pages/TermsPage.tsx` | `src/features/public/presentation/terms-page.tsx` | |

These pages don't cross-import with `broadcast`. If any share a layout
or header, extract to `shared/` as a followup, not in this pass.

### Layer 4 — orchestration/

| From | To | Notes |
|---|---|---|
| `src/App.tsx` | `src/orchestration/app.tsx` | Router + Routes only |
| `src/App.css` | delete | CSS unused — imports from globals.css already cover the app |

### Unchanged / root-level

- `src/main.tsx` stays at `src/main.tsx` — Vite entry, updates import
  to `./orchestration/app.tsx`.
- `src/globals.css` stays.
- `src/test/setup.ts` stays.
- `src/vite-env.d.ts` stays.

## Phased commit plan

Each phase ends with a clean CI pass. Use `git mv` so history follows
the file. Use per-phase `bun run --cwd frontend build` to confirm Vite
resolves all imports.

### Phase 0 — scaffold + typegen output path

- [ ] Create the empty target directories
      (`core/`, `shared/{audio,media,networking}/`,
      `features/{broadcast/{domain,data,presentation},public/presentation}/`,
      `orchestration/`).
- [ ] Update `frontend/openapi.config.ts` (or package.json script) so
      `bun run --cwd frontend openapi:generate` writes to
      `src/core/contracts/`. Regenerate.
- [ ] Commit: "frontend: scaffold layered dirs + repoint openapi
      codegen to core/contracts"

### Phase 1 — core/

- [ ] `git mv src/lib/cn.ts src/core/cn.ts` + test.
- [ ] Move generated files to `src/core/contracts/` (already
      regenerated in phase 0, delete the old path).
- [ ] Global import rewrite: `from "../lib/cn"` →
      `from "../core/cn"` (adjust depth per file).
- [ ] Commit: "frontend: move cn + generated types to core/"

### Phase 2 — shared/networking + shared/media

- [ ] `git mv` `SessionSocket.ts` + test into
      `shared/networking/session-socket.ts`, rename to kebab-case.
- [ ] `git mv` `VideoPlayer.ts` into `shared/media/video-player.ts`.
- [ ] Update all imports. These files currently live in `lib/`.
- [ ] Commit: "frontend: split networking + media out of lib/ into
      shared/"

### Phase 3 — shared/audio

- [ ] Move `AudioPipeline.ts` + test and `TtsPlayer.ts` into
      `shared/audio/`, kebab-case filenames.
- [ ] Verify `shared/audio/*` does not import from `shared/media` or
      `shared/networking` — if it does, break the edge (should be
      consumer's job at presentation layer).
- [ ] Commit: "frontend: move audio primitives into shared/audio"

### Phase 4 — features/broadcast/domain + data

- [ ] Create `features/broadcast/domain/langs.ts` with `Lang`,
      `LANGS`, `LANG_LABELS`. Delete `src/types.ts`.
- [ ] `git mv src/lib/api.ts` →
      `src/features/broadcast/data/api-client.ts` + test rename.
- [ ] Imports: every consumer of `types.ts` points at
      `features/broadcast/domain/langs.ts`; every consumer of
      `lib/api` points at `features/broadcast/data/api-client`.
- [ ] Commit: "frontend: move broadcast domain langs + data api-client"

### Phase 5 — features/broadcast/presentation

- [ ] `git mv` all of `components/`, `hooks/`, `state/host/`, and
      broadcast-scoped pages (Host/Dashboard/Session) into
      `features/broadcast/presentation/`, kebab-case throughout.
- [ ] `state/host/messageHandler` → `message-handler`, same for
      reducer. `useHostSession` → `use-host-session`, etc.
- [ ] Remove `src/components/`, `src/hooks/`, `src/state/` if empty.
- [ ] Commit: "frontend: collapse broadcast presentation into the
      feature slice"

### Phase 6 — features/public/ + orchestration/

- [ ] Move HomePage + HomePage.test + PrivacyPage + TermsPage into
      `features/public/presentation/`, kebab-case.
- [ ] Move `App.tsx` to `orchestration/app.tsx`. Update its route
      imports to the new paths.
- [ ] Update `src/main.tsx` to import `./orchestration/app`.
- [ ] Delete `src/App.css` (unused — import audit during phase).
- [ ] Remove `src/pages/` if empty.
- [ ] Commit: "frontend: move public pages + app bootstrap into
      features/public + orchestration"

### Phase 7 — dependency audit + cleanup

- [ ] `grep -rn "from \"\\.\\./\\.\\./lib" src/` — must be empty.
- [ ] `grep -rn "from \"\\.\\./components\"\\|\\.\\./hooks\\|\\.\\./state"
      src/` — must be empty.
- [ ] Verify CLAUDE.md §1.2 dependency direction with a scripted
      check: `core/` must not import from `shared/` or `features/` or
      `orchestration/`. `shared/` must not import from `features/` or
      `orchestration/`. Add a lint script or checked-in audit.
- [ ] `bun run --cwd frontend build` — final size comparison, should
      be within noise.
- [ ] Commit: "frontend: enforce layered import audit"

## Test impact

All tests are co-located — they move with their source file. No test
logic changes. Imports update mechanically.

Expected total moves: ~25 TS/TSX files (10 sources + 15 co-located
tests and siblings). A handful of tests import each other (shared
mock fixtures) — catch those during phase 5.

## Risks + mitigations

| Risk | Mitigation |
|---|---|
| Wide import rewrite breaks vitest or vite | Per-phase `bun run test + build` before commit; git revert is cheap |
| Case-only renames on macOS/Linux confuse git | Use `git mv A.ts tmp.ts && git mv tmp.ts a-kebab.ts` when the only change is case |
| Generated openapi path drift (phase 0) | Regenerate, git-diff, commit the new output path in the same phase 0 commit |
| Hidden `src/App.css` import breaks styling | Audit with `grep "App.css"` before deleting; if used, move CSS into `orchestration/` |
| Test-setup imports (`src/test/setup.ts`) point at old paths | `setup.ts` is test-only; update if needed in phase 7 |

## Out of scope for this pass

- React Compiler / build perf tuning.
- CSS module adoption. `globals.css` stays as-is.
- Component API churn. Files move, internals don't.
- Deduplicating DashboardPage vs SessionPage logic — refactor target,
  not a structural one.

## Success criteria

1. No file lives in `src/lib/`, `src/components/`, `src/hooks/`,
   `src/state/`, or `src/pages/` after phase 7. Those directories are
   deleted.
2. `bun run --cwd frontend typecheck` green.
3. `bun run --cwd frontend test` green, identical test count.
4. `bun run --cwd frontend build` green, bundle size within ±2% of
   pre-refactor.
5. No import crosses the dependency rule in CLAUDE.md §1.2. Verified
   by a grep-based check committed in phase 7.

## Effort estimate

| Phase | Hours |
|---|---|
| 0 — scaffold + codegen path | 0.5 |
| 1 — core/ | 0.5 |
| 2 — shared/networking + media | 0.5 |
| 3 — shared/audio | 0.5 |
| 4 — broadcast/domain + data | 1.0 |
| 5 — broadcast/presentation | 2.0 |
| 6 — public/ + orchestration | 0.5 |
| 7 — audit + cleanup | 1.0 |
| buffer (typecheck surprises) | 1.5 |
| **Total** | **~8 h** |

Single focused session. No need for a PR split — each phase is a
local commit.
