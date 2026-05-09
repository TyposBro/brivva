#!/usr/bin/env node
import fs from 'node:fs';
import https from 'node:https';
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
let cpuStress;
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
  writeGripOperatorNotes(config, streamRecords);
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
    if (stream.platform === 'grip') pageVideos.push({ page: watcher, target: `grip-live-evidence-${stream.artifactName}.webm` });
    await watcher.goto(stream.watch_url, { waitUntil: 'domcontentloaded', timeout: 60000 }).catch((error) => log('watch.goto_failed', { streamId: stream.id, platform: stream.platform, message: error.message }));
    await watcher.waitForTimeout(5000).catch(() => {});
    await watcher.mouse.click(720, 520).catch(() => {});
    await safeScreenshot(watcher, `screenshots/watch-before-${stream.artifactName}.png`);
    if (stream.platform === 'grip') await safeScreenshot(watcher, `grip-watch-before-${stream.artifactName}.png`);
    fs.writeFileSync(path.join(config.outDir, `watch-body-before-${stream.artifactName}.txt`), await bodyText(watcher));
    if (stream.platform === 'grip') fs.copyFileSync(path.join(config.outDir, `watch-body-before-${stream.artifactName}.txt`), path.join(config.outDir, `grip-body-before-${stream.artifactName}.txt`));
    watchers.push({ page: watcher, stream });
    log('watch.opened', { streamId: stream.id, kind: stream.kind, watchUrl: stream.watch_url });
  }

  await openHostSession(host, sessionId, config);
  const setupModeInfo = await assertSetupMediaIngestMode(host, config);
  Object.assign(config, setupModeInfo);
  writeMeta();
  await safeScreenshot(host, 'screenshots/setup.png');
  fs.writeFileSync(path.join(config.outDir, 'setup-body.txt'), await bodyText(host));
  await skipVoiceIfNeeded(host);
  await clickGoLive(host, sessionId);
  await assertRecordReadyForMode(host, config);
  const networkInfo = await applyNetworkProfile(host, config);
  Object.assign(config, networkInfo);
  cpuStress = startCpuStress(config);
  writeMeta();
  await safeScreenshot(host, 'screenshots/live-before-record.png');
  await clickRecord(host);
  const liveModeInfo = await assertLiveMediaIngestMode(host, config);
  Object.assign(config, liveModeInfo);
  writeMeta();
  log('record.started', { sessionId, durationSec: config.durationSec });

  monitor = startBrowserMonitor({ host, watchers, config });
  await wait(config.durationSec * 1000);
  await stopMonitor(monitor);
  if (cpuStress) {
    await stopCpuStress(cpuStress);
    cpuStress = null;
  }

  await safeScreenshot(host, 'screenshots/live-end.png');
  fs.writeFileSync(path.join(config.outDir, 'host-body-final.txt'), await bodyText(host));
  for (const { page, stream } of watchers) {
    await safeScreenshot(page, `screenshots/watch-end-${stream.artifactName}.png`);
    if (stream.platform === 'grip') await safeScreenshot(page, `grip-watch-end-${stream.artifactName}.png`);
    fs.writeFileSync(path.join(config.outDir, `watch-body-final-${stream.artifactName}.txt`), await bodyText(page));
    if (stream.platform === 'grip') fs.copyFileSync(path.join(config.outDir, `watch-body-final-${stream.artifactName}.txt`), path.join(config.outDir, `grip-body-final-${stream.artifactName}.txt`));
  }

  await clickStop(host);
  log('record.stopped', { sessionId });
  await waitForSessionEnded(sessionId, config);
  for (const { page, stream } of watchers) {
    await page.waitForTimeout(config.postStopWaitMs).catch(() => {});
    await page.reload({ waitUntil: 'domcontentloaded', timeout: 30000 }).catch(() => {});
    await page.waitForTimeout(3000).catch(() => {});
    await safeScreenshot(page, `screenshots/watch-poststop-${stream.artifactName}.png`);
    if (stream.platform === 'grip') await safeScreenshot(page, `grip-watch-poststop-${stream.artifactName}.png`);
    fs.writeFileSync(path.join(config.outDir, `watch-body-poststop-${stream.artifactName}.txt`), await bodyText(page));
    if (stream.platform === 'grip') fs.copyFileSync(path.join(config.outDir, `watch-body-poststop-${stream.artifactName}.txt`), path.join(config.outDir, `grip-body-poststop-${stream.artifactName}.txt`));
  }
} catch (error) {
  failed = true;
  finalError = error;
  log('run.error', { message: error.message, stack: error.stack });
} finally {
  config.failed = failed;
  if (finalError) config.error = finalError.message;
  if (monitor) await stopMonitor(monitor).catch(() => {});
  if (cpuStress) await stopCpuStress(cpuStress).catch(() => {});
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
  const requestedMediaIngestMode = validateMediaIngestMode(process.env.E2E_MEDIA_INGEST_MODE || process.env.MEDIA_INGEST_MODE || 'auto');
  const resolvedMediaIngestMode = resolveMediaIngestMode(requestedMediaIngestMode);
  const runId = safeName(process.env.E2E_RUN_ID || `${formatRunDate(startMs)}-${browser}-${shape}-${requestedMediaIngestMode}`);
  const artifactRoot = process.env.E2E_ARTIFACT_ROOT || path.join(ROOT, 'tmp', 'prod-media-stress-runs');
  const outDir = process.env.E2E_OUT_DIR ? path.resolve(expandHome(process.env.E2E_OUT_DIR)) : path.join(artifactRoot, runId);
  const platformMatrix = parseCsv(process.env.E2E_PLATFORM_MATRIX || 'youtube').map(validatePlatform);
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
    mediaIngestMode: resolvedMediaIngestMode,
    requestedMediaIngestMode,
    resolvedMediaIngestMode,
    platformMatrix,
    networkProfile: validateNetworkProfile(process.env.E2E_NETWORK_PROFILE || 'normal'),
    cpuStressEnabled: bool(process.env.E2E_CPU_STRESS, false),
    cpuStressWorkers: number(process.env.E2E_CPU_STRESS_WORKERS, Math.max(1, Math.floor(os.cpus().length / 2))),
    expectVideoDrops: bool(process.env.E2E_EXPECT_VIDEO_DROPS, false),
    maxWebCodecsBufferedMb: number(process.env.E2E_MAX_WEBCODECS_BUFFERED_MB, 4),
    maxWebCodecsQueueMs: number(process.env.E2E_MAX_WEBCODECS_QUEUE_MS, 500),
    maxWebCodecsDropRate: number(process.env.E2E_MAX_WEBCODECS_DROP_RATE, 0.001),
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
    gripRtmpUrl: process.env.E2E_GRIP_RTMP_URL || '',
    gripStreamKey: process.env.E2E_GRIP_STREAM_KEY || '',
    gripProductId: process.env.E2E_GRIP_PRODUCT_ID || '',
    gripWatchUrl: process.env.E2E_GRIP_WATCH_URL || '',
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
    requestedMediaIngestMode: config.requestedMediaIngestMode,
    resolvedMediaIngestMode: config.resolvedMediaIngestMode,
    activeMediaIngestMode: config.activeMediaIngestMode ?? null,
    webCodecsFrontendEnabled: config.webCodecsFrontendEnabled ?? null,
    webCodecsBrowserSupported: config.webCodecsBrowserSupported ?? null,
    webCodecsServerAdvertised: config.webCodecsServerAdvertised ?? null,
    platformMatrix: config.platformMatrix,
    networkProfile: config.networkProfile,
    networkThrottle: config.networkThrottle ?? null,
    cpuStressEnabled: config.cpuStressEnabled,
    cpuStressWorkers: config.cpuStressWorkers,
    cpuStressActive: config.cpuStressActive ?? false,
    expectVideoDrops: config.expectVideoDrops,
    maxWebCodecsBufferedMb: config.maxWebCodecsBufferedMb,
    maxWebCodecsQueueMs: config.maxWebCodecsQueueMs,
    maxWebCodecsDropRate: config.maxWebCodecsDropRate,
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
  const destinations = buildStreamDestinations(cfg);
  const platforms = [];
  for (const platform of cfg.platformMatrix) {
    destinations.forEach((destination, index) => {
      if (platform === 'youtube') {
        platforms.push({
          platform: 'youtube',
          lang: destination.lang,
          delay_ms: destination.delay_ms,
          host_gain: destination.host_gain,
        });
        return;
      }
      if (platform === 'grip') {
        platforms.push(buildGripPlatform(cfg, destination, index, destinations.length));
        return;
      }
      throw new Error(`unsupported platform=${platform}`);
    });
  }
  return platforms;
}

