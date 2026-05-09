#!/usr/bin/env node
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { compareRuns } from './analyze-prod-media-stress.mjs';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const ROOT = path.dirname(__dirname);

const started = Date.now();
const parentRunId = safeName(process.env.E2E_PARENT_RUN_ID || `${formatRunDate(started)}-media-ingest-ab`);
const parentRoot = path.resolve(expandHome(process.env.E2E_AB_ARTIFACT_ROOT || path.join(ROOT, 'tmp', 'prod-media-ingest-ab-runs')));
const parentDir = path.join(parentRoot, parentRunId);
const modes = csv(process.env.E2E_MEDIA_INGEST_MATRIX || 'webrtc,webcodecs_ws');
const browsers = csv(process.env.E2E_BROWSER_MATRIX || process.env.E2E_BROWSER || process.env.BROWSER || 'brave');
const cooldownMs = number(process.env.E2E_AB_COOLDOWN_MS, 30_000);
const strict = bool(process.env.E2E_STRICT_GATES, true);

fs.mkdirSync(parentDir, { recursive: true });
writeJson(path.join(parentDir, 'meta.json'), {
  parent_run_id: parentRunId,
  generated_at: new Date(started).toISOString(),
  modes,
  browsers,
  cooldown_ms: cooldownMs,
  platform_matrix: process.env.E2E_PLATFORM_MATRIX || 'youtube',
  test_shape: process.env.E2E_TEST_SHAPE || 'translated',
  record_seconds: Number(process.env.E2E_RECORD_SECONDS || 600),
});

const runDirs = [];
const childResults = [];
for (const browser of browsers) {
  for (const mode of modes) {
    const childDir = browsers.length === 1 ? path.join(parentDir, mode) : path.join(parentDir, browser, mode);
    fs.mkdirSync(childDir, { recursive: true });
    const childRunId = safeName(`${parentRunId}-${browser}-${mode}`);
    const env = {
      ...process.env,
      E2E_BROWSER: browser,
      E2E_MEDIA_INGEST_MODE: mode,
      E2E_RUN_ID: childRunId,
      E2E_OUT_DIR: childDir,
    };
    const result = await runChild({ env, cwd: ROOT, logFile: path.join(childDir, 'wrapper-child.log') });
    childResults.push({ browser, mode, out_dir: childDir, exit_code: result.code, signal: result.signal });
    runDirs.push(childDir);
    writeJson(path.join(parentDir, 'children.json'), childResults);
    if (cooldownMs > 0 && !(browser === browsers.at(-1) && mode === modes.at(-1))) {
      await wait(cooldownMs);
    }
  }
}

const comparison = await compareRuns(parentDir, runDirs);
comparison.children = childResults;
writeJson(path.join(parentDir, 'summary.json'), comparison);
if (childResults.some((child) => child.exit_code !== 0) || (strict && comparison.result !== 'pass')) {
  process.exitCode = 1;
}
console.log(JSON.stringify({ result: comparison.result, decision_hint: comparison.decision_hint, summary: path.join(parentDir, 'summary.json') }));

function runChild({ env, cwd, logFile }) {
  return new Promise((resolve) => {
    const out = fs.createWriteStream(logFile, { flags: 'a' });
    const child = spawn(process.execPath, [path.join(ROOT, 'scripts', 'prod-media-stress-e2e.mjs')], {
      cwd,
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    child.stdout.pipe(process.stdout);
    child.stderr.pipe(process.stderr);
    child.stdout.pipe(out, { end: false });
    child.stderr.pipe(out, { end: false });
    child.on('exit', (code, signal) => {
      out.end(`\n[wrapper] exit code=${code} signal=${signal || ''}\n`);
      resolve({ code, signal });
    });
  });
}

function csv(value) {
  return String(value || '').split(',').map((item) => item.trim()).filter(Boolean);
}

function number(value, fallback) {
  const n = Number(value);
  return Number.isFinite(n) ? n : fallback;
}

function bool(value, fallback = false) {
  if (typeof value === 'boolean') return value;
  if (value === undefined || value === null || value === '') return fallback;
  return !['0', 'false', 'no', 'off'].includes(String(value).toLowerCase());
}

function writeJson(file, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);
}

function formatRunDate(ms) {
  const d = new Date(ms);
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}`;
}

function expandHome(p) {
  if (!p) return p;
  if (p === '~') return os.homedir();
  if (p.startsWith('~/')) return path.join(os.homedir(), p.slice(2));
  return p;
}

function safeName(value) {
  return String(value ?? 'run').replace(/[^a-zA-Z0-9_.-]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 120) || 'run';
}

function wait(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
