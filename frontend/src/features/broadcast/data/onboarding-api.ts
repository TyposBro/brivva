// Direct-fetch wrappers for the onboarding endpoints.
//
// TODO(workers-agent): once Round 2 lands `users.onboarding_completed_at` and
// `POST /api/user/complete-onboarding` in the OpenAPI contract, regenerate
// frontend types (`bun run --cwd frontend openapi:generate`) and switch to
// the typed openapi-fetch client so this hand-written wrapper can go away.

import { z } from "zod";
import { appConfig } from "../../../core/config/app-config";

const OnboardingStateSchema = z.object({
  onboarding_completed_at: z.number().nullable().optional(),
});

export interface OnboardingState {
  onboardingCompletedAt: number | null;
}

export async function fetchOnboardingState(userId: string): Promise<OnboardingState> {
  const url = `${appConfig().workersApiBase}/api/user?user_id=${encodeURIComponent(userId)}`;
  const res = await fetch(url, { credentials: "omit" });
  if (!res.ok) throw new Error(`fetchOnboardingState: ${res.status}`);
  const json = OnboardingStateSchema.parse(await res.json());
  return { onboardingCompletedAt: json.onboarding_completed_at ?? null };
}

export interface CompleteOnboardingRequest {
  userId: string;
  defaultTargetLang: string;
}

export async function completeOnboarding(req: CompleteOnboardingRequest): Promise<void> {
  const url = `${appConfig().workersApiBase}/api/user/complete-onboarding`;
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "omit",
    body: JSON.stringify({
      user_id: req.userId,
      default_target_lang: req.defaultTargetLang,
    }),
  });
  if (!res.ok) throw new Error(`completeOnboarding: ${res.status}`);
}
