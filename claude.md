# CLAUDE.md — Clean Code, Clean Architecture & TDD Enforcement

> Every line of code you produce MUST comply with the rules below. No exceptions. No shortcuts. No "just this once." If a rule feels inconvenient, that is the moment it matters most.

---

## 0. TEST-DRIVEN DEVELOPMENT — The Way Code Gets Written

TDD is not a testing strategy. It is the **development methodology**. Every feature, bugfix, and refactor follows the Red-Green-Refactor cycle. No production code exists without a failing test that demanded it.

### 0.1 The Red-Green-Refactor Cycle

```
┌──────────────────────────────────────────────────────┐
│  1. RED     Write a failing test.                    │
│             It must fail for the RIGHT reason.       │
│             It must compile. It must run. It must    │
│             produce a red result.                    │
│                                                      │
│  2. GREEN   Write the MINIMUM production code to     │
│             make the test pass. No more. No          │
│             cleverness. No "while I'm here."         │
│             Hardcode if that's all it takes.         │
│             Temporary ugliness is permitted here —   │
│             Clean Code rules are enforced in step 3. │
│             *** COMMIT at GREEN. ***                 │
│                                                      │
│  3. REFACTOR Now — and ONLY now — clean up.          │
│             Remove duplication. Improve names.       │
│             Extract functions. Apply patterns.       │
│             Enforce EVERY Clean Code rule below.     │
│             ALL tests must stay green throughout.    │
│             *** COMMIT after REFACTOR. ***           │
│                                                      │
│  Repeat. Every 1–5 minutes. Relentlessly.            │
└──────────────────────────────────────────────────────┘
```

### 0.2 The Three Laws of TDD

1. You MUST NOT write production code until you have written a failing test.
2. You MUST NOT write more of a test than is sufficient to fail (including compilation failure).
3. You MUST NOT write more production code than is sufficient to pass the currently failing test.

**Clarification:** Law 2 governs the _incremental writing process_ — you build the test up piece by piece, stopping at each compilation or assertion failure to write the minimum production code. Section 0.5 governs the _finished test_ — once complete, the test must follow Arrange → Act → Assert structure with one logical assertion. These are complementary: Law 2 describes _how you get there_, Section 0.5 describes _what you end up with_.

### 0.3 TDD Workflow for a Feature (Layer by Layer)

```
1. CORE (if the feature needs a new pure utility)
   └─ RED → GREEN → COMMIT → REFACTOR → COMMIT.

2. SHARED (if the feature uses a shared capability that doesn't exist yet)
   └─ Treat the shared module as a mini-feature.
   └─ TDD its domain → data → presentation (if applicable) from inside out.

3. FEATURE — Domain layer first (innermost)
   └─ RED:  Failing test for entity / value object.
   └─ GREEN → COMMIT → REFACTOR → COMMIT.
   └─ RED:  Failing test for use case (with faked repository).
   └─ GREEN → COMMIT → REFACTOR → COMMIT.

4. FEATURE — Data layer next
   └─ RED:  Failing test for repository implementation
            (against in-memory DB / test container / mock API).
   └─ GREEN → COMMIT → REFACTOR → COMMIT.
   └─ RED:  Failing integration test for handler (backend) or API client (frontend).
   └─ GREEN → COMMIT → REFACTOR → COMMIT.

5. FEATURE — Presentation layer (frontend only)
   └─ RED:  Failing ViewModel / hook test (mocked use case).
   └─ GREEN → COMMIT → REFACTOR → COMMIT.
   └─ RED:  Failing UI component test (renders correct state).
   └─ GREEN → COMMIT → REFACTOR → COMMIT.

6. ORCHESTRATION: Wire DI. Smoke test the full vertical slice.
```

### 0.4 Commit Strategy

- **Commit at GREEN.** The behavior is proven. This is your safe rollback point if the refactor goes wrong.
- **Commit after REFACTOR.** The code is now clean. This captures the structural improvement.
- Two separate commits per cycle. Do NOT squash them during development — the GREEN commit is your safety net.
- Squashing is acceptable only when merging to main, if your team prefers linear history.
- **Commit messages:** Imperative mood, ≤72 chars subject, body explains WHY not WHAT.
- **One logical behavior per cycle.** No "fix everything" commits.
- **No secrets, keys, or credentials.** Ever.
- **No generated files** in version control.

### 0.5 Test Quality Rules

- **One logical assertion per test.** A test that checks five things is five tests hiding in a trench coat.
- **Arrange → Act → Assert.** Three sections separated by blank lines. No mixing.
- **Test names describe behavior:** `should_reject_transfer_when_insufficient_funds`, not `test3` or `testTransfer`.
- **No branching logic in tests.** No `if`, `switch`, `try/catch` in test bodies. Parameterized / data-driven tests using a test framework's built-in mechanism (e.g., `it.each`, `@ParameterizedTest`, `#[test_case]`, table-driven tests) are acceptable and encouraged for covering multiple cases cleanly.
- **No test interdependency.** Any test, any order, any subset.
- **Tests are first-class code.** They follow every naming, formatting, and clean code rule in this document during the REFACTOR phase.

### 0.6 Test Pyramid Per Layer

| Layer                  | Test Type                                          | Speed        | Coverage Target       |
| ---------------------- | -------------------------------------------------- | ------------ | --------------------- |
| Core                   | Unit tests (pure logic, zero deps)                 | < 1ms each   | 100% of behavior      |
| Shared (domain)        | Unit tests with fakes/stubs                        | < 5ms each   | 100% of behavior      |
| Shared (data)          | Integration tests                                  | < 500ms each | 90%+                  |
| Feature (domain)       | Unit tests (pure logic, faked repos)               | < 1ms each   | 100% of behavior      |
| Feature (data)         | Integration tests (test containers / mock servers) | < 500ms each | 90%+                  |
| Feature (presentation) | ViewModel/component unit tests                     | < 50ms each  | 90%+                  |
| Shared ↔ Feature       | Contract tests (see 0.8)                           | < 50ms each  | All public interfaces |
| Frontend ↔ Backend     | API contract tests (see 0.9)                       | < 2s each    | All endpoints         |
| Orchestration          | Thin smoke / wiring tests                          | < 2s each    | Critical paths only   |
| E2E                    | Full stack (CI only, not part of TDD cycle)        | Seconds      | Happy paths only      |

_"100% of behavior" means every meaningful code path and edge case is tested. It does not mean chasing line coverage on trivial data class getters or auto-generated boilerplate._

### 0.7 REFACTOR Phase Rules

The refactor phase is NOT optional. After every green:

