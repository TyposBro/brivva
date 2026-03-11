# Rust Learning Project — Translation CLI

## Purpose

Learn Rust by building a small CLI that mirrors part of the Brivva translation pipeline.
This is for interview prep — I need to demonstrate basic Rust literacy for Brivva's paid technical test.
I have ADHD — learn best by fixing real bugs, not reading textbooks.

## My Rust Level

- Read chapters 1-3 of the Rust Book weeks ago, didn't practice
- Don't know how to print hello world from memory
- Strong in TypeScript, Kotlin, React — can map concepts if explained in those terms
- Need to understand: ownership, borrowing, structs, enums, error handling, traits, async

## IMPORTANT: Teaching Mode Rules

This is a LEARNING project. The goal is for ME to learn Rust, not for Claude to write perfect code.

1. **Never write more than 20 lines at a time.** Give me a small task, let me attempt it, then review.
2. **When I hit a compiler error, don't fix it for me.** Explain what the error means and give me a hint. Let me fix it.
3. **After each milestone, ask me to explain what I learned** in my own words before moving on.
4. **Use TypeScript/Kotlin analogies** to explain Rust concepts (I think in those languages).
5. **Intentionally use patterns that will trigger common Rust errors** so I learn from the compiler:
   - Move a String into a function, then try to use it after → ownership lesson
   - Try to mutate a borrowed reference → borrow checker lesson
   - Return a reference to a local variable → lifetime lesson
   - Forget to handle a Result → error handling lesson
6. **Don't add error handling, cloning, or lifetime annotations preemptively.** Let me hit the wall first.

## Project: Translation CLI (build incrementally)

### Phase 1: Basics (Day 1)

Build up from zero. Each step is a separate task:

1. **Hello World** — `cargo new translate-cli`, print "Hello, world!", run it
2. **Variables & Types** — declare a string, an integer, print both with `println!` formatting
3. **Functions** — write a function that takes a `&str` and returns a `String` (uppercase version)
4. **Structs** — create a `TranslationRequest { text: String, source_lang: String, target_lang: String }`
5. **Impl block** — add a method `fn describe(&self) -> String` that formats the request
6. **Enums** — create `Language { English, Japanese, Chinese, Korean }` with Display trait
7. **Error handling** — create a function that can fail, return `Result<T, E>`, use `?` operator
8. **Collections** — store multiple TranslationRequests in a Vec, iterate and print

### Phase 2: Real Functionality (Day 1-2)

9. **CLI args** — use `std::env::args()` to accept text + source + target from command line
10. **HTTP client** — add `reqwest` crate, make a GET request to httpbin.org, print response
11. **Async** — convert to async with `tokio`, make async HTTP request
12. **JSON** — add `serde` + `serde_json`, deserialize API response into a struct
13. **Call M2M100** — send translation request to CF Workers AI API (or a mock endpoint), parse response
14. **Error types** — create custom `TranslateError` enum, implement `From` for reqwest and serde errors

### Phase 3: Server (Day 2-3)

15. **Axum hello** — create a basic axum HTTP server, one GET endpoint returns "hello"
16. **POST endpoint** — accept JSON body `{ text, source, target }`, return translated text
17. **State** — add shared state (request counter) using `Arc<Mutex<T>>`
18. **Multiple routes** — add health check, translate, and stats endpoints
19. **WebSocket** — add a WebSocket endpoint using axum's ws support
20. **Streaming** — accept text over WebSocket, translate, send back result

### Phase 4: Brivva-Relevant Patterns (Day 3)

21. **Room struct** — create a Room with host + guests (HashMap of connections)
22. **Fan-out** — translate once, send to multiple "guests" (simulated with channels)
23. **Concurrent translations** — use `tokio::join!` to translate to multiple languages in parallel
24. **Graceful shutdown** — handle Ctrl+C, clean up rooms

## Tech Stack

- **Rust** (latest stable via rustup)
- **tokio** — async runtime
- **axum** — HTTP/WebSocket server (same family as Brivva likely uses)
- **reqwest** — HTTP client
- **serde** + **serde_json** — JSON serialization
- **cargo** — build/run

## Project Structure

```
rust-learn/
├── CLAUDE.md          ← you are here
└── translate-cli/     ← cargo project (created by `cargo new`)
    ├── Cargo.toml
    └── src/
        └── main.rs
```

## Rules

- I type the code. Claude reviews and hints.
- One concept at a time. Don't rush.
- If I'm stuck for 5+ minutes, give a bigger hint but still don't write the full solution.
- Celebrate small wins — this is hard and that's okay.
- Compare to TypeScript/Kotlin whenever possible:
  - `String` vs `&str` → like `String` vs `string` in Kotlin, or owned vs borrowed
  - `Option<T>` → like `T?` in Kotlin
  - `Result<T, E>` → like try/catch but compile-time enforced
  - `match` → like `when` in Kotlin
  - `impl Trait` → like interfaces in Kotlin/TS
  - `Vec<T>` → like `Array<T>` in TS or `List<T>` in Kotlin
  - Ownership/move → like there's no garbage collector, you manually track who "owns" each value
