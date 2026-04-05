import { Youtube } from "lucide-react";
import { youtubeAuthUrl } from "../../../../shared/api";
import type { UserInfo } from "../../../../shared/api";

type Props = {
  user: UserInfo | null;
  userId: string;
};

export function YouTubeSection({ user, userId }: Props) {
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
