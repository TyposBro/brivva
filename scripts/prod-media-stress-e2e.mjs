#!/usr/bin/env node
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { spawn, spawnSync, execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import playwright from '../frontend/node_modules/@playwright/test/index.js';
import { analyzeRun } from './analyze-prod-media-stress.mjs';

const { chromium, firefox } = playwright;
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const CWD = process.cwd();
const ROOT = path.basename(CWD) === 'frontend' ? path.dirname(CWD) : path.dirname(__dirname);

const runStartMs = Date.now();
const config = loadConfig(runStartMs);
fs.mkdirSync(config.outDir, { recursive: true });
fs.mkdirSync(path.join(config.outDir, 'screenshots'), { recursive: true });

const eventLog = fs.createWriteStream(path.join(config.outDir, 'runner-events.ndjson'), { flags: 'a' });
const hostConsole = fs.createWriteStream(path.join(config.outDir, 'host-console.log'), { flags: 'a' });
const hostWs = fs.createWriteStream(path.join(config.outDir, 'host-ws.ndjson'), { flags: 'a' });

let failed = false;
let browser;
let context;
let fixtureServer;
let cfTail;
let monitor;
let poller;
let sessionId = null;
let streamRecords = [];
let finalError = null;
const pageVideos = [];
const watcherConsoleStreams = [];

writeMeta();
log('run.start', publicMeta());

try {
  validateMediaFile(config.mediaFile);
  writeInputMediaProbe(config.mediaFile, config.outDir);

  let fixtureUrl = null;
  let fakeDevice = null;
  if (config.mediaMode === 'shim') {
    fixtureServer = await startFixtureServer(config.mediaFile);
    fixtureUrl = fixtureServer.url;
    log('fixture.server_started', { fixtureUrl });
  } else if (config.mediaMode === 'fake-device') {
    fakeDevice = prepareFakeDeviceFixtures(config.mediaFile);
    log('fixture.fake_device_ready', fakeDevice);
  } else {
    throw new Error(`unsupported E2E_MEDIA_MODE=${config.mediaMode}; use shim or fake-device`);
  }

  cfTail = await startCloudflareTail(config);

  const created = await createProductionSession();
  sessionId = created.session.id;
  config.sessionId = sessionId;
  streamRecords = buildStreamRecords(created.streams, config);
  config.streams = streamRecords.map((s) => ({ id: s.id, kind: s.kind, lang: s.lang, platform: s.platform, watch_url: s.watch_url }));
  writeMeta();
  writeJsonRedacted(path.join(config.outDir, 'created.redacted.json'), created);
  writeJson(path.join(config.outDir, 'watch-urls.json'), {
    run_id: config.runId,
    session_id: sessionId,
    streams: streamRecords.map((stream) => ({
      stream_id: stream.id,
      artifact_name: stream.artifactName,
      platform: stream.platform,
      kind: stream.kind,
      lang: stream.lang,
      watch_url: stream.watch_url,
      platform_broadcast_id: stream.platform_broadcast_id ?? null,
      platform_stream_id: stream.platform_stream_id ?? null,
      delay_ms: stream.delay_ms ?? null,
      host_gain: stream.host_gain ?? null,
    })),
  });
  log('session.created', {
    sessionId,
    streamCount: streamRecords.length,
    streams: streamRecords.map(({ id, kind, platform, lang, watch_url, platform_broadcast_id }) => ({ id, kind, platform, lang, watch_url, platform_broadcast_id })),
  });

  poller = startApiPolling(sessionId, config);

  const launch = await launchBrowser(config, fakeDevice);
  browser = launch.browser;
  context = await browser.newContext({
    viewport: { width: 1440, height: 1100 },
    ignoreHTTPSErrors: true,
    recordVideo: { dir: config.outDir, size: { width: 1440, height: 1100 } },
  });
  if (config.mediaMode === 'shim') {
    await context.addInitScript({ content: buildMediaShimScript({ fixtureUrl, runId: config.runId }) });
  }

  const host = await context.newPage();
  attachPageDiagnostics(host, {
    name: 'host',
    consoleStream: hostConsole,
    wsStream: hostWs,
    outDir: config.outDir,
  });
  pageVideos.push({ page: host, target: 'host-video.webm' });

  const watchers = [];
  for (const stream of streamRecords.filter((s) => s.watch_url)) {
    const watcher = await context.newPage();
    const consolePath = path.join(config.outDir, `watcher-console-${stream.artifactName}.log`);
    const watcherConsole = fs.createWriteStream(consolePath, { flags: 'a' });
    watcherConsoleStreams.push(watcherConsole);
    attachPageDiagnostics(watcher, {
      name: `watcher-${stream.artifactName}`,
      consoleStream: watcherConsole,
      outDir: config.outDir,
    });
    pageVideos.push({ page: watcher, target: `watcher-video-${stream.artifactName}.webm` });
    await watcher.goto(stream.watch_url, { waitUntil: 'domcontentloaded', timeout: 60000 }).catch((error) => log('watch.goto_failed', { streamId: stream.id, message: error.message }));
    await watcher.waitForTimeout(5000).catch(() => {});
    await watcher.mouse.click(720, 520).catch(() => {});
    await safeScreenshot(watcher, `screenshots/watch-before-${stream.artifactName}.png`);
    fs.writeFileSync(path.join(config.outDir, `watch-body-before-${stream.artifactName}.txt`), await bodyText(watcher));
    watchers.push({ page: watcher, stream });
    log('watch.opened', { streamId: stream.id, kind: stream.kind, watchUrl: stream.watch_url });
  }

  await openHostSession(host, sessionId, config);
  await safeScreenshot(host, 'screenshots/setup.png');
  fs.writeFileSync(path.join(config.outDir, 'setup-body.txt'), await bodyText(host));
  await skipVoiceIfNeeded(host);
  await clickGoLive(host, sessionId);
  await safeScreenshot(host, 'screenshots/live-before-record.png');
  await clickRecord(host);
  log('record.started', { sessionId, durationSec: config.durationSec });

  monitor = startBrowserMonitor({ host, watchers, config });
  await wait(config.durationSec * 1000);
  await stopMonitor(monitor);

  await safeScreenshot(host, 'screenshots/live-end.png');
  fs.writeFileSync(path.join(config.outDir, 'host-body-final.txt'), await bodyText(host));
  for (const { page, stream } of watchers) {
    await safeScreenshot(page, `screenshots/watch-end-${stream.artifactName}.png`);
    fs.writeFileSync(path.join(config.outDir, `watch-body-final-${stream.artifactName}.txt`), await bodyText(page));
  }

  await clickStop(host);
  log('record.stopped', { sessionId });
  await waitForSessionEnded(sessionId, config);
  for (const { page, stream } of watchers) {
    await page.waitForTimeout(config.postStopWaitMs).catch(() => {});
    await page.reload({ waitUntil: 'domcontentloaded', timeout: 30000 }).catch(() => {});
    await page.waitForTimeout(3000).catch(() => {});
    await safeScreenshot(page, `screenshots/watch-poststop-${stream.artifactName}.png`);
    fs.writeFileSync(path.join(config.outDir, `watch-body-poststop-${stream.artifactName}.txt`), await bodyText(page));
  }
} catch (error) {
  failed = true;
  finalError = error;
  log('run.error', { message: error.message, stack: error.stack });
} finally {
  config.failed = failed;
  if (finalError) config.error = finalError.message;
  if (monitor) await stopMonitor(monitor).catch(() => {});
  if (poller) await poller.stop().catch(() => {});

  if (context) {
    await context.close().catch((error) => log('browser.context_close_failed', { message: error.message }));
  }
  await savePageVideos(pageVideos, config.outDir);
  if (browser) await browser.close().catch(() => {});

  if (sessionId) {
    await fetchSessionArtifacts(sessionId, config);
    if (config.cleanupSession) await cleanupSession(sessionId, config).catch((error) => log('session.cleanup_failed', { message: error.message }));
  }

  await stopCloudflareTail(cfTail).catch((error) => log('cf.tail_stop_failed', { message: error.message }));
  await fetchCloudWatchLogs({ config, runStartMs, sessionId, streams: streamRecords }).catch((error) => log('cloudwatch.fetch_unhandled', { message: error.message }));
  await fetchYoutubeVodMetadata(streamRecords, config).catch((error) => log('youtube.vod_metadata_unhandled', { message: error.message }));

  if (fixtureServer) await fixtureServer.close().catch(() => {});
  hostConsole.end();
  hostWs.end();
  for (const stream of watcherConsoleStreams) stream.end();
  writeMeta();

  let summary = null;
  try {
    summary = await analyzeRun(config.outDir);
    log('analysis.done', { result: summary.result, summary: path.join(config.outDir, 'summary.json') });
  } catch (error) {
    failed = true;
    log('analysis.failed', { message: error.message, stack: error.stack });
  }
  log('run.done', { failed, result: summary?.result ?? null, outDir: config.outDir, sessionId });
  eventLog.end();

  if (failed || (config.strictGates && summary?.result !== 'pass')) process.exitCode = 1;
}

function loadConfig(startMs) {
  const browser = (process.env.E2E_BROWSER || process.env.BROWSER || 'chromium').toLowerCase();
  const shape = (process.env.E2E_TEST_SHAPE || 'translated').toLowerCase();
  const headless = bool(process.env.E2E_HEADLESS ?? process.env.HEADLESS, true);
  const sourceLang = process.env.E2E_SOURCE_LANG || 'en';
  const targetLang = process.env.E2E_YOUTUBE_LANG || 'ko';
  const durationSec = number(process.env.E2E_RECORD_SECONDS, 1800);
  const mediaIngestMode = validateMediaIngestMode(process.env.E2E_MEDIA_INGEST_MODE || process.env.MEDIA_INGEST_MODE || 'auto');
  const runId = `${formatRunDate(startMs)}-${browser}-${shape}-${mediaIngestMode}`;
  const artifactRoot = process.env.E2E_ARTIFACT_ROOT || path.join(ROOT, 'tmp', 'prod-media-stress-runs');
  const outDir = path.join(artifactRoot, runId);
  return {
    root: ROOT,
    runId,
    outDir,
    api: process.env.BRIVVA_PROD_API_URL || 'https://brivva-api.milliytechnology.workers.dev',
    frontend: process.env.BRIVVA_PROD_FRONTEND_URL || 'https://brivva.pages.dev',
    userId: process.env.E2E_USER_ID || '111778147327359525059',
    sourceLang,
    targetLang,
    shape,
    browser,
    browserExecutable: process.env.BROWSER_EXECUTABLE || process.env.E2E_BROWSER_EXECUTABLE || '',
    headless,
    mediaFile: expandHome(process.env.E2E_MEDIA_FILE || path.join(os.homedir(), 'Desktop', 'text.mp4')),
    mediaMode: (process.env.E2E_MEDIA_MODE || (bool(process.env.E2E_MEDIA_SHIM, true) ? 'shim' : 'fake-device')).toLowerCase(),
    mediaIngestMode,
    durationSec,
    providerPollSeconds: number(process.env.E2E_PROVIDER_POLL_SECONDS, 10),
    screenshotSeconds: number(process.env.E2E_SCREENSHOT_SECONDS, 30),
    postStopWaitMs: number(process.env.E2E_POSTSTOP_WAIT_MS, 30000),
    cleanupSession: bool(process.env.E2E_CLEANUP_SESSION, false),
    fetchCloudWatch: bool(process.env.E2E_FETCH_CLOUDWATCH, true),
    fetchCloudflareObservability: bool(process.env.E2E_FETCH_CF_OBSERVABILITY, true),
    fetchYoutubeVod: bool(process.env.E2E_FETCH_YOUTUBE_VOD, true),
    strictGates: bool(process.env.E2E_STRICT_GATES, true),
    youtubeDelayMs: number(process.env.E2E_YOUTUBE_DELAY_MS, 4000),
    translatedHostGain: number(process.env.E2E_HOST_GAIN, 0.2),
    privacyStatus: process.env.E2E_YOUTUBE_PRIVACY_STATUS || 'unlisted',
    minProviderLiveRatio: number(process.env.E2E_MIN_PROVIDER_LIVE_RATIO, 0.9),
    minFfmpegSpeed: number(process.env.E2E_MIN_FFMPEG_SPEED, 0.98),
    minTtsCompletionRatio: number(process.env.E2E_MIN_TTS_COMPLETION_RATIO, 0.95),
    maxTtsDelayP95Ms: number(process.env.E2E_MAX_TTS_DELAY_P95_MS, 10000),
    maxTtsDriftMsPerMin: number(process.env.E2E_MAX_TTS_DRIFT_MS_PER_MIN, 500),
    allowTtsOverflow: bool(process.env.E2E_ALLOW_TTS_OVERFLOW, false),
    cloudflareWorker: process.env.E2E_CF_WORKER_NAME || 'brivva-api',
    cloudwatchGroup: process.env.E2E_CLOUDWATCH_GROUP || '/ecs/brivva',
    awsRegion: process.env.AWS_REGION || 'us-east-1',
  };
}

function publicMeta() {
  return {
    runId: config.runId,
    outDir: config.outDir,
    api: config.api,
    frontend: config.frontend,
    userId: config.userId,
    sourceLang: config.sourceLang,
    targetLang: config.targetLang,
    shape: config.shape,
    browser: config.browser,
    headless: config.headless,
    mediaFile: config.mediaFile,
    mediaMode: config.mediaMode,
    mediaIngestMode: config.mediaIngestMode,
    durationSec: config.durationSec,
    providerPollSeconds: config.providerPollSeconds,
    screenshotSeconds: config.screenshotSeconds,
    cleanupSession: config.cleanupSession,
    fetchCloudWatch: config.fetchCloudWatch,
    fetchCloudflareObservability: config.fetchCloudflareObservability,
    strictGates: config.strictGates,
    sessionId: config.sessionId ?? null,
    streams: config.streams ?? [],
    minProviderLiveRatio: config.minProviderLiveRatio,
    minFfmpegSpeed: config.minFfmpegSpeed,
    minTtsCompletionRatio: config.minTtsCompletionRatio,
    maxTtsDelayP95Ms: config.maxTtsDelayP95Ms,
    maxTtsDriftMsPerMin: config.maxTtsDriftMsPerMin,
    allowTtsOverflow: config.allowTtsOverflow,
    failed: config.failed ?? false,
    error: config.error ?? null,
  };
}

function writeMeta() {
  writeJson(path.join(config.outDir, 'meta.json'), publicMeta());
}

async function createProductionSession() {
  const tokenBody = await apiJson('/auth/token', { method: 'POST', body: JSON.stringify({ user_id: config.userId }) });
  config.authToken = tokenBody.token;
  const platforms = buildPlatforms(config);
  const targetLangs = [...new Set(platforms.map((p) => p.lang).filter(Boolean))];
  const title = `Brivva prod media stress ${config.runId}`;
  const created = await apiJson('/api/sessions', {
    method: 'POST',
    body: JSON.stringify({
      user_id: config.userId,
      title,
      source_lang: config.sourceLang,
      target_langs: targetLangs.length ? targetLangs : [config.targetLang],
      platforms,
      privacy_status: config.privacyStatus,
      translation_terms: [
        `run_id=${config.runId}`,
        `shape=${config.shape}`,
        'Brivva production synthetic media stress test.',
        'Ignore content; testing live stream health, source audio, translated TTS, FPS, bitrate, and drift.',
      ].join(' '),
    }),
  });
  return created;
}

function buildPlatforms(cfg) {
  if (cfg.shape === 'source') {
    return [{ platform: 'youtube', lang: cfg.sourceLang, delay_ms: 0, host_gain: 1.0 }];
  }
  if (cfg.shape === 'translated') {
    return [{ platform: 'youtube', lang: cfg.targetLang, delay_ms: cfg.youtubeDelayMs, host_gain: cfg.translatedHostGain }];
  }
  if (cfg.shape === 'dual') {
    return [
      { platform: 'youtube', lang: cfg.sourceLang, delay_ms: 0, host_gain: 1.0 },
      { platform: 'youtube', lang: cfg.targetLang, delay_ms: cfg.youtubeDelayMs, host_gain: cfg.translatedHostGain },
    ];
  }
  throw new Error(`unsupported E2E_TEST_SHAPE=${cfg.shape}; use source, translated, or dual`);
}

function buildStreamRecords(streams, cfg) {
  return streams.map((stream, index) => {
    const kind = classifyKind(stream, cfg);
    const artifactName = safeName(`${index}-${kind}-${stream.id ?? stream.platform_broadcast_id ?? 'stream'}`);
    return {
      ...stream,
      id: stream.id ?? `stream-${index}`,
      kind,
      artifactName,
      watch_url: stream.watch_url || (stream.platform_broadcast_id ? `https://www.youtube.com/watch?v=${stream.platform_broadcast_id}` : null),
    };
  });
}

function classifyKind(stream, cfg) {
  const delay = Number(stream.delay_ms);
  const gain = Number(stream.host_gain);
  if (stream.lang === cfg.sourceLang && (delay === 0 || gain === 1)) return 'source';
  return 'translated';
}

async function openHostSession(host, sessionId, cfg) {
  await host.goto(`${cfg.frontend}/?user_id=${encodeURIComponent(cfg.userId)}#token=${encodeURIComponent(cfg.authToken)}`, { waitUntil: 'domcontentloaded', timeout: 30000 });
  await host.waitForURL(/\/dashboard\b/, { timeout: 30000 }).catch(() => {});
  await host.evaluate((mode) => {
    localStorage.setItem('brivva:sessionLogs', '1');
    localStorage.setItem('brivva:mediaIngestMode', mode);
  }, cfg.mediaIngestMode);
  await host.goto(`${cfg.frontend}/session/${sessionId}/setup`, { waitUntil: 'domcontentloaded', timeout: 30000 });
}

async function skipVoiceIfNeeded(host) {
  const skip = host.getByRole('button', { name: /Skip \(use default voice\)/i });
  if (await skip.isVisible({ timeout: 5000 }).catch(() => false)) {
    await skip.click();
    log('setup.voice_skipped');
  }
}

async function clickGoLive(host, sessionId) {
  const goLive = host.getByRole('button', { name: /Go Live/i });
  await goLive.click({ timeout: 60000 });
  await host.waitForURL(new RegExp(`/session/${escapeRegExp(sessionId)}/live\\b`), { timeout: 60000 });
  log('host.live_page_loaded', { sessionId });
}

async function clickRecord(host) {
  await host.getByRole('button', { name: /^Record$/i }).click({ timeout: 60000 });
}

async function clickStop(host) {
  const stop = host.getByRole('button', { name: /^Stop$/i });
  if (await stop.isVisible({ timeout: 5000 }).catch(() => false)) await stop.click();
}

function startBrowserMonitor({ host, watchers, config: cfg }) {
  let stopped = false;
  const promise = (async () => {
    const started = Date.now();
    let nextScreenshot = 0;
    while (!stopped) {
      const elapsed = Math.round((Date.now() - started) / 1000);
      const hostText = (await bodyText(host)).slice(0, 12000);
      fs.writeFileSync(path.join(cfg.outDir, 'host-body-latest.txt'), hostText);
      const hostSignals = extractSignals(hostText);
      if (hostSignals.length) log('host.signal', { elapsed, signals: hostSignals });

      for (const { page, stream } of watchers) {
        const text = (await bodyText(page)).slice(0, 12000);
        fs.writeFileSync(path.join(cfg.outDir, `watch-body-latest-${stream.artifactName}.txt`), text);
        const signals = extractWatchSignals(text);
        if (signals.length) log('watch.signal', { elapsed, streamId: stream.id, signals });
      }

      if (elapsed >= nextScreenshot) {
        await safeScreenshot(host, `screenshots/host-${elapsed}s.png`);
        for (const { page, stream } of watchers) {
          await page.bringToFront().catch(() => {});
          await safeScreenshot(page, `screenshots/watch-${elapsed}s-${stream.artifactName}.png`);
        }
        await host.bringToFront().catch(() => {});
        nextScreenshot += cfg.screenshotSeconds;
      }
      await wait(10000);
    }
  })();
  return { stop: () => { stopped = true; }, promise };
}

async function stopMonitor(handle) {
  handle.stop();
  await Promise.race([handle.promise, wait(12000)]);
}

function startApiPolling(sessionId, cfg) {
  let stopped = false;
  let latestSummary = null;
  let latestUsage = null;
  const providerOut = fs.createWriteStream(path.join(cfg.outDir, 'provider-health.ndjson'), { flags: 'a' });
  const summaryOut = fs.createWriteStream(path.join(cfg.outDir, 'session-summary-samples.ndjson'), { flags: 'a' });
  const usageOut = fs.createWriteStream(path.join(cfg.outDir, 'session-usage-samples.ndjson'), { flags: 'a' });

  const poll = async (endpoint, out) => {
    const rec = { t: new Date().toISOString(), ts_ms: Date.now(), endpoint };
    try {
      rec.ok = true;
      rec.data = await apiJson(`/api/sessions/${sessionId}/${endpoint}`);
      if (endpoint === 'summary') latestSummary = rec.data;
      if (endpoint === 'usage') latestUsage = rec.data;
    } catch (error) {
      rec.ok = false;
      rec.error = error.message;
    }
    out.write(`${JSON.stringify(redactObject(rec))}\n`);
  };

  const promise = (async () => {
    while (!stopped) {
      await Promise.all([
        poll('provider-health', providerOut),
        poll('summary', summaryOut),
        poll('usage', usageOut),
      ]);
      await wait(cfg.providerPollSeconds * 1000);
    }
  })();

  return {
    stop: async () => {
      stopped = true;
      await Promise.race([promise, wait(cfg.providerPollSeconds * 1000 + 1000)]);
      providerOut.end();
      summaryOut.end();
      usageOut.end();
      if (latestSummary) writeJson(path.join(cfg.outDir, 'session-summary.json'), latestSummary);
      if (latestUsage) writeJson(path.join(cfg.outDir, 'session-usage.json'), latestUsage);
    },
  };
}

async function waitForSessionEnded(sessionId, cfg) {
  const timeoutMs = number(process.env.E2E_WAIT_ENDED_MS, 45000);
  const deadline = Date.now() + timeoutMs;
  let last = null;
  while (Date.now() < deadline) {
    try {
      last = await apiJson(`/api/sessions/${sessionId}/summary`);
      writeJson(path.join(cfg.outDir, 'session-summary.json'), last);
      if (last?.status === 'ended' || last?.is_final === true) return last;
    } catch (error) {
      last = { error: error.message };
    }
    await wait(3000);
  }
  log('session.ended_wait_timeout', { sessionId, last });
  return last;
}

async function fetchSessionArtifacts(sessionId, cfg) {
  try {
    const logs = await fetch(`${cfg.api}/api/sessions/${sessionId}/logs.ndjson?user_id=${encodeURIComponent(cfg.userId)}&limit=20000`);
    fs.writeFileSync(path.join(cfg.outDir, 'session-logs.ndjson'), await logs.text());
    log('logs.fetched', { sessionId, status: logs.status });
  } catch (error) {
    log('logs.fetch_failed', { message: error.message });
  }
  for (const endpoint of ['summary', 'usage']) {
    try {
      const data = await apiJson(`/api/sessions/${sessionId}/${endpoint}`);
      writeJson(path.join(cfg.outDir, `session-${endpoint}.json`), data);
    } catch (error) {
      log(`session.${endpoint}_fetch_failed`, { message: error.message });
    }
  }
}

async function cleanupSession(sessionId) {
  await apiJson(`/api/sessions/${sessionId}`, { method: 'DELETE' });
  log('session.deleted', { sessionId });
}

async function launchBrowser(cfg, fakeDevice) {
  const browserType = cfg.browser === 'firefox' || cfg.browser === 'zen' ? firefox : chromium;
  const executablePath = resolveBrowserExecutable(cfg);
  if (cfg.mediaMode === 'fake-device' && browserType !== chromium) {
    throw new Error('E2E_MEDIA_MODE=fake-device is Chromium/Brave-only; use E2E_MEDIA_MODE=shim for Firefox/Zen');
  }
  const args = browserType === chromium ? chromiumArgs(fakeDevice) : [];
  const launchOptions = { headless: cfg.headless, args };
  if (executablePath) launchOptions.executablePath = executablePath;
  if (browserType === firefox) {
    launchOptions.firefoxUserPrefs = {
      'media.autoplay.default': 0,
      'media.autoplay.blocking_policy': 0,
      'media.navigator.permission.disabled': true,
      'permissions.default.camera': 1,
      'permissions.default.microphone': 1,
    };
  }
  log('browser.launch', { browser: cfg.browser, headless: cfg.headless, executablePath: executablePath || null, args });
  return { browser: await browserType.launch(launchOptions) };
}

function chromiumArgs(fakeDevice) {
  const args = [
    '--autoplay-policy=no-user-gesture-required',
    '--disable-background-timer-throttling',
    '--disable-backgrounding-occluded-windows',
    '--disable-renderer-backgrounding',
    '--disable-features=CalculateNativeWinOcclusion,IntensiveWakeUpThrottling',
    '--use-fake-ui-for-media-stream',
  ];
  if (fakeDevice) {
    args.push('--use-fake-device-for-media-stream');
    args.push(`--use-file-for-fake-video-capture=${fakeDevice.video}`);
    args.push(`--use-file-for-fake-audio-capture=${fakeDevice.audio}`);
  }
  return args;
}

function resolveBrowserExecutable(cfg) {
  if (cfg.browser === 'chromium' || cfg.browser === 'firefox') return cfg.browserExecutable || undefined;
  if (cfg.browser === 'brave') {
    return cfg.browserExecutable || firstExisting(['/usr/bin/brave-browser', '/usr/bin/brave', '/snap/bin/brave', '/opt/brave.com/brave/brave']);
  }
  if (cfg.browser === 'zen') {
    return cfg.browserExecutable || firstExisting(['/usr/bin/zen', '/opt/zen/zen', path.join(os.homedir(), '.local/bin/zen')]);
  }
  throw new Error(`unsupported E2E_BROWSER=${cfg.browser}; use chromium, brave, firefox, or zen`);
}

function firstExisting(paths) {
  return paths.find((candidate) => candidate && fs.existsSync(candidate));
}

function attachPageDiagnostics(page, opts) {
  page.on('console', (msg) => {
    const line = `[${new Date().toISOString()}] [${msg.type()}] ${msg.text()}`;
    opts.consoleStream.write(`${redactString(line)}\n`);
    if (/error|warn|provider|ffmpeg|rtmp|media|websocket|soniox|tts|live|brivva-e2e/i.test(line)) {
      log(`${opts.name}.console`, { line: redactString(line).slice(0, 1500) });
    }
  });
  page.on('pageerror', (error) => log(`${opts.name}.pageerror`, { message: error.message, stack: error.stack }));
  page.on('requestfailed', (request) => log(`${opts.name}.requestfailed`, { url: request.url(), failure: request.failure()?.errorText ?? null }));
  if (opts.wsStream) {
    page.on('websocket', (ws) => {
      log(`${opts.name}.websocket.open`, { url: ws.url() });
      const writeFrame = (direction, event) => {
        const payload = Buffer.isBuffer(event.payload) ? `<binary:${event.payload.length}>` : String(event.payload ?? '');
        const json = payload.startsWith('{') ? parseJson(payload) : null;
        opts.wsStream.write(`${JSON.stringify(redactObject({
          t: new Date().toISOString(),
          ts_ms: Date.now(),
          page: opts.name,
          url: ws.url(),
          direction,
          payload: payload.length > 20000 ? `${payload.slice(0, 20000)}…<truncated>` : payload,
          json,
        }))}\n`);
      };
      ws.on('framesent', (event) => writeFrame('sent', event));
      ws.on('framereceived', (event) => writeFrame('received', event));
      ws.on('socketerror', (error) => log(`${opts.name}.websocket.error`, { url: ws.url(), message: String(error) }));
      ws.on('close', () => log(`${opts.name}.websocket.close`, { url: ws.url() }));
    });
  }
}

async function apiJson(pathname, init = {}) {
  const res = await fetch(`${config.api}${pathname}`, {
    ...init,
    headers: { 'content-type': 'application/json', ...(init.headers || {}) },
  });
  const text = await res.text();
  const body = text ? parseJson(text) ?? text : null;
  if (!res.ok) throw new Error(`${init.method || 'GET'} ${pathname} -> ${res.status}: ${redactString(text).slice(0, 2000)}`);
  return body;
}

function validateMediaFile(file) {
  if (!fs.existsSync(file)) throw new Error(`E2E_MEDIA_FILE not found: ${file}`);
  const stat = fs.statSync(file);
  if (!stat.isFile() || stat.size <= 0) throw new Error(`E2E_MEDIA_FILE is not a non-empty file: ${file}`);
}

function writeInputMediaProbe(file, outDir) {
  try {
    const out = execFileSync('ffprobe', ['-v', 'error', '-print_format', 'json', '-show_format', '-show_streams', file], { encoding: 'utf8', maxBuffer: 10 * 1024 * 1024 });
    fs.writeFileSync(path.join(outDir, 'input-media.ffprobe.json'), out);
  } catch (error) {
    writeJson(path.join(outDir, 'input-media.ffprobe.json'), { error: error.message, file });
    log('media.ffprobe_failed', { message: error.message });
  }
}

async function startFixtureServer(file) {
  const stat = fs.statSync(file);
  const contentType = contentTypeFor(file);
  const server = http.createServer((req, res) => {
    const url = new URL(req.url || '/', 'http://127.0.0.1');
    if (url.pathname !== '/fixture') {
      res.writeHead(404).end('not found');
      return;
    }
    const range = req.headers.range;
    res.setHeader('Access-Control-Allow-Origin', '*');
    res.setHeader('Cross-Origin-Resource-Policy', 'cross-origin');
    res.setHeader('Accept-Ranges', 'bytes');
    res.setHeader('Content-Type', contentType);
    if (range) {
      const match = range.match(/bytes=(\d+)-(\d*)/);
      const start = match ? Number(match[1]) : 0;
      const end = match?.[2] ? Number(match[2]) : stat.size - 1;
      if (!Number.isFinite(start) || !Number.isFinite(end) || start >= stat.size) {
        res.writeHead(416, { 'Content-Range': `bytes */${stat.size}` }).end();
        return;
      }
      res.writeHead(206, {
        'Content-Length': end - start + 1,
        'Content-Range': `bytes ${start}-${end}/${stat.size}`,
      });
      fs.createReadStream(file, { start, end }).pipe(res);
      return;
    }
    res.writeHead(200, { 'Content-Length': stat.size });
    fs.createReadStream(file).pipe(res);
  });
  await new Promise((resolve, reject) => {
    server.on('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const address = server.address();
  return {
    url: `http://127.0.0.1:${address.port}/fixture`,
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}

function buildMediaShimScript({ fixtureUrl, runId }) {
  return `(() => {
    const runId = ${JSON.stringify(runId)};
    const fixtureUrl = ${JSON.stringify(fixtureUrl)};
    const log = (event, fields = {}) => console.info('[brivva-e2e-media-shim]', JSON.stringify({ run_id: runId, event, ...fields }));
    const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
    const wants = (constraints, kind) => constraints && constraints[kind] !== false && constraints[kind] != null;
    const waitFor = async (predicate, label, timeoutMs = 15000) => {
      const start = Date.now();
      while (Date.now() - start < timeoutMs) {
        if (predicate()) return;
        await sleep(100);
      }
      throw new DOMException('fixture media shim timed out waiting for ' + label, 'NotFoundError');
    };
    async function createFixtureStream(constraints) {
      const wantVideo = wants(constraints, 'video');
      const wantAudio = wants(constraints, 'audio');
      if (!wantVideo && !wantAudio) throw new TypeError('audio or video constraint required');
      const video = document.createElement('video');
      video.src = fixtureUrl + '?run_id=' + encodeURIComponent(runId) + '&n=' + Math.random().toString(36).slice(2);
      video.loop = true;
      video.autoplay = true;
      video.playsInline = true;
      video.crossOrigin = 'anonymous';
      video.muted = false;
      video.volume = 1;
      video.style.cssText = 'position:fixed;left:-9999px;top:-9999px;width:1px;height:1px;opacity:0;pointer-events:none;';
      (document.body || document.documentElement).appendChild(video);
      await new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new DOMException('fixture metadata timeout', 'NotFoundError')), 15000);
        video.addEventListener('loadedmetadata', () => { clearTimeout(timer); resolve(); }, { once: true });
        video.addEventListener('error', () => { clearTimeout(timer); reject(new DOMException('fixture video error', 'NotFoundError')); }, { once: true });
      });
      await video.play().catch(async (error) => {
        log('video.play.retry_muted_then_unmuted', { message: String(error && error.message || error) });
        video.muted = true;
        await video.play();
        video.muted = false;
        video.volume = 1;
      });
      const capture = video.captureStream || video.mozCaptureStream;
      if (!capture) throw new DOMException('captureStream unavailable', 'NotSupportedError');
      const captured = capture.call(video);
      await waitFor(() => !wantVideo || captured.getVideoTracks().length > 0, 'video track');
      await waitFor(() => !wantAudio || captured.getAudioTracks().length > 0, 'audio track');
      const tracks = [];
      if (wantVideo) tracks.push(...captured.getVideoTracks().map((track) => track.clone()));
      if (wantAudio) tracks.push(...captured.getAudioTracks().map((track) => track.clone()));
      const stream = new MediaStream(tracks);
      for (const track of tracks) {
        const originalStop = track.stop.bind(track);
        track.stop = () => { originalStop(); if (tracks.every((t) => t.readyState === 'ended' || t === track)) video.remove(); };
      }
      log('gum.resolve', { constraints, tracks: tracks.map((track) => ({ kind: track.kind, label: track.label, settings: track.getSettings ? track.getSettings() : {} })) });
      return stream;
    }
    if (!navigator.mediaDevices) Object.defineProperty(navigator, 'mediaDevices', { configurable: true, value: {} });
    Object.defineProperty(navigator.mediaDevices, 'getUserMedia', { configurable: true, value: async (constraints = { audio: true, video: true }) => {
      log('gum.request', { constraints });
      return createFixtureStream(constraints || { audio: true, video: true });
    }});
    log('installed', { fixtureUrl });
  })();`;
}

function prepareFakeDeviceFixtures(file) {
  if (!commandExists('ffmpeg')) throw new Error('ffmpeg is required for E2E_MEDIA_MODE=fake-device');
  const fixtureDir = path.join(ROOT, 'tmp', 'prod-media-fixtures');
  fs.mkdirSync(fixtureDir, { recursive: true });
  const video = path.join(fixtureDir, 'text-720x1280-30.y4m');
  const audio = path.join(fixtureDir, 'text-audio-44k-mono.wav');
  const sourceMtime = fs.statSync(file).mtimeMs;
  if (!fs.existsSync(video) || fs.statSync(video).mtimeMs < sourceMtime) {
    execFileSync('ffmpeg', ['-y', '-i', file, '-vf', 'scale=720:1280:force_original_aspect_ratio=decrease,pad=720:1280:(ow-iw)/2:(oh-ih)/2,fps=30', '-pix_fmt', 'yuv420p', video], { stdio: 'inherit' });
  }
  if (!fs.existsSync(audio) || fs.statSync(audio).mtimeMs < sourceMtime) {
    execFileSync('ffmpeg', ['-y', '-i', file, '-vn', '-ar', '44100', '-ac', '1', '-sample_fmt', 's16', audio], { stdio: 'inherit' });
  }
  return { video, audio };
}

async function startCloudflareTail(cfg) {
  if (!cfg.fetchCloudflareObservability) {
    writeJson(path.join(cfg.outDir, 'cf-observability-summary.json'), { tail_captured: false, skipped: true, worker_errors: 0, worker_exceptions: 0, youtube_api_errors: 0, d1_errors: 0, provider_health_polls: 0, session_log_writes: 0, session_log_write_errors: 0 });
    return null;
  }
  const bin = resolveWranglerBin();
  if (!bin) {
    const message = 'wrangler not found; cannot start Cloudflare tail';
    writeJson(path.join(cfg.outDir, 'cf-worker-tail-error.json'), { message });
    writeJson(path.join(cfg.outDir, 'cf-observability-summary.json'), { tail_captured: false, error: message, worker_errors: 0, worker_exceptions: 0, youtube_api_errors: 0, d1_errors: 0, provider_health_polls: 0, session_log_writes: 0, session_log_write_errors: 0 });
    log('cf.tail_skipped', { message });
    return null;
  }
  const tryStart = async (args) => {
    const stdout = fs.createWriteStream(path.join(cfg.outDir, 'cf-worker-tail.ndjson'), { flags: 'a' });
    const stderr = fs.createWriteStream(path.join(cfg.outDir, 'cf-worker-tail.stderr.log'), { flags: 'a' });
    const child = spawn(bin, args, { cwd: ROOT, env: process.env, stdio: ['ignore', 'pipe', 'pipe'] });
    child.stdout.pipe(stdout);
    child.stderr.pipe(stderr);
    const early = await waitForExit(child, 4000);
    if (early) {
      stdout.end();
      stderr.end();
      return { child, stdout, stderr, exited: early };
    }
    return { child, stdout, stderr, args };
  };
  let tail = await tryStart(['tail', cfg.cloudflareWorker, '--format=json', '--sampling-rate=1']);
  if (tail.exited && tail.exited.code !== 0) {
    log('cf.tail_sampling_failed_retrying', { code: tail.exited.code, signal: tail.exited.signal });
    tail = await tryStart(['tail', cfg.cloudflareWorker, '--format=json']);
  }
  if (tail.exited && tail.exited.code !== 0) {
    const message = `wrangler tail exited early with code ${tail.exited.code}`;
    writeJson(path.join(cfg.outDir, 'cf-worker-tail-error.json'), { message });
    writeJson(path.join(cfg.outDir, 'cf-observability-summary.json'), { tail_captured: false, error: message, worker_errors: 0, worker_exceptions: 0, youtube_api_errors: 0, d1_errors: 0, provider_health_polls: 0, session_log_writes: 0, session_log_write_errors: 0 });
    log('cf.tail_failed', { message });
    return null;
  }
  log('cf.tail_started', { worker: cfg.cloudflareWorker, args: tail.args });
  return tail;
}

async function stopCloudflareTail(tail) {
  if (!tail?.child || tail.child.exitCode !== null) return;
  tail.child.kill('SIGINT');
  await waitForExit(tail.child, 5000) || tail.child.kill('SIGKILL');
  tail.stdout?.end();
  tail.stderr?.end();
  log('cf.tail_stopped');
}

async function fetchCloudWatchLogs({ config: cfg, runStartMs }) {
  if (!cfg.fetchCloudWatch) {
    writeJson(path.join(cfg.outDir, 'cloudwatch-all.json'), { events: [], skipped: true });
    log('cloudwatch.skipped');
    return;
  }
  const start = String(Math.max(0, runStartMs - 60000));
  const end = String(Date.now() + 60000);
  try {
    const output = execFileSync('aws', [
      'logs', 'filter-log-events',
      '--region', cfg.awsRegion,
      '--log-group-name', cfg.cloudwatchGroup,
      '--start-time', start,
      '--end-time', end,
      '--output', 'json',
    ], { encoding: 'utf8', maxBuffer: 100 * 1024 * 1024 });
    fs.writeFileSync(path.join(cfg.outDir, 'cloudwatch-all.json'), output);
    log('cloudwatch.fetched', { group: cfg.cloudwatchGroup, region: cfg.awsRegion });
  } catch (error) {
    writeJson(path.join(cfg.outDir, 'cloudwatch-error.json'), { message: error.message, group: cfg.cloudwatchGroup, region: cfg.awsRegion });
    log('cloudwatch.fetch_failed', { message: error.message, group: cfg.cloudwatchGroup, region: cfg.awsRegion });
  }
}

async function fetchYoutubeVodMetadata(streams, cfg) {
  const artifact = { attempted: false, streams: {}, generated_at: new Date().toISOString() };
  const analysis = { attempted: false, streams: {}, generated_at: new Date().toISOString() };
  if (!cfg.fetchYoutubeVod) {
    artifact.skipped = true;
    analysis.skipped = true;
    writeJson(path.join(cfg.outDir, 'youtube-vod-metadata.json'), artifact);
    writeJson(path.join(cfg.outDir, 'youtube-vod-analysis.json'), analysis);
    return;
  }
  if (!commandExists('yt-dlp')) {
    artifact.reason = 'yt-dlp not found';
    analysis.reason = 'yt-dlp not found';
    writeJson(path.join(cfg.outDir, 'youtube-vod-metadata.json'), artifact);
    writeJson(path.join(cfg.outDir, 'youtube-vod-analysis.json'), analysis);
    log('youtube.vod_metadata_skipped', { reason: artifact.reason });
    return;
  }
  artifact.attempted = true;
  analysis.attempted = true;
  for (const stream of streams.filter((s) => s.watch_url)) {
    try {
      const output = execFileSync('yt-dlp', ['--dump-json', '--skip-download', '--no-warnings', stream.watch_url], { encoding: 'utf8', timeout: 120000, maxBuffer: 20 * 1024 * 1024 });
      const meta = parseJson(output.trim().split('\n').at(-1));
      artifact.streams[stream.id] = redactObject(meta ?? { raw: output.slice(0, 2000) });
      analysis.streams[stream.id] = summarizeYtdlp(meta);
    } catch (error) {
      artifact.streams[stream.id] = { error: error.message };
      analysis.streams[stream.id] = { error: error.message };
    }
  }
  writeJson(path.join(cfg.outDir, 'youtube-vod-metadata.json'), artifact);
  writeJson(path.join(cfg.outDir, 'youtube-vod-analysis.json'), analysis);
}

function summarizeYtdlp(meta) {
  if (!meta) return null;
  return {
    id: meta.id,
    title: meta.title,
    duration: meta.duration,
    width: meta.width,
    height: meta.height,
    fps: meta.fps,
    live_status: meta.live_status,
    availability: meta.availability,
  };
}

async function savePageVideos(items, outDir) {
  for (const { page, target } of items) {
    try {
      const video = page.video?.();
      if (!video) continue;
      const source = await video.path();
      if (source && fs.existsSync(source)) fs.copyFileSync(source, path.join(outDir, target));
    } catch (error) {
      log('browser.video_save_failed', { target, message: error.message });
    }
  }
}

async function safeScreenshot(page, relativePath) {
  try {
    const target = path.join(config.outDir, relativePath);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    await page.screenshot({ path: target, fullPage: true, timeout: 10000 });
  } catch (error) {
    log('screenshot.failed', { path: relativePath, message: error.message });
  }
}

async function bodyText(page) {
  try {
    return await page.locator('body').innerText({ timeout: 3000 });
  } catch {
    return '';
  }
}

function extractSignals(text) {
  return text.match(/([^\n]*(Media connection|Provider health|ERR|error|ffmpeg|rtmp|Cannot load|speed=|decode_slice|utterance|Live|Recording|WebRTC|YouTube)[^\n]*)/ig)?.slice(0, 16) || [];
}

function extractWatchSignals(text) {
  return text.match(/([^\n]*(Live|Waiting|Started|watching|unavailable|offline|Playback|error|premiere|scheduled|chat|Share|Subscribe)[^\n]*)/ig)?.slice(0, 16) || [];
}

function resolveWranglerBin() {
  const candidates = [
    process.env.WRANGLER_BIN,
    path.join(ROOT, 'workers', 'node_modules', '.bin', 'wrangler'),
    path.join(ROOT, 'node_modules', '.bin', 'wrangler'),
  ].filter(Boolean);
  for (const candidate of candidates) if (fs.existsSync(candidate)) return candidate;
  return commandExists('wrangler') ? 'wrangler' : null;
}

function commandExists(cmd) {
  return spawnSync('bash', ['-lc', `command -v ${shellQuote(cmd)} >/dev/null 2>&1`], { stdio: 'ignore' }).status === 0;
}

function waitForExit(child, timeoutMs) {
  if (!child || child.exitCode !== null) return Promise.resolve({ code: child?.exitCode ?? 0, signal: child?.signalCode ?? null });
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      cleanup();
      resolve(null);
    }, timeoutMs);
    const onExit = (code, signal) => {
      cleanup();
      resolve({ code, signal });
    };
    const cleanup = () => {
      clearTimeout(timer);
      child.off('exit', onExit);
    };
    child.once('exit', onExit);
  });
}

function writeJson(file, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);
}

