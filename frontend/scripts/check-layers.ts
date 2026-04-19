// Dependency-direction audit for CLAUDE.md §1.2.
//
// Rules:
//   core/          may NOT import from shared/ | features/ | orchestration/
//   shared/        may NOT import from features/ | orchestration/
//   shared/<dir>/  may NOT import from a sibling shared/<other>/ (§1.3)
//   features/<a>/  may NOT import from another features/<b>/ or orchestration/
//   orchestration/ may import anything
//
// Run:     bun run --cwd frontend check:layers
// Exit 0 if all layers comply.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const ROOT = new URL("../src", import.meta.url).pathname;

type Violation = { file: string; importPath: string; rule: string };

function walk(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    const stat = statSync(p);
    if (stat.isDirectory()) walk(p, out);
    else if (/\.(ts|tsx)$/.test(entry) && !/\.test\.tsx?$/.test(entry)) out.push(p);
  }
  return out;
}

const IMPORT_RE = /from\s+["']([^"']+)["']/g;

function resolveImport(fromFile: string, spec: string): string | null {
  if (!spec.startsWith(".")) return null;
  const abs = new URL(spec, `file://${fromFile}`).pathname;
  return relative(ROOT, abs);
}

function layerOf(path: string): string | null {
  if (path.startsWith("core/")) return "core";
  if (path.startsWith("shared/")) return "shared";
  if (path.startsWith("features/")) return "features";
  if (path.startsWith("orchestration/")) return "orchestration";
  return null;
}

function sharedSubdir(path: string): string | null {
  const match = /^shared\/([^/]+)/.exec(path);
  return match?.[1] ?? null;
}

function featureName(path: string): string | null {
  const match = /^features\/([^/]+)/.exec(path);
  return match?.[1] ?? null;
}

const violations: Violation[] = [];

for (const file of walk(ROOT)) {
  const relFile = relative(ROOT, file);
  const fromLayer = layerOf(relFile);
  if (!fromLayer) continue;

  const src = readFileSync(file, "utf-8");
  for (const match of src.matchAll(IMPORT_RE)) {
    const spec = match[1]!;
    const target = resolveImport(file, spec);
    if (!target) continue;
    const toLayer = layerOf(target);
    if (!toLayer) continue;

    if (fromLayer === "core" && toLayer !== "core") {
      violations.push({ file: relFile, importPath: target, rule: "core may not import non-core" });
    } else if (fromLayer === "shared" && (toLayer === "features" || toLayer === "orchestration")) {
      violations.push({ file: relFile, importPath: target, rule: "shared may not import features/orchestration" });
    } else if (fromLayer === "shared" && toLayer === "shared") {
      const a = sharedSubdir(relFile);
      const b = sharedSubdir(target);
      if (a && b && a !== b) {
        violations.push({ file: relFile, importPath: target, rule: `shared/${a} may not import sibling shared/${b}` });
      }
    } else if (fromLayer === "features" && toLayer === "orchestration") {
      violations.push({ file: relFile, importPath: target, rule: "features may not import orchestration" });
    } else if (fromLayer === "features" && toLayer === "features") {
      const a = featureName(relFile);
      const b = featureName(target);
      if (a && b && a !== b) {
        violations.push({ file: relFile, importPath: target, rule: `feature ${a} may not import sibling feature ${b}` });
      }
    }
  }
}

if (violations.length === 0) {
  console.log("[check-layers] 0 violations — dependency graph matches CLAUDE.md §1.2");
  process.exit(0);
}

console.error(`[check-layers] ${violations.length} violation(s):`);
for (const v of violations) {
  console.error(`  ${v.file}`);
  console.error(`    → ${v.importPath}`);
  console.error(`    rule: ${v.rule}`);
}
process.exit(1);
