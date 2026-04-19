// Pre-stream quote + post-stream summary fetchers.
//
// TODO(workers-agent): Round 2 will add `/api/sessions/:id/quote` and
// `/api/sessions/:id/summary` to the OpenAPI contract. Once shipped,
// regenerate frontend types (`bun run --cwd frontend openapi:generate`)
// and replace these direct fetches with the typed openapi-fetch client.

import { z } from "zod";
import { appConfig } from "../../../core/config/app-config";

const QuoteBreakdownItemSchema = z.object({
  lang: z.string(),
  minutes: z.number(),
  cost_usd: z.number(),
});

const QuoteResponseSchema = z.object({
  estimated_cost_usd: z.number(),
  expected_minutes: z.number().optional(),
  breakdown: z.array(QuoteBreakdownItemSchema).optional(),
});

export interface QuoteBreakdownItem {
  lang: string;
  minutes: number;
  costUsd: number;
}

export interface QuoteResponse {
  estimatedCostUsd: number;
  expectedMinutes: number | null;
  breakdown: QuoteBreakdownItem[];
}

export async function fetchSessionQuote(
  sessionId: string,
  expectedMinutes: number,
): Promise<QuoteResponse> {
  const url = `${appConfig().workersApiBase}/api/sessions/${encodeURIComponent(sessionId)}/quote?expected_minutes=${expectedMinutes}`;
  const res = await fetch(url, { credentials: "omit" });
  if (!res.ok) throw new Error(`fetchSessionQuote: ${res.status}`);
  const parsed = QuoteResponseSchema.parse(await res.json());
  return {
    estimatedCostUsd: parsed.estimated_cost_usd,
    expectedMinutes: parsed.expected_minutes ?? expectedMinutes,
    breakdown:
      parsed.breakdown?.map((b) => ({
        lang: b.lang,
        minutes: b.minutes,
        costUsd: b.cost_usd,
      })) ?? [],
  };
}

const SummaryBreakdownItemSchema = z.object({
  lang: z.string(),
  minutes: z.number(),
  cost_usd: z.number(),
});

const SummaryResponseSchema = z.object({
  total_minutes: z.number(),
  total_cost_usd: z.number(),
  breakdown: z.array(SummaryBreakdownItemSchema).optional(),
});

export interface SummaryResponse {
  totalMinutes: number;
  totalCostUsd: number;
  breakdown: QuoteBreakdownItem[];
}

export async function fetchSessionSummary(sessionId: string): Promise<SummaryResponse> {
  const url = `${appConfig().workersApiBase}/api/sessions/${encodeURIComponent(sessionId)}/summary`;
  const res = await fetch(url, { credentials: "omit" });
  if (!res.ok) throw new Error(`fetchSessionSummary: ${res.status}`);
  const parsed = SummaryResponseSchema.parse(await res.json());
  return {
    totalMinutes: parsed.total_minutes,
    totalCostUsd: parsed.total_cost_usd,
    breakdown:
      parsed.breakdown?.map((b) => ({
        lang: b.lang,
        minutes: b.minutes,
        costUsd: b.cost_usd,
      })) ?? [],
  };
}
