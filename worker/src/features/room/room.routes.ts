import { Hono } from "hono";
import type { Bindings } from "../../core/types";

const roomApp = new Hono<{ Bindings: Bindings }>();

function genRoomId(): string {
  return Math.random().toString(36).slice(2, 8).toUpperCase();
}

// Thin proxy — all room logic lives in the RoomDO Durable Object.
// Worker generates the roomId for hosts, then forwards the WS upgrade to the DO.
roomApp.get("/room", async (c) => {
  if (c.req.header("Upgrade") !== "websocket") {
    return c.text("WebSocket upgrade required", 426);
  }

  const role = c.req.query("role");
  if (!role) return c.text("Missing role param", 400);

  let roomId: string;
  if (role === "host") {
    roomId = genRoomId();
  } else if (role === "guest") {
    roomId = c.req.query("roomId") ?? "";
    if (!roomId) return c.text("Missing roomId", 400);
  } else {
    return c.text("Invalid role", 400);
  }

  const doId = c.env.ROOMS.idFromName(roomId);
  const stub = c.env.ROOMS.get(doId);

  // Forward the WebSocket upgrade to the DO, with roomId injected into URL
  const url = new URL(c.req.url);
  url.searchParams.set("roomId", roomId);

  return stub.fetch(new Request(url.toString(), c.req.raw));
});

export default roomApp;
