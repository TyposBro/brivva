# Contracts

Brivva contracts are the source of truth for:

- shared Zod runtime validation between `workers` and `frontend`
- exported OpenAPI for the Workers HTTP API
- generated frontend client types
- Rust-side contract checks for the internal Workers API

Current contract version: `0.2.0`

## Rules

1. Any breaking contract change must update `contracts/src/meta.ts`.
2. Any contract change must add one entry to `contracts/CHANGELOG.md`.
3. After changing Workers route shapes, run `bun run contracts:generate`.
4. If the change affects internal Workers routes used by `server-rs`, run `cargo test -p server-rs`.
