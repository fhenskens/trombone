#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  ./scripts/bench-compare.sh [--serial <adb-serial>] [--mode <output|input|duplex>] [--backend <auto|aaudio|opensl>] [--seconds <n>] [--sample-rate <hz>] [--channels <n>] [--frames-per-burst <n>] [--gain <n>] [--release]

Description:
  Runs Trombone benchmark and oboe benchmark with matching settings,
  then prints a compact side-by-side comparison.

Examples:
  ./scripts/bench-compare.sh --serial 3B010DLJG001GR --mode output --backend aaudio --seconds 10
  ./scripts/bench-compare.sh --serial 3B010DLJG001GR --mode duplex --backend aaudio --seconds 10 --channels 1 --frames-per-burst 192 --gain 1.0
EOF
}

fail() {
  echo "error: $*" >&2
  exit 1
}

SERIAL=""
MODE="output"
BACKEND="aaudio"
SECONDS="10"
SAMPLE_RATE="48000"
CHANNELS="1"
FRAMES_PER_BURST="192"
GAIN="1.0"
PROFILE_FLAG="--release"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --serial)
      [ "$#" -ge 2 ] || fail "--serial needs a value"
      SERIAL="$2"
      shift 2
      ;;
    --mode)
      [ "$#" -ge 2 ] || fail "--mode needs a value"
      MODE="$2"
      shift 2
      ;;
    --backend)
      [ "$#" -ge 2 ] || fail "--backend needs a value"
      BACKEND="$2"
      shift 2
      ;;
    --seconds)
      [ "$#" -ge 2 ] || fail "--seconds needs a value"
      SECONDS="$2"
      shift 2
      ;;
    --sample-rate)
      [ "$#" -ge 2 ] || fail "--sample-rate needs a value"
      SAMPLE_RATE="$2"
      shift 2
      ;;
    --channels)
      [ "$#" -ge 2 ] || fail "--channels needs a value"
      CHANNELS="$2"
      shift 2
      ;;
    --frames-per-burst)
      [ "$#" -ge 2 ] || fail "--frames-per-burst needs a value"
      FRAMES_PER_BURST="$2"
      shift 2
      ;;
    --gain)
      [ "$#" -ge 2 ] || fail "--gain needs a value"
      GAIN="$2"
      shift 2
      ;;
    --release)
      PROFILE_FLAG="--release"
      shift
      ;;
    --debug)
      PROFILE_FLAG=""
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
done

[ -n "$SERIAL" ] || fail "pass --serial <adb-serial>"

COMMON_ARGS=(
  --mode "$MODE"
  --backend "$BACKEND"
  --seconds "$SECONDS"
  --sample-rate "$SAMPLE_RATE"
  --channels "$CHANNELS"
  --frames-per-burst "$FRAMES_PER_BURST"
  --format csv
)

if [ "$MODE" = "duplex" ]; then
  COMMON_ARGS+=(--gain "$GAIN")
fi

run_csv() {
  local example="$1"
  local tmpfile
  tmpfile="$(mktemp)"

  if [ -n "$PROFILE_FLAG" ]; then
    ./scripts/run-android.sh "$PROFILE_FLAG" --example "$example" --serial "$SERIAL" -- "${COMMON_ARGS[@]}" | tee "$tmpfile" >/dev/null
  else
    ./scripts/run-android.sh --example "$example" --serial "$SERIAL" -- "${COMMON_ARGS[@]}" | tee "$tmpfile" >/dev/null
  fi

  tail -n 1 "$tmpfile" | tr -d '\r'
  rm -f "$tmpfile"
}

to_num() {
  local v="$1"
  if [ -z "$v" ]; then
    echo "0"
  else
    echo "$v"
  fi
}

delta_pct() {
  local a="$1"
  local b="$2"
  awk -v a="$a" -v b="$b" 'BEGIN {
    if (b == 0) { print "n/a"; exit 0; }
    d=((a-b)/b)*100.0;
    printf("%+.2f%%", d);
  }'
}

parse_row() {
  local row="$1"
  echo "$row" | awk -F',' '
    {
      printf("mode=%s\n", $1);
      printf("backend=%s\n", $2);
      printf("out_callbacks=%s\n", $13);
      printf("out_samples=%s\n", $14);
      printf("out_xruns=%s\n", $15);
      printf("out_obs_frames=%s\n", $16);
      printf("out_avg_us=%s\n", $17);
      printf("out_p95_us=%s\n", $20);
      printf("out_interval_samples=%s\n", $21);
      printf("out_p50_us=%s\n", $32);
      printf("out_p99_us=%s\n", $33);
      printf("out_p95_trimmed_2x_us=%s\n", $34);
      printf("out_outliers_over_2x_median=%s\n", $35);
      printf("out_outliers_over_5x_median=%s\n", $36);
      printf("in_callbacks=%s\n", $22);
      printf("in_samples=%s\n", $23);
      printf("in_xruns=%s\n", $24);
      printf("in_obs_frames=%s\n", $25);
      printf("in_avg_us=%s\n", $26);
      printf("in_p95_us=%s\n", $29);
      printf("in_interval_samples=%s\n", $30);
      printf("in_p50_us=%s\n", $37);
      printf("in_p99_us=%s\n", $38);
      printf("in_p95_trimmed_2x_us=%s\n", $39);
      printf("in_outliers_over_2x_median=%s\n", $40);
      printf("in_outliers_over_5x_median=%s\n", $41);
      printf("zero_filled=%s\n", $31);
    }'
}

