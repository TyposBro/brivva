import { Youtube, Tv, Monitor, Globe } from "lucide-react";

const ICONS: Record<string, typeof Globe> = {
  youtube: Youtube,
  twitch: Tv,
  "local-test": Monitor,
};

export function PlatformIcon({ id, className }: { id: string; className?: string }) {
  const Icon = ICONS[id] ?? Globe;
  return <Icon className={className} />;
}
