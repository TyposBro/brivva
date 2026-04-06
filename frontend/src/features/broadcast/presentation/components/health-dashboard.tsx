import { LANGS } from "../../domain/broadcast-types";
import type { PipelineHealthSnapshot, StreamStatus } from "../../domain/health-types";

type Props = {
  health: PipelineHealthSnapshot;
};

export function HealthDashboard({ health }: Props) {
  return (
    <div className="bg-surface-container-low rounded-xl p-4 space-y-3">
      <h2 className="font-headline text-sm font-semibold text-on-surface-variant uppercase tracking-wider">
        Pipeline Health
      </h2>

      <div className="grid grid-cols-2 gap-2 text-xs">
        <MetricCell label="STT" value={health.sttConnected ? "Connected" : "Disconnected"} />
        <MetricCell label="E2E Latency" value={`${health.e2eLatencyMs}ms`} />
        <MetricCell label="TTS Timeouts" value={String(health.ttsTimeouts)} />
        <MetricCell label="Translate Errors" value={String(health.translateErrors)} />
      </div>

      {health.languages.length > 0 && (
        <LanguageTable languages={health.languages} />
      )}
    </div>
  );
}

function MetricCell({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between px-2 py-1 bg-surface-container rounded">
      <span className="text-outline">{label}</span>
      <span className="font-mono text-on-surface">{value}</span>
    </div>
  );
}

function LanguageTable({ languages }: { languages: PipelineHealthSnapshot["languages"] }) {
  return (
    <table className="w-full text-xs">
      <thead>
        <tr className="text-outline border-b border-outline-variant">
          <th className="text-left py-1 font-medium">Language</th>
          <th className="text-right py-1 font-medium">Queue</th>
          <th className="text-right py-1 font-medium">Drift</th>
          <th className="text-center py-1 font-medium">Status</th>
        </tr>
      </thead>
      <tbody>
        {languages.map((lang) => (
          <LanguageRow
            key={lang.lang}
            langCode={lang.lang}
            queueDepth={lang.queueDepth}
            driftMs={lang.driftMs}
            status={lang.status}
          />
        ))}
      </tbody>
    </table>
  );
}

function LanguageRow({ langCode, queueDepth, driftMs, status }: {
  langCode: string;
  queueDepth: number;
  driftMs: number;
  status: StreamStatus;
}) {
  const langInfo = LANGS.find((l) => l.code === langCode);
  const label = langInfo ? `${langInfo.flag} ${langInfo.label}` : langCode;

  return (
    <tr className="border-b border-outline-variant/30">
      <td className="py-1 text-on-surface">{label}</td>
      <td className="py-1 text-right font-mono text-on-surface">{queueDepth}</td>
      <td className="py-1 text-right font-mono text-on-surface">{driftMs}ms</td>
      <td className="py-1 text-center">
        <StatusDot status={status} />
      </td>
    </tr>
  );
}

const STATUS_COLORS: Record<StreamStatus, string> = {
  green: "bg-green-500",
  yellow: "bg-yellow-500",
  red: "bg-red-500",
};

function StatusDot({ status }: { status: StreamStatus }) {
  return (
    <span
      className={`inline-block w-2.5 h-2.5 rounded-full ${STATUS_COLORS[status]}`}
      title={status}
    />
  );
}