function buildStreamDestinations(cfg) {
  if (cfg.shape === 'source') {
    return [{ kind: 'source', lang: cfg.sourceLang, delay_ms: 0, host_gain: 1.0 }];
  }
  if (cfg.shape === 'passthrough') {
    return [{ kind: 'passthrough', lang: 'pass', delay_ms: 0, host_gain: 1.0 }];
  }
  if (cfg.shape === 'translated') {
    return [{ kind: 'translated', lang: cfg.targetLang, delay_ms: cfg.youtubeDelayMs, host_gain: cfg.translatedHostGain }];
  }
  if (cfg.shape === 'dual') {
    return [
      { kind: 'source', lang: cfg.sourceLang, delay_ms: 0, host_gain: 1.0 },
      { kind: 'translated', lang: cfg.targetLang, delay_ms: cfg.youtubeDelayMs, host_gain: cfg.translatedHostGain },
    ];
  }
  if (cfg.shape === 'translated-pass' || cfg.shape === 'pass-translated') {
    return [
      { kind: 'passthrough', lang: 'pass', delay_ms: 0, host_gain: 1.0 },
      { kind: 'translated', lang: cfg.targetLang, delay_ms: cfg.youtubeDelayMs, host_gain: cfg.translatedHostGain },
    ];
  }
  throw new Error(`unsupported E2E_TEST_SHAPE=${cfg.shape}; use source, passthrough, translated, dual, or translated-pass`);
}

