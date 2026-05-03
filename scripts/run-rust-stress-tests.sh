#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="$ROOT/tmp/rust-stress-logs/$(date +%Y%m%d-%H%M%S)"

MP4="${MP4_FANOUT_SMOKE_MP4:-/home/typosbro/Desktop/text.mp4}"
SOURCE_LANG="${MP4_FANOUT_SMOKE_SOURCE_LANG:-ko}"
DURATION="${RUST_STRESS_DURATION:-180}"
LONG_DURATION="${RUST_STRESS_LONG_DURATION:-900}"
FOUR_K_MP4="${RUST_STRESS_4K_MP4:-}"
AUDIO_MP4="${RUST_STRESS_AUDIO_MP4:-}"
MAX_TTS_OVERFLOWS="${RUST_STRESS_MAX_TTS_OVERFLOWS:-0}"
MAX_HARD_RECOVERY="${RUST_STRESS_MAX_HARD_RECOVERY:-0}"
MAX_SLOW_ENCODE_TICKS="${RUST_STRESS_MAX_SLOW_ENCODE_TICKS:-20}"
INCLUDE_NETWORK=0
NET_IFACE="${RUST_STRESS_NET_IFACE:-}"
ONLY=""

usage() {
	cat <<'EOF'
Usage: ./scripts/run-rust-stress-tests.sh [flags]

Runs server-rs MP4 live stress scenarios in sequence.

Flags:
  --mp4 path                 Main MP4 fixture. Default: /home/typosbro/Desktop/text.mp4
  --source en|ko|ja|zh       Source language. Default: ko
  --duration seconds         Per-scenario duration. Default: 180
  --long-duration seconds    Long-run scenario duration. Default: 900
  --4k-mp4 path              Enable high-resolution scenario with this MP4
  --audio-mp4 path           Enable difficult-audio scenario with this MP4
  --include-network          Enable sudo tc netem scenario
  --iface name               Network interface for --include-network
  --only name                Run one scenario by name

Environment assertions:
  RUST_STRESS_MAX_TTS_OVERFLOWS   Allowed whole-segment TTS drops. Default: 0
  RUST_STRESS_MAX_HARD_RECOVERY   Allowed hard-recovery TTS events. Default: 0
  RUST_STRESS_MAX_SLOW_ENCODE_TICKS
                                  Allowed consecutive below-realtime FFmpeg ticks. Default: 20

Expected to be run inside Infisical:
  infisical run --env=dev --path=/ -- ./scripts/run-rust-stress-tests.sh

Scenario names:
  single_720p15 single_1080p30 cpu_720p30 many_outputs_1080p30
  single_4k30 many_outputs_4k30 high_res_capped low_quality
  backlog_catchup backlog_catchup_many_outputs bad_destination tts_failure stt_failure long_run difficult_audio network
EOF
}

while [[ $# -gt 0 ]]; do
	case "$1" in
		--help | -h)
			usage
			exit 0
			;;
		--mp4)
			MP4="${2:-}"
			shift 2
			;;
		--source)
			SOURCE_LANG="${2:-}"
			shift 2
			;;
		--duration)
			DURATION="${2:-}"
			shift 2
			;;
		--long-duration)
			LONG_DURATION="${2:-}"
			shift 2
			;;
		--4k-mp4)
			FOUR_K_MP4="${2:-}"
			shift 2
			;;
		--audio-mp4)
			AUDIO_MP4="${2:-}"
			shift 2
			;;
		--include-network)
			INCLUDE_NETWORK=1
			shift
			;;
		--iface)
			NET_IFACE="${2:-}"
			shift 2
			;;
		--only)
			ONLY="${2:-}"
			shift 2
			;;
		*)
			echo "unknown flag: $1" >&2
			usage >&2
			exit 2
			;;
	esac
done

mkdir -p "$LOG_DIR"

if [[ ! -f "$MP4" ]]; then
	echo "main MP4 not found: $MP4" >&2
	exit 2
