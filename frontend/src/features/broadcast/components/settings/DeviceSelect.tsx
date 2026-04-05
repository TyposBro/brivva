const DEVICE_ID_PREVIEW_LEN = 8;

type Props = {
  label: string;
  kind: string;
  devices: MediaDeviceInfo[];
  value: string;
  disabled: boolean;
  onChange: (v: string) => void;
};

export function DeviceSelect({ label, kind, devices, value, disabled, onChange }: Props) {
  const filtered = devices.filter((d) => d.kind === kind);
  const fallback = kind === "videoinput" ? "Camera" : "Mic";

  return (
    <div className="space-y-1">
      <label className="text-sm text-on-surface-variant">{label}</label>
      <select
        disabled={disabled}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="w-full px-3 py-2 rounded-lg bg-surface-container border border-outline-variant text-sm text-on-surface disabled:opacity-50"
      >
        <option value="">Default</option>
        {filtered.map((d) => (
          <option key={d.deviceId} value={d.deviceId}>
            {d.label || `${fallback} ${d.deviceId.slice(0, DEVICE_ID_PREVIEW_LEN)}`}
          </option>
        ))}
      </select>
    </div>
  );
}