function buildGripPlatform(cfg, destination, index, gripDestinationCount) {
  const suffixes = gripEnvSuffixes(destination, index);
  const productId = firstEnv([...suffixes.map((suffix) => `E2E_GRIP_PRODUCT_ID_${suffix}`), 'E2E_GRIP_PRODUCT_ID']) || cfg.gripProductId;
  if (productId) {
    return {
      platform: 'grip',
      lang: destination.lang,
      product_id: productId,
      delay_ms: destination.delay_ms,
      host_gain: destination.host_gain,
    };
  }
  const rtmpUrl = firstEnv([...suffixes.map((suffix) => `E2E_GRIP_RTMP_URL_${suffix}`), gripDestinationCount === 1 ? 'E2E_GRIP_RTMP_URL' : '']);
  const streamKey = firstEnv([...suffixes.map((suffix) => `E2E_GRIP_STREAM_KEY_${suffix}`), gripDestinationCount === 1 ? 'E2E_GRIP_STREAM_KEY' : '']);
  if (!rtmpUrl || !streamKey) {
    throw new Error(`E2E_PLATFORM_MATRIX includes grip ${destination.kind}/${destination.lang}, but Grip credentials are missing. Set E2E_GRIP_RTMP_URL/E2E_GRIP_STREAM_KEY for one Grip stream, per-destination E2E_GRIP_RTMP_URL_${suffixes[0]}/E2E_GRIP_STREAM_KEY_${suffixes[0]}, or E2E_GRIP_PRODUCT_ID.`);
  }
  return {
    platform: 'grip',
    lang: destination.lang,
    rtmp_url: rtmpUrl,
    stream_key: streamKey,
    delay_ms: destination.delay_ms,
    host_gain: destination.host_gain,
  };
}

function gripEnvSuffixes(destination, index) {
  const lang = String(destination.lang || '').toUpperCase().replace(/[^A-Z0-9]+/g, '_');
  const kind = String(destination.kind || '').toUpperCase().replace(/[^A-Z0-9]+/g, '_');
  return [`${index}`, kind, lang].filter(Boolean);
}

function firstEnv(names) {
  for (const name of names.filter(Boolean)) {
    const value = process.env[name];
    if (value) return value;
  }
  return '';
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
      watch_url: stream.watch_url || gripWatchUrlFor(stream, index) || (stream.platform_broadcast_id ? `https://www.youtube.com/watch?v=${stream.platform_broadcast_id}` : null),
    };
  });
}

