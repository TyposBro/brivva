import { useSyncExternalStore } from "react";
import { getUserId, subscribe } from "./auth-store";

export function useAuth(): { userId: string | null; isSignedIn: boolean } {
  const userId = useSyncExternalStore(subscribe, getUserId, getUserId);
  return { userId, isSignedIn: userId !== null };
}
