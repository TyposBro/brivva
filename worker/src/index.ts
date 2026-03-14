import { Hono } from "hono";
import { cors } from "hono/cors";
import { globalErrorHandler } from "./core/middleware/error.middleware";
import roomApp from "./features/room/room.routes";
import type { Bindings } from "./core/types";

export { RoomDO } from "./features/room/room.do";

const app = new Hono<{ Bindings: Bindings }>();

app.use(
  "*",
  cors({
    origin: (origin) => origin, // allow all origins — demo only
    allowMethods: ["GET", "POST", "OPTIONS"],
    allowHeaders: ["Content-Type"],
  })
);

app.get("/live", (c) => c.json({ status: "ok", env: c.env.ENVIRONMENT }));

app.route("/api", roomApp);

app.onError(globalErrorHandler);

export default { fetch: app.fetch };
