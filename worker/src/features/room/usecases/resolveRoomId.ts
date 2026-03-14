import { generateRoomId } from "./generateRoomId";

type RouteResult =
  | { ok: true; roomId: string }
  | { ok: false; error: string; status: 400 };

export function resolveRoomId(role: string | undefined, roomId: string | undefined): RouteResult {
  if (!role) return { ok: false, error: "Missing role param", status: 400 };

  if (role === "host") return { ok: true, roomId: generateRoomId() };

  if (role === "guest") {
    if (!roomId) return { ok: false, error: "Missing roomId", status: 400 };
    return { ok: true, roomId };
  }

  return { ok: false, error: "Invalid role", status: 400 };
}