fi

case "$SOURCE_LANG" in
	en | ko | ja | zh) ;;
	*)
		echo "--source must be en|ko|ja|zh, got '$SOURCE_LANG'" >&2
		exit 2
		;;
esac

if ! [[ "$DURATION" =~ ^[0-9]+$ && "$LONG_DURATION" =~ ^[0-9]+$ ]]; then
	echo "--duration and --long-duration must be integer seconds" >&2
	exit 2
fi

if ! [[ "$MAX_TTS_OVERFLOWS" =~ ^[0-9]+$ && "$MAX_HARD_RECOVERY" =~ ^[0-9]+$ && "$MAX_SLOW_ENCODE_TICKS" =~ ^[0-9]+$ ]]; then
	echo "RUST_STRESS_MAX_TTS_OVERFLOWS, RUST_STRESS_MAX_HARD_RECOVERY, and RUST_STRESS_MAX_SLOW_ENCODE_TICKS must be integer counts" >&2
	exit 2
fi

declare -a RESULTS=()

have_youtube_output() {
	for key in \
		STREAM_KEY_YOUTUBE \
		STREAM_KEY_YOUTUBE_PASS \
		STREAM_KEY_YOUTUBE_KO \
		STREAM_KEY_YOUTUBE_EN \
		STREAM_KEY_YOUTUBE_JA \
		STREAM_KEY_YOUTUBE_ZH; do
		if [[ -n "${!key:-}" ]]; then
			return 0
		fi
	done
	return 1
}

have_multi_youtube_output() {
	local count=0
	for key in \
		STREAM_KEY_YOUTUBE_PASS \
		STREAM_KEY_YOUTUBE_KO \
		STREAM_KEY_YOUTUBE_EN \
		STREAM_KEY_YOUTUBE_JA \
		STREAM_KEY_YOUTUBE_ZH; do
		if [[ -n "${!key:-}" ]]; then
			count=$((count + 1))
		fi
	done
	[[ "$count" -ge 2 ]]
}

have_output_key() {
	local output="$1"
	case "$output" in
		pass) [[ -n "${STREAM_KEY_YOUTUBE_PASS:-}" ]] ;;
		ko) [[ -n "${STREAM_KEY_YOUTUBE_KO:-}" ]] ;;
		en) [[ -n "${STREAM_KEY_YOUTUBE_EN:-}" ]] ;;
		ja) [[ -n "${STREAM_KEY_YOUTUBE_JA:-}" ]] ;;
		zh) [[ -n "${STREAM_KEY_YOUTUBE_ZH:-}" ]] ;;
		legacy) [[ -n "${STREAM_KEY_YOUTUBE:-}" || -n "${MP4_FANOUT_SMOKE_RTMP_URLS:-}" ]] ;;
		*) return 1 ;;
	esac
}

first_available_output() {
	for output in pass "$SOURCE_LANG" en ko ja zh legacy; do
		if have_output_key "$output"; then
			echo "$output"
			return 0
		fi
	done
	return 1
}

first_translated_output() {
	for output in ja zh en ko; do
		if [[ "$output" != "$SOURCE_LANG" ]] && have_output_key "$output"; then
			echo "$output"
			return 0
		fi
	done
	return 1
}

all_available_outputs() {
	local outputs=()
	for output in pass ko en ja zh; do
		if have_output_key "$output"; then
			outputs+=("$output")
		fi
	done
	(IFS=,; echo "${outputs[*]}")
}

should_run() {
	local name="$1"
	[[ -z "$ONLY" || "$ONLY" == "$name" ]]
}

record_skip() {
	local name="$1"
	local reason="$2"
	echo "SKIP $name: $reason"
	RESULTS+=("SKIP $name: $reason")
}

count_log_matches() {
	local pattern="$1"
	local logfile="$2"
	rg -c "$pattern" "$logfile" 2>/dev/null || true
}

