import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const navigate = vi.fn();
vi.mock("react-router-dom", async (orig) => {
  const actual = await orig<typeof import("react-router-dom")>();
  return {
    ...actual,
    useNavigate: () => navigate,
    useParams: () => ({ id: "s1" }),
  };
});

const getSession = vi.fn();
const deleteSession = vi.fn();
vi.mock("../data/api-client", async () => {
  const actual = await vi.importActual<typeof import("../data/api-client")>(
    "../data/api-client",
  );
  return {
    ...actual,
    getSession: (...args: unknown[]) => getSession(...args),
    deleteSession: (...args: unknown[]) => deleteSession(...args),
  };
});

const fetchSessionSummary = vi.fn();
vi.mock("../data/quote-api", () => ({
  fetchSessionSummary: (...args: unknown[]) => fetchSessionSummary(...args),
}));

import SessionPage from "./session-page";
import { MemoryRouter } from "react-router-dom";

function makeSession(overrides: Partial<Record<string, unknown>> = {}) {
  return {
    id: "s1",
    user_id: "u1",
    voice_id: "v1",
    title: "Test session",
    source_lang: "ko",
    target_langs: '["zh","ja"]',
    status: "live",
    live_session_id: "live-1",
    created_at: Math.floor(Date.now() / 1000) - 600,
    ...overrides,
  };
}

const sampleStreams = [
  {
    id: "st1",
    session_id: "s1",
    lang: "zh",
    platform: "grip",
    rtmp_url: "rtmps://grip/x",
    stream_key: "k",
    status: "ready",
    delay_ms: 3000,
    host_gain: 0.2,
    created_at: 1,
    broadcast_id: undefined,
    stream_id: undefined,
  },
];

describe("SessionPage", () => {
  beforeEach(() => {
    navigate.mockReset();
    getSession.mockReset();
    deleteSession.mockReset();
    fetchSessionSummary.mockReset();
  });

  it("renders observability panel with computed live minutes + cost (happy)", async () => {
    getSession.mockResolvedValue({ session: makeSession(), streams: sampleStreams });
    render(
      <MemoryRouter initialEntries={["/session/s1"]}>
        <SessionPage />
      </MemoryRouter>,
    );
    expect(await screen.findByText("Test session")).toBeInTheDocument();
    // Observability stat — live minutes ≥ 9 (created 600s ago / 60).
    expect(screen.getByText(/Live minutes/i)).toBeInTheDocument();
    expect(screen.getByText(/Estimated cost/i)).toBeInTheDocument();
  });

  it("Resume routes to /live when voice exists, /setup otherwise", async () => {
    getSession.mockResolvedValueOnce({ session: makeSession(), streams: [] });
    const { unmount } = render(
      <MemoryRouter initialEntries={["/session/s1"]}>
        <SessionPage />
      </MemoryRouter>,
    );
    const resume = await screen.findByRole("button", { name: /Resume Broadcast/i });
    await userEvent.click(resume);
    expect(navigate).toHaveBeenCalledWith("/session/s1/live");
    unmount();

    navigate.mockReset();
    getSession.mockResolvedValueOnce({ session: makeSession({ voice_id: null }), streams: [] });
    render(
      <MemoryRouter initialEntries={["/session/s1"]}>
        <SessionPage />
      </MemoryRouter>,
    );
    const resume2 = await screen.findByRole("button", { name: /Resume Broadcast/i });
    await userEvent.click(resume2);
    expect(navigate).toHaveBeenCalledWith("/session/s1/setup");
  });

  it("End Session triggers summary modal (happy)", async () => {
    getSession.mockResolvedValue({ session: makeSession(), streams: [] });
    deleteSession.mockResolvedValue({ status: "ok" });
    fetchSessionSummary.mockResolvedValue({
      totalMinutes: 12,
      totalCostUsd: 18,
      breakdown: [],
    });
    render(
      <MemoryRouter initialEntries={["/session/s1"]}>
        <SessionPage />
      </MemoryRouter>,
    );
    await screen.findByText("Test session");
    await userEvent.click(screen.getByRole("button", { name: /End Session/i }));
    await waitFor(() => expect(deleteSession).toHaveBeenCalledWith("s1"));
    await waitFor(() => expect(fetchSessionSummary).toHaveBeenCalledWith("s1"));
    expect(await screen.findByText("12m")).toBeInTheDocument();
  });

  it("session-not-found renders fallback (sad)", async () => {
    getSession.mockResolvedValue({ session: null, streams: [] });
    render(
      <MemoryRouter initialEntries={["/session/missing"]}>
        <SessionPage />
      </MemoryRouter>,
    );
    expect(await screen.findByText(/Session not found/i)).toBeInTheDocument();
  });

  // Prod incident 2026-04-20 — End-Session used to flash "Session not found"
  // because Workers hard-deleted the row and the post-End refresh GET returned
  // session:null. Soft-end + an optimistic status flip on End should mean the
  // fallback fires only on a truly missing session, never on the End path.
  it("End Session never shows 'Session not found' even if the post-End GET returns null (regression)", async () => {
    getSession.mockResolvedValueOnce({ session: makeSession(), streams: [] });
    deleteSession.mockResolvedValue({ status: "ended" });
    fetchSessionSummary.mockResolvedValue({
      totalMinutes: 5,
      totalCostUsd: 7.5,
      breakdown: [],
    });
    // Simulate the prod failure mode — the refresh GET right after delete
    // happens to return session:null (D1 hiccup, race, anything). The
    // optimistic state flip in handleEnd guards us from regressing.
    getSession.mockResolvedValueOnce({ session: null, streams: [] });

    render(
      <MemoryRouter initialEntries={["/session/s1"]}>
        <SessionPage />
      </MemoryRouter>,
    );
    await screen.findByText("Test session");
    await userEvent.click(screen.getByRole("button", { name: /End Session/i }));

    await waitFor(() => expect(deleteSession).toHaveBeenCalledWith("s1"));
    expect(await screen.findByText(/Session ended/i)).toBeInTheDocument();
    expect(screen.queryByText(/Session not found/i)).toBeNull();
  });
});
