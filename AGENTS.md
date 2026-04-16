# CLAUDE.md — Clean Architecture & Code Quality

> Pragmatic rules derived from production codebases. Keep code clean, keep shipping.

---

## 0. TESTING — Cover What Matters

Tests exist to catch regressions in critical paths. Write them for domain logic, auth, payments, data transformations, and complex algorithms. Test before or after — coverage of critical paths matters, not the order.

### 0.1 What to Test

| Priority | What | Example |
|----------|------|---------|
| **Must test** | Domain logic, use cases, business rules | `classify_emotion()`, `compute_tts_deadline()` |
| **Must test** | Auth flows, token handling | Sign-in, refresh, session management |
| **Must test** | Data transformations, mappers, pure functions | `pcm_to_wav()`, `strip_context_prefix()`, `toTranscriptEntry()` |
| **Must test** | Payment/subscription logic | Tier checks, entitlements, trial grants |
| **Should test** | Repository implementations | Against test DB or mocks |
| **Should test** | Complex async coordination | Pipeline orchestration, reconnection logic |
| **Nice to have** | UI components, handlers | Smoke tests for critical flows |

### 0.2 Test Quality

- **One logical assertion per test.** Arrange → Act → Assert.
- **Test names describe behavior:** `should_reject_when_insufficient_funds`, not `test3`.
- **No test interdependency.** Any test, any order.
- **Co-locate tests with source.** `budget.rs` → `#[cfg(test)] mod tests` in same file. `speaking.service.ts` → `speaking.service.test.ts` next to it.
- **Integration tests** in a `tests/` or `__tests__/` directory at the feature root.

### 0.3 Commit Strategy

- Commit logical units. Descriptive messages in imperative mood.
- No secrets, keys, or credentials. No generated files in version control.

---

## 1. ARCHITECTURE — Core → Shared → Features → Orchestration

### 1.1 The Four Layers

```
┌─────────────────────────────────────────────────────────────────────┐
│  ORCHESTRATION (Layer 4 — outermost)                                │
│  DI containers, app bootstrap, routing, env config, middleware.     │
│  The ONLY layer that sees and wires everything.                     │
├─────────────────────────────────────────────────────────────────────┤
│  FEATURES (Layer 3 — vertical slices)                               │
│  Each feature is isolated. Depends on Shared AND Core.              │
│  Features NEVER import from sibling features.                       │
├─────────────────────────────────────────────────────────────────────┤
│  SHARED (Layer 2 — reusable functional modules)                     │
│  Cross-feature capabilities. Depends on Core only.                  │
│  Shared modules NEVER import sibling shared modules.                │
├─────────────────────────────────────────────────────────────────────┤
│  CORE (Layer 1 — innermost, pure)                                   │
│  Pure functions, generic types, utilities, protocol interfaces.     │
│  ZERO domain knowledge. ZERO framework dependencies.                │
│  Core modules MAY import other core modules.                        │
└─────────────────────────────────────────────────────────────────────┘
```

### 1.2 The Dependency Rule

| Layer | May Depend On | MUST NOT Depend On |
|-------|---------------|-------------------|
| Core | Other Core modules, standard library | Shared, Features, Orchestration |
| Shared | Core only | Other Shared, Features, Orchestration |
| Features | Shared and Core | Other Features, Orchestration |
| Orchestration | Everything | — |

### 1.3 The No-Sibling Rule

- `features/exam/` MUST NOT import from `features/reading/`.
- `shared/stt/` MUST NOT import from `shared/tts/`.
- Core modules are exempt (they naturally build on each other).

Cross-feature communication goes through shared abstractions or orchestration workflows.

### 1.4 Feature Internal Structure

Each feature is a vertical slice with up to 3 layers:

**Backend features:**
```
features/broadcast/
├── domain/       # Entities, use cases, repository interfaces
├── data/         # Handlers, repo impls, external adapters, DTOs
└── tests/        # Integration tests (optional)
```

**Frontend features:**
```
features/broadcast/
├── domain/         # Types, interfaces, constants
├── data/           # API clients, DTOs, mappers
└── presentation/   # Hooks/ViewModels, components
```

| Feature Layer | May Depend On | MUST NOT Depend On |
|---------------|---------------|--------------------|
| domain/ | Shared, Core | data/, presentation/, other features |
| data/ | domain/ (same feature), Shared, Core | presentation/, other features |
| presentation/ | domain/ (same feature), Shared, Core | data/, other features |

### 1.5 Boundary Crossing

Data that crosses layer boundaries SHOULD be mapped when the shapes diverge. When the DTO and domain entity are identical, skip the mapper — don't add ceremony for its own sake.

