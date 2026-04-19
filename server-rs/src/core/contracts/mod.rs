//! Cross-language contract types shared with the Workers API.
//!
//! `workers` holds the generated Rust bindings for the subset of the Workers
//! OpenAPI schema that server-rs deserializes. Do not hand-edit that file.
//! Regenerate it with `bun run --cwd contracts gen:rust`.

pub mod workers;