max_log_counter() {
	local key="$1"
	local logfile="$2"
	local max=0
	local value
	while IFS= read -r value; do
		value="${value#*=}"
		if (( value > max )); then
			max="$value"
		fi
	done < <(rg -o "${key}=[0-9]+" "$logfile" 2>/dev/null || true)
	echo "$max"
}

assert_counter_at_most() {
	local name="$1"
	local logfile="$2"
	local label="$3"
	local key="$4"
	local max_allowed="$5"
	local max_seen
	max_seen="$(max_log_counter "$key" "$logfile")"
	if (( max_seen > max_allowed )); then
		echo "ASSERT FAIL $name: $label max_seen=$max_seen max=$max_allowed"
		echo "  grep: rg '${key}=[1-9]' '$logfile'"
		return 1
	fi
	echo "ASSERT OK $name: $label max_seen=$max_seen max=$max_allowed"
	return 0
}

assert_count_at_most() {
	local name="$1"
	local logfile="$2"
	local label="$3"
	local pattern="$4"
	local max="$5"
	local count
	count="$(count_log_matches "$pattern" "$logfile")"
	if (( count > max )); then
		echo "ASSERT FAIL $name: $label count=$count max=$max"
		echo "  grep: rg '$pattern' '$logfile'"
		return 1
	fi
	echo "ASSERT OK $name: $label count=$count max=$max"
	return 0
}

analyze_log() {
	local name="$1"
	local logfile="$2"
	local failed=0

	assert_counter_at_most "$name" "$logfile" "video stale drops" \
		"video_stale_chunks_dropped" 0 || failed=1
	assert_counter_at_most "$name" "$logfile" "video keyframe wait drops" \
		"video_keyframe_wait_chunks_dropped" 0 || failed=1
	assert_counter_at_most "$name" "$logfile" "host audio stale drops" \
		"host_audio_stale_chunks_dropped" 0 || failed=1
	assert_counter_at_most "$name" "$logfile" "ready host audio drops" \
		"ready_host_bytes_dropped" 0 || failed=1
	assert_count_at_most "$name" "$logfile" "FFmpeg process crash/restart" \
		"ffmpeg rtmp process crashed" 0 || failed=1
	assert_counter_at_most "$name" "$logfile" "sustained below-realtime encode ticks" \
		"consecutive_ticks" "$MAX_SLOW_ENCODE_TICKS" || failed=1
	assert_count_at_most "$name" "$logfile" "whole TTS segment overflow" \
		"tts segment queue overflow" "$MAX_TTS_OVERFLOWS" || failed=1
	assert_count_at_most "$name" "$logfile" "TTS hard recovery" \
		"final_policy=\"hard_recovery\"" "$MAX_HARD_RECOVERY" || failed=1

	if (( failed != 0 )); then
		return 1
	fi
	return 0
}

run_case() {
	local name="$1"
	shift
	local logfile="$LOG_DIR/${name}.log"
	if ! should_run "$name"; then
		return 0
	fi
	if ! have_youtube_output && [[ -z "${MP4_FANOUT_SMOKE_RTMP_URLS:-}" ]]; then
		record_skip "$name" "no YouTube/RTMP destination secrets"
		return 0
	fi
	echo
	echo "=== RUN $name ==="
	echo "log: $logfile"
	(
		cd "$ROOT"
		env "$@" cargo test -p server-rs --test mp4_fanout_smoke -- --ignored --nocapture
	) 2>&1 | tee "$logfile"
	local status="${PIPESTATUS[0]}"
	if [[ "$status" == "0" ]]; then
		if analyze_log "$name" "$logfile"; then
			RESULTS+=("PASS $name")
		else
			RESULTS+=("FAIL $name assertions log=$logfile")
		fi
	else
		RESULTS+=("FAIL $name status=$status log=$logfile")
	fi
	return 0
}

