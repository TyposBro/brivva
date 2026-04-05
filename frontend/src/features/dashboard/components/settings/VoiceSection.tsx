import { Mic, Trash2 } from "lucide-react";
import { cn } from "../../../../lib/cn";
import type { Voice } from "../../../../shared/api";

type Props = {
  voices: Voice[];
  selectedVoice: string;
  onSelectVoice: (id: string) => void;
  onDeleteVoice: (id: string) => void;
};

export function VoiceSection({ voices, selectedVoice, onSelectVoice, onDeleteVoice }: Props) {
  return (
    <div>
      <span className="text-on-surface-variant text-xs font-label block mb-2">
        Saved Voices
      </span>
      {voices.length === 0 ? (
        <p className="text-on-surface-variant/60 text-xs">
          No saved voices. Record one when broadcasting.
        </p>
      ) : (
        <div className="space-y-1.5">
          {voices.map((v) => (
            <div
              key={v.id}
              className={cn(
                "flex items-center gap-3 px-3 py-2 rounded-lg cursor-pointer transition-colors text-sm",
                selectedVoice === v.id
                  ? "bg-primary-container/20"
                  : "bg-surface-container-high hover:bg-surface-bright",
              )}
              onClick={() => onSelectVoice(v.id)}
            >
              <Mic className="w-3.5 h-3.5 text-primary" />
              <span className="text-on-surface font-label flex-1 truncate">
                {v.name}
              </span>
              <button
                className="text-on-surface-variant hover:text-error transition-colors p-0.5"
                onClick={(e) => {
                  e.stopPropagation();
                  onDeleteVoice(v.id);
                }}
              >
                <Trash2 className="w-3 h-3" />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
