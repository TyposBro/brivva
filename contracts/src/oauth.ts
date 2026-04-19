import { z } from "zod";

export const YoutubeAuthQuerySchema = z.object({
  user_id: z.string().min(1),
});

export const YoutubeCallbackQuerySchema = z.object({
  code: z.string().min(1).optional(),
  state: z.string().min(1).optional(),
  error: z.string().min(1).optional(),
});

// Google sign-in flow (scope: openid email profile). The state is an opaque
// CSRF token the Worker mints on /auth/google and verifies on the callback.
// User identity is the Google `sub` (never user-supplied), so no user_id in
// the authorize query.
export const GoogleSigninCallbackQuerySchema = z.object({
  code: z.string().min(1).optional(),
  state: z.string().min(1).optional(),
  error: z.string().min(1).optional(),
});

export const FrontendOAuthLandingQuerySchema = z.object({
  user_id: z.string().min(1).optional(),
  oauth_error: z.string().min(1).optional(),
});

export const FrontendOAuthTokenFragmentSchema = z.object({
  token: z.string().min(1).optional(),
});

export type YoutubeAuthQuery = z.infer<typeof YoutubeAuthQuerySchema>;
export type YoutubeCallbackQuery = z.infer<typeof YoutubeCallbackQuerySchema>;
export type GoogleSigninCallbackQuery = z.infer<typeof GoogleSigninCallbackQuerySchema>;
export type FrontendOAuthLandingQuery = z.infer<typeof FrontendOAuthLandingQuerySchema>;
export type FrontendOAuthTokenFragment = z.infer<typeof FrontendOAuthTokenFragmentSchema>;