common_env=(
	"MP4_FANOUT_SMOKE_SOURCE_LANG=$SOURCE_LANG"
	"BRIVVA_SUBTITLE_FONTFILE=${BRIVVA_SUBTITLE_FONTFILE:-/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc}"
)

SINGLE_OUTPUT="$(first_available_output || true)"
TRANSLATED_OUTPUT="$(first_translated_output || true)"
MULTI_OUTPUTS="$(all_available_outputs)"

if [[ -z "$SINGLE_OUTPUT" ]]; then
	should_run single_720p15 && record_skip single_720p15 "no YouTube/RTMP destination secrets"
	should_run single_1080p30 && record_skip single_1080p30 "no YouTube/RTMP destination secrets"
	should_run cpu_720p30 && record_skip cpu_720p30 "no YouTube/RTMP destination secrets"
else
	run_case single_720p15 \
		"${common_env[@]}" \
		"MP4_FANOUT_SMOKE_MP4=$MP4" \
		"MP4_FANOUT_SMOKE_OUTPUTS=$SINGLE_OUTPUT" \
		"MP4_FANOUT_SMOKE_DURATION=$DURATION" \
		"MP4_FANOUT_SMOKE_ENCODER=nvenc" \
		"MP4_FANOUT_SMOKE_CAPTURE_WIDTH=1280" \
		"MP4_FANOUT_SMOKE_CAPTURE_HEIGHT=720" \
		"MP4_FANOUT_SMOKE_CAPTURE_FPS=15" \
		"BRIVVA_VIDEO_MAX_WIDTH=1280" \
		"BRIVVA_VIDEO_MAX_HEIGHT=720" \
		"BRIVVA_VIDEO_MAX_FPS=15"

	run_case single_1080p30 \
		"${common_env[@]}" \
		"MP4_FANOUT_SMOKE_MP4=$MP4" \
		"MP4_FANOUT_SMOKE_OUTPUTS=$SINGLE_OUTPUT" \
		"MP4_FANOUT_SMOKE_DURATION=$DURATION" \
		"MP4_FANOUT_SMOKE_ENCODER=nvenc" \
		"BRIVVA_VIDEO_MAX_WIDTH=1920" \
		"BRIVVA_VIDEO_MAX_HEIGHT=1080" \
		"BRIVVA_VIDEO_MAX_FPS=30"

	run_case cpu_720p30 \
		"${common_env[@]}" \
		"MP4_FANOUT_SMOKE_MP4=$MP4" \
		"MP4_FANOUT_SMOKE_OUTPUTS=$SINGLE_OUTPUT" \
		"MP4_FANOUT_SMOKE_DURATION=$DURATION" \
		"MP4_FANOUT_SMOKE_ENCODER=x264" \
		"BRIVVA_VIDEO_MAX_WIDTH=1280" \
		"BRIVVA_VIDEO_MAX_HEIGHT=720" \
		"BRIVVA_VIDEO_MAX_FPS=30"
fi

if should_run many_outputs_1080p30 && ! have_multi_youtube_output; then
	record_skip many_outputs_1080p30 "need at least two STREAM_KEY_YOUTUBE_{PASS,KO,EN,JA,ZH}"
else
	run_case many_outputs_1080p30 \
		"${common_env[@]}" \
		"MP4_FANOUT_SMOKE_MP4=$MP4" \
		"MP4_FANOUT_SMOKE_OUTPUTS=$MULTI_OUTPUTS" \
		"MP4_FANOUT_SMOKE_DURATION=$DURATION" \
		"MP4_FANOUT_SMOKE_ENCODER=nvenc" \
		"BRIVVA_VIDEO_MAX_WIDTH=1920" \
		"BRIVVA_VIDEO_MAX_HEIGHT=1080" \
		"BRIVVA_VIDEO_MAX_FPS=30"
fi

