// Generate Rust structs from the Workers OpenAPI export.
//
// Why this exists:
//   server-rs calls the Workers /internal/sessions/* endpoints. The payload
//   shapes are defined once in contracts/src/http.ts (zod) and emitted into
//   contracts/openapi/brivva-workers.json. This script turns the subset that
//   server-rs consumes into Rust structs with serde derives, so a schema
//   change on the Workers side hard-breaks the Rust build rather than
//   surviving as a silent deserialization bug.
//
// Scope is intentionally narrow — only the types server-rs actually
// deserializes. Adding a new type here is a conscious step, not a sweep.
//
// Run:
//   bun run --cwd contracts gen:rust
//
// Output:
//   server-rs/src/core/contracts/workers.rs (checked in, header-tagged
//   generated, CI fails if regenerating produces a diff)

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";

type SchemaRef = { $ref: string };
type Primitive = "string" | "integer" | "number" | "boolean";
type JsonSchema =
  | { type: Primitive; nullable?: boolean; enum?: unknown[] }
  | { type: "array"; items: JsonSchema | SchemaRef; nullable?: boolean }
  | {
      type: "object";
      required?: string[];
      properties?: Record<string, JsonSchema | SchemaRef>;
      nullable?: boolean;
    }
  | { allOf: (JsonSchema | SchemaRef)[]; nullable?: boolean }
  | SchemaRef;

type OpenApi = {
  components: { schemas: Record<string, JsonSchema> };
};

// ── Config ───────────────────────────────────────────────
// Rust struct name → OpenAPI schema name. Hand-curated.
const EXPORTS: Array<{ rustName: string; schemaName: string }> = [
  { rustName: "Session", schemaName: "Session" },
  { rustName: "Stream", schemaName: "Stream" },
  { rustName: "Voice", schemaName: "Voice" },
  { rustName: "SessionBundle", schemaName: "InternalSessionBundle" },
  { rustName: "SessionStatusUpdate", schemaName: "InternalSessionStatusUpdate" },
];

const REPO_ROOT = resolve(import.meta.dir, "..", "..");
const OPENAPI_PATH = resolve(REPO_ROOT, "contracts/openapi/brivva-workers.json");
const OUT_PATH = resolve(REPO_ROOT, "server-rs/src/core/contracts/workers.rs");

// ── Resolver ─────────────────────────────────────────────
function resolveRef(spec: OpenApi, ref: string): JsonSchema {
  const match = /^#\/components\/schemas\/(.+)$/.exec(ref);
  if (!match) throw new Error(`unsupported $ref: ${ref}`);
  const schema = spec.components.schemas[match[1]!];
  if (!schema) throw new Error(`missing schema: ${match[1]}`);
  return schema;
}

function isRef(node: unknown): node is SchemaRef {
  return typeof node === "object" && node !== null && "$ref" in (node as object);
}

// For $ref-to-exported type, return the Rust name we'll emit for it.
function rustNameForRef(ref: string): string | null {
  const match = /^#\/components\/schemas\/(.+)$/.exec(ref);
  if (!match) return null;
  const schemaName = match[1]!;
  const found = EXPORTS.find((e) => e.schemaName === schemaName);
  return found?.rustName ?? null;
}

// ── Type mapping ─────────────────────────────────────────
function mapType(
  node: JsonSchema,
  spec: OpenApi,
  nullableOverride?: boolean,
): string {
  if (isRef(node)) {
    const rustName = rustNameForRef(node.$ref);
    if (!rustName) {
      throw new Error(
        `referenced schema ${node.$ref} is not in the Rust export allow-list. Add it to EXPORTS in contracts/scripts/gen-rust.ts.`,
      );
    }
    return nullableOverride ? `Option<${rustName}>` : rustName;
  }

  // allOf with a single $ref + nullable is the OpenAPI idiom for optional refs.
  if ("allOf" in node) {
    if (node.allOf.length !== 1) {
      throw new Error(
        `allOf with >1 child not supported yet: ${JSON.stringify(node)}`,
      );
    }
    return mapType(node.allOf[0]!, spec, nullableOverride ?? node.nullable);
  }

  const nullable = nullableOverride ?? node.nullable ?? false;

  if (node.type === "array") {
    const inner = mapType(node.items as JsonSchema, spec);
    return nullable ? `Option<Vec<${inner}>>` : `Vec<${inner}>`;
  }

  if (node.type === "object") {
    throw new Error(
      "inline object types are not supported — promote to a named schema in contracts/src/http.ts",
    );
  }

  const primitive =
    node.type === "string"
      ? "String"
      : node.type === "integer"
      ? "i64"
      : node.type === "number"
      ? "f64"
      : node.type === "boolean"
      ? "bool"
      : null;
  if (!primitive) {
    throw new Error(`unsupported primitive: ${JSON.stringify(node)}`);
  }
  return nullable ? `Option<${primitive}>` : primitive;
}

// Some OpenAPI fields serialize JSON as numbers that server-rs already binds
// to narrower Rust types. Apply overrides per (rustStructName, fieldName).
// Keeps the generator dumb while letting us match the hand-rolled shape.
const FIELD_OVERRIDES: Record<string, Record<string, string>> = {
  Stream: {
    delay_ms: "u64",
    host_gain: "f32",
  },
};

// ── Emit ─────────────────────────────────────────────────
function emitStruct(rustName: string, schemaName: string, spec: OpenApi): string {
  const schema = spec.components.schemas[schemaName];
  if (!schema || isRef(schema) || !("type" in schema) || schema.type !== "object") {
    throw new Error(`expected object schema for ${schemaName}`);
  }
  const required = new Set(schema.required ?? []);
  const overrides = FIELD_OVERRIDES[rustName] ?? {};

  const lines: string[] = [];
  lines.push(`#[derive(Debug, Clone, Deserialize, Serialize)]`);
  lines.push(`pub struct ${rustName} {`);
  for (const [name, prop] of Object.entries(schema.properties ?? {})) {
    const isRequired = required.has(name);
    let rustType: string;
    if (overrides[name]) {
      rustType = isRequired ? overrides[name]! : `Option<${overrides[name]}>`;
    } else {
      rustType = mapType(prop as JsonSchema, spec, !isRequired ? true : undefined);
    }
    if (!isRequired) {
      lines.push(`    #[serde(default)]`);
    }
    lines.push(`    pub ${name}: ${rustType},`);
  }
  lines.push(`}`);
  return lines.join("\n");
}

function emitFile(spec: OpenApi): string {
  const header = [
    "// @generated — DO NOT EDIT.",
    "//",
    "// Rust bindings for the subset of the Workers OpenAPI schema that",
    "// server-rs deserializes on the /internal/sessions/* endpoints.",
    "//",
    "// Regenerate with:  bun run --cwd contracts gen:rust",
    "// Source of truth:  contracts/openapi/brivva-workers.json",
    "",
    "#![allow(clippy::too_many_lines)]",
    "",
    "use serde::{Deserialize, Serialize};",
    "",
  ].join("\n");

  const bodies = EXPORTS.map((e) => emitStruct(e.rustName, e.schemaName, spec));
  return `${header}\n${bodies.join("\n\n")}\n`;
}

// ── Main ─────────────────────────────────────────────────
function main(): void {
  const raw = readFileSync(OPENAPI_PATH, "utf-8");
  const spec = JSON.parse(raw) as OpenApi;
  const rust = emitFile(spec);
  mkdirSync(dirname(OUT_PATH), { recursive: true });
  writeFileSync(OUT_PATH, rust, "utf-8");
  console.log(`[gen-rust] wrote ${OUT_PATH} (${rust.length} bytes)`);
}

main();
