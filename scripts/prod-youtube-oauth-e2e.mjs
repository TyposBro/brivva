import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import playwright from '../frontend/node_modules/@playwright/test/index.js';
const { chromium } = playwright;

const CWD = process.cwd();
const ROOT = path.basename(CWD) === 'frontend' ? path.dirname(CWD) : CWD;
const API = process.env.BRIVVA_PROD_API_URL || 'https://brivva-api.milliytechnology.workers.dev';
const FRONTEND = process.env.BRIVVA_PROD_FRONTEND_URL || 'https://brivva.pages.dev';
const USER_ID = process.env.E2E_USER_ID || '111778147327359525059'; // typosbro@proton.me, YouTube OAuth connected
const SOURCE_LANG = process.env.E2E_SOURCE_LANG || 'en';
const DEST_LANG = process.env.E2E_YOUTUBE_LANG || 'ko';
const DURATION = Number(process.env.E2E_RECORD_SECONDS || '180');
const HEADLESS = (process.env.E2E_HEADLESS || 'true') !== 'false';
const CLEANUP_SESSION = (process.env.E2E_CLEANUP_SESSION || 'false') === 'true';
const ARTIFACT_ROOT = path.join(ROOT, 'tmp', 'prod-e2e-runs');
const runStartMs = Date.now();
const runId = new Date(runStartMs).toISOString().replace(/[:.]/g, '-');
const outDir = path.join(ARTIFACT_ROOT, runId);
fs.mkdirSync(outDir, { recursive: true });

function redact(v) {
  return JSON.stringify(v, null, 2)
    .replace(/("stream_key"\s*:\s*")[^"]+/g, '$1<redacted>')
    .replace(/("rtmp_url"\s*:\s*")[^"]+/g, '$1<redacted>')
    .replace(/(live2\/)[A-Za-z0-9_-]+/g, '$1<redacted>');
}
function log(event, fields = {}) {
  console.log(JSON.stringify({ t: new Date().toISOString(), event, ...fields }));
}
async function apiJson(pathname, init = {}) {
  const res = await fetch(`${API}${pathname}`, { ...init, headers: { 'content-type': 'application/json', ...(init.headers || {}) } });
  const text = await res.text();
  let body; try { body = text ? JSON.parse(text) : null; } catch { body = text; }
  if (!res.ok) throw new Error(`${init.method || 'GET'} ${pathname} -> ${res.status}: ${text}`);
  return body;
}
async function safeScreenshot(page, name) { try { await page.screenshot({ path: path.join(outDir, name), fullPage: true }); } catch {} }
async function bodyText(page) { try { return await page.locator('body').innerText({ timeout: 3000 }); } catch { return ''; } }
async function waitForSessionEnded(sessionId, timeoutMs = Number(process.env.E2E_WAIT_ENDED_MS || '45000')) {
  const deadline = Date.now() + timeoutMs;
  let last = null;
  while (Date.now() < deadline) {
    try {
      last = await apiJson(`/api/sessions/${sessionId}/summary`);
      fs.writeFileSync(path.join(outDir, 'summary-latest.json'), JSON.stringify(last, null, 2));
      if (last?.status === 'ended' || last?.is_final === true) return last;
    } catch (e) {
      last = { error: e.message };
    }
    await new Promise(resolve => setTimeout(resolve, 3000));
  }
  log('session.ended_wait_timeout', { sessionId, last });
  return last;
}

const meta = { runId, outDir, api: API, frontend: FRONTEND, userId: USER_ID, sourceLang: SOURCE_LANG, destLang: DEST_LANG, durationSec: DURATION, headless: HEADLESS, cleanupSession: CLEANUP_SESSION };
fs.writeFileSync(path.join(outDir, 'meta.json'), JSON.stringify(meta, null, 2));
log('run.start', meta);

let sessionId, streamId, watchUrl, broadcastId;
let failed = false;
const consoleLines = [];
const watcherLines = [];

