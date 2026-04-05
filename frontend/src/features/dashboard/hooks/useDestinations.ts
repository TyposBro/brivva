import { useState } from "react";
import { PLATFORMS, PLATFORM_LANG, detectPlatform } from "../../../shared/platforms";
import type { PlatformCredential } from "../../../shared/api";
import type { Destination } from "../types";

export function useDestinations(
  savedCreds: Record<string, PlatformCredential>,
  sourceLang: string,
) {
  const [destinations, setDestinations] = useState<Destination[]>([]);
  const [magicPaste, setMagicPaste] = useState("");

  function addDestination(platformId: string) {
    const platform = PLATFORMS.find((p) => p.id === platformId);
    if (!platform) return;

    const autoLang = PLATFORM_LANG[platformId];
    const lang = autoLang ?? (sourceLang === "en" ? "ja" : "en");
    const cred = savedCreds[platformId];

    setDestinations((prev) => [
      ...prev,
      {
        uid: crypto.randomUUID(),
        platform: platformId,
        lang,
        rtmp_url: cred?.rtmp_url ?? platform.defaultRtmp ?? "",
        stream_key: cred?.stream_key ?? "",
      },
    ]);
  }

  function removeDestination(uid: string) {
    setDestinations((prev) => prev.filter((d) => d.uid !== uid));
  }

  function updateDestination(uid: string, patch: Partial<Destination>) {
    setDestinations((prev) =>
      prev.map((d) => (d.uid === uid ? { ...d, ...patch } : d)),
    );
  }

  function handleMagicPaste(value: string) {
    setMagicPaste(value);
    const detected = detectPlatform(value);
    if (!detected) return;

    const autoLang = PLATFORM_LANG[detected.platform];
    setDestinations((prev) => [
      ...prev,
      {
        uid: crypto.randomUUID(),
        platform: detected.platform,
        lang: autoLang ?? "en",
        rtmp_url: detected.rtmpUrl,
        stream_key: detected.streamKey,
      },
    ]);
    setMagicPaste("");
  }

  return {
    destinations,
    magicPaste,
    addDestination,
    removeDestination,
    updateDestination,
    handleMagicPaste,
  };
}