function writeJsonRedacted(file, value) {
  writeJson(file, redactObject(value));
}

function log(event, fields = {}) {
  const rec = redactObject({ t: new Date().toISOString(), ts_ms: Date.now(), event, ...fields });
  const line = JSON.stringify(rec);
  console.log(line);
  eventLog.write(`${line}\n`);
}

function redactObject(input) {
  if (Array.isArray(input)) return input.map(redactObject);
  if (!input || typeof input !== 'object') return typeof input === 'string' ? redactString(input) : input;
  const out = {};
  for (const [key, value] of Object.entries(input)) {
    if (/token|secret|authorization|password|credential|stream_key|rtmp_url/i.test(key)) out[key] = '[redacted]';
    else out[key] = redactObject(value);
  }
  return out;
}

function redactString(input) {
  return String(input)
    .replace(/(live2\/)[A-Za-z0-9_-]+/g, '$1<redacted>')
    .replace(/("stream_key"\s*:\s*")[^"]+/g, '$1<redacted>')
    .replace(/("rtmp_url"\s*:\s*")[^"]+/g, '$1<redacted>')
    .replace(/(token=)[A-Za-z0-9._-]+/gi, '$1<redacted>')
    .replace(/(Authorization:\s*Bearer\s+)[A-Za-z0-9._-]+/gi, '$1<redacted>');
}

function parseJson(text) {
  try { return JSON.parse(text); } catch { return null; }
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

function validateMediaIngestMode(value) {
  const mode = String(value || 'auto').trim().toLowerCase();
  if (['auto', 'webrtc', 'webcodecs_ws'].includes(mode)) return mode;
  throw new Error(`unsupported E2E_MEDIA_INGEST_MODE=${value}; use auto, webrtc, or webcodecs_ws`);
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
  return String(value ?? 'stream').replace(/[^a-zA-Z0-9_.-]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 120) || 'stream';
}

function contentTypeFor(file) {
  const ext = path.extname(file).toLowerCase();
  if (ext === '.webm') return 'video/webm';
  if (ext === '.mov') return 'video/quicktime';
  return 'video/mp4';
}

function escapeRegExp(value) {
  return String(value).replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function shellQuote(value) {
  return `'${String(value).replace(/'/g, `'\\''`)}'`;
}

function wait(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
