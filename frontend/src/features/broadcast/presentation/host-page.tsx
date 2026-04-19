import { useSearchParams } from "react-router-dom";
import { BroadcastView } from "./broadcast-view";
import { SignInGate } from "../../../shared/auth/sign-in-gate";
import { useAuth } from "../../../shared/auth/use-auth";

// Legacy entry point: /host?sessionId=X&sourceLang=Y. Kept so existing links
// (including the resume button on /session/:id) keep working alongside the
// new split /session/:id/setup → /session/:id/live flow.
export default function HostPage() {
  return (
    <SignInGate>
      <Inner />
    </SignInGate>
  );
}

function Inner() {
  const [searchParams] = useSearchParams();
  const sessionId = searchParams.get("sessionId") ?? undefined;
  const sourceLang = searchParams.get("sourceLang") ?? "en";
  const userId = useAuth().userId!;
  return (
    <BroadcastView sessionId={sessionId} sourceLang={sourceLang} userId={userId} />
  );
}