run_case low_quality \
	"${common_env[@]}" \
	"MP4_FANOUT_SMOKE_MP4=$MP4" \
	"MP4_FANOUT_SMOKE_OUTPUTS=$SINGLE_OUTPUT" \
	"MP4_FANOUT_SMOKE_DURATION=$DURATION" \
	"MP4_FANOUT_SMOKE_ENCODER=nvenc" \
	"MP4_FANOUT_SMOKE_CAPTURE_WIDTH=1280" \
	"MP4_FANOUT_SMOKE_CAPTURE_HEIGHT=720" \
	"MP4_FANOUT_SMOKE_CAPTURE_FPS=15" \
	"BRIVVA_VIDEO_MAX_WIDTH=1920" \
	"BRIVVA_VIDEO_MAX_HEIGHT=1080" \
	"BRIVVA_VIDEO_MAX_FPS=30"

if [[ -z "$TRANSLATED_OUTPUT" ]]; then
	should_run backlog_catchup && record_skip backlog_catchup "need a translated STREAM_KEY_YOUTUBE_{EN,KO,JA,ZH} different from --source"
else
	run_case backlog_catchup \
		"${common_env[@]}" \
		"MP4_FANOUT_SMOKE_MP4=$MP4" \
		"MP4_FANOUT_SMOKE_OUTPUTS=$TRANSLATED_OUTPUT" \
		"MP4_FANOUT_SMOKE_DURATION=$DURATION" \
		"MP4_FANOUT_SMOKE_ENCODER=nvenc" \
		"BRIVVA_VIDEO_MAX_WIDTH=1920" \
		"BRIVVA_VIDEO_MAX_HEIGHT=1080" \
		"BRIVVA_VIDEO_MAX_FPS=30" \
		"BRIVVA_VIDEO_MAX_LAG_MS=3000" \
		"BRIVVA_AUDIO_MAX_LAG_MS=5000" \
		"BRIVVA_AUDIO_FIFO_WRITE_BUDGET_MS=60" \
		"BRIVVA_TTS_QUEUE_CAP_MS=30000" \
		"BRIVVA_STT_FORCE_FINALIZE_MS=3000"
fi

if [[ -z "$MULTI_OUTPUTS" || "$MULTI_OUTPUTS" != *,* ]]; then
	should_run backlog_catchup_many_outputs && record_skip backlog_catchup_many_outputs "need at least two STREAM_KEY_YOUTUBE_{PASS,KO,EN,JA,ZH}"
else
	run_case backlog_catchup_many_outputs \
		"${common_env[@]}" \
		"MP4_FANOUT_SMOKE_MP4=$MP4" \
		"MP4_FANOUT_SMOKE_OUTPUTS=$MULTI_OUTPUTS" \
		"MP4_FANOUT_SMOKE_DURATION=$DURATION" \
		"MP4_FANOUT_SMOKE_ENCODER=nvenc" \
		"BRIVVA_VIDEO_MAX_WIDTH=1920" \
		"BRIVVA_VIDEO_MAX_HEIGHT=1080" \
		"BRIVVA_VIDEO_MAX_FPS=30" \
		"BRIVVA_VIDEO_MAX_LAG_MS=3000" \
		"BRIVVA_AUDIO_MAX_LAG_MS=5000" \
		"BRIVVA_AUDIO_FIFO_WRITE_BUDGET_MS=60" \
		"BRIVVA_TTS_QUEUE_CAP_MS=30000" \
		"BRIVVA_STT_FORCE_FINALIZE_MS=3000"
fi