Mappers are pure functions. Test them when the mapping is non-trivial.

### 1.6 Error Handling Across Layers

- **Core:** Defines generic error types. Pure — never throws.
- **Shared/Data:** Catches infrastructure exceptions. Maps to domain errors. No infrastructure errors escape.
- **Domain:** Returns `Result<T, E>` or sealed result types for expected failures.
- **Presentation:** Maps domain errors to user-facing messages.
- **Orchestration:** Global error boundary as a safety net.

Use sealed result types for domain logic with multiple outcomes:
```rust
enum PracticeCheckResult {
    Allowed,
    RequiresAds { count: u32 },
    RequiresUpgrade { tier: String },
    Denied { reason: String },
}
```

---

## 2. PROJECT STRUCTURE

### 2.1 Backend Layout (Rust)

```
server-rs/src/
├── core/                    # Layer 1 — pure types, config constants, utilities
├── shared/                  # Layer 2 — reusable capabilities (stt, tts, translation)
├── features/                # Layer 3 — vertical slices
│   └── broadcast/
│       ├── domain/          # Entities, use cases, interfaces
│       └── data/            # Handlers, repos, streaming, pipelines
├── orchestration/           # Layer 4 — DI, config, router
├── lib.rs                   # Module declarations + run_server()
└── main.rs                  # Entry point
```

### 2.2 Frontend Layout (React/TypeScript)

```
frontend/src/
├── core/                    # Layer 1 — pure utilities, types
├── shared/                  # Layer 2 — UI kit, media, networking
├── features/                # Layer 3 — vertical slices
│   └── broadcast/
│       ├── domain/          # Types, constants
│       ├── data/            # DTOs, mappers, API clients
│       └── presentation/    # Hooks, components
├── orchestration/           # Layer 4 — config, DI/providers, router
└── main.tsx                 # Entry point
```

### 2.3 File Naming

- **Rust:** `snake_case.rs`
- **TypeScript:** `kebab-case.ts`
- One primary export per file. Exceptions: `mappers`, `dtos`, `types` files may group related items.
- Test files: `{name}_test.rs` or inline `#[cfg(test)]` (Rust), `{name}.test.ts` (TypeScript).

### 2.4 Use Case Rules

A use case is a single class/struct with a single public method (`execute()`, `invoke()`).

- One use case = one application action.
- Constructor injection for dependencies.
- Same-feature use cases may call each other.
- Cross-feature composition happens in orchestration only.

---

## 3. FUNCTIONS

### 3.1 Size

- Target: **≤50 lines.** Hard limit: **80 lines.**
- If a function exceeds 80 lines and you can extract a meaningful helper, do it. If splitting would just scatter the logic without improving clarity, add a `// PRAGMATIC:` comment.

### 3.2 Do One Thing

A function does one thing if you cannot extract another meaningful function from it. Read top-to-bottom as descending abstraction.

### 3.3 Arguments

- Ideal: 0–2. Acceptable: 3. Over 3: wrap into a struct/object.
- Named structs make call sites self-documenting.

### 3.4 Error Handling

- `Result<T, E>` for expected failures. Panics for truly unexpected situations.
- `.unwrap()` is acceptable on infallible operations (JSON serializing a known-good struct, mutex locks in single-threaded test code). Add a comment if the infallibility is non-obvious.
- `try-catch` / `match` at layer boundaries only.

---

## 4. NAMING

### 4.1 Intent-Revealing Names

Every name reveals **why it exists, what it does, and how it is used**. If a name requires a comment, the name is wrong.

### 4.2 Conventions

- **Types:** `PascalCase`. Noun or noun phrase.
- **Functions:** `camelCase` (TS) or `snake_case` (Rust). Verb or verb phrase.
- **Booleans:** `is_active`, `has_permission`, `can_retry`.
- **Constants:** `UPPER_SNAKE_CASE`.
- **Files:** `kebab-case` (TS) or `snake_case` (Rust).

### 4.3 Consistency

One word per concept across the entire project. Don't mix `fetch`/`get`/`retrieve`/`load` for the same operation.

---

## 5. FILES & FORMATTING

### 5.1 File Size

- Target: **≤500 lines.** A focused service or module can go up to **800 lines** if well-organized.
- Over 800 → split into focused submodules.

### 5.2 Formatting

- **100 chars soft limit. 120 hard limit.**
- Automated formatters enforced (rustfmt, Prettier).
- Blank lines separate concepts. Variables near usage. Caller above callee.

---

## 6. DESIGN PRINCIPLES

### 6.1 Data/Object Anti-Symmetry

