import { Hono } from "hono";
import type { Bindings } from "../../core/types";
import { resolveRoomId } from "./usecases/resolveRoomId";

const roomApp = new Hono<{ Bindings: Bindings }>();

roomApp.get("/room", async (c) => {
  if (c.req.header("Upgrade") !== "websocket") {
    return c.text("WebSocket upgrade required", 426);
  }

  const result = resolveRoomId(c.req.query("role"), c.req.query("roomId"));
  if (!result.ok) return c.text(result.error, result.status);

  const doId = c.env.ROOMS.idFromName(result.roomId);
  const stub = c.env.ROOMS.get(doId);

  const url = new URL(c.req.url);
  url.searchParams.set("roomId", result.roomId);

  return stub.fetch(new Request(url.toString(), c.req.raw));
});

export default roomApp;
