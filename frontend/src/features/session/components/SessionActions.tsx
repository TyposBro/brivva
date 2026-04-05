import { ArrowLeft, Radio, Square, Copy } from "lucide-react";
import type { Session } from "../../../shared/api";

type Props = {
  session: Session;
  isLive: boolean;
  ending: boolean;
  onEnd: () => void;
  onNavigate: (path: string) => void;
};

export function SessionActions({ session, isLive, ending, onEnd, onNavigate }: Props) {
  return (
    <section className="flex flex-col sm:flex-row gap-3">
      {isLive && !session.room_id && (
        <StartBroadcastButton session={session} onNavigate={onNavigate} />
      )}

      {isLive && session.room_id && (
        <RoomCodeDisplay roomId={session.room_id} />
      )}

      {isLive && (
        <button
          className="bg-error-container text-on-error-container px-6 py-3 rounded-xl font-headline font-bold hover:opacity-90 transition-opacity flex items-center justify-center gap-2"
          onClick={onEnd}
          disabled={ending}
        >
          <Square className="w-4 h-4" />
          {ending ? "Ending..." : "End Session"}
        </button>
      )}

      <button
        className="bg-surface-container-high hover:bg-surface-bright text-on-surface px-6 py-3 rounded-xl font-headline font-bold transition-colors flex items-center justify-center gap-2"
        onClick={() => onNavigate("/dashboard")}
      >
        <ArrowLeft className="w-4 h-4" />
        Back to Dashboard
      </button>
    </section>
  );
}

function StartBroadcastButton({
  session,
  onNavigate,
}: {
  session: Session;
  onNavigate: (path: string) => void;
}) {
  return (
    <button
      className="monolith-gradient text-white px-8 py-3 rounded-xl font-headline font-extrabold hover:scale-[0.98] transition-all shadow-xl flex items-center justify-center gap-2"
      onClick={() =>
        onNavigate(`/host?sessionId=${session.id}&sourceLang=${session.source_lang}`)
      }
    >
      <Radio className="w-5 h-5" />
      Start Broadcasting
    </button>
  );
}

function RoomCodeDisplay({ roomId }: { roomId: string }) {
  return (
    <div className="flex items-center gap-3 bg-surface-container-low px-5 py-3 rounded-xl">
      <span className="text-on-surface-variant text-sm font-label">
        Room Code:
      </span>
      <code className="text-primary font-mono font-bold text-lg">{roomId}</code>
      <button
        className="text-on-surface-variant hover:text-primary transition-colors"
        onClick={() => navigator.clipboard.writeText(roomId)}
      >
        <Copy className="w-4 h-4" />
      </button>
    </div>
  );
}
