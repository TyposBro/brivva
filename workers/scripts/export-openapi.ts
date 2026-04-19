import { buildOpenApiDocument } from "../src/openapi";

const file = new URL("../../contracts/openapi/brivva-workers.json", import.meta.url);
await Bun.write(file, `${JSON.stringify(buildOpenApiDocument(), null, 2)}\n`);
