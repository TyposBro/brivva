import { Hono } from "hono";
import { cors } from "hono/cors";
import { globalErrorHandler } from "./core/middleware/error.middleware";
import translationApp from "./features/translation/api/translation.routes";
import type { Bindings, Variables } from "./core/types";

const app = new Hono<{ Bindings: Bindings; Variables: Variables }>();

app.use(
  "*",
  cors({
    origin: (origin) => origin, // allow all origins — demo only
    allowMethods: ["GET", "POST", "OPTIONS"],
    allowHeaders: ["Content-Type"],
  })
);

app.get("/live", (c) => c.json({ status: "ok", env: c.env.ENVIRONMENT }));

app.route("/api", translationApp);

app.onError(globalErrorHandler);

export default { fetch: app.fetch };
