import { X, Youtube, Mic, Trash2 } from "lucide-react";
import { cn } from "../../../lib/cn";
import { youtubeAuthUrl } from "../../../shared/api";
import type { UserInfo, Voice } from "../../../shared/api";

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

function YouTubeSection({ user, userId }: { user: UserInfo | null; userId: string }) {
  return (
    <div>
      <span className="text-on-surface-variant text-xs font-label block mb-2">
        YouTube Account
      </span>
      {user?.youtube_connected ? (
        <div className="flex items-center gap-2">
          <Youtube className="w-4 h-4 text-[#ff0000]" />
          <span className="text-on-surface font-label text-sm">
            {user.youtube_channel_name}
          </span>
          <span className="text-[10px] font-label font-bold uppercase tracking-widest text-success bg-success/10 px-2 py-0.5 rounded">
            Connected
          </span>
        </div>
      ) : (
        <a
          href={youtubeAuthUrl(userId)}
          className="inline-flex items-center gap-2 bg-surface-container-high hover:bg-surface-bright text-on-surface px-4 py-2 rounded-lg font-label text-sm transition-colors"
        >
          <Youtube className="w-3.5 h-3.5" />
          Connect YouTube
        </a>
      )}
    </div>
  );
}

function VoiceSection({
  voices,
  selectedVoice,
  onSelectVoice,
  onDeleteVoice,
}: {
  voices: Voice[];
  selectedVoice: string;
  onSelectVoice: (id: string) => void;
  onDeleteVoice: (id: string) => void;
}) {
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