function writeGripOperatorNotes(cfg, streams) {
  const gripStreams = streams.filter((stream) => stream.platform === 'grip');
  if (!gripStreams.length) return;
  const lines = [
    '# Grip Operator Evidence',
    '',
    `Run: ${cfg.runId}`,
    `Session: ${cfg.sessionId ?? 'unknown'}`,
    '',
    'Fill this during/after the run if Grip API/watch automation cannot prove live state.',
    '',
    '| Stream | Kind | Lang | Watch URL | Operator live? | Notes |',
    '| --- | --- | --- | --- | --- | --- |',
    ...gripStreams.map((stream) => `| ${stream.id} | ${stream.kind} | ${stream.lang ?? ''} | ${stream.watch_url ?? ''} |  |  |`),
    '',
    'Evidence accepted by analyzer: Grip watch/seller page body/screenshots/video, `grip-provider-health.ndjson`, or this file edited with operator live confirmation.',
  ];
  fs.writeFileSync(path.join(cfg.outDir, 'grip-operator-notes.md'), `${lines.join('\n')}\n`);
}

function gripWatchUrlFor(stream, index) {
  if (stream.platform !== 'grip') return null;
  const kind = classifyKind(stream, config);
  const suffixes = gripEnvSuffixes({ kind, lang: stream.lang }, index);
  return firstEnv([...suffixes.map((suffix) => `E2E_GRIP_WATCH_URL_${suffix}`), 'E2E_GRIP_WATCH_URL']) || null;
}

function classifyKind(stream, cfg) {
  const delay = Number(stream.delay_ms);
  const gain = Number(stream.host_gain);
  if (stream.lang === 'pass') return 'passthrough';
  if (stream.lang === cfg.sourceLang && (delay === 0 || gain === 1)) return 'source';
  return 'translated';
}

async function openHostSession(host, sessionId, cfg) {
  await host.goto(`${cfg.frontend}/?user_id=${encodeURIComponent(cfg.userId)}#token=${encodeURIComponent(cfg.authToken)}`, { waitUntil: 'domcontentloaded', timeout: 30000 });
  await host.waitForURL(/\/dashboard\b/, { timeout: 30000 }).catch(() => {});
  await host.evaluate((mode) => {
    localStorage.setItem('brivva:sessionLogs', '1');
    localStorage.setItem('brivva:mediaIngestMode', mode);
  }, cfg.requestedMediaIngestMode);
  await host.goto(`${cfg.frontend}/session/${sessionId}/setup`, { waitUntil: 'domcontentloaded', timeout: 30000 });
}

async function assertSetupMediaIngestMode(host, cfg) {
  await host.locator('input[name="media-ingest-mode"]').first().waitFor({ state: 'attached', timeout: 30000 });
  const info = await host.evaluate((requested) => {
    const input = document.querySelector(`input[name="media-ingest-mode"][value="${requested}"]`);
    const webcodecs = document.querySelector('input[name="media-ingest-mode"][value="webcodecs_ws"]');
    const body = document.body?.innerText || '';
    const runtime = window;
    return {
      localStorageMode: localStorage.getItem('brivva:mediaIngestMode'),
      requestedChecked: Boolean(input && input.checked),
      requestedDisabled: Boolean(input && input.disabled),
      webCodecsRadioDisabled: Boolean(webcodecs && webcodecs.disabled),
      webCodecsBrowserSupported: typeof runtime.VideoEncoder !== 'undefined' && typeof runtime.VideoFrame !== 'undefined' && typeof runtime.MediaStreamTrackProcessor !== 'undefined',
      frontendFlagOff: /VITE_WEBCODECS_INGEST_ENABLED is off/i.test(body),
      bodySample: body.slice(0, 2000),
    };
  }, cfg.requestedMediaIngestMode);
  if (info.localStorageMode !== cfg.requestedMediaIngestMode) {
    throw new Error(`media ingest localStorage mismatch: expected ${cfg.requestedMediaIngestMode}, got ${info.localStorageMode}`);
  }
  if (!info.requestedChecked) {
    throw new Error(`setup UI did not select requested media ingest mode ${cfg.requestedMediaIngestMode}`);
  }
  if (cfg.requestedMediaIngestMode === 'webcodecs_ws' && info.requestedDisabled) {
    throw new Error(`explicit WebCodecs requested but setup UI disabled it: ${info.bodySample}`);
  }
  log('mode.setup_asserted', {
    requested: cfg.requestedMediaIngestMode,
    resolved: cfg.resolvedMediaIngestMode,
    webCodecsRadioDisabled: info.webCodecsRadioDisabled,
    webCodecsBrowserSupported: info.webCodecsBrowserSupported,
    frontendFlagOff: info.frontendFlagOff,
  });
  return {
    setupMediaIngestMode: cfg.requestedMediaIngestMode,
    webCodecsFrontendEnabled: !info.frontendFlagOff,
    webCodecsBrowserSupported: info.webCodecsBrowserSupported,
  };
}

