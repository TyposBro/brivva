// In-memory JWT store with auto-refresh ahead of expiry.
//
// Per ARCHITECTURE.md §"Auth bridge", Workers issues short-lived JWTs (~15min).
// Tokens MUST NOT live in localStorage (XSS exfiltration). user_id, on the
// other hand, is a stable opaque identifier and persists across reloads so the
// SPA can re-fetch a token without forcing the user through OAuth again.

const USER_ID_KEY = "brivva_user_id";
const REFRESH_LEAD_MS = 30_000;
const FALLBACK_TOKEN_TTL_MS = 5 * 60_000;

type Listener = () => void;
type StoredToken = { value: string; expiresAt: number };
type Fetcher = (userId: string) => Promise<string>;

interface AuthState {
  userId: string | null;
  token: StoredToken | null;
}

let state: AuthState = { userId: null, token: null };
let listeners: Listener[] = [];
let refreshTimer: ReturnType<typeof setTimeout> | null = null;
let inflight: Promise<string> | null = null;
let fetcher: Fetcher | null = null;

function notify(): void {
  for (const l of listeners) l();
}

function clearRefreshTimer(): void {
  if (refreshTimer) {
    clearTimeout(refreshTimer);
    refreshTimer = null;
  }
}

function persistUserId(id: string | null): void {
  try {
    if (id) localStorage.setItem(USER_ID_KEY, id);
    else localStorage.removeItem(USER_ID_KEY);
  } catch {
    // Private mode / SSR — identity will not survive reload, but auth still works.
  }
}

function readPersistedUserId(): string | null {
  try {
    return localStorage.getItem(USER_ID_KEY);
  } catch {
    return null;
  }
}

export function decodeJwtExpiry(token: string): number {
  const parts = token.split(".");
  if (parts.length !== 3) return Date.now() + FALLBACK_TOKEN_TTL_MS;
  try {
    const payload = atob(parts[1].replace(/-/g, "+").replace(/_/g, "/"));
    const obj = JSON.parse(payload) as { exp?: number };
    if (typeof obj.exp === "number") return obj.exp * 1000;
  } catch {
    // fall through
  }
  return Date.now() + FALLBACK_TOKEN_TTL_MS;
}

function scheduleRefresh(token: StoredToken): void {
  clearRefreshTimer();
  const delay = Math.max(1_000, token.expiresAt - Date.now() - REFRESH_LEAD_MS);
  refreshTimer = setTimeout(() => {
    void ensureFreshToken().catch(() => {
      // Refresh failed — token will be re-fetched on the next ensure call.
    });
  }, delay);
}

export function configureAuth(opts: { fetchToken: Fetcher }): void {
  fetcher = opts.fetchToken;
}

export function loadPersistedUser(): void {
  const id = readPersistedUserId();
  if (id && id !== state.userId) {
    state = { userId: id, token: null };
    notify();
  }
}

export function signIn(userId: string, initialToken?: string): void {
  clearRefreshTimer();
  const token = initialToken
    ? { value: initialToken, expiresAt: decodeJwtExpiry(initialToken) }
    : null;
  state = { userId, token };
  persistUserId(userId);
  if (token) scheduleRefresh(token);
  notify();
}

export function signOut(): void {
  clearRefreshTimer();
  state = { userId: null, token: null };
  persistUserId(null);
  notify();
}

export function getUserId(): string | null {
  return state.userId;
}

export function isSignedIn(): boolean {
  return state.userId !== null;
}

export function getCachedToken(): string | null {
  return state.token?.value ?? null;
}

export async function ensureFreshToken(): Promise<string> {
  if (!state.userId) throw new Error("Sign-in required");
  if (!fetcher) throw new Error("Auth not configured");

  const cached = state.token;
  if (cached && cached.expiresAt - Date.now() > REFRESH_LEAD_MS) {
    return cached.value;
  }

  if (inflight) return inflight;
  const userId = state.userId;
  inflight = (async () => {
    try {
      const value = await fetcher!(userId);
      const fresh: StoredToken = { value, expiresAt: decodeJwtExpiry(value) };
      state = { ...state, token: fresh };
      scheduleRefresh(fresh);
      notify();
      return value;
    } finally {
      inflight = null;
    }
  })();
  return inflight;
}

export function subscribe(fn: Listener): () => void {
  listeners.push(fn);
  return () => {
    listeners = listeners.filter((l) => l !== fn);
  };
}

export function _resetForTesting(): void {
  clearRefreshTimer();
  state = { userId: null, token: null };
  inflight = null;
  fetcher = null;
  listeners = [];
}
