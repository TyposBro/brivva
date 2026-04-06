type Props = {
  onDismiss: () => void;
};

export function DriftWarningBanner({ onDismiss }: Props) {
  return (
    <div className="flex items-center justify-between bg-warning-container text-on-warning-container rounded-lg px-4 py-2 text-sm">
      <span>
        High latency detected — consider switching to subtitles only
      </span>
      <button
        onClick={onDismiss}
        className="ml-4 text-on-warning-container/60 hover:text-on-warning-container text-lg leading-none"
      >
        x
      </button>
    </div>
  );
}
