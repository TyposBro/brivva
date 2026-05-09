#!/usr/bin/env node
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const DIRECT = process.argv[1] && path.resolve(process.argv[1]) === __filename;

const HEALTH_RANK = new Map([
  ['good', 0],
  ['ok', 1],
  ['noData', 2],
  ['no_data', 2],
  ['checking', 2],
  ['unknown', 2],
  ['bad', 3],
  ['error', 4],
  ['failed', 4],
]);

export async function analyzeRun(runDirInput, options = {}) {
  const runDir = path.resolve(runDirInput);
  const meta = readJson(path.join(runDir, 'meta.json')) ?? {};
  const created = readJson(path.join(runDir, 'created.redacted.json')) ?? {};
  const watchUrls = readJson(path.join(runDir, 'watch-urls.json')) ?? { streams: [] };
  const runnerEvents = readNdjson(path.join(runDir, 'runner-events.ndjson'));
  const sessionLogs = readNdjson(path.join(runDir, 'session-logs.ndjson'));
  const providerSamples = readNdjson(path.join(runDir, 'provider-health.ndjson'));
  const wsFrames = readNdjson(path.join(runDir, 'host-ws.ndjson'));

  const sessionId = meta.sessionId ?? created?.session?.id ?? firstEventValue(runnerEvents, 'session.created', 'sessionId') ?? null;
  const liveSessionId = latestSessionSummary(runDir)?.live_session_id ?? latestSessionSummary(runDir)?.liveSessionId ?? findLiveSessionId(sessionLogs) ?? null;
  const streamsRaw = normalizeStreams(created, watchUrls, meta);
  const identifiers = collectIdentifiers(meta, created, streamsRaw, sessionId, liveSessionId);

  const cloudwatch = analyzeCloudWatch(runDir, identifiers);
  const cloudflare = analyzeCloudflare(runDir, meta);
  const frontend = analyzeFrontend(runDir, sessionLogs);
  const wsTiming = analyzeWebSocketTiming(wsFrames);
  const tts = analyzeTts(wsTiming, cloudwatch, meta);
  const provider = analyzeProviderSamples(providerSamples, meta, streamsRaw);
  const vod = readJson(path.join(runDir, 'youtube-vod-metadata.json')) ?? {};

  const streams = streamsRaw.map((stream, index) => {
    const providerStats = provider.byStream.get(stream.id) ?? provider.byIndex.get(index) ?? {};
    const vodMeta = vod?.streams?.[stream.id] ?? vod?.streams?.[stream.artifactName] ?? null;
    return {
      platform: stream.platform ?? 'youtube',
      kind: stream.kind,
      stream_id: stream.id,
      lang: stream.lang ?? null,
      watch_url: stream.watch_url ?? null,
      provider_confirmed_live_sec: round(providerStats.confirmedLiveSec ?? 0, 1),
      health_status_p95: providerStats.healthStatusP95 ?? null,
      stream_status_last: providerStats.streamStatusLast ?? null,
      provider_confirmed_live_ratio: round(providerStats.confirmedLiveRatio ?? 0, 4),
      vod_duration_sec: numberOrNull(vodMeta?.duration ?? vodMeta?.duration_sec),
      vod_width: numberOrNull(vodMeta?.width),
      vod_height: numberOrNull(vodMeta?.height),
      vod_fps: numberOrNull(vodMeta?.fps),
    };
  });

  const durationSec = Number(meta.durationSec ?? meta.duration_sec ?? process.env.E2E_RECORD_SECONDS ?? 0) || 0;
  const gates = buildGates({
    runDir,
    meta,
    runnerEvents,
    streams,
    frontend,
    cloudwatch,
    cloudflare,
    tts,
    durationSec,
    streamsRaw,
    sessionId,
  });
  const result = gates.every((g) => g.pass) ? 'pass' : 'fail';

  const summary = {
    result,
    run_id: meta.runId ?? meta.run_id ?? path.basename(runDir),
    browser: meta.browser ?? meta.e2eBrowser ?? null,
    headless: boolOrNull(meta.headless),
    shape: meta.shape ?? meta.testShape ?? null,
    media_ingest_mode: meta.mediaIngestMode ?? meta.media_ingest_mode ?? null,
    duration_sec: durationSec,
    session_id: sessionId,
    live_session_id: liveSessionId,
    streams,
    frontend,
    aws_media: cloudwatch.aws_media,
    tts,
    cloudflare,
    gates,
    artifacts_dir: path.relative(process.cwd(), runDir) || runDir,
  };

  writeJson(path.join(runDir, 'summary.json'), summary);
  writeVerdict(path.join(runDir, 'verdict.md'), summary);
  if (options.throwOnFail && result !== 'pass') {
    throw new Error(`prod media stress gates failed; see ${path.join(runDir, 'verdict.md')}`);
  }
  return summary;
}

export async function compareRuns(outDirInput, runDirInputs) {
  if (!runDirInputs.length) throw new Error('compare requires at least one run dir');
  const outDir = path.resolve(outDirInput);
  fs.mkdirSync(outDir, { recursive: true });
  const runs = [];
  for (const runDir of runDirInputs) {
    const summary = await analyzeRun(runDir);
    runs.push(summary);
  }
  const comparison = {
    result: runs.every((run) => run.result === 'pass') ? 'pass' : 'fail',
    generated_at: new Date().toISOString(),
    run_count: runs.length,
    modes: runs.map(compareRow),
  };
  writeJson(path.join(outDir, 'summary.json'), comparison);
  writeCompareVerdict(path.join(outDir, 'verdict.md'), comparison);
  return comparison;
}

