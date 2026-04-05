import { X } from "lucide-react";
import type { UserInfo, Voice } from "../../../shared/api";

import { YouTubeSection } from "./settings/YouTubeSection";
import { VoiceSection } from "./settings/VoiceSection";

type Props = {
  user: UserInfo | null;
  voices: Voice[];
  selectedVoice: string;
  userId: string;
  onSelectVoice: (id: string) => void;
  onDeleteVoice: (id: string) => void;
  onClose: () => void;
};

export function SettingsDrawer({
  user,
  voices,
  selectedVoice,
  userId,
  onSelectVoice,
  onDeleteVoice,
  onClose,
}: Props) {
  return (
    <div className="space-y-4 bg-surface-container-low rounded-xl p-5">
      <div className="flex items-center justify-between">
        <span className="text-xs font-label font-bold uppercase tracking-widest text-on-surface-variant">
          Settings
        </span>
        <button
          className="text-on-surface-variant hover:text-on-surface p-1"
          onClick={onClose}
        >
          <X className="w-3.5 h-3.5" />
        </button>
      </div>

      <YouTubeSection user={user} userId={userId} />
      <VoiceSection
        voices={voices}
        selectedVoice={selectedVoice}
        onSelectVoice={onSelectVoice}
        onDeleteVoice={onDeleteVoice}
      />
    </div>
  );
}
