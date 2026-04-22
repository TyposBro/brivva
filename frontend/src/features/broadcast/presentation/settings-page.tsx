import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { AlertTriangle, ArrowLeft, Loader2, Trash2 } from "lucide-react";
import * as api from "../data/api-client";
import { ApiError } from "../data/api-client";
import { SignInGate } from "../../../shared/auth/sign-in-gate";
import { signOut } from "../../../shared/auth/auth-store";
import { useAuth } from "../../../shared/auth/use-auth";

const ACTIVE_STATUSES = new Set(["setup", "live"]);
const CONFIRM_PHRASE = "delete";

export default function SettingsPage() {
  return (
    <SignInGate>
      <SettingsInner />
    </SignInGate>
  );
}

function SettingsInner() {
  const { userId } = useAuth();
  const navigate = useNavigate();
  const [voices, setVoices] = useState<api.Voice[]>([]);
  const [sessions, setSessions] = useState<api.Session[]>([]);
  const [user, setUser] = useState<api.UserInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [deleting, setDeleting] = useState<string | null>(null);
  const [voiceError, setVoiceError] = useState<{ id: string; message: string } | null>(null);
  const [accountError, setAccountError] = useState("");
  const [accountConfirm, setAccountConfirm] = useState("");
  const [accountDeleting, setAccountDeleting] = useState(false);

  const reload = useCallback(async () => {
    if (!userId) return;
    setLoading(true);
    setError("");
    try {
      const [voicesRes, sessionsRes, userRes] = await Promise.all([
        api.listVoices(userId),
        api.listSessions(userId),
        api.getUser(userId),
      ]);
      setVoices(voicesRes.voices);
      setSessions(sessionsRes.sessions);
      setUser(userRes);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load account");
    } finally {
      setLoading(false);
    }
  }, [userId]);

  useEffect(() => {
    void reload();
  }, [reload]);

  // A voice is "in use" when any session in setup/live references it.
  // Keep the computation client-side so we don't need a new endpoint.
  const activeVoiceIds = useMemo(() => {
    const ids = new Set<string>();
    for (const s of sessions) {
      if (s.voice_id && ACTIVE_STATUSES.has(s.status)) ids.add(s.voice_id);
    }
    return ids;
  }, [sessions]);

  const hasActiveSession = activeVoiceIds.size > 0 || sessions.some((s) => ACTIVE_STATUSES.has(s.status));

  const onDeleteVoice = async (voiceId: string) => {
    setVoiceError(null);
    setDeleting(voiceId);
    try {
      await api.deleteVoice(voiceId);
      await reload();
    } catch (err) {
      if (err instanceof ApiError && err.status === 409) {
        setVoiceError({
          id: voiceId,
          message: "This clone is attached to a live session. End the session, then retry.",
        });
      } else {
        setVoiceError({
          id: voiceId,
          message: err instanceof Error ? err.message : "Delete failed",
        });
      }
    } finally {
      setDeleting(null);
    }
  };

  const onDeleteAccount = async () => {
    if (!userId) return;
    setAccountError("");
    setAccountDeleting(true);
    try {
      await api.deleteAccount(userId);
      signOut();
      navigate("/", { replace: true });
    } catch (err) {
      if (err instanceof ApiError && err.status === 409) {
        setAccountError("End all live sessions before deleting your account.");
      } else {
        setAccountError(err instanceof Error ? err.message : "Delete failed");
      }
      setAccountDeleting(false);
    }
  };

  return (
    <div className="min-h-screen bg-background">
      <header className="fixed top-0 w-full z-50 bg-background/60 backdrop-blur-xl">
        <div className="flex justify-between items-center max-w-2xl mx-auto px-6 h-16">
          <button
            onClick={() => navigate("/dashboard")}
            className="flex items-center gap-2 text-on-surface-variant hover:text-on-surface transition-colors font-label text-sm"
            data-testid="settings-back"
          >
            <ArrowLeft className="w-4 h-4" />
            Dashboard
          </button>
          <h1 className="text-sm font-label font-bold uppercase tracking-widest text-on-surface-variant">
            Account
          </h1>
        </div>
      </header>

      <main className="max-w-2xl mx-auto px-6 pt-24 pb-16 space-y-6">
        {loading && (
          <div className="flex items-center gap-2 text-on-surface-variant font-label text-sm">
            <Loader2 className="w-4 h-4 animate-spin" />
            Loading…
          </div>
        )}
        {error && (
          <div className="bg-error/10 text-error px-4 py-2.5 rounded-xl font-label text-sm">
            {error}
          </div>
        )}

        {!loading && (
          <>
            <section
              className="space-y-4 bg-surface-container-low rounded-xl p-5"
              data-testid="voice-clones-section"
            >
              <div>
                <h2 className="text-xs font-label font-bold uppercase tracking-widest text-on-surface-variant">
                  Voice clones
                </h2>
                <p className="text-on-surface-variant text-xs font-label mt-1">
                  Deleting a clone also removes it from ElevenLabs.
                </p>
              </div>

              {voices.length === 0 ? (
                <p className="text-on-surface-variant font-label text-sm">
                  No voice clones yet.
                </p>
              ) : (
                <ul className="space-y-2">
                  {voices.map((v) => {
                    const inUse = activeVoiceIds.has(v.id);
                    const isActive = user?.active_voice_id === v.id;
                    const isDeleting = deleting === v.id;
                    const thisVoiceError = voiceError?.id === v.id ? voiceError.message : null;
                    return (
                      <li
                        key={v.id}
                        className="flex flex-col gap-2 bg-surface-container-high rounded-lg p-3"
                        data-testid={`voice-row-${v.id}`}
                      >
                        <div className="flex items-center justify-between gap-3">
                          <div className="min-w-0">
                            <div className="flex items-center gap-2">
                              <span className="text-on-surface font-label text-sm truncate">
                                {v.name}
                              </span>
                              {isActive && (
                                <span className="text-[10px] font-label font-bold uppercase tracking-widest text-success bg-success/10 px-2 py-0.5 rounded">
                                  Active
                                </span>
                              )}
                              {inUse && (
                                <span className="text-[10px] font-label font-bold uppercase tracking-widest text-warning bg-warning/10 px-2 py-0.5 rounded">
                                  In session
                                </span>
                              )}
                            </div>
                            <div className="text-on-surface-variant text-xs font-label mt-0.5">
                              {v.source_lang ?? "unknown lang"}
                            </div>
                          </div>
                          <button
                            onClick={() => void onDeleteVoice(v.id)}
                            disabled={inUse || isDeleting}
                            className="inline-flex items-center gap-1.5 bg-error/10 hover:bg-error/20 disabled:opacity-40 disabled:cursor-not-allowed text-error px-3 py-1.5 rounded-lg font-label text-xs transition-colors"
                            data-testid={`voice-delete-${v.id}`}
                          >
                            {isDeleting ? (
                              <Loader2 className="w-3.5 h-3.5 animate-spin" />
                            ) : (
                              <Trash2 className="w-3.5 h-3.5" />
                            )}
                            Delete
                          </button>
                        </div>
                        {thisVoiceError && (
                          <div className="text-error text-xs font-label">
                            {thisVoiceError}
                          </div>
                        )}
                      </li>
                    );
                  })}
                </ul>
              )}
            </section>

            <section
              className="space-y-4 bg-error/5 border border-error/20 rounded-xl p-5"
              data-testid="danger-zone"
            >
              <div className="flex items-start gap-3">
                <AlertTriangle className="w-5 h-5 text-error mt-0.5" />
                <div>
                  <h2 className="text-xs font-label font-bold uppercase tracking-widest text-error">
                    Danger zone
                  </h2>
                  <p className="text-on-surface-variant text-xs font-label mt-1">
                    Hard-deletes your account, voice clones (ElevenLabs + local),
                    sessions, credentials. This cannot be undone.
                  </p>
                </div>
              </div>

              {hasActiveSession && (
                <div className="bg-warning/10 text-warning px-3 py-2 rounded-lg font-label text-xs">
                  You have an active session. End it before deleting your account.
                </div>
              )}

              <label className="block">
                <span className="text-on-surface-variant text-xs font-label">
                  Type <span className="font-mono text-error">{CONFIRM_PHRASE}</span> to confirm:
                </span>
                <input
                  type="text"
                  value={accountConfirm}
                  onChange={(e) => setAccountConfirm(e.target.value)}
                  disabled={accountDeleting}
                  className="mt-1 w-full bg-surface-container-highest border-none rounded-lg px-3 py-2 text-on-surface font-mono text-sm focus:ring-2 focus:ring-error/50 outline-none"
                  data-testid="delete-account-confirm-input"
                  autoComplete="off"
                />
              </label>

              {accountError && (
                <div className="text-error text-xs font-label">{accountError}</div>
              )}

              <button
                onClick={() => void onDeleteAccount()}
                disabled={
                  accountConfirm !== CONFIRM_PHRASE ||
                  accountDeleting ||
                  hasActiveSession
                }
                className="inline-flex items-center gap-2 bg-error hover:bg-error/90 disabled:opacity-40 disabled:cursor-not-allowed text-on-error px-4 py-2 rounded-lg font-label text-sm transition-colors"
                data-testid="delete-account-submit"
              >
                {accountDeleting ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : (
                  <Trash2 className="w-4 h-4" />
                )}
                Delete account
              </button>
            </section>
          </>
        )}
      </main>
    </div>
  );
}