function buildGates(ctx) {
  const { runDir, meta, runnerEvents, streams, frontend, cloudwatch, cloudflare, tts, durationSec, streamsRaw, sessionId } = ctx;
  const shape = String(meta.shape ?? meta.testShape ?? '').toLowerCase();
  const translatedShape = shape === 'translated' || shape === 'dual';
  const pollSeconds = Number(meta.providerPollSeconds ?? 10) || 10;
  const minLiveRatio = Number(meta.minProviderLiveRatio ?? process.env.E2E_MIN_PROVIDER_LIVE_RATIO ?? 0.9);
  const minSpeed = Number(meta.minFfmpegSpeed ?? process.env.E2E_MIN_FFMPEG_SPEED ?? 0.98);
  const minTtsCompletion = Number(meta.minTtsCompletionRatio ?? process.env.E2E_MIN_TTS_COMPLETION_RATIO ?? 0.95);
  const maxTtsDelay = Number(meta.maxTtsDelayP95Ms ?? process.env.E2E_MAX_TTS_DELAY_P95_MS ?? 10000);
  const maxTtsDrift = Number(meta.maxTtsDriftMsPerMin ?? process.env.E2E_MAX_TTS_DRIFT_MS_PER_MIN ?? 500);
  const allowOverflow = bool(meta.allowTtsOverflow ?? process.env.E2E_ALLOW_TTS_OVERFLOW, false);
  const fetchCloudWatch = bool(meta.fetchCloudWatch ?? meta.fetch_cloudwatch ?? process.env.E2E_FETCH_CLOUDWATCH, true);
  const fetchCf = bool(meta.fetchCloudflareObservability ?? meta.fetch_cf_observability ?? process.env.E2E_FETCH_CF_OBSERVABILITY, true);

  const record = recordingWindow(runnerEvents);
  const runnerError = meta.failed === true || runnerEvents.some((row) => row.event === 'run.error');
  const expectedStreamCount = streamsRaw.length;
  const allStreamsLiveEnough = streams.length > 0 && streams.every((stream) => stream.provider_confirmed_live_ratio >= minLiveRatio);
  const watchPagesOk = streamsRaw.length > 0 && streamsRaw.every((stream) => watchPageExists(runDir, stream));
  const dropTotal = cloudwatch.aws_media.video_stale_chunks_dropped
    + cloudwatch.aws_media.host_audio_stale_chunks_dropped
    + cloudwatch.aws_media.ready_host_bytes_dropped;
  const speedSamples = cloudwatch.aws_media.ffmpeg_speed_samples ?? 0;

  const gates = [];
  gates.push(gate('runner_completed_without_error', !runnerError, { failed: meta.failed === true, errors: runnerEvents.filter((row) => row.event === 'run.error').map((row) => row.message).filter(Boolean) }));
  gates.push(gate('session_created', Boolean(sessionId), { sessionId }));
  gates.push(gate('expected_streams_created', expectedStreamCount > 0, { expectedStreamCount }));
  gates.push(gate('browser_recorded_full_duration', record.durationSec >= durationSec * 0.95, { observed_sec: round(record.durationSec, 1), expected_sec: durationSec }));
  gates.push(gate('youtube_provider_confirmed_live_ratio', allStreamsLiveEnough, { min_ratio: minLiveRatio, poll_seconds: pollSeconds, streams: streams.map((s) => ({ stream_id: s.stream_id, ratio: s.provider_confirmed_live_ratio })) }));
  gates.push(gate('youtube_watch_or_vod_page_exists', watchPagesOk, { streams: streamsRaw.map((s) => ({ stream_id: s.id, artifact_name: s.artifactName })) }));
  if (fetchCloudWatch) {
    gates.push(gate('aws_cloudwatch_export_succeeded', cloudwatch.exportSucceeded, { error: cloudwatch.error ?? null }));
  }
  gates.push(gate('ffmpeg_no_restarts', cloudwatch.aws_media.ffmpeg_restarts === 0, { ffmpeg_restarts: cloudwatch.aws_media.ffmpeg_restarts }));
  gates.push(gate('ffmpeg_no_exits', cloudwatch.aws_media.ffmpeg_exit_count === 0, { ffmpeg_exit_count: cloudwatch.aws_media.ffmpeg_exit_count }));
  gates.push(gate('ffmpeg_speed_realtime_after_warmup', speedSamples > 0 && cloudwatch.aws_media.ffmpeg_speed_min_after_warmup >= minSpeed, { min_speed: minSpeed, observed: cloudwatch.aws_media.ffmpeg_speed_min_after_warmup, samples: speedSamples }));
  gates.push(gate('no_normal_run_media_stale_drops', dropTotal === 0, { video_stale_chunks_dropped: cloudwatch.aws_media.video_stale_chunks_dropped, host_audio_stale_chunks_dropped: cloudwatch.aws_media.host_audio_stale_chunks_dropped, ready_host_bytes_dropped: cloudwatch.aws_media.ready_host_bytes_dropped }));
  gates.push(gate('webrtc_no_failed_or_closed_before_stop', frontend.webrtc_disconnects === 0, { webrtc_disconnects: frontend.webrtc_disconnects }));
  if (String(meta.mediaIngestMode ?? meta.media_ingest_mode ?? '').toLowerCase() === 'webcodecs_ws') {
    gates.push(gate('webcodecs_no_failed_or_blocked_start', frontend.webcodecs_failures === 0, { webcodecs_failures: frontend.webcodecs_failures }));
    gates.push(gate('webcodecs_frames_reached_server', frontend.webcodecs_server_accepted_frames > 0, { server_accepted_frames: frontend.webcodecs_server_accepted_frames, sent_frames: frontend.webcodecs_sent_frames }));
  }
  if (translatedShape) {
    gates.push(gate('tts_completion_ratio', tts.utterances_source > 0 && tts.completion_ratio >= minTtsCompletion, { min_ratio: minTtsCompletion, observed: tts.completion_ratio, source: tts.utterances_source, completed: tts.utterances_tts_completed }));
    gates.push(gate('tts_delay_observed', tts.delay_samples > 0, { delay_samples: tts.delay_samples }));
    gates.push(gate('tts_delay_p95_under_threshold', tts.delay_ms_p95 !== null && tts.delay_ms_p95 <= maxTtsDelay, { max_ms: maxTtsDelay, observed_ms: tts.delay_ms_p95 }));
    gates.push(gate('tts_drift_under_threshold', tts.drift_ms_per_min !== null && Math.abs(tts.drift_ms_per_min) <= maxTtsDrift, { max_abs_ms_per_min: maxTtsDrift, observed_ms_per_min: tts.drift_ms_per_min }));
  }
  gates.push(gate('tts_segment_overflow_allowed', allowOverflow || tts.segment_overflows === 0, { segment_overflows: tts.segment_overflows, allow_overflow: allowOverflow }));
  if (fetchCf) {
    gates.push(gate('cloudflare_tail_captured', cloudflare.tail_captured === true, { tail_captured: cloudflare.tail_captured, error: cloudflare.error ?? null }));
  }
  gates.push(gate('cloudflare_worker_exceptions_zero', cloudflare.worker_exceptions === 0 && cloudflare.worker_errors === 0, { worker_errors: cloudflare.worker_errors, worker_exceptions: cloudflare.worker_exceptions }));
  gates.push(gate('cloudflare_d1_errors_zero', cloudflare.d1_errors === 0, { d1_errors: cloudflare.d1_errors }));
  gates.push(gate('cloudflare_youtube_api_errors_zero', cloudflare.youtube_api_errors === 0, { youtube_api_errors: cloudflare.youtube_api_errors }));
  gates.push(gate('cloudflare_session_log_write_errors_zero', cloudflare.session_log_write_errors === 0, { session_log_write_errors: cloudflare.session_log_write_errors }));
  return gates;
}