if should_run single_4k30 || should_run many_outputs_4k30 || should_run high_res_capped; then
	if [[ -z "$FOUR_K_MP4" || ! -f "$FOUR_K_MP4" ]]; then
		record_skip single_4k30 "provide --4k-mp4 /path/to/h264-4k.mp4"
		record_skip many_outputs_4k30 "provide --4k-mp4 /path/to/h264-4k.mp4"
		record_skip high_res_capped "provide --4k-mp4 /path/to/4k.mp4"
	else
		run_case single_4k30 \
			"${common_env[@]}" \
			"MP4_FANOUT_SMOKE_MP4=$FOUR_K_MP4" \
			"MP4_FANOUT_SMOKE_OUTPUTS=$SINGLE_OUTPUT" \
			"MP4_FANOUT_SMOKE_DURATION=$DURATION" \
			"MP4_FANOUT_SMOKE_ENCODER=nvenc" \
			"BRIVVA_VIDEO_MAX_WIDTH=3840" \
			"BRIVVA_VIDEO_MAX_HEIGHT=2160" \
			"BRIVVA_VIDEO_MAX_FPS=30"
		if [[ -z "$MULTI_OUTPUTS" || "$MULTI_OUTPUTS" != *,* ]]; then
			record_skip many_outputs_4k30 "need at least two STREAM_KEY_YOUTUBE_{PASS,KO,EN,JA,ZH}"
		else
			run_case many_outputs_4k30 \
				"${common_env[@]}" \
				"MP4_FANOUT_SMOKE_MP4=$FOUR_K_MP4" \
				"MP4_FANOUT_SMOKE_OUTPUTS=$MULTI_OUTPUTS" \
				"MP4_FANOUT_SMOKE_DURATION=$DURATION" \
				"MP4_FANOUT_SMOKE_ENCODER=nvenc" \
				"BRIVVA_VIDEO_MAX_WIDTH=3840" \
				"BRIVVA_VIDEO_MAX_HEIGHT=2160" \
				"BRIVVA_VIDEO_MAX_FPS=30"
		fi
		run_case high_res_capped \
			"${common_env[@]}" \
			"MP4_FANOUT_SMOKE_MP4=$FOUR_K_MP4" \
			"MP4_FANOUT_SMOKE_OUTPUTS=$SINGLE_OUTPUT" \
			"MP4_FANOUT_SMOKE_DURATION=$DURATION" \
			"MP4_FANOUT_SMOKE_ENCODER=nvenc" \
			"BRIVVA_VIDEO_MAX_WIDTH=1920" \
			"BRIVVA_VIDEO_MAX_HEIGHT=1080" \
			"BRIVVA_VIDEO_MAX_FPS=30"
	fi
fi

run_case bad_destination \
	"${common_env[@]}" \
	"MP4_FANOUT_SMOKE_MP4=$MP4" \
	"MP4_FANOUT_SMOKE_OUTPUTS=$SINGLE_OUTPUT,legacy" \
	"MP4_FANOUT_SMOKE_DURATION=$DURATION" \
	"MP4_FANOUT_SMOKE_ENCODER=nvenc" \
	"BRIVVA_VIDEO_MAX_WIDTH=1920" \
	"BRIVVA_VIDEO_MAX_HEIGHT=1080" \
	"BRIVVA_VIDEO_MAX_FPS=30" \
	"MP4_FANOUT_SMOKE_RTMP_URLS=rtmp://127.0.0.1:1/live/bad"

if [[ -z "$MULTI_OUTPUTS" || "$MULTI_OUTPUTS" != *,* ]]; then
	should_run tts_failure && record_skip tts_failure "need at least two STREAM_KEY_YOUTUBE_{PASS,KO,EN,JA,ZH}"
	should_run stt_failure && record_skip stt_failure "need at least two STREAM_KEY_YOUTUBE_{PASS,KO,EN,JA,ZH}"
