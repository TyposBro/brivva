const MAX_BATCH = 100;
const MAX_FIELD_BYTES = 16_000;
const LEVELS = new Set(["debug", "info", "warn", "error"]);
const SOURCES = new Set(["frontend", "workers", "server-rs"]);

export type SessionLogInput = {
  session_id: string;
  live_session_id?: string | null;
  source: string;
  level: string;
  event: string;
  message?: string | null;
  fields?: Record<string, unknown>;
  ts_ms?: number;
};

export type NormalizedSessionLogEvent = {
  sessionId: string;
  liveSessionId: string | null;
  source: string;
  level: string;
  event: string;
  message: string | null;
  fields: Record<string, unknown>;
  tsMs: number;
};

export function sessionLogsEnabled(value: string | undefined): boolean {
  return envFlag(value);
}

export function sessionLogConsoleEnabled(value: string | undefined): boolean {
  return envFlag(value);
}

export function envFlag(value: string | undefined): boolean {
  return ["1", "true", "yes", "on"].includes(
    (value ?? "").trim().toLowerCase(),
  );
}

export function normalizeSessionLogBatch(
  input: unknown,
  expectedSessionId?: string,
): NormalizedSessionLogEvent[] {
  const events = normalizeBatch(input);
  if (
    expectedSessionId &&
    events.some((event) => event.sessionId !== expectedSessionId)
  ) {
    throw new Error("session log batch contains mismatched session_id");
  }
  return events;
}

export function normalizeWorkerSessionLogEvent(event: {
  sessionId: string;
  event: string;
  fields: Record<string, unknown>;
  level?: string;
  message?: string | null;
  liveSessionId?: string | null;
}): NormalizedSessionLogEvent {
  return {
    sessionId: event.sessionId,
    liveSessionId: event.liveSessionId ?? null,
    source: "workers",
    level: event.level ?? "info",
    event: event.event,
    message: event.message ?? null,
    fields: sanitizeFields(event.fields),
    tsMs: Date.now(),
  };
}

function normalizeBatch(input: unknown): NormalizedSessionLogEvent[] {
  const raw = Array.isArray(input) ? input : readEventsEnvelope(input);
  return raw.slice(0, MAX_BATCH).map(normalizeEvent);
}

function readEventsEnvelope(input: unknown): unknown[] {
  if (isRecord(input) && Array.isArray(input.events)) return input.events;
  throw new Error("session log payload must be { events: [...] }");
}

function normalizeEvent(input: unknown): NormalizedSessionLogEvent {
  if (!isRecord(input)) throw new Error("session log event must be object");
  const sessionId = readString(input.session_id, "session_id");
  const source = readEnum(input.source, SOURCES, "source");
  const level = readEnum(input.level, LEVELS, "level");
  const event = readString(input.event, "event").slice(0, 120);
  const liveSessionId = readNullableString(input.live_session_id);
  const message = readNullableString(input.message)?.slice(0, 500) ?? null;
  const fields = sanitizeFields(input.fields);
  const tsMs =
    typeof input.ts_ms === "number" && Number.isFinite(input.ts_ms)
      ? input.ts_ms
      : Date.now();
  return {
    sessionId,
    liveSessionId,
    source,
    level,
    event,
    message,
    fields,
    tsMs,
  };
}

function sanitizeFields(input: unknown): Record<string, unknown> {
  if (!isRecord(input)) return {};
  const fields = stripSecrets(input);
  let text = JSON.stringify(fields);
  if (text.length > MAX_FIELD_BYTES) {
    text = text.slice(0, MAX_FIELD_BYTES);
    return { truncated_json: text, truncated: true };
  }
  return fields;
}

function stripSecrets(input: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(input)) {
    if (/token|secret|key|authorization|password|rtmp/i.test(key)) {
      out[key] = "[redacted]";
    } else if (isRecord(value)) {
      out[key] = stripSecrets(value);
    } else {
      out[key] = value;
    }
  }
  return out;
}

export function writeSessionLogConsole(event: NormalizedSessionLogEvent): void {
  const line = JSON.stringify({
    message: event.event,
    source: event.source,
    level: event.level,
    session_id: event.sessionId,
    live_session_id: event.liveSessionId,
    fields: event.fields,
  });
  if (event.level === "error") console.error(line);
  else if (event.level === "warn") console.warn(line);
  else console.log(line);
}

function readString(value: unknown, field: string): string {
  if (typeof value !== "string" || value.trim() === "") {
    throw new Error(`${field} must be non-empty string`);
  }
  return value.trim();
}

function readNullableString(value: unknown): string | null {
  return typeof value === "string" && value.trim() !== "" ? value.trim() : null;
}

function readEnum(value: unknown, allowed: Set<string>, field: string): string {
  const text = readString(value, field).toLowerCase();
  if (!allowed.has(text)) throw new Error(`${field} invalid`);
  return text;
}

function isRecord(input: unknown): input is Record<string, unknown> {
  return typeof input === "object" && input !== null && !Array.isArray(input);
}