function gate(name, pass, details = {}) {
  return { name, pass: Boolean(pass), details };
}

function analyzeProviderSamples(samples, meta, streamsRaw) {
  const pollSeconds = Number(meta.providerPollSeconds ?? 10) || 10;
  const durationSec = Number(meta.durationSec ?? meta.duration_sec ?? 0) || 0;
  const byStream = new Map();
  const byIndex = new Map();

  for (const sample of samples) {
    const ts = Number(sample.ts_ms ?? Date.parse(sample.t ?? ''));
    const list = sample?.data?.streams ?? sample?.streams ?? [];
    if (!Number.isFinite(ts) || !Array.isArray(list)) continue;
    list.forEach((stream, index) => {
      const key = stream.streamId ?? stream.stream_id ?? stream.id ?? streamsRaw[index]?.id ?? String(index);
      const row = {
        ts,
        live: stream.providerConfirmedLive === true || stream.provider_confirmed_live === true,
        healthStatus: stream.healthStatus ?? stream.health_status ?? null,
        streamStatus: stream.streamStatus ?? stream.stream_status ?? null,
      };
      if (!byStream.has(key)) byStream.set(key, []);
      byStream.get(key).push(row);
      if (!byIndex.has(index)) byIndex.set(index, []);
      byIndex.get(index).push(row);
    });
  }

  const summarizedByStream = new Map();
  const summarizedByIndex = new Map();
  for (const [key, rows] of byStream) summarizedByStream.set(key, summarizeProviderRows(rows, durationSec, pollSeconds));
  for (const [key, rows] of byIndex) summarizedByIndex.set(key, summarizeProviderRows(rows, durationSec, pollSeconds));
  return { byStream: summarizedByStream, byIndex: summarizedByIndex };
}

function summarizeProviderRows(rows, durationSec, pollSeconds) {
  const sorted = [...rows].sort((a, b) => a.ts - b.ts);
  let liveMs = 0;
  for (let i = 0; i < sorted.length; i++) {
    if (!sorted[i].live) continue;
    const nextTs = sorted[i + 1]?.ts ?? sorted[i].ts + pollSeconds * 1000;
    liveMs += Math.max(0, Math.min(nextTs - sorted[i].ts, pollSeconds * 1500));
  }
  const health = sorted.map((row) => row.healthStatus).filter(Boolean);
  const ranked = health.sort((a, b) => healthRank(a) - healthRank(b));
  const idx = ranked.length ? Math.min(ranked.length - 1, Math.floor(ranked.length * 0.95)) : -1;
  return {
    confirmedLiveSec: liveMs / 1000,
    confirmedLiveRatio: durationSec > 0 ? liveMs / 1000 / durationSec : 0,
    healthStatusP95: idx >= 0 ? ranked[idx] : null,
    streamStatusLast: sorted.at(-1)?.streamStatus ?? null,
  };
}

function healthRank(status) {
  return HEALTH_RANK.get(String(status ?? 'unknown')) ?? 2;
}