try {
  const tokenBody = await apiJson('/auth/token', { method: 'POST', body: JSON.stringify({ user_id: USER_ID }) });
  const token = tokenBody.token;
  const title = `Brivva prod e2e ${runId}`;
  const created = await apiJson('/api/sessions', {
    method: 'POST',
    body: JSON.stringify({
      user_id: USER_ID,
      title,
      source_lang: SOURCE_LANG,
      target_langs: [DEST_LANG],
      platforms: [{ platform: 'youtube', lang: DEST_LANG, delay_ms: 4000, host_gain: 0.2 }],
      privacy_status: 'unlisted',
      translation_terms: 'Brivva production synthetic test. Ignore content; testing live stream health.',
    }),
  });
  sessionId = created.session.id;
  const stream = created.streams.find(s => s.platform === 'youtube') || created.streams[0];
  streamId = stream?.id || null;
  watchUrl = stream?.watch_url || (stream?.platform_broadcast_id ? `https://www.youtube.com/watch?v=${stream.platform_broadcast_id}` : null);
  broadcastId = stream?.platform_broadcast_id || null;
  fs.writeFileSync(path.join(outDir, 'created.redacted.json'), redact(created));
  fs.writeFileSync(path.join(outDir, 'watch-url.txt'), `${watchUrl || ''}\n`);
  log('session.created', { sessionId, broadcastId, watchUrl, streamCount: created.streams.length, streams: created.streams.map(s => ({ id: s.id, platform: s.platform, lang: s.lang, status: s.status, watch_url: s.watch_url || null, platform_broadcast_id: s.platform_broadcast_id || null })) });

  const fixtureVideo = process.env.E2E_VIDEO_FILE || path.join(ROOT, 'tests/e2e/fixtures/fake-cam.y4m');
  const fixtureAudio = process.env.E2E_AUDIO_FILE || path.join(ROOT, 'tests/e2e/fixtures/fake-mic.wav');
  const args = ['--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream', '--autoplay-policy=no-user-gesture-required', '--disable-background-timer-throttling', '--disable-backgrounding-occluded-windows'];
  if (fs.existsSync(fixtureVideo)) args.push(`--use-file-for-fake-video-capture=${fixtureVideo}`);
  if (fs.existsSync(fixtureAudio)) args.push(`--use-file-for-fake-audio-capture=${fixtureAudio}`);

  const browser = await chromium.launch({ headless: HEADLESS, args });
  const context = await browser.newContext({ viewport: { width: 1440, height: 1100 }, recordVideo: { dir: outDir, size: { width: 1440, height: 1100 } } });
  const host = await context.newPage();
  host.on('console', msg => { const line = `[${msg.type()}] ${msg.text()}`; consoleLines.push(line); if (/error|warn|provider|ffmpeg|rtmp|media|websocket|soniox|tts|live/i.test(line)) log('host.console', { line: line.slice(0, 1000) }); });
  host.on('pageerror', err => log('host.pageerror', { message: err.message, stack: err.stack }));

  let watcher;
  if (watchUrl) {
    watcher = await context.newPage();
    watcher.on('console', msg => watcherLines.push(`[${msg.type()}] ${msg.text()}`));
    await watcher.goto(watchUrl, { waitUntil: 'domcontentloaded', timeout: 60000 }).catch(e => log('watch.goto_failed', { message: e.message }));
    await watcher.waitForTimeout(5000);
    await watcher.mouse.click(720, 520).catch(()=>{});
    await safeScreenshot(watcher, 'youtube-watch-before.png');
    log('watch.opened', { watchUrl });
  }

  await host.goto(`${FRONTEND}/?user_id=${encodeURIComponent(USER_ID)}#token=${encodeURIComponent(token)}`, { waitUntil: 'domcontentloaded', timeout: 30000 });
  await host.waitForURL(/\/dashboard\b/, { timeout: 30000 }).catch(() => {});
  await host.evaluate(() => localStorage.setItem('brivva:sessionLogs', '1'));
  await host.goto(`${FRONTEND}/session/${sessionId}/setup`, { waitUntil: 'domcontentloaded', timeout: 30000 });
  await safeScreenshot(host, 'setup.png');
  const setupTxt = await bodyText(host);
  fs.writeFileSync(path.join(outDir, 'setup-body.txt'), setupTxt);
  const skip = host.getByRole('button', { name: /Skip \(use default voice\)/i });
  if (await skip.isVisible({ timeout: 5000 }).catch(() => false)) await skip.click();
  const goLive = host.getByRole('button', { name: /Go Live/i });
  await goLive.click({ timeout: 60000 });
  await host.waitForURL(new RegExp(`/session/${sessionId}/live\\b`), { timeout: 60000 });
  await safeScreenshot(host, 'live-before-record.png');
  await host.getByRole('button', { name: /^Record$/i }).click({ timeout: 60000 });
  log('record.started', { sessionId, watchUrl });

  const started = Date.now();
  const poll = setInterval(async () => {
    const elapsed = Math.round((Date.now() - started) / 1000);
    const htxt = (await bodyText(host)).slice(0, 6000);
    fs.writeFileSync(path.join(outDir, 'host-latest-body.txt'), htxt);
    const signals = htxt.match(/([^\n]*(Media connection|Provider health|ERR|error|ffmpeg|rtmp|Cannot load|speed=|decode_slice|utterance|Live|Recording)[^\n]*)/ig)?.slice(0, 12) || [];
    if (signals.length) log('host.signal', { elapsed, signals });
    if (watcher) {
      const wtxt = (await bodyText(watcher)).slice(0, 6000);
      fs.writeFileSync(path.join(outDir, 'watch-latest-body.txt'), wtxt);
      const ws = wtxt.match(/([^\n]*(Live|Waiting|Started|watching|unavailable|offline|Playback|error|premiere|scheduled|chat)[^\n]*)/ig)?.slice(0, 12) || [];
      if (ws.length) log('watch.signal', { elapsed, signals: ws });
      if (elapsed % 30 === 0) { await watcher.reload({ waitUntil: 'domcontentloaded', timeout: 30000 }).catch(()=>{}); await watcher.waitForTimeout(3000).catch(()=>{}); await safeScreenshot(watcher, `youtube-watch-${elapsed}s.png`); }
    }
  }, 10000);

  await host.waitForTimeout(DURATION * 1000);
  clearInterval(poll);
  await safeScreenshot(host, 'live-end.png');
  if (watcher) await safeScreenshot(watcher, 'youtube-watch-end.png');
  fs.writeFileSync(path.join(outDir, 'host-body-final.txt'), await bodyText(host));
  if (watcher) fs.writeFileSync(path.join(outDir, 'watch-body-final.txt'), await bodyText(watcher));
  const stop = host.getByRole('button', { name: /^Stop$/i });
  if (await stop.isVisible({ timeout: 5000 }).catch(() => false)) await stop.click();
  log('record.stopped', { sessionId });
  await waitForSessionEnded(sessionId);
  if (watcher) {
    await watcher.waitForTimeout(Number(process.env.E2E_POSTSTOP_WAIT_MS || '30000'));
    await watcher.reload({ waitUntil: 'domcontentloaded', timeout: 30000 }).catch(()=>{});
    await watcher.waitForTimeout(3000).catch(()=>{});
    await safeScreenshot(watcher, 'youtube-watch-poststop.png');
    fs.writeFileSync(path.join(outDir, 'watch-body-poststop.txt'), await bodyText(watcher));
  }
  await context.close();
  await browser.close();
} catch (e) {
  failed = true;
  log('run.error', { message: e.message, stack: e.stack });
} finally {
  fs.writeFileSync(path.join(outDir, 'host-console.log'), consoleLines.join('\n'));
  fs.writeFileSync(path.join(outDir, 'watch-console.log'), watcherLines.join('\n'));
  if (sessionId) {
    try {
      const res = await fetch(`${API}/api/sessions/${sessionId}/logs.ndjson?user_id=${encodeURIComponent(USER_ID)}&limit=20000`);
      fs.writeFileSync(path.join(outDir, 'session-logs.ndjson'), await res.text());
      log('logs.fetched', { sessionId });
    } catch (e) { log('logs.fetch_failed', { message: e.message }); }
    if (CLEANUP_SESSION) {
      try { await apiJson(`/api/sessions/${sessionId}`, { method: 'DELETE' }); log('session.deleted', { sessionId }); } catch (e) { log('session.delete_failed', { message: e.message }); }
    }
  }
  await fetchCloudWatchLogs({ outDir, runStartMs, sessionId, streamId });
  log('run.done', { failed, outDir, sessionId, streamId, watchUrl, broadcastId });
  if (failed) process.exit(1);
}

async function fetchCloudWatchLogs({ outDir, runStartMs, sessionId, streamId }) {
  if ((process.env.E2E_FETCH_CLOUDWATCH || 'true') === 'false') return;
  const group = process.env.E2E_CLOUDWATCH_GROUP || '/ecs/brivva';
  const region = process.env.AWS_REGION || 'us-east-1';
  const start = String(Math.max(0, runStartMs - 60_000));
  const end = String(Date.now() + 60_000);
  try {
    const output = execFileSync('aws', [
      'logs', 'filter-log-events',
      '--region', region,
      '--log-group-name', group,
      '--start-time', start,
      '--end-time', end,
      '--output', 'json',
    ], { encoding: 'utf8', maxBuffer: 50 * 1024 * 1024 });
    fs.writeFileSync(path.join(outDir, 'cloudwatch-all.json'), output);
    log('cloudwatch.fetched', { group, region, sessionId, streamId });
  } catch (e) {
    log('cloudwatch.fetch_failed', { message: e.message, group, region });
  }
}
