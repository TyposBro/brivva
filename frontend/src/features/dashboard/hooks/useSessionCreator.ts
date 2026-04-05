import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { PLATFORMS, langLabel } from "../../../shared/platforms";
import { createSession } from "../../../shared/api";
import type { UserInfo, PlatformConfig } from "../../../shared/api";
import type { Destination } from "../types";

type Params = {
  userId: string;
  title: string;
  sourceLang: string;
  selectedVoice: string;
  destinations: Destination[];
  privacyStatus: string;
  user: UserInfo | null;
};

export function useSessionCreator() {
  const navigate = useNavigate();
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState("");

  async function handleCreateSession(params: Params) {
    const validationError = validate(params);
    if (validationError) {
      setError(validationError);
      return;
    }

    setCreating(true);
    setError("");

    try {
      const result = await submitSession(params);
      if (result.errors?.length) setError(result.errors.join("; "));
      navigate(`/session/${result.session.id}`);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : "Failed to create session");
      setCreating(false);
    }
  }

  return { creating, error, handleCreateSession };
}

function validate({ title, destinations, sourceLang, user }: Params): string | null {
  if (!title.trim()) return "Enter a session title";
  if (destinations.length === 0) return "Add at least one destination";

  for (const dest of destinations) {
    if (dest.lang === sourceLang) {
      return `${langLabel(dest.lang)} is your source language — remove or change the destination`;
    }

    const p = PLATFORMS.find((x) => x.id === dest.platform);
    if (dest.platform === "youtube" && !user?.youtube_connected) {
      return "Connect your YouTube account first";
    }
    if (p && !p.auto) {
      if (!p.keyOnly && !dest.rtmp_url) return `Enter server URL for ${p.label}`;
      if (!dest.stream_key) return `Enter stream key for ${p.label}`;
    }
  }

  return null;
}

function submitSession({
  userId,
  title,
  sourceLang,
  selectedVoice,
  destinations,
  privacyStatus,
}: Params) {
  const targetLangs = [...new Set(destinations.map((d) => d.lang))];
  const platforms = buildPlatformConfigs(destinations);
  const hasYoutube = destinations.some((d) => d.platform === "youtube");

  return createSession({
    user_id: userId,
    title: title.trim(),
    source_lang: sourceLang,
    target_langs: targetLangs,
    voice_id: selectedVoice || undefined,
    platforms,
    privacy_status: hasYoutube ? privacyStatus : undefined,
  });
}

function buildPlatformConfigs(destinations: Destination[]): PlatformConfig[] {
  return destinations.map((d) => {
    const p = PLATFORMS.find((x) => x.id === d.platform);
    if (p?.auto) return { platform: d.platform, lang: d.lang };

    const rtmpUrl = p?.keyOnly ? p.defaultRtmp : d.rtmp_url || p?.defaultRtmp || "";
    return {
      platform: d.platform,
      lang: d.lang,
      rtmp_url: rtmpUrl,
      stream_key: d.stream_key,
    };
  });
}