function analyzeCloudWatch(runDir, identifiers) {
  const allPath = path.join(runDir, 'cloudwatch-all.json');
  const errorPath = path.join(runDir, 'cloudwatch-error.json');
  const raw = readJson(allPath);
  const exportSucceeded = Boolean(raw && Array.isArray(raw.events));
  const events = exportSucceeded ? raw.events : [];
  const filtered = filterCloudWatchEvents(events, identifiers);
  writeNdjson(path.join(runDir, 'cloudwatch-filtered.ndjson'), filtered.map((event) => ({
    timestamp: event.timestamp,
    logStreamName: event.logStreamName,
    message: event.message,
  })));

  const speedValues = [];
  let ffmpegRestarts = 0;
  let ffmpegExitCount = 0;
  let videoDrops = 0;
  let audioDrops = 0;
  let readyDrops = 0;
  let overflows = 0;
  let hardRecovery = 0;
  let ttsComplete = 0;
  let outputProfile = null;
  const messages = filtered.map((event) => String(event.message ?? ''));

  for (const message of messages) {
    const lower = message.toLowerCase();
    if (/ffmpeg/.test(lower) && /(restart|restarting|max ffmpeg restart|rtmp process crashed)/.test(lower)) ffmpegRestarts += 1;
    if (/(rtmp process crashed|ffmpeg[^\n]*(crash|failed)|exit_code=-(?:\d+)|exit_code=(?!0\b)\d+)/.test(lower) && !/intentional|stopped by operator|normal stop|session ended/.test(lower)) ffmpegExitCount += 1;
    for (const match of message.matchAll(/speed=\s*([0-9]+(?:\.[0-9]+)?)x/g)) speedValues.push(Number(match[1]));
    videoDrops += sumKeyValues(message, 'video_stale_chunks_dropped');
    audioDrops += sumKeyValues(message, 'host_audio_stale_chunks_dropped');
    readyDrops += sumKeyValues(message, 'ready_host_bytes_dropped');
    if (/tts segment queue overflow/i.test(message)) overflows += 1;
    if (/hard[_ -]?recovery|policy=hard_recovery|policy = hard_recovery/i.test(message)) hardRecovery += 1;
    if (/tts complete/i.test(message)) ttsComplete += 1;
    const profile = message.match(/(\d{3,4}x\d{3,4}[^"'\n]*(?:h264|nvenc|libx264)[^"'\n]*)/i)?.[1];
    if (profile) outputProfile = profile.trim();
  }

  const warmupCut = Math.min(3, Math.floor(speedValues.length / 10));
  const speedAfterWarmup = speedValues.slice(warmupCut);
  const speedMin = speedAfterWarmup.length ? Math.min(...speedAfterWarmup) : null;
  const summary = {
    ffmpeg_restarts: ffmpegRestarts,
    ffmpeg_exit_count: ffmpegExitCount,
    ffmpeg_speed_min_after_warmup: speedMin,
    ffmpeg_speed_p50: percentile(speedAfterWarmup, 0.5),
    ffmpeg_speed_p95: percentile(speedAfterWarmup, 0.95),
    ffmpeg_speed_samples: speedValues.length,
    video_stale_chunks_dropped: videoDrops,
    host_audio_stale_chunks_dropped: audioDrops,
    ready_host_bytes_dropped: readyDrops,
    output_profile: outputProfile,
    tts_complete_log_count: ttsComplete,
    tts_segment_overflows: overflows,
    tts_hard_recovery_count: hardRecovery,
  };
  return {
    exportSucceeded,
    error: readJson(errorPath)?.message ?? null,
    events: filtered,
    aws_media: summary,
  };
}

function filterCloudWatchEvents(events, identifiers) {
  const ids = [...identifiers].filter((id) => id && String(id).length >= 4).map(String);
  const keyword = /ffmpeg|webrtc|webcodecs|video_ingest|provider_health|provider health|tts|soniox|elevenlabs|rtmp|video_stale|host_audio_stale|ready_host|output\.live|output\.publishing|streamStatus|healthStatus/i;
  return events.filter((event) => {
    const message = String(event.message ?? '');
    if (!keyword.test(message)) return false;
    if (ids.length === 0) return true;
    return ids.some((id) => message.includes(id));
  });
}

function analyzeCloudflare(runDir, meta) {
  const tailPath = path.join(runDir, 'cf-worker-tail.ndjson');
  const summaryPath = path.join(runDir, 'cf-observability-summary.json');
  const existing = readJson(summaryPath);
  const lines = readLines(tailPath);
  let workerErrors = 0;
  let workerExceptions = 0;
  let youtubeApiErrors = 0;
  let d1Errors = 0;
  let providerHealthPolls = 0;
  let sessionLogWrites = 0;
  let sessionLogWriteErrors = 0;

  for (const line of lines) {
    const lower = line.toLowerCase();
    if (/provider-health/.test(lower)) providerHealthPolls += 1;
    if (/session[_ -]?logs?|session-log/.test(lower)) sessionLogWrites += 1;
    if (/exception|outcome["': ]+exception|uncaught/.test(lower)) workerExceptions += 1;
    if (/\berror\b|\bfailed\b/.test(lower)) workerErrors += 1;
    if (/youtube/.test(lower) && /(error|failed|401|403|429|500|502|503)/.test(lower)) youtubeApiErrors += 1;
    if (/\bd1\b|database|sqlite/.test(lower)) {
      if (/(error|failed|exception)/.test(lower)) d1Errors += 1;
    }
    if (/session[_ -]?logs?|session-log/.test(lower)) {
      if (/(error|failed|exception)/.test(lower)) sessionLogWriteErrors += 1;
    }
  }

  const summary = {
    worker_errors: existing?.worker_errors ?? workerErrors,
    worker_exceptions: existing?.worker_exceptions ?? workerExceptions,
    youtube_api_errors: existing?.youtube_api_errors ?? youtubeApiErrors,
    d1_errors: existing?.d1_errors ?? d1Errors,
    provider_health_polls: existing?.provider_health_polls ?? providerHealthPolls,
    session_log_writes: existing?.session_log_writes ?? sessionLogWrites,
    session_log_write_errors: existing?.session_log_write_errors ?? sessionLogWriteErrors,
    tail_captured: lines.length > 0 || existing?.tail_captured === true,
    error: existing?.error ?? readJson(path.join(runDir, 'cf-worker-tail-error.json'))?.message ?? null,
  };
  writeJson(summaryPath, summary);
  return summary;
}

function analyzeFrontend(runDir, sessionLogs) {
  const captureWidths = [];
  const captureHeights = [];
  const captureFps = [];
  const outboundWidths = [];
  const outboundHeights = [];
  const outboundFps = [];
  let disconnects = 0;
  let webcodecsFailures = 0;
  let webcodecsSentFrames = 0;
  let webcodecsDroppedFrames = 0;
  let webcodecsServerAcceptedFrames = 0;
  const webcodecsBufferedBytes = [];

  for (const event of sessionLogs) {
    const eventName = String(event.event ?? '');
    if (eventName === 'frontend.webrtc_issue') disconnects += 1;
    if (eventName === 'frontend.webcodecs_issue' || eventName === 'frontend.recording_start_blocked') webcodecsFailures += 1;
    if (eventName === 'frontend.ws_closed') continue;
    const stats = event?.fields?.stats;
    const webcodecs = stats?.webcodecsVideo ?? event?.fields?.stats;
    if (webcodecs && typeof webcodecs === 'object') {
      webcodecsSentFrames = Math.max(webcodecsSentFrames, Number(webcodecs.sentFrames ?? 0) || 0);
      webcodecsDroppedFrames = Math.max(webcodecsDroppedFrames, Number(webcodecs.droppedFrames ?? 0) || 0);
      webcodecsServerAcceptedFrames = Math.max(webcodecsServerAcceptedFrames, Number(webcodecs.serverAcceptedFrames ?? 0) || 0);
      pushNum(webcodecsBufferedBytes, webcodecs.wsBufferedBytes);
    }
    if (!stats || typeof stats !== 'object') continue;
    pushNum(captureWidths, stats?.sourceTrack?.width);
    pushNum(captureHeights, stats?.sourceTrack?.height);
    pushNum(captureFps, stats?.sourceTrack?.frameRate);
    pushNum(outboundWidths, stats?.outboundVideo?.frameWidth);
    pushNum(outboundHeights, stats?.outboundVideo?.frameHeight);
    pushNum(outboundFps, stats?.outboundVideo?.framesPerSecond);
  }

  const body = readText(path.join(runDir, 'host-body-final.txt')) || readText(path.join(runDir, 'host-latest-body.txt')) || '';
  const captureMatch = body.match(/Browser capture:\s*(\d+)\s*[×x]\s*(\d+)\s*@\s*([0-9.]+)/i);
  const outboundMatch = body.match(/WebRTC outbound:\s*(\d+)\s*[×x]\s*(\d+)\s*@\s*([0-9.]+)/i);
  if (captureMatch) {
    pushNum(captureWidths, captureMatch[1]);
    pushNum(captureHeights, captureMatch[2]);
    pushNum(captureFps, captureMatch[3]);
  }
  if (outboundMatch) {
    pushNum(outboundWidths, outboundMatch[1]);
    pushNum(outboundHeights, outboundMatch[2]);
    pushNum(outboundFps, outboundMatch[3]);
  }

  const consoleText = readText(path.join(runDir, 'host-console.log')) || '';
  disconnects += (consoleText.match(/webrtc[^\n]*(failed|closed|disconnected)/gi) ?? []).length;

  return {
    capture_width: percentile(captureWidths, 0.5),
    capture_height: percentile(captureHeights, 0.5),
    capture_fps_p50: percentile(captureFps, 0.5),
    outbound_width: percentile(outboundWidths, 0.5),
    outbound_height: percentile(outboundHeights, 0.5),
    outbound_fps_p50: percentile(outboundFps, 0.5),
    webrtc_disconnects: disconnects,
    webcodecs_failures: webcodecsFailures,
    webcodecs_sent_frames: webcodecsSentFrames,
    webcodecs_dropped_frames: webcodecsDroppedFrames,
    webcodecs_server_accepted_frames: webcodecsServerAcceptedFrames,
    webcodecs_ws_buffered_mb_p95: percentile(webcodecsBufferedBytes, 0.95) === null ? null : round(percentile(webcodecsBufferedBytes, 0.95) / 1024 / 1024, 3),
  };
}

function analyzeWebSocketTiming(frames) {
  const finals = new Map();
  const translations = new Map();
  const ttsEnds = [];
  const videoEnds = [];
  for (const frame of frames) {
    if (frame.direction !== 'received') continue;
    const msg = frame.json ?? parseJson(frame.payload);
    if (!msg || typeof msg !== 'object') continue;
    const ts = Number(frame.ts_ms ?? Date.parse(frame.t ?? ''));
    if (!Number.isFinite(ts)) continue;
    const utteranceId = String(msg.utteranceId ?? msg.utterance_id ?? '');
    if (!utteranceId) continue;
    if (msg.type === 'final') finals.set(utteranceId, ts);
    if (msg.type === 'translation') {
      const key = `${utteranceId}:${msg.targetLang ?? msg.target_lang ?? ''}`;
      translations.set(key, ts);
      if (!finals.has(utteranceId)) translations.set(utteranceId, ts);
    }
    if (msg.type === 'tts_end') ttsEnds.push({ ts, utteranceId, targetLang: msg.targetLang ?? msg.target_lang ?? null, ttsMs: numberOrNull(msg.ttsMs ?? msg.tts_ms) });
    if (msg.type === 'video_end') videoEnds.push({ ts, utteranceId });
  }
  const delays = [];
  for (const end of ttsEnds) {
    const key = `${end.utteranceId}:${end.targetLang ?? ''}`;
    const start = finals.get(end.utteranceId) ?? translations.get(key) ?? translations.get(end.utteranceId);
    if (Number.isFinite(start)) delays.push({ ts: end.ts, utteranceId: end.utteranceId, targetLang: end.targetLang, delayMs: end.ts - start, ttsMs: end.ttsMs });
  }
  delays.sort((a, b) => a.ts - b.ts);
  return { finals, translations, ttsEnds, videoEnds, delays };
}

function analyzeTts(wsTiming, cloudwatch, meta) {
  const delays = wsTiming.delays.map((row) => row.delayMs).filter((n) => Number.isFinite(n) && n >= 0);
  const sourceUtterances = Math.max(wsTiming.finals.size, uniqueUtteranceCount([...wsTiming.translations.keys()]));
  const completed = Math.max(wsTiming.ttsEnds.length, cloudwatch.aws_media.tts_complete_log_count ?? 0);
  const durationSec = Number(meta.durationSec ?? meta.duration_sec ?? 0) || 0;
  const drift = delays.length >= 2 && durationSec > 0
    ? (delays.at(-1) - delays[0]) / (durationSec / 60)
    : null;
  return {
    utterances_source: sourceUtterances,
    utterances_tts_completed: completed,
    completion_ratio: sourceUtterances > 0 ? round(completed / sourceUtterances, 4) : null,
    segment_overflows: cloudwatch.aws_media.tts_segment_overflows ?? 0,
    hard_recovery: cloudwatch.aws_media.tts_hard_recovery_count ?? 0,
    delay_samples: delays.length,
    delay_ms_p50: percentile(delays, 0.5),
    delay_ms_p95: percentile(delays, 0.95),
    drift_ms_per_min: drift === null ? null : round(drift, 1),
  };
}

function normalizeStreams(created, watchUrls, meta) {
  const raw = Array.isArray(created?.streams) ? created.streams : [];
  const fromWatch = Array.isArray(watchUrls?.streams) ? watchUrls.streams : [];
  const merged = raw.length ? raw : fromWatch;
  return merged.map((stream, index) => {
    const watch = fromWatch.find((item) => item.stream_id === stream.id || item.id === stream.id) ?? {};
    const id = stream.id ?? stream.stream_id ?? watch.stream_id ?? `stream-${index}`;
    const lang = stream.lang ?? watch.lang ?? null;
    return {
      id,
      artifactName: safeName(watch.artifact_name ?? `${index}-${id}`),
      platform: stream.platform ?? watch.platform ?? 'youtube',
      lang,
      delay_ms: stream.delay_ms ?? watch.delay_ms ?? null,
      host_gain: stream.host_gain ?? watch.host_gain ?? null,
      watch_url: stream.watch_url ?? watch.watch_url ?? (stream.platform_broadcast_id ? `https://www.youtube.com/watch?v=${stream.platform_broadcast_id}` : null),
      kind: classifyStreamKind(stream, watch, meta),
      platform_broadcast_id: stream.platform_broadcast_id ?? watch.platform_broadcast_id ?? null,
      platform_stream_id: stream.platform_stream_id ?? watch.platform_stream_id ?? null,
    };
  });
}

function classifyStreamKind(stream, watch, meta) {
  if (watch.kind) return watch.kind;
  if (stream.kind) return stream.kind;
  const sourceLang = meta.sourceLang ?? meta.source_lang;
  const gain = Number(stream.host_gain ?? watch.host_gain);
  const delay = Number(stream.delay_ms ?? watch.delay_ms);
  if ((stream.lang === sourceLang && (delay === 0 || !Number.isFinite(delay))) || gain === 1) return 'source';
  return 'translated';
}

function collectIdentifiers(meta, created, streams, sessionId, liveSessionId) {
  const ids = new Set([meta.runId, meta.run_id, sessionId, liveSessionId].filter(Boolean));
  for (const stream of streams) {
    ids.add(stream.id);
    ids.add(stream.platform_broadcast_id);
    ids.add(stream.platform_stream_id);
  }
  if (created?.session?.id) ids.add(created.session.id);
  return ids;
}

function latestSessionSummary(runDir) {
  return readJson(path.join(runDir, 'session-summary.json'))
    ?? readJson(path.join(runDir, 'summary-latest.json'))
    ?? latestSampleData(readNdjson(path.join(runDir, 'session-summary-samples.ndjson')));
}

function latestSampleData(samples) {
  const last = samples.filter((s) => s.ok !== false).at(-1);
  return last?.data ?? last ?? null;
}

function findLiveSessionId(sessionLogs) {
  return sessionLogs.find((row) => row.live_session_id)?.live_session_id ?? null;
}

function firstEventValue(events, event, key) {
  return events.find((row) => row.event === event)?.[key] ?? null;
}

function recordingWindow(events) {
  const started = events.find((row) => row.event === 'record.started')?.ts_ms;
  const stopped = events.find((row) => row.event === 'record.stopped')?.ts_ms;
  if (!Number.isFinite(started) || !Number.isFinite(stopped)) return { durationSec: 0 };
  return { durationSec: Math.max(0, (stopped - started) / 1000) };
}

function watchPageExists(runDir, stream) {
  const names = [
    `watch-body-poststop-${stream.artifactName}.txt`,
    `watch-body-final-${stream.artifactName}.txt`,
    `watch-body-final-${stream.id}.txt`,
    'watch-body-poststop.txt',
    'watch-body-final.txt',
  ];
  for (const name of names) {
    const body = readText(path.join(runDir, name));
    if (!body) continue;
    if (/video unavailable|this video is unavailable|offline|removed|private video/i.test(body)) return false;
    if (/youtube|live|watch|chat|views|share|subscribe/i.test(body)) return true;
    if (body.trim().length > 50) return true;
  }
  return false;
}

function uniqueUtteranceCount(keys) {
  const ids = new Set(keys.map((key) => String(key).split(':')[0]).filter(Boolean));
  return ids.size;
}

function sumKeyValues(message, key) {
  let total = 0;
  for (const match of message.matchAll(new RegExp(`${key}\\D+([0-9]+)`, 'g'))) total += Number(match[1]);
  return total;
}

function percentile(values, p) {
  const nums = values.map(Number).filter((n) => Number.isFinite(n));
  if (!nums.length) return null;
  nums.sort((a, b) => a - b);
  const idx = Math.min(nums.length - 1, Math.max(0, Math.ceil(nums.length * p) - 1));
  return round(nums[idx], 3);
}

function pushNum(list, value) {
  const n = Number(value);
  if (Number.isFinite(n)) list.push(n);
}

function numberOrNull(value) {
  const n = Number(value);
  return Number.isFinite(n) ? n : null;
}

function bool(value, fallback = false) {
  if (typeof value === 'boolean') return value;
  if (value === undefined || value === null || value === '') return fallback;
  return !['0', 'false', 'no', 'off'].includes(String(value).toLowerCase());
}

function boolOrNull(value) {
  if (value === undefined || value === null) return null;
  return bool(value);
}

function round(value, digits = 0) {
  if (value === null || value === undefined || !Number.isFinite(Number(value))) return null;
  const factor = 10 ** digits;
  return Math.round(Number(value) * factor) / factor;
}

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch {
    return null;
  }
}

function parseJson(text) {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

function readText(file) {
  try {
    return fs.readFileSync(file, 'utf8');
  } catch {
    return '';
  }
}

function readLines(file) {
  const text = readText(file);
  return text ? text.split(/\r?\n/).filter(Boolean) : [];
}

function readNdjson(file) {
  return readLines(file).map((line) => parseJson(line)).filter(Boolean);
}

function writeJson(file, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);
}

function writeNdjson(file, rows) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, rows.map((row) => JSON.stringify(row)).join('\n') + (rows.length ? '\n' : ''));
}

function compareRow(run) {
  const ratios = run.streams.map((stream) => stream.provider_confirmed_live_ratio ?? 0);
  return {
    run_id: run.run_id,
    media_ingest_mode: run.media_ingest_mode ?? 'unknown',
    browser: run.browser,
    result: run.result,
    provider_confirmed_live_ratio_min: ratios.length ? round(Math.min(...ratios), 4) : 0,
    ffmpeg_speed_min_after_warmup: run.aws_media.ffmpeg_speed_min_after_warmup,
    ffmpeg_speed_p50: run.aws_media.ffmpeg_speed_p50,
    ffmpeg_restarts: run.aws_media.ffmpeg_restarts,
    ffmpeg_exit_count: run.aws_media.ffmpeg_exit_count,
    capture_fps_p50: run.frontend.capture_fps_p50,
    outbound_fps_p50: run.frontend.outbound_fps_p50,
    webcodecs_sent_frames: run.frontend.webcodecs_sent_frames,
    webcodecs_dropped_frames: run.frontend.webcodecs_dropped_frames,
    webcodecs_server_accepted_frames: run.frontend.webcodecs_server_accepted_frames,
    tts_delay_ms_p95: run.tts.delay_ms_p95,
    tts_drift_ms_per_min: run.tts.drift_ms_per_min,
    failed_gates: run.gates.filter((gate) => !gate.pass).map((gate) => gate.name),
    artifacts_dir: run.artifacts_dir,
  };
}

function writeVerdict(file, summary) {
  const failed = summary.gates.filter((gate) => !gate.pass);
  const lines = [
    `# Prod Media Stress Verdict — ${summary.result.toUpperCase()}`,
    '',
    `- Run: \`${summary.run_id}\``,
    `- Browser: ${summary.browser ?? 'unknown'} (${summary.headless ? 'headless' : 'headed'})`,
    `- Shape: ${summary.shape ?? 'unknown'}`,
    `- Media ingest: ${summary.media_ingest_mode ?? 'unknown'}`,
    `- Session: ${summary.session_id ?? 'unknown'}`,
    `- Artifacts: \`${summary.artifacts_dir}\``,
    '',
    '## Gates',
    '',
    ...summary.gates.map((gate) => `- ${gate.pass ? '✅' : '❌'} ${gate.name}`),
    '',
  ];
  if (failed.length) {
    lines.push('## Failed gate details', '');
    for (const gate of failed) {
      lines.push(`### ${gate.name}`, '', '```json', JSON.stringify(gate.details, null, 2), '```', '');
    }
  }
  fs.writeFileSync(file, `${lines.join('\n')}\n`);
}

function writeCompareVerdict(file, comparison) {
  const lines = [
    `# Media Ingest A/B Verdict — ${comparison.result.toUpperCase()}`,
    '',
    `- Runs: ${comparison.run_count}`,
    '',
    '| Mode | Browser | Result | Live ratio min | FFmpeg speed min | Restarts | Capture FPS | Outbound FPS | WebCodecs sent/drop/server | TTS p95 |',
    '| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |',
    ...comparison.modes.map((run) => `| ${run.media_ingest_mode} | ${run.browser ?? 'unknown'} | ${run.result} | ${run.provider_confirmed_live_ratio_min} | ${run.ffmpeg_speed_min_after_warmup ?? '?'} | ${run.ffmpeg_restarts ?? '?'} | ${run.capture_fps_p50 ?? '?'} | ${run.outbound_fps_p50 ?? '?'} | ${(run.webcodecs_sent_frames ?? 0)}/${(run.webcodecs_dropped_frames ?? 0)}/${(run.webcodecs_server_accepted_frames ?? 0)} | ${run.tts_delay_ms_p95 ?? '?'} |`),
    '',
  ];
  const failed = comparison.modes.filter((run) => run.failed_gates.length > 0);
  if (failed.length) {
    lines.push('## Failed gates', '');
    for (const run of failed) {
      lines.push(`- ${run.media_ingest_mode} / ${run.run_id}: ${run.failed_gates.join(', ')}`);
    }
    lines.push('');
  }
  fs.writeFileSync(file, `${lines.join('\n')}\n`);
}

function safeName(value) {
  return String(value ?? 'stream').replace(/[^a-zA-Z0-9_.-]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 120) || 'stream';
}

async function runSelfTest() {
  const dir = path.join(os.tmpdir(), 'brivva-prod-media-stress-analyzer-self-test');
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  const start = Date.now();
  writeJson(path.join(dir, 'meta.json'), {
    runId: 'self-test',
    browser: 'chromium',
    headless: true,
    shape: 'translated',
    mediaIngestMode: 'webrtc',
    durationSec: 120,
    providerPollSeconds: 10,
    sourceLang: 'en',
    sessionId: 'session-self-test',
    fetchCloudWatch: false,
    fetchCloudflareObservability: false,
  });
  writeJson(path.join(dir, 'created.redacted.json'), {
    session: { id: 'session-self-test' },
    streams: [{ id: 'stream-ko', platform: 'youtube', lang: 'ko', delay_ms: 4000, host_gain: 0.2, watch_url: 'https://www.youtube.com/watch?v=abc123' }],
  });
  writeJson(path.join(dir, 'watch-urls.json'), { streams: [{ stream_id: 'stream-ko', artifact_name: '0-stream-ko', kind: 'translated', watch_url: 'https://www.youtube.com/watch?v=abc123' }] });
  writeNdjson(path.join(dir, 'runner-events.ndjson'), [
    { ts_ms: start, event: 'session.created', sessionId: 'session-self-test' },
    { ts_ms: start + 1000, event: 'record.started' },
    { ts_ms: start + 121000, event: 'record.stopped' },
  ]);
  writeNdjson(path.join(dir, 'provider-health.ndjson'), Array.from({ length: 12 }, (_, i) => ({
    ts_ms: start + i * 10000,
    ok: true,
    data: { streams: [{ streamId: 'stream-ko', providerConfirmedLive: true, healthStatus: 'good', streamStatus: 'active' }] },
  })));
  writeNdjson(path.join(dir, 'session-logs.ndjson'), [
    { ts_ms: start + 1000, source: 'frontend', event: 'frontend.media_stats', fields: { stats: { sourceTrack: { width: 720, height: 1280, frameRate: 30 }, outboundVideo: { frameWidth: 720, frameHeight: 1280, framesPerSecond: 30 } } } },
  ]);
  writeNdjson(path.join(dir, 'host-ws.ndjson'), [
    { ts_ms: start + 10000, direction: 'received', json: { type: 'final', utteranceId: 1 } },
    { ts_ms: start + 11000, direction: 'received', json: { type: 'translation', utteranceId: 1, targetLang: 'ko' } },
    { ts_ms: start + 13000, direction: 'received', json: { type: 'tts_end', utteranceId: 1, targetLang: 'ko', ttsMs: 500 } },
    { ts_ms: start + 70000, direction: 'received', json: { type: 'final', utteranceId: 2 } },
    { ts_ms: start + 71000, direction: 'received', json: { type: 'translation', utteranceId: 2, targetLang: 'ko' } },
    { ts_ms: start + 73300, direction: 'received', json: { type: 'tts_end', utteranceId: 2, targetLang: 'ko', ttsMs: 500 } },
  ]);
  writeJson(path.join(dir, 'cloudwatch-all.json'), { events: [
    { timestamp: start + 20000, message: 'session-self-test ffmpeg progress speed=1.00x' },
    { timestamp: start + 30000, message: 'session-self-test ffmpeg progress speed=1.01x' },
  ] });
  fs.writeFileSync(path.join(dir, 'watch-body-poststop-0-stream-ko.txt'), 'YouTube live watch page Share Subscribe');
  const summary = await analyzeRun(dir);
  assert.equal(summary.result, 'pass');
  assert.equal(summary.tts.delay_ms_p95, 3300);
  console.log(`self-test pass: ${dir}`);
}

if (DIRECT) {
  const arg = process.argv[2];
  if (arg === '--self-test') {
    await runSelfTest();
  } else if (arg === '--compare') {
    const outDir = process.argv[3];
    const runDirs = process.argv.slice(4).filter((value) => value !== '--strict');
    if (!outDir || runDirs.length === 0) {
      console.error('usage: node scripts/analyze-prod-media-stress.mjs --compare <out-dir> <run-dir...> [--strict]');
      process.exit(2);
    }
    const comparison = await compareRuns(outDir, runDirs);
    console.log(JSON.stringify({ result: comparison.result, summary: path.join(path.resolve(outDir), 'summary.json') }));
    if (comparison.result !== 'pass' && process.argv.includes('--strict')) process.exit(1);
  } else if (arg) {
    const summary = await analyzeRun(arg, { throwOnFail: process.argv.includes('--strict') });
    console.log(JSON.stringify({ result: summary.result, summary: path.join(path.resolve(arg), 'summary.json') }));
    if (summary.result !== 'pass' && process.argv.includes('--strict')) process.exit(1);
  } else {
    console.error('usage: node scripts/analyze-prod-media-stress.mjs <run-dir> [--strict]\n       node scripts/analyze-prod-media-stress.mjs --compare <out-dir> <run-dir...> [--strict]\n       node scripts/analyze-prod-media-stress.mjs --self-test');
    process.exit(2);
  }
}
