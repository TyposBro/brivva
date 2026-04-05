import type { TranslationTier } from "../constants";

type TierOption = { tier: TranslationTier; label: string; desc: string; cost: string; ready: boolean };

const TIERS: TierOption[] = [
  { tier: 1, label: "Subtitles Only", desc: "No voice translation", cost: "Free", ready: true },
  { tier: 2, label: "Voice + Subtitles", desc: "AI-translated voice", cost: "$5/hr", ready: true },
  { tier: 3, label: "Voice + Lipsync (Live)", desc: "Real-time lipsync", cost: "$40/hr", ready: false },
  { tier: 4, label: "Voice + Lipsync (Post)", desc: "Post-processed lipsync", cost: "$30-40/hr", ready: false },
];

type Props = { tier: TranslationTier; isLive: boolean; onTierChange: (t: TranslationTier) => void };

export function TierSelector({ tier, isLive, onTierChange }: Props) {
  return (
    <div className="bg-surface-container-low rounded-xl p-4 space-y-3">
      <h2 className="font-headline text-sm font-semibold text-on-surface-variant uppercase tracking-wider">
        Translation Mode
      </h2>
      <div className="grid grid-cols-2 gap-2">
        {TIERS.map((opt) => (
          <button
            key={opt.tier}
            disabled={!opt.ready || isLive}
            onClick={() => onTierChange(opt.tier)}
            className={`text-left p-3 rounded-lg border transition-colors ${
              tier === opt.tier
                ? "border-primary bg-surface-container-high"
                : "border-outline-variant bg-surface-container"
            } ${!opt.ready ? "opacity-40 cursor-not-allowed" : "hover:border-primary/60"}`}
          >
            <div className="flex items-center justify-between">
              <span className="text-sm font-semibold">Option {opt.tier}</span>
              <span className="text-xs text-outline">{opt.cost}</span>
            </div>
            <div className="text-sm text-on-surface mt-0.5">{opt.label}</div>
            <div className="text-xs text-outline mt-0.5">{opt.desc}</div>
            {!opt.ready && <div className="text-xs text-secondary mt-1">Coming Soon</div>}
          </button>
        ))}
      </div>
    </div>
  );
}