outlier_pct() {
  local outliers="$1"
  local samples="$2"
  awk -v o="$outliers" -v s="$samples" 'BEGIN {
    if (s == 0) { print "n/a"; exit 0; }
    printf("%.2f%%", (o / s) * 100.0);
  }'
}

echo "Running Trombone benchmark..."
TROMBONE_ROW="$(run_csv bench)"
echo "Running oboe benchmark..."
OBOE_ROW="$(run_csv oboe_bench)"

eval "$(parse_row "$TROMBONE_ROW" | sed 's/^/TROMBONE_/')"
eval "$(parse_row "$OBOE_ROW" | sed 's/^/OBOE_/')"

echo
echo "=== Benchmark Comparison ==="
echo "mode=${MODE} backend=${BACKEND} serial=${SERIAL}"
echo
printf "%-30s %-16s %-16s %-12s\n" "metric" "trombone" "oboe" "delta"
printf "%-30s %-16s %-16s %-12s\n" "out_callbacks" "$TROMBONE_out_callbacks" "$OBOE_out_callbacks" "$(delta_pct "$(to_num "$TROMBONE_out_callbacks")" "$(to_num "$OBOE_out_callbacks")")"
printf "%-30s %-16s %-16s %-12s\n" "out_samples" "$TROMBONE_out_samples" "$OBOE_out_samples" "$(delta_pct "$(to_num "$TROMBONE_out_samples")" "$(to_num "$OBOE_out_samples")")"
printf "%-30s %-16s %-16s %-12s\n" "out_xruns" "$TROMBONE_out_xruns" "$OBOE_out_xruns" "$(delta_pct "$(to_num "$TROMBONE_out_xruns")" "$(to_num "$OBOE_out_xruns")")"
printf "%-30s %-16s %-16s %-12s\n" "out_observed_frames" "$TROMBONE_out_obs_frames" "$OBOE_out_obs_frames" "$(delta_pct "$(to_num "$TROMBONE_out_obs_frames")" "$(to_num "$OBOE_out_obs_frames")")"
printf "%-30s %-16s %-16s %-12s\n" "out_avg_us" "$TROMBONE_out_avg_us" "$OBOE_out_avg_us" "$(delta_pct "$(to_num "$TROMBONE_out_avg_us")" "$(to_num "$OBOE_out_avg_us")")"
printf "%-30s %-16s %-16s %-12s\n" "out_p95_us" "$TROMBONE_out_p95_us" "$OBOE_out_p95_us" "$(delta_pct "$(to_num "$TROMBONE_out_p95_us")" "$(to_num "$OBOE_out_p95_us")")"
printf "%-30s %-16s %-16s %-12s\n" "out_p50_us" "$TROMBONE_out_p50_us" "$OBOE_out_p50_us" "$(delta_pct "$(to_num "$TROMBONE_out_p50_us")" "$(to_num "$OBOE_out_p50_us")")"
printf "%-30s %-16s %-16s %-12s\n" "out_p99_us" "$TROMBONE_out_p99_us" "$OBOE_out_p99_us" "$(delta_pct "$(to_num "$TROMBONE_out_p99_us")" "$(to_num "$OBOE_out_p99_us")")"
printf "%-30s %-16s %-16s %-12s\n" "out_p95_trimmed_2x_us" "$TROMBONE_out_p95_trimmed_2x_us" "$OBOE_out_p95_trimmed_2x_us" "$(delta_pct "$(to_num "$TROMBONE_out_p95_trimmed_2x_us")" "$(to_num "$OBOE_out_p95_trimmed_2x_us")")"
printf "%-30s %-16s %-16s %-12s\n" "out_outliers_over_2x" "$TROMBONE_out_outliers_over_2x_median" "$OBOE_out_outliers_over_2x_median" "$(delta_pct "$(to_num "$TROMBONE_out_outliers_over_2x_median")" "$(to_num "$OBOE_out_outliers_over_2x_median")")"
printf "%-30s %-16s %-16s %-12s\n" "out_outlier_rate_over_2x" "$(outlier_pct "$(to_num "$TROMBONE_out_outliers_over_2x_median")" "$(to_num "$TROMBONE_out_interval_samples")")" "$(outlier_pct "$(to_num "$OBOE_out_outliers_over_2x_median")" "$(to_num "$OBOE_out_interval_samples")")" "n/a"
printf "%-30s %-16s %-16s %-12s\n" "out_outliers_over_5x" "$TROMBONE_out_outliers_over_5x_median" "$OBOE_out_outliers_over_5x_median" "$(delta_pct "$(to_num "$TROMBONE_out_outliers_over_5x_median")" "$(to_num "$OBOE_out_outliers_over_5x_median")")"
printf "%-30s %-16s %-16s %-12s\n" "out_outlier_rate_over_5x" "$(outlier_pct "$(to_num "$TROMBONE_out_outliers_over_5x_median")" "$(to_num "$TROMBONE_out_interval_samples")")" "$(outlier_pct "$(to_num "$OBOE_out_outliers_over_5x_median")" "$(to_num "$OBOE_out_interval_samples")")" "n/a"