**Objects** hide data, expose behavior. **Data structures** expose data, no behavior. Don't mix.

### 6.2 Law of Demeter

No `a.get_b().get_c().do_thing()` chains (fluent builders are fine).

### 6.3 SOLID

| Principle | Rule |
|-----------|------|
| **SRP** | One class/struct, one reason to change. |
| **OCP** | Polymorphism/traits over `if/else` type switches. |
| **LSP** | Subtypes substitutable for base types. |
| **ISP** | Many small interfaces. No client depends on unused methods. |
| **DIP** | Inner layers define interfaces, outer layers implement. |

---

## 7. DEPENDENCY INJECTION

- **Orchestration is the composition root.** All DI wiring happens here.
- Manual DI is preferred over frameworks when the dependency graph is small. A simple `AppContext` struct or service container with lazy-initialized fields works well.
- Shared modules do NOT wire their own dependencies.
- Environment variables are read ONLY in orchestration. Lower layers receive configuration through constructor injection.

```rust
// orchestration/config.rs — the ONLY place env vars are read
pub struct AppConfig {
    pub stt_api_key: String,
    pub tts_api_key: String,
}
impl AppConfig {
    pub fn from_env() -> Self { /* reads std::env::var here */ }
}
```

---

## 8. CONFIGURATION

- Centralize business rules in config files, not scattered in code.
- Thresholds, limits, timing constants, feature flags → dedicated config modules.
- All environment variables read once at startup in orchestration, distributed via DI.

---

## 9. CONCURRENCY & REAL-TIME

### 9.1 General Rules

- Concurrent code is **separate** from business logic.
- Structured concurrency (tokio tasks with supervision, async/await).
- Immutable data and message passing over shared mutable state.

### 9.2 Streaming & Real-Time

- **Core** defines generic stream interfaces (protocol-aware, framework-free).
- **Shared** implements connection management (WebSocket, reconnection, heartbeat).
- **Features** consume streams via interfaces. Domain layer is transport-unaware.
- Map wire format → domain entities at the data layer boundary.

---

## 10. LOGGING

| Layer | Logging Allowed? | What to Log |
|-------|-----------------|-------------|
| Core | **No** | Pure functions — nothing to log |
| Shared (data) | **Yes** | HTTP calls, connection events, retries |
| Feature (data) | **Yes** | External API calls, cache decisions |
| Feature (domain) | **No** | Use cases stay pure |
| Feature (presentation) | **No** | Use error boundaries |
| Orchestration | **Yes** | Startup, unhandled errors, request lifecycle |

Structured logging (JSON) in production. Human-readable in development. No sensitive data in logs.

---

## 11. CODE SMELLS — Refactoring Triggers

| Smell | Action |
|-------|--------|
| Function > 80 lines | Extract helpers |
| File > 800 lines | Split into submodules |
| 4+ function arguments | Wrap in struct |
| Nested `if` > 3 levels | Guard clauses or extract |
| Duplicate code (3×+) | Extract |
| Cross-layer import violation | Fix dependency direction |
| Domain entity leaking out of feature | Add mapper + DTO |
| Framework type in domain layer | Wrap behind trait/interface |
| Dead code | Delete |
| Commented-out code | Delete |

---

## 12. COMMENTS — Minimal

### Acceptable

- Non-obvious business rule explanation.
- Clarification of an unmodifiable external API.
- Warning of consequences.
- `TODO(TICKET-ID)` with a real ticket number.
- Public API doc comments for shared/library code.
- `// PRAGMATIC: <reason>` for intentional rule exceptions.

### Banned

- Redundant comments restating code.
- Journal/changelog comments.
- Commented-out code. **Delete it.**

---

## 13. NON-NEGOTIABLES

1. **Dependencies point inward.** Core ← Shared ← Features ← Orchestration.
2. **No sibling imports** between shared modules or between features.
3. **Env config in orchestration only.** Read once, inject everywhere.
4. **Test critical paths.** Domain logic, auth, payments, data transformations.
5. **Tests co-located** with source files.
6. **Repository interfaces in domain.** Implementations in data.
7. **Errors mapped at boundaries.** No infrastructure errors leak to domain.
8. **Names reveal intent.** Follow language casing conventions.
9. **Functions do one thing.** ≤50 lines target, 80 hard limit.
10. **No dead code, no commented-out code.**
11. **Formatting is automated.**
12. **Pragmatic exceptions require a `// PRAGMATIC:` comment.** Architectural boundaries are never overridden.

---

_"Clean code always looks like it was written by someone who cares." — Robert C. Martin_

_"Make it work. Make it right. Make it fast. In that order." — Kent Beck_