- Eliminate duplication introduced by the green step.
- Improve names (variables, functions, classes, files).
- Extract methods/classes that do more than one thing.
- Apply the architecture rules from this document (layers, boundaries, SOLID).
- Run all tests **in the affected module** after every refactoring move. If red, undo immediately.
- Run the **full test suite** before committing the refactor.
- Never refactor and add behavior simultaneously. Refactoring changes structure, not behavior.

### 0.8 Contract Tests (Within a Project)

When a feature depends on a shared module's interface, **contract tests** verify the agreement holds as both sides evolve.

- A contract test defines the expected behavior of an interface (e.g., `MessageRepository`) using a **shared test suite that runs against both the fake and the real implementation**.
- When `shared/messaging/` changes its `MessageRepository` interface, the contract test fails in every feature that depends on it.
- Contract tests live next to the interface they protect, in the shared module.

```
// shared/messaging/domain/message-repository.contract-test.ts
// Exported test suite that any implementation must pass.
// Run against FakeMessageRepository in shared/ unit tests.
// Run against SqlMessageRepository in features/chat/data/ integration tests.
```

### 0.9 API Contract Tests (Between Frontend and Backend)

Frontend and backend are independent projects. **API contract tests** prevent drift between them.

- Define API contracts in a **shared schema** (OpenAPI spec, protobuf definitions, or a shared JSON schema file). This schema lives in the `api-contracts/` directory at the monorepo root, accessible to both projects.
- **Backend** validates that its handlers conform to the schema (response shapes, status codes, error formats).
- **Frontend** validates that its API client DTOs conform to the same schema.
- **Both run in CI.** If either side drifts from the schema, CI fails.
- **API versioning:** Use URL path versioning (`/v1/posts`, `/v2/posts`). Version is part of the route definition in Orchestration. API contract schemas are versioned accordingly.

If a shared schema repo is impractical, consumer-driven contract testing (e.g., Pact) is an acceptable alternative where the frontend defines the contracts and backend verifies against them.

### 0.10 Test File Placement

- **Unit tests:** Co-located with the source file they test. `create-post-usecase.test.ts` next to `create-post-usecase.ts`.
- **Integration tests:** Placed in a `tests/` directory at the feature root. Integration tests hit real databases (via test containers), real APIs (via mock servers), or test full request/response cycles.

```
features/create-post/
├── domain/
│   ├── create-post-usecase.ts
│   └── create-post-usecase.test.ts      # Unit test (co-located)
├── data/
│   ├── post-repo-impl.ts
│   └── post-repo-impl.test.ts           # Unit test (co-located)
└── tests/
    ├── post-repo-integration.test.ts     # Hits test container DB
    └── create-post-handler.test.ts       # Full HTTP request/response
```

---

## 1. ARCHITECTURE — Core → Shared → Features → Orchestration

### 1.1 Monorepo Structure

The project is a **monorepo** containing two independent projects and a shared contract directory:

```
project-root/
├── api-contracts/              # Shared API schema (language-agnostic)
│   ├── openapi.yaml            # Or protobuf, JSON schema, etc.
│   └── README.md
├── backend/                    # e.g., Rust — independent project
└── frontend/                   # e.g., React/TypeScript — independent project
```

Backend and frontend are **completely independent projects**. They may be written in different languages. They communicate exclusively via API contracts (REST, GraphQL, gRPC). They share NO source code. The `api-contracts/` directory is the single source of truth for their communication interface.

### 1.2 What Each Layer IS

**Each project independently follows the same four-layer architecture:**

```
backend/                             frontend/
├── core/                            ├── core/
├── shared/                          ├── shared/
├── features/                        ├── features/
└── orchestration/                   └── orchestration/
```

Every rule in this document applies identically within each project. The layers, dependency rules, no-sibling rule, and boundary crossing rules are the same regardless of language.

**Core** is the project's **private standard library**. Pure functions, generic types, base abstractions, utility extensions, and protocol-level interfaces. Core includes:

- Generic types and utilities: `Result<T, E>`, `Either<L, R>`, `Validator<T>`, date/string/collection helpers, branded type helpers, generic base error types.
- Protocol-level interfaces: HTTP client interface, WebSocket interface, generic stream/event interfaces. These are abstract contracts — not framework-specific implementations.

Core has **zero domain knowledge** — it does not know what a "user," "post," or "invoice" is. It has **zero framework dependencies** — protocol interfaces define abstract contracts without depending on any specific library. Core modules MAY import other core modules — they are all generic utilities that naturally build on each other (e.g., `core/validation` may use `core/result`, `core/streams` may use `core/result`).

**Shared** contains **reusable functional modules** — coherent capabilities that multiple features need. Each shared module is a mini vertical slice: it can have its own internal domain/data/presentation layering if the complexity warrants it, or be a simpler flat structure for lightweight utilities. Shared modules exist to eliminate duplication of _functionality_ across features. Shared modules **do NOT wire their own dependencies** — all DI for shared modules is configured in the Orchestration layer.

Examples by project:

- **Backend shared:** `networking` (HTTP client implementation wrapping Core's interface), `messaging` (message queue implementation), `auth-contracts` (auth middleware, token validation), `db` (connection pool, migration runner), `validation` (domain-aware validation rules — email format, phone format, etc.).
- **Frontend shared:** `ui-kit` (design system components), `networking` (API client implementation wrapping Core's interface), `media-picker`, `rich-text-editor`, `analytics`, `validation` (shared form validation rules).

**Note on Shared and Core's interface relationship:** When a shared module implements a Core interface (e.g., `shared/networking/` implements `core/http-client`), the shared module depends on Core — never the reverse. Other shared modules that need the HTTP client depend on Core's interface; Orchestration injects the shared module's implementation. This eliminates sibling imports between shared modules entirely.

**Features** are **full vertical slices**, each a self-contained Clean Architecture mini-app. A feature is the unit of business value. Features may depend on **both Core and Shared**.

- **Backend features** contain: domain (entities, use cases, repository interfaces) and data (handlers/controllers, repository implementations, DB models, external service adapters, DTOs, mappers).
- **Frontend features** contain: domain (entities, use cases, repository interfaces), data (API clients, repository implementations, DTOs, mappers), and presentation (ViewModels/state, UI components, UI models, mappers).

**Orchestration** is the **composition root**. DI containers, app entry points, route definitions, cross-cutting middleware, environment configuration. The only layer that sees and wires all other layers — including shared module internals. Orchestration is where DI lifetimes, scoping, and container mechanics are decided — these are language- and framework-specific and are not prescribed by this document.

### 1.3 The Four Layers — Visual (Per Project)

```
┌─────────────────────────────────────────────────────────────────────┐
│                                                                     │
│   ORCHESTRATION (Layer 4 — outermost)                               │
│   DI containers, composition roots, app bootstrap, routing,         │
│   environment configuration, cross-feature workflows.               │
│   Wires ALL layers including shared module internals.               │
│                                                                     │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│   FEATURES (Layer 3 — vertical slices)                              │
│   Each feature is an isolated vertical slice.                       │
│   Depends on Shared AND Core.                                       │
│   Features NEVER import from sibling features.                      │
│                                                                     │
│   Backend features:               Frontend features:                │
│   ┌──────────────┐               ┌──────────────┐                  │
│   │ create-post  │               │ create-post  │                  │
│   │ ├─ domain/   │               │ ├─ domain/   │                  │
│   │ ├─ data/     │               │ ├─ data/     │                  │
│   │ └─ tests/    │               │ ├─ present./ │                  │
│   └──────────────┘               │ └─ tests/    │                  │
│                                  └──────────────┘                  │
│                  DEPENDS ON ↓ Shared AND Core                       │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│   SHARED (Layer 2 — reusable functional modules)                    │
│   Cross-feature capabilities with optional internal layering.       │
│   Implements Core interfaces (HTTP, WebSocket, streams).            │
│   Shared modules NEVER import sibling shared modules.               │
│   Shared modules do NOT contain their own DI wiring.                │
│   DEPENDS ON ↓ Core only                                            │
│                                                                     │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│   CORE (Layer 1 — innermost, pure standard library)                 │
│   Pure functions, generic types, utilities, protocol interfaces.    │
│   ZERO domain knowledge. ZERO framework dependencies.               │
│   Core modules MAY import other core modules.                       │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 1.4 The Dependency Rule

**Dependencies point INWARD and ONLY inward. No exceptions.**

| Layer         | May Depend On                              | MUST NOT Depend On                             |
| ------------- | ------------------------------------------ | ---------------------------------------------- |
| Core          | Other Core modules. Standard library only. | Shared, Features, Orchestration, any framework |
| Shared        | Core only                                  | Other Shared modules, Features, Orchestration  |
| Features      | Shared and Core                            | Other Features, Orchestration                  |
| Orchestration | Core, Shared, Features (all layers)        | Nothing restricts it — it is the wiring root   |

**How shared modules avoid sibling imports:** Protocol-level interfaces (HTTP client, WebSocket, stream interfaces) live in Core. Shared modules depend on these Core interfaces — never on sibling shared modules. Orchestration injects the concrete implementations. For example, `shared/messaging/data/` depends on `core/http-client` (the interface), and Orchestration injects `shared/networking/http-client-impl` (the implementation).

### 1.5 The No-Sibling Rule

**Shared modules and feature modules are isolated from their siblings.**

- `features/create-post/` MUST NOT import from `features/chat/`.
- `shared/messaging/` MUST NOT import from `shared/networking/`.

**Core modules are exempt from the no-sibling rule.** Core is a pure utility library — its modules naturally build on each other (`core/validation` using `core/result` is expected and correct).

**Within a feature's domain/ layer**, use cases in the same feature MAY call each other — they are in the same module, not siblings. `CreatePostUseCase` may call `ValidatePostContentUseCase` if both live in `features/create-post/domain/`. Use cases across different features MUST NEVER call each other.

If two sibling modules need to communicate:

1. An **interface defined in Core** that both depend on (Orchestration injects the implementation).
2. **Orchestration layer** coordination — composing outputs of both via `workflows/`.

### 1.6 The Boundary Crossing Rule

Data that crosses any layer boundary MUST be:

- A **DTO / data class / plain object** specific to that boundary.
- **Mapped** at the boundary via a dedicated mapper (see Section 1.9).
- Never a raw framework object.

### 1.7 Error Propagation Across Layers

Errors are mapped at every boundary. Never let an inner layer's error type leak outward raw.

```
Core:        Defines generic error base types (e.g., AppError, ValidationError).
             Pure — never throws framework exceptions.

Shared:      Catches infrastructure exceptions internally.
             Maps them to shared-level domain errors or Core error types.
             Exposes Result<T, E> or throws shared domain exceptions.

Feature
  domain:    Defines feature-specific domain errors (e.g., InsufficientFundsError).
             Uses Core base types. Returns domain errors only.

  data:      Catches ALL infrastructure exceptions (SQL, HTTP, I/O).
             Maps to feature domain errors at the repository boundary.
             No infrastructure exception escapes the data layer.

  present.:  Catches domain errors from use cases. (frontend only)
             Maps to UI error state (user-facing messages).
             Never displays raw errors, stack traces, or codes.

Orchestration: Global error boundary / middleware as a safety net.
               Logs unhandled errors. Returns generic error responses.
```

### 1.8 Auth Context Propagation

Use cases often need to know WHO is performing an action. Auth context flows through layers as an **injectable interface**:

- Define an `AuthContext` interface in the **feature's domain/ layer** (or in `shared/` if multiple features need the same contract).
- The interface exposes the current user's identity, roles, or permissions — no framework types.
- **Handlers** (data layer) extract auth info from the request (token, session) and populate the auth context.
- **Orchestration** wires the auth context implementation into use cases via DI.
- Use cases receive `AuthContext` as a constructor dependency, keeping them testable — fakes/stubs replace the real auth context in tests.

```
// feature/create-post/domain/auth-context.ts (or shared/auth-contracts/)
interface AuthContext {
  currentUserId: UserId;
  hasPermission(permission: Permission): boolean;
}

// feature/create-post/domain/create-post-usecase.ts
class CreatePostUseCase {
  constructor(
    private auth: AuthContext,
    private postRepo: PostRepository,
  ) {}
  execute(input: CreatePostInput): Result<Post, PostError> { ... }
}
```

### 1.9 Mapper Rules

Mappers convert data at boundaries. They are critical and bug-prone — treat them with precision.

**Shape:** Mappers are **pure functions**. No classes unless the mapping requires injected configuration (rare). A mapper takes one type and returns another. No side effects, no I/O, no state. If a dependency is needed, pass it as a function argument to keep the mapper pure.

```
// CORRECT
fn to_post_entity(dto: PostApiResponse) -> Post { ... }
function toPostUiModel(entity: Post): PostUiModel { ... }

// CORRECT — dependency as argument, still pure
function toPostUiModel(entity: Post, formatDate: (d: Date) => string): PostUiModel { ... }

// WRONG — mapper as a class with injected dependencies
class PostMapper {
  constructor(private dateFormatter: DateFormatter) {}
  toUiModel(entity: Post): PostUiModel { ... }
}
```

**Organization:** One mapper file per boundary per feature/module. Mapper files are an **explicit exception** to the one-primary-export-per-file rule — they group related mapping functions by boundary:

- `features/X/data/mappers.rs` — maps DTOs ↔ domain entities.
- `features/X/presentation/mappers.ts` — maps domain entities ↔ UI models.

**Composition:** Mappers within the same file MAY call each other. Cross-file mapper imports are not allowed — if two files share mapping logic, extract the shared part to a lower layer.

**Testing:** Mappers are pure functions and MUST be tested. Test every field mapping, especially null/optional handling, enum translation, date/currency formatting, and edge cases (empty collections, missing nested objects). Mapper tests live alongside the mapper file.

### 1.10 Shared-to-Feature Domain Model Relationship

When a feature uses a shared module's domain model, the feature has three options:

**Option 1 — Use directly (default).** When the shared model is sufficient as-is.

**Option 2 — Wrap with feature-specific context.** When the feature needs additional fields. The feature defines its own entity that _contains_ the shared model via composition — never inheritance.

```
// features/chat/domain/chat-message.ts
interface ChatMessage {
  message: Message;           // from shared/messaging
  chatRoomId: ChatRoomId;     // feature-specific
  isHighlighted: boolean;     // feature-specific
}
```

**Option 3 — Map to a feature-local model.** When the feature's concept has diverged significantly. Map at the feature's data layer boundary.

**Never extend/inherit shared models.** Composition over inheritance.

---

## 2. PROJECT STRUCTURE

### 2.1 File Naming Convention

All file names use **kebab-case** (TypeScript, JavaScript) or **snake_case** (Rust, Python, Go — per language convention).

- One **primary export** per file. The file name matches the primary export.
- `PostRepository` interface → `post-repository.ts` / `post_repository.rs`.
- `CreatePostUseCase` class → `create-post-usecase.ts` / `create_post_usecase.rs`.
- **Explicit exceptions:** `mappers` and `dtos` files group related items by boundary — these are the only files that may contain multiple exports.
- Test files: `{name}.test.ts` / `{name}_test.rs` / `test_{name}.py`. Follow language convention.

### 2.2 Backend Project Layout (e.g., Rust)

```
backend/
├── core/                                  # Layer 1 — PURE STANDARD LIBRARY
│   ├── result/
│   │   ├── result.rs                      # Result<T, E>, custom error traits
│   │   └── result_test.rs
│   ├── validation/
│   │   ├── validator.rs                   # Generic Validator<T>, ValidationRule
│   │   └── validator_test.rs              # May use core/result — core siblings OK
│   ├── extensions/
│   │   ├── string_ext.rs
│   │   ├── date_ext.rs
│   │   └── *_test.rs
│   ├── types/
│   │   ├── branded_types.rs               # Newtype/branded type helpers
│   │   └── base_error.rs                  # Base error enum/trait
│   ├── http/
│   │   ├── http_client.rs                 # HTTP client trait (protocol interface)
│   │   ├── http_types.rs                  # Request, Response, Method, StatusCode
│   │   └── *_test.rs
│   └── streams/
│       ├── event_stream.rs                # Generic stream/event interfaces
│       └── *_test.rs
│
├── shared/                                # Layer 2 — REUSABLE MODULES
│   ├── messaging/                         # Complex (internal layers)
│   │   ├── domain/
│   │   │   ├── message.rs
│   │   │   ├── message_repository.rs      # Trait (interface)
│   │   │   ├── send_message_usecase.rs
│   │   │   ├── messaging_errors.rs
│   │   │   ├── message_repository_contract_test.rs
│   │   │   └── *_test.rs
│   │   └── data/
│   │       ├── message_repo_impl.rs
│   │       ├── message_dtos.rs
│   │       ├── mappers.rs
│   │       ├── mappers_test.rs
│   │       └── *_test.rs
│   │
│   ├── networking/                        # Implements core/http/ interface
│   │   ├── http_client_impl.rs            # Concrete HTTP client (reqwest, etc.)
│   │   ├── interceptors.rs
│   │   ├── api_error.rs
│   │   └── *_test.rs
│   │
│   ├── validation/                        # Domain-aware validation rules
│   │   ├── email_rules.rs                 # isValidEmail, etc.
│   │   ├── phone_rules.rs
│   │   └── *_test.rs
│   │
│   ├── auth_middleware/                   # Simple (flat)
│   │   ├── token_validator.rs
│   │   ├── auth_guard.rs
│   │   └── *_test.rs
│   │
│   └── db/                                # Connection pool, migration runner
│       ├── connection_pool.rs
│       ├── transaction.rs                 # Transaction-scoped repo factory
│       └── *_test.rs
│
├── features/                              # Layer 3 — VERTICAL SLICES
│   ├── create_post/
│   │   ├── domain/
│   │   │   ├── post.rs                    # Entity, value objects
│   │   │   ├── post_repository.rs         # Trait (defined HERE)
│   │   │   ├── create_post_usecase.rs     # Single struct, single execute()
│   │   │   ├── validate_post_usecase.rs   # Same-feature use case, callable by create_post
│   │   │   ├── post_errors.rs
│   │   │   └── *_test.rs
│   │   ├── data/
│   │   │   ├── create_post_handler.rs     # HTTP handler / controller
│   │   │   ├── post_repo_impl.rs          # Implements domain/post_repository trait
│   │   │   ├── post_db_model.rs           # DB model (never leaves data/)
│   │   │   ├── dtos.rs                    # Request/response DTOs
│   │   │   ├── mappers.rs
│   │   │   ├── mappers_test.rs
│   │   │   └── *_test.rs
│   │   └── tests/                         # Integration tests
│   │       ├── post_repo_integration_test.rs
│   │       └── create_post_handler_test.rs
│   │
│   ├── chat/
│   │   ├── domain/ ...
│   │   ├── data/ ...
│   │   └── tests/ ...
│   │
│   └── profile/
│       ├── domain/ ...
│       ├── data/ ...
│       └── tests/ ...
│
└── orchestration/                         # Layer 4 — WIRING
    ├── di/
    │   ├── container.rs
    │   ├── shared_modules/
    │   │   ├── messaging_module.rs
    │   │   └── networking_module.rs
    │   └── feature_modules/
    │       ├── create_post_module.rs
    │       ├── chat_module.rs
    │       └── profile_module.rs
    ├── config/
    │   └── app_config.rs                  # Reads ALL env vars, distributes via DI
    ├── main.rs                            # App entry point
    ├── router.rs                          # Route definitions (includes API version prefix)
    ├── workflows/                         # Cross-feature orchestration
    │   └── post_and_notify.rs
    └── middleware/
        ├── logging.rs
        └── error_boundary.rs
```

### 2.3 Frontend Project Layout (e.g., React/TypeScript)

```
frontend/
├── core/                                  # Layer 1 — PURE STANDARD LIBRARY
│   ├── result/
│   │   ├── result.ts
│   │   └── result.test.ts
│   ├── validation/
│   │   ├── validator.ts
│   │   └── validator.test.ts
│   ├── extensions/
│   │   ├── string-extensions.ts
│   │   ├── date-extensions.ts
│   │   └── *.test.ts
│   ├── types/
│   │   ├── branded-types.ts
│   │   └── base-error.ts
│   ├── http/
│   │   ├── http-client.ts                 # HTTP client interface (protocol)
│   │   └── http-types.ts
│   └── streams/
│       ├── event-stream.ts                # Generic stream interfaces
│       └── event-stream.test.ts
│
├── shared/                                # Layer 2 — REUSABLE MODULES
│   ├── messaging/                         # Complex (internal layers)
│   │   ├── domain/
│   │   │   ├── message.ts
│   │   │   ├── message-repository.ts
│   │   │   ├── send-message-usecase.ts
│   │   │   ├── messaging-errors.ts
│   │   │   ├── message-repository.contract-test.ts
│   │   │   └── *.test.ts
│   │   ├── data/
│   │   │   ├── message-api-client.ts
│   │   │   ├── message-repo-impl.ts
│   │   │   ├── message-dtos.ts
│   │   │   ├── mappers.ts
│   │   │   ├── mappers.test.ts
│   │   │   └── *.test.ts
│   │   └── presentation/
│   │       ├── message-input.tsx
│   │       ├── message-bubble.tsx
│   │       ├── message-ui-state.ts
│   │       ├── mappers.ts
│   │       ├── mappers.test.ts
│   │       └── *.test.tsx
│   │
│   ├── ui-kit/                            # Simple (flat)
│   │   ├── button.tsx
│   │   ├── input.tsx
│   │   ├── modal.tsx
│   │   ├── design-tokens.ts
│   │   └── *.test.tsx
│   │
│   ├── networking/                        # Implements core/http/ interface
│   │   ├── http-client-impl.ts            # fetch/axios wrapper
│   │   ├── interceptors.ts
│   │   ├── api-error.ts
│   │   └── *.test.ts
│   │
│   ├── validation/                        # Domain-aware validation rules
│   │   ├── email-rules.ts
│   │   ├── phone-rules.ts
│   │   └── *.test.ts
│   │
│   ├── realtime/                          # Implements core/streams/ interfaces
│   │   ├── websocket-client.ts            # WebSocket connection management
│   │   ├── sse-client.ts                  # Server-Sent Events client
│   │   ├── reconnection-strategy.ts
│   │   └── *.test.ts
│   │
│   └── media-picker/                      # Complex (internal layers)
│       ├── domain/ ...
│       ├── data/ ...
│       └── presentation/ ...
│
├── features/                              # Layer 3 — VERTICAL SLICES
│   ├── create-post/
│   │   ├── domain/
│   │   │   ├── post.ts                    # Entity, value objects
│   │   │   ├── post-repository.ts         # Interface (defined HERE)
│   │   │   ├── create-post-usecase.ts
│   │   │   ├── validate-post-usecase.ts
│   │   │   ├── post-errors.ts
│   │   │   └── *.test.ts
│   │   ├── data/
│   │   │   ├── post-api-client.ts         # Calls backend API
│   │   │   ├── post-remote-repo-impl.ts   # Implements domain/post-repository
│   │   │   ├── dtos.ts                    # API response DTOs
│   │   │   ├── mappers.ts
│   │   │   ├── mappers.test.ts
│   │   │   └── *.test.ts
│   │   ├── presentation/
│   │   │   ├── create-post-viewmodel.ts   # ViewModel / hook / store
│   │   │   ├── create-post-screen.tsx     # Container component
│   │   │   ├── post-editor.tsx            # Presenter component (pure render)
│   │   │   ├── create-post-ui-state.ts    # UI state type
│   │   │   ├── mappers.ts                 # Entity ↔ UI model
│   │   │   ├── mappers.test.ts
│   │   │   └── *.test.tsx
│   │   └── tests/                         # Integration tests
│   │       ├── post-api-client.integration.test.ts
│   │       └── create-post-flow.test.ts
│   │
│   ├── chat/
│   │   ├── domain/ ...
│   │   ├── data/ ...
│   │   ├── presentation/ ...
│   │   └── tests/ ...
│   │
│   └── profile/
│       ├── domain/ ...
│       ├── data/ ...
│       ├── presentation/ ...
│       └── tests/ ...
│
└── orchestration/                         # Layer 4 — WIRING
    ├── di/
    │   ├── container.ts
    │   ├── shared-modules/
    │   │   ├── messaging-module.ts
    │   │   ├── networking-module.ts
    │   │   └── media-picker-module.ts
    │   └── feature-modules/
    │       ├── create-post-module.ts
    │       ├── chat-module.ts
    │       └── profile-module.ts
    ├── config/
    │   └── app-config.ts                  # Reads ALL env vars, distributes via DI
    ├── app-entry.ts                       # App bootstrap
    ├── router.ts                          # Route definitions (versioned: /v1/...)
    ├── workflows/                         # Cross-feature orchestration
    │   └── post-and-notify.ts
    └── middleware/
        ├── auth-guard.ts
        └── error-boundary.ts
```

### 2.4 Use Case Rules

A use case is a single class/struct with a single public method (`execute()`, `invoke()`, or `operator fun invoke()`).

- **One use case = one application action.** `CreatePostUseCase`, not `PostUseCase` with `create()`, `update()`, `delete()`.
- **Constructor/field injection only.** Use cases receive repository interfaces, auth context, and domain services via constructor. No service locators, no ambient context.
- **Same-feature use cases MAY call each other.** They are in the same module.
- **Cross-feature use case calls are FORBIDDEN.** Use Orchestration `workflows/` to compose.
- **Use cases return `Result<T, E>` for expected failures.** Exceptions/panics are reserved for truly unexpected situations.

### 2.5 Internal Feature Architecture — Dependency Table

**Backend features (domain + data):**

```
feature/create-post/
│   data/  ──depends on──▶  domain/
│   domain/ depends on NOTHING inside the feature.
│   domain/ may import from shared/ and core/.
│   data/ implements interfaces defined in domain/.
```

**Frontend features (domain + data + presentation):**

```
feature/create-post/
│   presentation/  ──depends on──▶  domain/
│                                     ▲
│   data/  ────────depends on────────┘
│   data/ and presentation/ NEVER import each other.
```

| Feature Layer | May Depend On                        | MUST NOT Depend On                   |
| ------------- | ------------------------------------ | ------------------------------------ |
| domain/       | Shared modules, Core                 | data/, presentation/, other features |
| data/         | domain/ (same feature), Shared, Core | presentation/, other features        |
| presentation/ | domain/ (same feature), Shared, Core | data/ (same feature), other features |

### 2.6 When Does a Shared Module Need Internal Layers?

**Use domain/data/presentation** when the shared module has its own entities, data persistence, or reusable UI tied to that domain.

**Use flat structure** when the shared module is a collection of utilities, stateless, purely presentational, or a thin SDK wrapper.

### 2.7 Cross-Feature Communication

Features NEVER import siblings. When features must interact:

**Pattern 1 — Shared Abstraction:**
Both features depend on a shared module's interface. Neither knows the other exists.

**Pattern 2 — Orchestration Composition:**

```
// orchestration/workflows/post-and-notify.ts
// Composes CreatePostUseCase + SendNotificationUseCase.
// The ONLY place cross-feature logic lives.
```

These are the **only two patterns** for cross-feature communication. If you find yourself reaching for anything more complex (event bus, message queue between features, etc.), reconsider whether the features are correctly bounded — the need for complex inter-feature communication is often a sign that the feature boundaries are wrong.

---

## 3. FRONTEND-SPECIFIC RULES

### 3.1 State Management

- UI state is a **single immutable data class/object** per screen/feature.
- State updates happen through a **ViewModel / store / reducer** — never in UI components.
- UI components are **pure rendering functions**: `(state) → UI`. No business logic.

### 3.2 Component Rules

- Components MUST be **small and single-purpose**.
- **Container/Presenter split:** Containers connect to state. Presenters receive props and render. Presenters have ZERO dependencies on state management.
- **No API calls in components.**
- **No business logic in components.** Formatting for display is OK. Calculations are NOT.
- **No direct Shared module usage in Presenters.** Only Containers connect to shared modules.

### 3.3 Frontend Testing Strategy (TDD)

```
1. RED:  Failing ViewModel test → GREEN → COMMIT → REFACTOR → COMMIT.
2. RED:  Failing Presenter test → GREEN → COMMIT → REFACTOR → COMMIT.
3. RED:  Failing Container test (optional) → GREEN → COMMIT → REFACTOR → COMMIT.
```

### 3.4 Styling

- Co-locate styles with components.
- Design tokens from `shared/ui-kit/design-tokens` for colors, spacing, typography.
- No magic numbers.

### 3.5 Navigation/Routing

- Route definitions live in `orchestration/router.ts`.
- Features expose their root screen. They do NOT define their own routes.
- Navigation between features goes through the router — never direct imports.

---

## 4. BACKEND-SPECIFIC RULES

### 4.1 Handler/Controller Rules

- Each handler handles **one endpoint / one action**.
- Handler responsibility: parse input → validate shape → call use case → format response.
- Handlers are **thin.** Over 20 lines means logic is leaking.
- Shape validation in the handler. Business validation in the use case.

### 4.2 Repository Implementation

- Implements the trait/interface defined in the **feature's own domain/ layer**.
- Encapsulates ALL database/storage details.
- Returns domain entities via mappers.
- DB models are **private to data/** and never exported.
- **Caching is a data layer concern.** Each repository implementation manages its own caching strategy internally (TTL, invalidation, etc.). The domain layer is unaware of caching.

### 4.3 External Service Adapters

- Every third-party API wrapped in an adapter implementing a domain/ or shared/ interface.
- Adapter handles: HTTP calls, retries, rate limiting, parsing, error mapping.

### 4.4 Backend Testing Strategy (TDD)

```
1. RED:  Failing domain test → GREEN → COMMIT → REFACTOR → COMMIT.
2. RED:  Failing use case test (faked repo) → GREEN → COMMIT → REFACTOR → COMMIT.
3. RED:  Failing repo impl test (test container) → GREEN → COMMIT → REFACTOR → COMMIT.
4. RED:  Failing handler integration test → GREEN → COMMIT → REFACTOR → COMMIT.
```

### 4.5 API Design

- RESTful or RPC — pick one, be consistent.
- **URL path versioning:** All routes include a version prefix (`/v1/posts`, `/v2/posts`). Version prefix is defined in `orchestration/router`.
- Request/response DTOs in `data/dtos`. NOT domain entities.
- Error responses follow a consistent schema (defined in `shared/networking/` or `api-contracts/`).

### 4.6 Database

- Migrations version-controlled and sequential.
- DB models in `data/`. Never leave that layer. Map at the boundary.
- **Transaction ownership:** The data layer exposes a **transaction-scoped repository factory**. When a use case or orchestration workflow needs atomic operations across multiple repositories, it requests a transaction scope from the factory — all repositories created within that scope share the same transaction. The domain layer defines the _need_ for atomicity; the data layer provides the _mechanism_.

```
// shared/db/transaction.rs (or feature domain interface)
trait TransactionScope {
  fn post_repo(&self) -> &dyn PostRepository;
  fn notification_repo(&self) -> &dyn NotificationRepository;
  async fn commit(self) -> Result<(), DbError>;
  async fn rollback(self) -> Result<(), DbError>;
}
```

---

## 5. NAMING

### 5.1 Intent-Revealing Names

- Every name MUST reveal **why it exists, what it does, and how it is used**.
- If a name requires a comment, the name is wrong.
- **BANNED names:** `data`, `info`, `temp`, `tmp`, `val`, `item`, `obj`, `result`, `res`, `ret`, `resp`, `payload`, `stuff`, `thing`, `misc`, `utils` (as a class/module name).

### 5.2 Naming Conventions

- **Classes/Types/Structs/Traits:** `PascalCase`. Noun or noun phrase.
- **Functions/Methods:** `camelCase` (TS/JS/Kotlin) or `snake_case` (Rust/Python/Go). Verb or verb phrase.
- **Booleans:** `is_active`/`isActive`, `has_permission`/`hasPermission`, `can_retry`/`canRetry`.
- **Constants:** `UPPER_SNAKE_CASE`.
- **Enums:** `PascalCase` type, `UPPER_SNAKE_CASE` members (or `PascalCase` variants in Rust).
- **Files:** `kebab-case` (TS/JS) or `snake_case` (Rust/Python/Go). Follow language convention.
- **No single-letter variables** except trivial loop counters in ≤3-line loops and obvious closure params.

### 5.3 Scope-Length Rule

- Longer scope → longer, more descriptive name. Shorter scope → shorter acceptable.

### 5.4 Consistency

- One word per concept across the entire project. Never mix `fetch`/`get`/`retrieve`/`load` for the same operation.

---

## 6. FUNCTIONS

### 6.1 Size

- Target: **≤20 lines**. Hard limit: **30 lines**. Extract immediately if exceeded.

### 6.2 Do One Thing

- A function does one thing if you cannot extract another meaningful function from it.
- **Step-Down Rule:** Code reads top-to-bottom as descending abstraction.

### 6.3 Arguments

- Ideal: 0. Acceptable: 1–2. Suspicious: 3. Forbidden: 4+ (wrap into a struct/object).
- **No boolean/flag arguments.** Split into two functions.

### 6.4 CQS & Side Effects

- No hidden side effects.
- **Command Query Separation:** Does something (returns void/unit) OR answers something (changes nothing). Standard mutation-and-return operations (`pop()`, `dequeue()`, `entry().or_insert()`) are acknowledged exceptions.

### 6.5 Error Handling

- `Result<T, E>` for expected domain failures. Exceptions/panics for unexpected failures.
- Never return `null`/`None` where a meaningful result is expected. Use `Option`, `Result`, or empty collections.
- `try-catch` / `match` on errors at layer boundaries only.

### 6.6 DRY

- Same logic twice → extract. No copy-paste. Ever.

---

## 7. COMMENTS — Minimized Ruthlessly

### 7.1 Acceptable (the ONLY kinds)

- Legal headers.
- Non-obvious business rule explanation with regulatory/domain citation.
- Clarification of an unmodifiable external API.
- Warning of consequences.
- `TODO(TICKET-ID)` with a real ticket number only.
- Public API doc comments for library / shared module code consumed by others.
- **Pragmatic exception comments** (see Section 14).

### 7.2 Banned

- Redundant comments restating code.
- Journal/changelog comments.
- Noise comments.
- Closing brace comments.
- **Commented-out code. DELETE IT.**
- Section markers.
- Attribution comments.

---

## 8. FORMATTING

### 8.1 Vertical

- Files: **50–500 lines**. Over 500 → split. Test files may exceed when covering one module comprehensively.
- Blank lines separate concepts. Variables near usage. Caller above callee.

### 8.2 Horizontal

- **100 chars soft limit. 120 hard limit.**
- Automated formatters enforced via CI. No exceptions. Use language-standard tools (Prettier, rustfmt, Black, gofmt, ktfmt, etc.).

---

## 9. OBJECTS, DATA STRUCTURES & SOLID

### 9.1 Data/Object Anti-Symmetry

- **Objects** hide data, expose behavior. **Data structures** expose data, no behavior. No hybrids.

### 9.2 Law of Demeter

- A method calls methods on: itself, its parameters, objects it creates, its own fields.
- **BANNED:** `a.get_b().get_c().do_thing()`. Refactor into delegation. Fluent builder APIs that return `self`/`this` for chaining are an acknowledged exception — the chain operates on the same object, not traversing a dependency graph.

### 9.3 SOLID

| Principle | Rule                                                                                        |
| --------- | ------------------------------------------------------------------------------------------- |
| **SRP**   | One class/struct, one reason to change. Description uses "and" → split.                     |
| **OCP**   | Open for extension, closed for modification. Polymorphism/traits over `if/else`.            |
| **LSP**   | Subtypes substitutable for base types without correctness change.                           |
| **ISP**   | Many small interfaces/traits. No client depends on unused methods.                          |
| **DIP**   | High-level depends on abstractions. Inner layers define interfaces, outer layers implement. |

---

## 10. CONCURRENCY & REAL-TIME

### 10.1 General Concurrency Rules

- Concurrent code is **separate** from business logic.
- Structured concurrency (tokio tasks with supervision, async/await, coroutines with `supervisorScope`).
- Immutable data and message passing over shared mutable state.
- Critical sections as small as possible.

### 10.2 Streaming & Real-Time Architecture

For WebSocket, SSE, and other persistent/streaming connections:

**Layer placement:**

- **Core** defines generic stream interfaces (`EventStream<T>`, `StreamSubscription`, connection state enums). These are protocol-aware but framework-free.
- **Shared** implements connection management (`shared/realtime/`). WebSocket clients, SSE clients, reconnection strategies, heartbeat logic. Implements Core's stream interfaces using concrete libraries.
- **Features** consume streams via Core's interfaces. A feature's data layer subscribes to streams and maps incoming messages. A feature's domain layer works with domain entities — it never knows the transport is a WebSocket.

**Boundary mapping for streams:** Streams follow the same pattern as repositories — **map at source, map at presentation**:

- The **data layer** subscribes to the raw stream and maps each incoming message from wire DTOs to domain entities. The domain layer receives a stream of domain types.
- The **presentation layer** (frontend) maps domain entities to UI models for display.
- The domain layer works exclusively with domain types — it is unaware of the transport mechanism or wire format.

```
// Feature data layer: maps at source
class PostStreamRepoImpl implements PostStreamRepository {
  constructor(private ws: EventStream<RawMessage>) {}  // Core interface

  postUpdates(): AsyncIterable<Post> {
    return this.ws.messages().map(raw => toPostEntity(raw));  // DTO → domain
  }
}

// Feature presentation: maps for display
function usePostStream(repo: PostStreamRepository) {
  // domain entity → UI model
  const uiPosts = repo.postUpdates().map(post => toPostUiModel(post));
}
```

**Connection lifecycle** is managed by `shared/realtime/` — reconnection, backoff, heartbeat. Features subscribe/unsubscribe via the interface; they don't manage connections.

**Testing:** Stream-producing code is tested with fake/mock streams that emit predetermined sequences. Test the mapping independently from the stream mechanics.

---

## 11. CODE SMELLS — Automatic Refactoring Triggers

If ANY appear, refactor BEFORE committing:

| Smell                                         | Action                                           |
| --------------------------------------------- | ------------------------------------------------ |
| Function > 20 lines                           | Extract                                          |
| Class/file > 500 lines                        | Extract                                          |
| 3+ function arguments                         | Struct/object parameter                          |
| Nested `if` > 2 levels                        | Guard clauses, extract, or polymorphism          |
| `switch`/`match` on type                      | Polymorphism (unless idiomatic pattern matching) |
| Duplicate code (even 2×)                      | Extract                                          |
| Feature Envy                                  | Move to the struct/class whose data it uses      |
| Data Clump (same 3+ fields)                   | Value object / struct                            |
| Primitive Obsession                           | Branded type / newtype                           |
| God Class / God Function                      | Decompose                                        |
| Dead code                                     | Delete                                           |
| Commented-out code                            | Delete                                           |
| Cross-layer import violation                  | Fix dependency direction                         |
| Sibling import (shared or feature level)      | Extract abstraction to Core or lower layer       |
| Domain entity leaking out of feature boundary | Add mapper + DTO                                 |
| Framework type in domain layer                | Wrap behind trait/interface                      |

---

## 12. DEPENDENCY ENFORCEMENT

Architecture rules are only real if enforced by tooling.

- **TypeScript:** ESLint `eslint-plugin-boundaries` or `import/no-restricted-paths`. Nx / Turborepo module boundaries. Separate `tsconfig.json` per layer.
- **Rust:** Cargo workspace with per-crate `Cargo.toml` dependencies. If it's not in `[dependencies]`, it can't be imported. Compile-time enforcement.
- **Kotlin/Android:** Gradle multi-module with `api`/`implementation`. Detekt custom rules.
- **Go:** Package-level imports. `go vet` or custom linters.
- **General:** CI checks that grep for forbidden import patterns and fail the build.

If a layer violation compiles, the enforcement is inadequate. Fix the tooling.

---

## 13. LOGGING

### 13.1 Where Logging Is Allowed

| Layer                  | Logging Allowed? | What to Log                                                                     |
| ---------------------- | ---------------- | ------------------------------------------------------------------------------- |
| Core                   | **No**           | Pure functions — nothing to log                                                 |
| Shared (domain)        | **No**           | Domain logic stays pure                                                         |
| Shared (data)          | **Yes**          | Infrastructure operations: HTTP calls, connection events, retries               |
| Feature (domain)       | **No**           | Use cases stay pure — no side effects                                           |
| Feature (data)         | **Yes**          | DB queries, external API calls, cache hits/misses, mapping failures             |
| Feature (presentation) | **No**           | UI layer — use error boundaries, not logging                                    |
| Orchestration          | **Yes**          | Startup, DI wiring, unhandled errors, workflow orchestration, request lifecycle |

### 13.2 Logging Format

- **Structured logging** (JSON) in production. Human-readable in development.
- Every log entry includes: timestamp, level, correlation/request ID, source module.
- No sensitive data in logs (tokens, passwords, PII).

### 13.3 Log Levels

| Level     | Usage                                                                                                   |
| --------- | ------------------------------------------------------------------------------------------------------- |
| **ERROR** | Unexpected failures requiring investigation. Unhandled exceptions caught at boundaries.                 |
| **WARN**  | Expected but unusual conditions. Rate limit approaches, retry attempts, degraded service.               |
| **INFO**  | Significant business operations. Request lifecycle, workflow completion, external calls.                |
| **DEBUG** | Detailed diagnostic info. Query parameters, response shapes, cache decisions. Development/staging only. |

---

## 14. ENVIRONMENT CONFIGURATION

- **All environment variables are read in Orchestration only** — specifically in `orchestration/config/`.
- Configuration values are read once at application bootstrap and distributed to lower layers via DI (as typed config structs/objects, never raw strings).
- Lower layers (Core, Shared, Features) MUST NOT read environment variables directly. They receive configuration through constructor injection.
- Sensitive configuration (API keys, database URLs) follows the same rule — injected, never imported.

```
// orchestration/config/app-config.ts
interface AppConfig {
  database: DatabaseConfig;
  auth: AuthConfig;
  features: FeatureFlags;
}

// Read once, inject everywhere
const config = loadFromEnvironment();  // reads process.env
container.register(config.database);   // injected into shared/db/
container.register(config.auth);       // injected into shared/auth/
```

---

## 15. PRAGMATIC EXCEPTIONS

Rules exist to prevent bad defaults, not to override good judgment. **Pragmatic exceptions are permitted** when strict compliance would produce worse code — but they carry requirements:

1. **The exception must be commented.** A brief `// PRAGMATIC: <reason>` comment explains why the rule is broken and what would be worse if it weren't.
2. **The exception must be local.** It applies to this specific instance, not as a new general policy.
3. **The exception must not violate architectural boundaries.** Layer dependencies, sibling isolation, and boundary mapping rules are never overridden — these are structural, not stylistic.

Examples of acceptable exceptions:

- A function is 25 lines because extracting it would require passing 6 arguments and reduce readability. `// PRAGMATIC: extraction here trades one smell (length) for a worse one (argument count).`
- A test has two closely related assertions that are meaningless in isolation. `// PRAGMATIC: these two assertions verify a single atomic behavior (request + response).`

Examples of things that are **never** exceptions:

- Cross-feature imports. Restructure instead.
- Skipping tests. Write the test.
- Domain entities crossing boundaries unmapped. Add the mapper.

---

## 16. ABSOLUTE NON-NEGOTIABLES

1. **TDD Red-Green-Refactor for every change.** Tests first. GREEN permits temporary mess. REFACTOR enforces Clean Code.
2. **Commit at GREEN. Commit again after REFACTOR.** Two commits per cycle.
3. **Monorepo:** Backend, frontend, and API contracts live in one repository. Backend and frontend are independent projects that share NO source code.
4. **Core is a pure standard library.** Zero domain knowledge. Zero frameworks. Includes protocol-level interfaces (HTTP, WebSocket, streams). Core modules may import each other.
5. **Shared modules are reusable capabilities.** Depend only on Core. No sibling imports. No self-wiring DI. Includes domain-aware validation rules.
6. **Features are vertical slices with domain/data(/presentation).** Depend on Shared and Core. No sibling imports. No cross-feature imports.
7. **Orchestration is the only layer that sees everything.** All DI wiring, environment config, and route definitions live here.
8. **Use cases are single-class, single-method.** Same-feature composition OK. Cross-feature composition in Orchestration only.
9. **Auth context is injectable.** Use cases receive an `AuthContext` interface, never extract auth from requests.
10. **Repository interfaces live in the consumer's domain/ layer.**
11. **Data crosses boundaries as DTOs, always mapped.** Mappers are pure functions, tested, one file per boundary. Mappers and DTOs are explicit exceptions to one-primary-export-per-file.
12. **Streams map at source and at presentation.** Data layer maps wire format → domain entities. Presentation maps domain → UI models. Domain layer is transport-unaware.
13. **Errors are mapped at every boundary.**
14. **API contracts between frontend and backend are schema-defined, versioned (URL path), and tested in CI.**
15. **Caching is a data layer concern.** Repositories manage their own caching. Domain layer is cache-unaware.
16. **Transactions use a factory pattern.** Data layer exposes transaction-scoped repo factories.
17. **Logging in data layer and orchestration only.** Domain and presentation layers do not log. Structured JSON format.
18. **Environment config in orchestration only.** Read once at bootstrap, injected via DI.
19. **Names reveal intent.** Follow language-specific casing conventions.
20. **Functions are small and do one thing.** ≤20 lines target. 30 hard limit.
21. **No dead code, no commented-out code, no TODOs without tickets.**
22. **Formatting is automated and consistent.** Enforced in CI.
23. **SOLID is not optional.**
24. **Layer enforcement is automated.** If a violation compiles, fix the tooling.
25. **Pragmatic exceptions require a `// PRAGMATIC: <reason>` comment.** Architectural boundaries are never overridden.
26. **When in doubt, write a test first, then refactor toward simplicity.**

---

_"Clean code always looks like it was written by someone who cares." — Robert C. Martin_

_"Make it work. Make it right. Make it fast. In that order." — Kent Beck_
