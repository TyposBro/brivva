import { type ComponentType, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { Building2, Clock3, Radio, ShieldCheck, Upload, WandSparkles } from "lucide-react";
import {
  FrontendOAuthLandingQuerySchema,
  FrontendOAuthTokenFragmentSchema,
} from "@brivva/contracts/oauth";
import { signIn, isSignedIn } from "../../../shared/auth/auth-store";

type IconComponent = ComponentType<{ className?: string }>;

function VoiceTierCard({
  icon: Icon,
  eyebrow,
  title,
  body,
}: {
  icon: IconComponent;
  eyebrow: string;
  title: string;
  body: string;
}) {
  return (
    <article className="bg-surface-container-low/80 border border-outline-variant/30 rounded-2xl p-5 backdrop-blur text-left">
      <div className="flex items-center justify-between gap-3 mb-5">
        <Icon className="w-6 h-6 text-primary" />
        <span className="text-primary text-xs uppercase tracking-widest font-label font-bold">
          {eyebrow}
        </span>
      </div>
      <h2 className="font-headline font-bold text-lg text-on-surface mb-2">
        {title}
      </h2>
      <p className="text-on-surface-variant text-sm leading-relaxed font-label">
        {body}
      </p>
    </article>
  );
}

function TrustPoint({ icon: Icon, label }: { icon: IconComponent; label: string }) {
  return (
    <div className="flex items-center gap-3 bg-surface-container/60 rounded-xl px-4 py-3 text-on-surface-variant font-label text-sm">
      <Icon className="w-4 h-4 text-primary shrink-0" />
      <span>{label}</span>
    </div>
  );
}

export default function HomePage() {
  const navigate = useNavigate();

  // OAuth callback lands at `/?user_id=X#token=Y`. Extract the JWT from the
  // fragment (fragments never hit the server → safe to carry a bearer token),
  // hand it to the in-memory auth store, and forward the visitor to /dashboard.
  useEffect(() => {
    const query = FrontendOAuthLandingQuerySchema.parse(
      Object.fromEntries(new URLSearchParams(window.location.search).entries()),
    );
    const fragment = FrontendOAuthTokenFragmentSchema.parse(
      Object.fromEntries(new URLSearchParams(window.location.hash.replace(/^#/, "")).entries()),
    );
    const userId = query.user_id;
    const token = fragment.token;
    if (userId) {
      signIn(userId, token);
    }
    if (token) {
      window.history.replaceState(null, "", "/");
      navigate("/dashboard", { replace: true });
    } else if (userId && isSignedIn()) {
      navigate("/dashboard", { replace: true });
    }
  }, [navigate]);

  return (
    <div className="min-h-screen bg-background relative overflow-hidden">
      {/* Background glow */}
      <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[800px] h-[800px] bg-primary-container/10 rounded-full blur-[120px]" />
      <div className="absolute top-1/4 right-0 w-[400px] h-[400px] bg-secondary-container/5 rounded-full blur-[100px]" />

      {/* Nav */}
      <nav className="fixed top-0 w-full z-50 bg-background/60 backdrop-blur-xl">
        <div className="flex justify-between items-center max-w-7xl mx-auto px-8 h-20">
          <div className="text-2xl font-black tracking-tighter text-on-surface font-headline">
            BRIVVA
          </div>
        </div>
      </nav>

      {/* Hero */}
      <main className="relative z-10 flex flex-col items-center justify-center min-h-screen px-6">
        <div className="max-w-4xl w-full text-center">
          <h1 className="font-headline font-extrabold text-6xl md:text-8xl tracking-tighter mb-8 leading-[0.9] text-on-surface">
            BREAK THE{" "}
            <span className="text-primary">LANGUAGE</span>{" "}
            BARRIER IN REAL-TIME.
          </h1>
          <p className="max-w-2xl mx-auto text-on-surface-variant text-lg md:text-xl mb-10 leading-relaxed">
            Real-time multilingual live commerce broadcasting. One host speaks;
            every platform receives translated audio with natural voice options,
            from instant clones to verified enterprise voice models.
          </p>

          <div className="grid md:grid-cols-3 gap-4 mb-14 text-left">
            <VoiceTierCard
              icon={WandSparkles}
              title="Instant clone"
              eyebrow="~2 minutes"
              body="Go live fast with Brivva's instant Yuna and Gitae voice presets, available across every supported language."
            />
            <VoiceTierCard
              icon={Upload}
              title="Host voice upload"
              eyebrow="Up to 10 minutes"
              body="Upload or record a longer host sample when you want translated audio to keep more of the speaker's own timbre."
            />
            <VoiceTierCard
              icon={Building2}
              title="Enterprise B2B voice"
              eyebrow="3+ hours + on-site verification"
              body="For brands, celebrities, and agencies that need higher-fidelity cloning with consent checks and guided capture."
            />
          </div>

          <div className="mb-14 grid sm:grid-cols-3 gap-3 text-left">
            <TrustPoint icon={Clock3} label="Live setup in minutes" />
            <TrustPoint icon={ShieldCheck} label="Consent-first voice workflows" />
            <TrustPoint icon={Radio} label="RTMP output for live platforms" />
          </div>

          {/* Sole CTA — Brivva is host-only; viewer-side flows live on the
              broadcast platforms themselves (vision.md "What's Intentionally
              Not In The Product"). */}
          <div className="flex justify-center">
            <button
              onClick={() => navigate("/dashboard")}
              className="monolith-gradient group flex flex-col items-center justify-center p-8 rounded-xl hover:scale-[0.98] transition-all duration-300 shadow-xl w-full max-w-xs"
            >
              <Radio className="w-10 h-10 mb-4 text-white" />
              <span className="font-headline font-bold text-lg text-white">
                Stream Dashboard
              </span>
              <span className="text-white/60 text-xs mt-2 uppercase tracking-widest font-label">
                Go Live Now
              </span>
            </button>
          </div>
        </div>
      </main>

      {/* Footer */}
      <footer className="relative z-10 pb-8 text-center">
        <div className="flex items-center justify-center gap-6 text-sm text-on-surface-variant/60">
          <button onClick={() => navigate("/privacy")} className="hover:text-on-surface-variant transition-colors">
            Privacy
          </button>
          <span className="text-on-surface-variant/20">|</span>
          <button onClick={() => navigate("/terms")} className="hover:text-on-surface-variant transition-colors">
            Terms
          </button>
        </div>
      </footer>
    </div>
  );
}
