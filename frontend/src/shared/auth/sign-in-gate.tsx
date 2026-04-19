import type { ReactNode } from "react";
import { LogIn } from "lucide-react";
import { useAuth } from "./use-auth";
import { appConfig } from "../../core/config/app-config";

// Workers exposes /auth/google as the canonical sign-in entry point. The
// callback redirects back to the SPA with `?user_id=<sub>#token=<jwt>`,
// which the home-page handler picks up and hands to the auth-store. The
// add-channel `/auth/youtube` flow stays separate (linked from the dashboard
// once a user is signed in).
function startGoogleOAuth(): void {
  window.location.assign(`${appConfig().workersApiBase}/auth/google`);
}

export function SignInGate({ children }: { children: ReactNode }) {
  const { isSignedIn } = useAuth();
  if (isSignedIn) return <>{children}</>;

  return (
    <div className="min-h-screen bg-background flex items-center justify-center px-6">
      <div className="max-w-sm w-full bg-surface-container-low rounded-xl p-8 text-center space-y-6">
        <div>
          <h1 className="font-headline font-bold text-2xl tracking-tight text-on-surface mb-2">
            Sign in to Brivva
          </h1>
          <p className="text-on-surface-variant text-sm font-label leading-relaxed">
            We use your Google account to manage YouTube broadcasts and saved
            sessions.
          </p>
        </div>
        <button
          className="monolith-gradient w-full text-white py-3 rounded-xl font-headline font-bold hover:scale-[0.98] transition-all flex items-center justify-center gap-2"
          onClick={startGoogleOAuth}
        >
          <LogIn className="w-4 h-4" />
          Sign in with Google
        </button>
      </div>
    </div>
  );
}
