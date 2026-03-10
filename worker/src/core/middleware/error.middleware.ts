import type { Context } from "hono";

export const globalErrorHandler = (err: Error, c: Context) => {
  console.error({ error: err.message, stack: err.stack });
  return c.json({ message: err.message || "Internal Server Error" }, 500);
};