if [ "$MODE" = "input" ] || [ "$MODE" = "duplex" ]; then
  printf "%-30s %-16s %-16s %-12s\n" "in_callbacks" "$TROMBONE_in_callbacks" "$OBOE_in_callbacks" "$(delta_pct "$(to_num "$TROMBONE_in_callbacks")" "$(to_num "$OBOE_in_callbacks")")"
  printf "%-30s %-16s %-16s %-12s\n" "in_samples" "$TROMBONE_in_samples" "$OBOE_in_samples" "$(delta_pct "$(to_num "$TROMBONE_in_samples")" "$(to_num "$OBOE_in_samples")")"
  printf "%-30s %-16s %-16s %-12s\n" "in_xruns" "$TROMBONE_in_xruns" "$OBOE_in_xruns" "$(delta_pct "$(to_num "$TROMBONE_in_xruns")" "$(to_num "$OBOE_in_xruns")")"
  printf "%-30s %-16s %-16s %-12s\n" "in_observed_frames" "$TROMBONE_in_obs_frames" "$OBOE_in_obs_frames" "$(delta_pct "$(to_num "$TROMBONE_in_obs_frames")" "$(to_num "$OBOE_in_obs_frames")")"
  printf "%-30s %-16s %-16s %-12s\n" "in_avg_us" "$TROMBONE_in_avg_us" "$OBOE_in_avg_us" "$(delta_pct "$(to_num "$TROMBONE_in_avg_us")" "$(to_num "$OBOE_in_avg_us")")"
  printf "%-30s %-16s %-16s %-12s\n" "in_p95_us" "$TROMBONE_in_p95_us" "$OBOE_in_p95_us" "$(delta_pct "$(to_num "$TROMBONE_in_p95_us")" "$(to_num "$OBOE_in_p95_us")")"
  printf "%-30s %-16s %-16s %-12s\n" "in_p50_us" "$TROMBONE_in_p50_us" "$OBOE_in_p50_us" "$(delta_pct "$(to_num "$TROMBONE_in_p50_us")" "$(to_num "$OBOE_in_p50_us")")"
  printf "%-30s %-16s %-16s %-12s\n" "in_p99_us" "$TROMBONE_in_p99_us" "$OBOE_in_p99_us" "$(delta_pct "$(to_num "$TROMBONE_in_p99_us")" "$(to_num "$OBOE_in_p99_us")")"
  printf "%-30s %-16s %-16s %-12s\n" "in_p95_trimmed_2x_us" "$TROMBONE_in_p95_trimmed_2x_us" "$OBOE_in_p95_trimmed_2x_us" "$(delta_pct "$(to_num "$TROMBONE_in_p95_trimmed_2x_us")" "$(to_num "$OBOE_in_p95_trimmed_2x_us")")"
  printf "%-30s %-16s %-16s %-12s\n" "in_outliers_over_2x" "$TROMBONE_in_outliers_over_2x_median" "$OBOE_in_outliers_over_2x_median" "$(delta_pct "$(to_num "$TROMBONE_in_outliers_over_2x_median")" "$(to_num "$OBOE_in_outliers_over_2x_median")")"
  printf "%-30s %-16s %-16s %-12s\n" "in_outlier_rate_over_2x" "$(outlier_pct "$(to_num "$TROMBONE_in_outliers_over_2x_median")" "$(to_num "$TROMBONE_in_interval_samples")")" "$(outlier_pct "$(to_num "$OBOE_in_outliers_over_2x_median")" "$(to_num "$OBOE_in_interval_samples")")" "n/a"
  printf "%-30s %-16s %-16s %-12s\n" "in_outliers_over_5x" "$TROMBONE_in_outliers_over_5x_median" "$OBOE_in_outliers_over_5x_median" "$(delta_pct "$(to_num "$TROMBONE_in_outliers_over_5x_median")" "$(to_num "$OBOE_in_outliers_over_5x_median")")"
  printf "%-30s %-16s %-16s %-12s\n" "in_outlier_rate_over_5x" "$(outlier_pct "$(to_num "$TROMBONE_in_outliers_over_5x_median")" "$(to_num "$TROMBONE_in_interval_samples")")" "$(outlier_pct "$(to_num "$OBOE_in_outliers_over_5x_median")" "$(to_num "$OBOE_in_interval_samples")")" "n/a"
fi

if [ "$MODE" = "duplex" ]; then
  printf "%-30s %-16s %-16s %-12s\n" "duplex_zero_filled" "$TROMBONE_zero_filled" "$OBOE_zero_filled" "$(delta_pct "$(to_num "$TROMBONE_zero_filled")" "$(to_num "$OBOE_zero_filled")")"
fi

echo
echo "raw_trombone_csv=$TROMBONE_ROW"
echo "raw_oboe_csv=$OBOE_ROW"
