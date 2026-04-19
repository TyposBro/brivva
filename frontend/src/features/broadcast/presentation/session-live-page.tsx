import { useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import { Loader2 } from "lucide-react";
import { BroadcastView } from "./broadcast-view";
import { SignInGate } from "../../../shared/auth/sign-in-gate";
import { useAuth } from "../../../shared/auth/use-auth";
import * as api from "../data/api-client";

export default function SessionLivePage() {
  return (
    <SignInGate>
      <Inner />
    </SignInGate>
  );
}

function Inner() {
  const { id } = useParams<{ id: string }>();
  const userId = useAuth().userId!;
  const [sourceLang, setSourceLang] = useState<string | null>(null);

  useEffect(() => {
    if (!id) return;
    void api.getSession(id).then((data) => {
      setSourceLang(data.session?.source_lang ?? "en");
    });
  }, [id]);

  if (!id || sourceLang === null) {
    return (
      <div className="min-h-screen bg-background flex items-center justify-center">
        <Loader2 className="w-6 h-6 text-primary animate-spin" />
      </div>
    );
  }

  return (
    <BroadcastView
      sessionId={id}
      sourceLang={sourceLang}
      userId={userId}
      autoSkipVoice
    />
  );
}
