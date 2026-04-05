export function PasteBar({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <div className="p-3 border-b border-outline-variant/10">
      <input
        className="w-full bg-surface-container-highest border-none rounded-lg px-3 py-2 text-on-surface placeholder:text-on-surface-variant/40 focus:ring-2 focus:ring-primary/50 transition-all font-label text-sm outline-none"
        placeholder="Paste RTMP URL to auto-detect..."
        value={value}
        onChange={(e) => onChange(e.target.value)}
        autoFocus
      />
    </div>
  );
}
