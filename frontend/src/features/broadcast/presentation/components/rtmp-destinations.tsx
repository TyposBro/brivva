import { LANGS } from "../../domain/broadcast-types";

type Props = {
  sourceLang: string;
  targetLangs: string[];
  rtmpUrls: Record<string, string>;
  isLive: boolean;
  onRtmpChange: (lang: string, url: string) => void;
};

export function RtmpDestinations({ sourceLang, targetLangs, rtmpUrls, isLive, onRtmpChange }: Props) {
  const srcLang = LANGS.find((l) => l.code === sourceLang);

  return (
    <div className="bg-surface-container-low rounded-xl p-4 space-y-3">
      <h2 className="font-headline text-sm font-semibold text-on-surface-variant uppercase tracking-wider">
        RTMP Destinations
      </h2>
      <p className="text-xs text-outline">
        Enter RTMP URL + stream key for each language. Leave blank to skip RTMP.
      </p>

      {srcLang && (
        <RtmpInput
          label={`${srcLang.flag} ${srcLang.label} (passthrough)`}
          value={rtmpUrls[sourceLang] || ""}
          disabled={isLive}
          onChange={(v) => onRtmpChange(sourceLang, v)}
        />
      )}

      {targetLangs.filter((l) => l !== sourceLang).map((code) => {
        const lang = LANGS.find((l) => l.code === code);
        return lang ? (
          <RtmpInput
            key={code}
            label={`${lang.flag} ${lang.label} (translated)`}
            value={rtmpUrls[code] || ""}
            disabled={isLive}
            onChange={(v) => onRtmpChange(code, v)}
          />
        ) : null;
      })}
    </div>
  );
}

function RtmpInput({ label, value, disabled, onChange }: {
  label: string;
  value: string;
  disabled: boolean;
  onChange: (v: string) => void;
}) {
  return (
    <div className="space-y-1">
      <label className="text-sm text-on-surface-variant">{label}</label>
      <input
        type="text"
        disabled={disabled}
        placeholder="rtmp://live.example.com/stream/key"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="w-full px-3 py-2 rounded-lg bg-surface-container border border-outline-variant text-sm text-on-surface placeholder:text-outline focus:border-primary focus:outline-none disabled:opacity-50"
      />
    </div>
  );
}