async function assertRecordReadyForMode(host, cfg) {
  const record = host.getByRole('button', { name: /^Record$/i });
  await record.waitFor({ state: 'visible', timeout: 60000 });
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    if (!(await record.isDisabled().catch(() => true))) {
      log('mode.record_ready', { requested: cfg.requestedMediaIngestMode, resolved: cfg.resolvedMediaIngestMode });
      return;
    }
    await wait(1000);
  }
  const body = await bodyText(host);
  throw new Error(`Record stayed disabled for requested media ingest mode ${cfg.requestedMediaIngestMode}: ${body.slice(0, 2000)}`);
}

async function assertLiveMediaIngestMode(host, cfg) {
  const expectedLabel = cfg.resolvedMediaIngestMode === 'webcodecs_ws' ? 'WebCodecs over WebSocket' : 'WebRTC';
  const deadline = Date.now() + 20000;
  let body = '';
  while (Date.now() < deadline) {
    body = await bodyText(host);
    if (body.includes('Media ingest') && body.includes(expectedLabel)) {
      log('mode.live_asserted', { requested: cfg.requestedMediaIngestMode, resolved: cfg.resolvedMediaIngestMode, expectedLabel });
      return { activeMediaIngestMode: cfg.resolvedMediaIngestMode };
    }
    await wait(1000);
  }
  throw new Error(`live diagnostics did not show ${expectedLabel}: ${body.slice(0, 2000)}`);
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

async function applyNetworkProfile(page, cfg) {
  if (cfg.networkProfile === 'normal') return { networkThrottle: { applied: false, profile: 'normal' } };
  if (!['chromium', 'brave'].includes(cfg.browser)) {
    const throttle = { applied: false, profile: cfg.networkProfile, reason: 'CDP network emulation is Chromium/Brave-only' };
    writeJson(path.join(cfg.outDir, 'network-throttle.json'), throttle);
    log('network.throttle_skipped', throttle);
    return { networkThrottle: throttle };
  }
  const profiles = {
    moderate_uplink: { downloadThroughput: 5_000_000 / 8, uploadThroughput: 2_200_000 / 8, latency: 60 },
    severe_uplink: { downloadThroughput: 3_000_000 / 8, uploadThroughput: 1_200_000 / 8, latency: 120 },
  };
  const selected = profiles[cfg.networkProfile];
  const session = await page.context().newCDPSession(page);
  await session.send('Network.enable');
  await session.send('Network.emulateNetworkConditions', {
    offline: false,
    latency: selected.latency,
    downloadThroughput: selected.downloadThroughput,
    uploadThroughput: selected.uploadThroughput,
    connectionType: 'cellular3g',
  });
  const throttle = { applied: true, profile: cfg.networkProfile, ...selected };
  writeJson(path.join(cfg.outDir, 'network-throttle.json'), throttle);
  log('network.throttle_applied', throttle);
  return { networkThrottle: throttle };
}

function startCpuStress(cfg) {
  if (!cfg.cpuStressEnabled) return null;
  const workers = Math.max(1, Math.min(64, Math.floor(cfg.cpuStressWorkers || 1)));
  const children = [];
  const code = 'let x=0; setInterval(()=>{}, 1000); while (true) { x = (x + Math.random()) % 1000000 }';
  for (let i = 0; i < workers; i++) {
    children.push(spawn(process.execPath, ['-e', code], { stdio: 'ignore' }));
  }
  const info = { enabled: true, workers, pids: children.map((child) => child.pid).filter(Boolean), started_at: new Date().toISOString() };
  cfg.cpuStressActive = true;
  writeJson(path.join(cfg.outDir, 'cpu-stress.json'), info);
  log('cpu_stress.started', info);
  return { children, info };
}

async function stopCpuStress(handle) {
  for (const child of handle.children) {
    if (child.exitCode === null) child.kill('SIGTERM');
  }
  await Promise.all(handle.children.map((child) => waitForExit(child, 2000).then((exit) => {
    if (!exit && child.exitCode === null) child.kill('SIGKILL');
  })));
  config.cpuStressActive = false;
  writeJson(path.join(config.outDir, 'cpu-stress.json'), { ...handle.info, stopped_at: new Date().toISOString() });
  log('cpu_stress.stopped', { workers: handle.info.workers });
}

function chromiumArgs(fakeDevice) {
  const args = [
    '--autoplay-policy=no-user-gesture-required',
    '--disable-background-timer-throttling',
    '--disable-backgrounding-occluded-windows',
    '--disable-renderer-backgrounding',
    '--disable-web-security',
    '--allow-running-insecure-content',
    '--allow-insecure-localhost',
    '--disable-features=CalculateNativeWinOcclusion,IntensiveWakeUpThrottling,BlockInsecurePrivateNetworkRequests,PrivateNetworkAccessSendPreflights,PrivateNetworkAccessRespectPreflightResults',
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

function privateNetworkCorsHeaders() {
  return {
    'Access-Control-Allow-Origin': '*',
    'Access-Control-Allow-Methods': 'GET, HEAD, OPTIONS',
    'Access-Control-Allow-Headers': 'Range, Content-Type, Access-Control-Request-Private-Network',
    'Access-Control-Allow-Private-Network': 'true',
  };
}

function createFixtureTlsOptions() {
  const certDir = path.join(config.outDir, 'fixture-tls');
  fs.mkdirSync(certDir, { recursive: true });
  const key = path.join(certDir, 'key.pem');
  const cert = path.join(certDir, 'cert.pem');
  if (!fs.existsSync(key) || !fs.existsSync(cert)) {
    execFileSync('openssl', [
      'req',
      '-x509',
      '-newkey',
      'rsa:2048',
      '-nodes',
      '-keyout',
      key,
      '-out',
      cert,
      '-sha256',
      '-days',
      '1',
      '-subj',
      '/CN=127.0.0.1',
      '-addext',
      'subjectAltName=IP:127.0.0.1,DNS:localhost',
    ], { stdio: ['ignore', 'ignore', 'pipe'] });
  }
  return { key: fs.readFileSync(key), cert: fs.readFileSync(cert) };
}

async function startFixtureServer(file) {
  const stat = fs.statSync(file);
  const contentType = contentTypeFor(file);
  const tls = createFixtureTlsOptions();
  const server = https.createServer(tls, (req, res) => {
    const url = new URL(req.url || '/', 'https://127.0.0.1');
    if (url.pathname !== '/fixture') {
      res.writeHead(404).end('not found');
      return;
    }
    if (req.method === 'OPTIONS') {
      res.writeHead(204, privateNetworkCorsHeaders()).end();
      return;
    }
    const range = req.headers.range;
    for (const [name, value] of Object.entries(privateNetworkCorsHeaders())) res.setHeader(name, value);
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
      const clampedEnd = Math.min(end, stat.size - 1);
      res.writeHead(206, {
        'Content-Length': clampedEnd - start + 1,
        'Content-Range': `bytes ${start}-${clampedEnd}/${stat.size}`,
      });
      if (req.method === 'HEAD') {
        res.end();
        return;
      }
      fs.createReadStream(file, { start, end: clampedEnd }).pipe(res);
      return;
    }
    res.writeHead(200, { 'Content-Length': stat.size });
    if (req.method === 'HEAD') {
      res.end();
      return;
    }
    fs.createReadStream(file).pipe(res);
  });
  await new Promise((resolve, reject) => {
    server.on('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const address = server.address();
  return {
    url: `https://127.0.0.1:${address.port}/fixture`,
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

function resolveMediaIngestMode(requested) {
  return requested === 'webcodecs_ws' ? 'webcodecs_ws' : 'webrtc';
}

function validatePlatform(value) {
  const platform = String(value || '').trim().toLowerCase();
  if (['youtube', 'grip'].includes(platform)) return platform;
  throw new Error(`unsupported E2E_PLATFORM_MATRIX entry=${value}; use youtube, grip, or youtube,grip`);
}

function validateNetworkProfile(value) {
  const profile = String(value || 'normal').trim().toLowerCase();
  if (['normal', 'moderate_uplink', 'severe_uplink'].includes(profile)) return profile;
  throw new Error(`unsupported E2E_NETWORK_PROFILE=${value}; use normal, moderate_uplink, or severe_uplink`);
}

function parseCsv(value) {
  const items = String(value || '').split(',').map((item) => item.trim()).filter(Boolean);
  return items.length ? [...new Set(items)] : [];
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