else
	run_case tts_failure \
		"${common_env[@]}" \
		"MP4_FANOUT_SMOKE_MP4=$MP4" \
		"MP4_FANOUT_SMOKE_OUTPUTS=$MULTI_OUTPUTS" \
		"MP4_FANOUT_SMOKE_DURATION=$DURATION" \
		"MP4_FANOUT_SMOKE_ENCODER=nvenc" \
		"BRIVVA_VIDEO_MAX_WIDTH=1920" \
		"BRIVVA_VIDEO_MAX_HEIGHT=1080" \
		"BRIVVA_VIDEO_MAX_FPS=30" \
		"ELEVENLABS_API_KEY=bad"

	run_case stt_failure \
		"${common_env[@]}" \
		"MP4_FANOUT_SMOKE_MP4=$MP4" \
		"MP4_FANOUT_SMOKE_OUTPUTS=$MULTI_OUTPUTS" \
		"MP4_FANOUT_SMOKE_DURATION=$DURATION" \
		"MP4_FANOUT_SMOKE_ENCODER=nvenc" \
		"BRIVVA_VIDEO_MAX_WIDTH=1920" \
		"BRIVVA_VIDEO_MAX_HEIGHT=1080" \
		"BRIVVA_VIDEO_MAX_FPS=30" \
		"SONIOX_API_KEY=bad"
fi

run_case long_run \
	"${common_env[@]}" \
	"MP4_FANOUT_SMOKE_MP4=$MP4" \
	"MP4_FANOUT_SMOKE_OUTPUTS=$SINGLE_OUTPUT" \
	"MP4_FANOUT_SMOKE_DURATION=$LONG_DURATION" \
	"MP4_FANOUT_SMOKE_ENCODER=nvenc" \
	"BRIVVA_VIDEO_MAX_WIDTH=1920" \
	"BRIVVA_VIDEO_MAX_HEIGHT=1080" \
	"BRIVVA_VIDEO_MAX_FPS=30"

if should_run difficult_audio; then
	if [[ -z "$AUDIO_MP4" || ! -f "$AUDIO_MP4" ]]; then
		record_skip difficult_audio "provide --audio-mp4 /path/to/difficult-audio.mp4"
	else
		run_case difficult_audio \
			"${common_env[@]}" \
			"MP4_FANOUT_SMOKE_MP4=$AUDIO_MP4" \
			"MP4_FANOUT_SMOKE_OUTPUTS=$SINGLE_OUTPUT" \
			"MP4_FANOUT_SMOKE_DURATION=$DURATION" \
			"MP4_FANOUT_SMOKE_ENCODER=nvenc" \
			"BRIVVA_VIDEO_MAX_WIDTH=1920" \
			"BRIVVA_VIDEO_MAX_HEIGHT=1080" \
			"BRIVVA_VIDEO_MAX_FPS=30"
	fi
fi

cleanup_network() {
	if [[ "$INCLUDE_NETWORK" == "1" && -n "$NET_IFACE" ]]; then
		sudo tc qdisc del dev "$NET_IFACE" root >/dev/null 2>&1 || true
	fi
}
trap cleanup_network EXIT

if should_run network; then
	if [[ "$INCLUDE_NETWORK" != "1" ]]; then
		record_skip network "pass --include-network --iface <iface>"
	elif [[ -z "$NET_IFACE" ]]; then
		record_skip network "missing --iface"
	else
		echo "Applying network impairment on $NET_IFACE"
		if sudo tc qdisc add dev "$NET_IFACE" root netem delay 150ms 50ms loss 1% rate 6mbit; then
			run_case network \
				"${common_env[@]}" \
				"MP4_FANOUT_SMOKE_MP4=$MP4" \
				"MP4_FANOUT_SMOKE_OUTPUTS=$SINGLE_OUTPUT" \
				"MP4_FANOUT_SMOKE_DURATION=$DURATION" \
				"MP4_FANOUT_SMOKE_ENCODER=nvenc" \
				"BRIVVA_VIDEO_MAX_WIDTH=1920" \
				"BRIVVA_VIDEO_MAX_HEIGHT=1080" \
				"BRIVVA_VIDEO_MAX_FPS=30"
			cleanup_network
		else
			record_skip network "sudo tc failed"
		fi
	fi
fi

echo
echo "=== SUMMARY ==="
printf '%s\n' "${RESULTS[@]}"
echo "logs: $LOG_DIR"

for result in "${RESULTS[@]}"; do
	if [[ "$result" == FAIL* ]]; then
		exit 1
	fi
done
