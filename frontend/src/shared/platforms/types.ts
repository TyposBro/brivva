export type Platform = {
  id: string;
  label: string;
  region: string;
  auto: boolean;
  defaultRtmp: string;
  help: string;
  settingsUrl: string;
  keyOnly: boolean;
};

export type Lang = { code: string; label: string; flag: string };
