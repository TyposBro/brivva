import { appConfig } from "../../core/config/app-config";

type Level = "debug" | "info" | "warn" | "error";

type SessionLogEvent = {
  session_id: string;
  live_session_id?: string | null;
  source: "frontend";
  level: Level;
  event: string;
  message?: string | null;
  fields?: Record<string, unknown>;
  ts_ms: number;
};

type SessionLogContext = {
  sessionId: string;
  userId: string;
  liveSessionId?: string | null;
};

const FLUSH_SIZE = 20;
const FLUSH_INTERVAL_MS = 5000;
const SECRET_KEY = /token|secret|key|authorization|password|rtmp/i;

let context: SessionLogContext | null = null;
let queue: SessionLogEvent[] = [];
let timer: number | null = null;
const flushOnPageHide = () => void flushSessionLogs();

export function configureSessionLogger(next: SessionLogContext | null): void {
  if (!next) {
    void flushSessionLogs();
    context = null;
    stopTimer();
    return;
  }
  context = next;
  startTimer();
}

export function sessionLog(
  level: Level,
  event: string,
  fields: Record<string, unknown> = {},
  message?: string,
): void {
  if (!isEnabled() || !context) return;
  const entry: SessionLogEvent = {
    session_id: context.sessionId,
    live_session_id: context.liveSessionId ?? null,
    source: "frontend",
    level,
    event,
    message: message ?? null,
    fields: stripSecrets(fields),
    ts_ms: Date.now(),
  };
  queue.push(entry);
  writeConsole(entry);
  if (queue.length >= FLUSH_SIZE) void flushSessionLogs();
}

export async function flushSessionLogs(): Promise<void> {
  if (!context || queue.length === 0) return;
  const batch = queue;
  queue = [];
  const payload = JSON.stringify({ user_id: context.userId, events: batch });
  const url = `${appConfig().workersApiBase}/api/session-logs/client?user_id=${encodeURIComponent(context.userId)}`;
  if (navigator.sendBeacon) {
    const blob = new Blob([payload], { type: "application/json" });
    if (navigator.sendBeacon(url, blob)) return;
  }
  await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: payload,
    keepalive: true,
  }).catch(() => {
    queue = [...batch, ...queue].slice(-FLUSH_SIZE * 2);
  });
}

export function sessionLogsEnabled(): boolean {
  return isEnabled();
}

function isEnabled(): boolean {
  return (
    appConfig().sessionLogsEnabled ||
    localStorage.getItem("brivva:sessionLogs") === "1"
  );
}

function startTimer(): void {
  if (timer !== null) return;
  timer = window.setInterval(() => void flushSessionLogs(), FLUSH_INTERVAL_MS);
  window.addEventListener("pagehide", flushOnPageHide);
}

function stopTimer(): void {
  if (timer !== null) window.clearInterval(timer);
  timer = null;
  window.removeEventListener("pagehide", flushOnPageHide);
}

function writeConsole(entry: SessionLogEvent): void {
  if (!appConfig().sessionLogConsole && !import.meta.env.DEV) return;
  const line = { message: entry.event, ...entry };
  if (entry.level === "error") console.error(line);
  else if (entry.level === "warn") console.warn(line);
  else console.info(line);
}

function stripSecrets(input: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(input)) {
    if (SECRET_KEY.test(key)) out[key] = "[redacted]";
    else if (isRecord(value)) out[key] = stripSecrets(value);
    else out[key] = value;
  }
  return out;
}

function isRecord(input: unknown): input is Record<string, unknown> {
  return typeof input === "object" && input !== null && !Array.isArray(input);
}
