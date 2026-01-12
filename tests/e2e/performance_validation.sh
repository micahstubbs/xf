#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOG_DIR="$ROOT_DIR/test-output"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/performance_validation.log"
RUN_LOG="$LOG_DIR/performance_validation_$(date +%Y%m%d_%H%M%S).log"

XF_BIN="${XF_BIN:-$ROOT_DIR/target/debug/xf}"
CORPUS_DIR="${XF_PERF_ARCHIVE:-$ROOT_DIR/tests/fixtures/perf_corpus}"
MANIFEST_PATH="$CORPUS_DIR/corpus_manifest.json"

WORK_DIR="$(mktemp -d)"
trap 'log "Preserving work dir: $WORK_DIR"' EXIT

DB_PATH="${XF_DB:-$WORK_DIR/xf.db}"
INDEX_PATH="${XF_INDEX:-$WORK_DIR/index}"
export ROOT_DIR DB_PATH INDEX_PATH

PERF_RUNS="${PERF_RUNS:-20}"
PERF_WARMUPS="${PERF_WARMUPS:-3}"

PASS_COUNT=0
FAIL_COUNT=0

log() {
  local msg="$*"
  local ts
  ts="$(date '+%Y-%m-%d %H:%M:%S.%3N')"
  echo "[$ts] $msg" | tee -a "$LOG_FILE" "$RUN_LOG"
}

fail() {
  log "FAIL: $*"
  FAIL_COUNT=$((FAIL_COUNT + 1))
}

pass() {
  log "PASS: $*"
  PASS_COUNT=$((PASS_COUNT + 1))
}

require_file() {
  local path="$1"
  if [ ! -f "$path" ]; then
    fail "Missing required file: $path"
    return 1
  fi
}

if [ ! -x "$XF_BIN" ]; then
  log "xf binary not found at $XF_BIN; building"
  (cd "$ROOT_DIR" && cargo build)
fi

if ! command -v python3 >/dev/null 2>&1; then
  fail "python3 is required for percentile calculations"
  exit 1
fi

if [ ! -d "$CORPUS_DIR/data" ]; then
  fail "Perf corpus not found at $CORPUS_DIR"
  exit 1
fi

log "=== XF Performance Validation ==="
log "Date: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
log "Commit: $(git -C "$ROOT_DIR" rev-parse HEAD 2>/dev/null || echo 'unknown')"
log "Binary: $XF_BIN"
log "Corpus: $CORPUS_DIR"
log "ENV: XF_DB=$DB_PATH XF_INDEX=$INDEX_PATH NO_COLOR=1"
log "Perf runs: $PERF_RUNS (warmups: $PERF_WARMUPS)"

if [ -f "$MANIFEST_PATH" ]; then
  if command -v jq >/dev/null 2>&1; then
    log "Corpus manifest: $MANIFEST_PATH"
    log "Corpus total records: $(jq -r '.total_records' "$MANIFEST_PATH")"
    log "Corpus files: $(jq -r '.files | keys | join(", ")' "$MANIFEST_PATH")"
  else
    log "Corpus manifest present but jq not available"
  fi
else
  log "Corpus manifest missing: $MANIFEST_PATH"
fi

LAST_STDOUT=""
LAST_STDERR=""
LAST_STATUS=0
LAST_DURATION=0
LAST_START_TS=""
LAST_END_TS=""

run_cmd() {
  local name="$1"
  shift
  local stdout_file="$WORK_DIR/${name}.out"
  local stderr_file="$WORK_DIR/${name}.err"
  local start_ns end_ns duration_ms

  LAST_START_TS="$(date -u '+%Y-%m-%dT%H:%M:%S.%3NZ')"
  start_ns=$(date +%s%N)
  if "$@" >"$stdout_file" 2>"$stderr_file"; then
    LAST_STATUS=0
  else
    LAST_STATUS=$?
  fi
  end_ns=$(date +%s%N)
  LAST_END_TS="$(date -u '+%Y-%m-%dT%H:%M:%S.%3NZ')"
  duration_ms=$(( (end_ns - start_ns) / 1000000 ))

  LAST_STDOUT="$stdout_file"
  LAST_STDERR="$stderr_file"
  LAST_DURATION=$duration_ms

  log "RUN: $name => $*"
  log "START: $LAST_START_TS"
  log "END: $LAST_END_TS"
  log "EXIT: $LAST_STATUS (duration: ${duration_ms}ms)"
  log "STDOUT_FILE: $stdout_file"
  log "STDERR_FILE: $stderr_file"
  log "STDOUT: $(cat "$stdout_file")"
  log "STDERR: $(cat "$stderr_file")"
}

run_timed_quiet() {
  local start_ns end_ns duration_ms
  start_ns=$(date +%s%N)
  if "$@" >/dev/null 2>/dev/null; then
    :
  else
    return 1
  fi
  end_ns=$(date +%s%N)
  duration_ms=$(( (end_ns - start_ns) / 1000000 ))
  echo "$duration_ms"
}

percentiles() {
  python3 - <<'PY' "$@"
import math
import sys
values = [float(v) for v in sys.argv[1:]]
if not values:
    print("nan nan nan")
    sys.exit(0)
values.sort()

def pct(p):
    if len(values) == 1:
        return values[0]
    k = (len(values) - 1) * (p / 100.0)
    f = math.floor(k)
    c = math.ceil(k)
    if f == c:
        return values[int(k)]
    return values[f] + (values[c] - values[f]) * (k - f)

print(f"{pct(50):.3f} {pct(95):.3f} {pct(99):.3f}")
PY
}

measure_cmd() {
  local name="$1"
  shift
  local warmups="$PERF_WARMUPS"
  local runs="$PERF_RUNS"
  local durations=()
  local i

  for i in $(seq 1 "$warmups"); do
    if ! run_timed_quiet "$@" >/dev/null; then
      fail "$name warmup run failed"
      return 1
    fi
  done

  for i in $(seq 1 "$runs"); do
    local duration
    duration=$(run_timed_quiet "$@") || {
      fail "$name run $i failed"
      return 1
    }
    durations+=("$duration")
  done

  local pcts
  pcts=$(percentiles "${durations[@]}")
  local p50
  local p95
  local p99
  p50="$(echo "$pcts" | awk '{print $1}')"
  p95="$(echo "$pcts" | awk '{print $2}')"
  p99="$(echo "$pcts" | awk '{print $3}')"
  log "PERF $name p50=${p50}ms p95=${p95}ms p99=${p99}ms"
  echo "$p50 $p95 $p99"
}

compare_json() {
  local name="$1"
  local actual="$2"
  local golden="$3"

  if ! require_file "$golden"; then
    return 1
  fi
  if ! command -v jq >/dev/null 2>&1; then
    fail "$name requires jq for JSON normalization"
    return 1
  fi
  if ! jq -S '.' "$actual" >"$WORK_DIR/${name}_actual.json" 2>/dev/null; then
    fail "$name actual JSON invalid"
    return 1
  fi
  if ! jq -S '.' "$golden" >"$WORK_DIR/${name}_golden.json" 2>/dev/null; then
    fail "$name golden JSON invalid"
    return 1
  fi
  if diff -q "$WORK_DIR/${name}_actual.json" "$WORK_DIR/${name}_golden.json" >/dev/null 2>&1; then
    pass "$name matches golden output"
  else
    fail "$name differs from golden output"
    log "DIFF ($name):"
    diff "$WORK_DIR/${name}_golden.json" "$WORK_DIR/${name}_actual.json" | head -40 | tee -a "$LOG_FILE" "$RUN_LOG"
  fi
}

compare_text() {
  local name="$1"
  local actual="$2"
  local golden="$3"

  if ! require_file "$golden"; then
    return 1
  fi
  if diff -q "$actual" "$golden" >/dev/null 2>&1; then
    pass "$name matches golden output"
  else
    fail "$name differs from golden output"
    log "DIFF ($name):"
    diff "$golden" "$actual" | head -40 | tee -a "$LOG_FILE" "$RUN_LOG"
  fi
}

# Index performance test
run_cmd "index" env NO_COLOR=1 XF_DB="$DB_PATH" XF_INDEX="$INDEX_PATH" "$XF_BIN" index "$CORPUS_DIR" --force
if [ $LAST_STATUS -ne 0 ]; then
  fail "index command failed"
else
  pass "index command succeeded"
fi
if [ -d "$INDEX_PATH" ]; then
  log "Index size: $(du -sh "$INDEX_PATH" 2>/dev/null | awk '{print $1}')"
fi
if [ -f "$DB_PATH" ]; then
  log "DB size: $(du -sh "$DB_PATH" 2>/dev/null | awk '{print $1}')"
fi

# Stats tests
run_cmd "stats_text" env NO_COLOR=1 XF_DB="$DB_PATH" XF_INDEX="$INDEX_PATH" "$XF_BIN" stats
if [ $LAST_STATUS -ne 0 ]; then
  fail "stats command failed"
else
  pass "stats command succeeded"
fi

run_cmd "stats_json" env NO_COLOR=1 XF_DB="$DB_PATH" XF_INDEX="$INDEX_PATH" "$XF_BIN" stats --format json
if [ $LAST_STATUS -ne 0 ]; then
  fail "stats --format json failed"
else
  pass "stats --format json succeeded"
fi
if command -v jq >/dev/null 2>&1; then
  if jq . "$LAST_STDOUT" >/dev/null 2>&1; then
    log "Stats JSON valid"
  else
    fail "Stats JSON invalid"
  fi
fi

run_cmd "stats_detailed_json" env NO_COLOR=1 XF_DB="$DB_PATH" XF_INDEX="$INDEX_PATH" "$XF_BIN" stats --detailed --format json
if [ $LAST_STATUS -ne 0 ]; then
  fail "stats --detailed --format json failed"
else
  pass "stats --detailed --format json succeeded"
fi

# Search tests
run_cmd "search_hybrid" env NO_COLOR=1 XF_DB="$DB_PATH" XF_INDEX="$INDEX_PATH" "$XF_BIN" search "rust" --limit 100 --format json
if [ $LAST_STATUS -ne 0 ]; then
  fail "hybrid search failed"
else
  pass "hybrid search succeeded"
fi
if command -v jq >/dev/null 2>&1; then
  log "Hybrid result count: $(jq 'length' "$LAST_STDOUT" 2>/dev/null || echo 'n/a')"
fi

run_cmd "search_lexical" env NO_COLOR=1 XF_DB="$DB_PATH" XF_INDEX="$INDEX_PATH" "$XF_BIN" search "machine" --mode lexical --limit 100 --format json
if [ $LAST_STATUS -ne 0 ]; then
  fail "lexical search failed"
else
  pass "lexical search succeeded"
fi
if command -v jq >/dev/null 2>&1; then
  log "Lexical result count: $(jq 'length' "$LAST_STDOUT" 2>/dev/null || echo 'n/a')"
fi

run_cmd "search_semantic" env NO_COLOR=1 XF_DB="$DB_PATH" XF_INDEX="$INDEX_PATH" "$XF_BIN" search "stress" --mode semantic --limit 100 --format json
if [ $LAST_STATUS -ne 0 ]; then
  fail "semantic search failed"
else
  pass "semantic search succeeded"
fi
if command -v jq >/dev/null 2>&1; then
  log "Semantic result count: $(jq 'length' "$LAST_STDOUT" 2>/dev/null || echo 'n/a')"
fi

run_cmd "search_hybrid_golden" env NO_COLOR=1 XF_DB="$DB_PATH" XF_INDEX="$INDEX_PATH" "$XF_BIN" search "rust" --limit 20 --format json
if [ $LAST_STATUS -ne 0 ]; then
  fail "hybrid search (golden) failed"
else
  pass "hybrid search (golden) succeeded"
fi

run_cmd "search_lexical_golden" env NO_COLOR=1 XF_DB="$DB_PATH" XF_INDEX="$INDEX_PATH" "$XF_BIN" search "machine" --mode lexical --limit 20 --format json
if [ $LAST_STATUS -ne 0 ]; then
  fail "lexical search (golden) failed"
else
  pass "lexical search (golden) succeeded"
fi

# Isomorphism checks
GOLDEN_DIR="$ROOT_DIR/tests/fixtures/golden_outputs"
compare_json "search_hybrid_rust" "$WORK_DIR/search_hybrid_golden.out" "$GOLDEN_DIR/search_hybrid_rust.json"
compare_json "search_lexical_machine" "$WORK_DIR/search_lexical_golden.out" "$GOLDEN_DIR/search_lexical_machine.json"
compare_text "stats_basic" "$WORK_DIR/stats_text.out" "$GOLDEN_DIR/stats_basic.txt"
compare_json "stats_detailed" "$WORK_DIR/stats_detailed_json.out" "$GOLDEN_DIR/stats_detailed.json"

# Cache effectiveness check (warm vs cold)
run_cmd "cache_cold" env NO_COLOR=1 XF_DB="$DB_PATH" XF_INDEX="$INDEX_PATH" "$XF_BIN" search "rust" --limit 100 --format json
cold_duration=$LAST_DURATION
run_cmd "cache_warm" env NO_COLOR=1 XF_DB="$DB_PATH" XF_INDEX="$INDEX_PATH" "$XF_BIN" search "rust" --limit 100 --format json
warm_duration=$LAST_DURATION
if [ "$warm_duration" -lt "$cold_duration" ]; then
  pass "cache warm search faster (${cold_duration}ms -> ${warm_duration}ms)"
else
  fail "cache warm search not faster (${cold_duration}ms -> ${warm_duration}ms)"
fi

# Memory usage
if [ -x /usr/bin/time ]; then
  TIME_STDERR="$WORK_DIR/time_search.err"
  if /usr/bin/time -v env NO_COLOR=1 XF_DB="$DB_PATH" XF_INDEX="$INDEX_PATH" "$XF_BIN" search "rust" --limit 100 --format json >"$WORK_DIR/time_search.out" 2>"$TIME_STDERR"; then
    log "Memory/time report:"
    log "$(grep -E 'Elapsed|User time|System time|Maximum resident set size' "$TIME_STDERR" || true)"
    pass "memory usage captured"
  else
    fail "memory usage capture failed"
  fi
else
  log "WARN: /usr/bin/time not available; skipping memory capture"
fi

# Performance sampling (quiet runs)
SEARCH_HYBRID_METRICS=""
SEARCH_LEXICAL_METRICS=""
SEARCH_SEMANTIC_METRICS=""
STATS_METRICS=""

SEARCH_HYBRID_METRICS=$(measure_cmd "search_hybrid" env NO_COLOR=1 XF_DB="$DB_PATH" XF_INDEX="$INDEX_PATH" "$XF_BIN" search "rust" --limit 100 --format json | tail -n 1)
SEARCH_LEXICAL_METRICS=$(measure_cmd "search_lexical" env NO_COLOR=1 XF_DB="$DB_PATH" XF_INDEX="$INDEX_PATH" "$XF_BIN" search "machine" --mode lexical --limit 100 --format json | tail -n 1)
SEARCH_SEMANTIC_METRICS=$(measure_cmd "search_semantic" env NO_COLOR=1 XF_DB="$DB_PATH" XF_INDEX="$INDEX_PATH" "$XF_BIN" search "stress" --mode semantic --limit 100 --format json | tail -n 1)
STATS_METRICS=$(measure_cmd "stats" env NO_COLOR=1 XF_DB="$DB_PATH" XF_INDEX="$INDEX_PATH" "$XF_BIN" stats | tail -n 1)

log "=== Summary ==="
log "Passed: $PASS_COUNT"
log "Failed: $FAIL_COUNT"

if [ "${UPDATE_BASELINE_DOC:-}" = "1" ]; then
  BASELINE_DOC="$ROOT_DIR/docs/performance_baseline.md"
  export BASELINE_DOC SEARCH_HYBRID_METRICS SEARCH_LEXICAL_METRICS SEARCH_SEMANTIC_METRICS STATS_METRICS
  log "Updating baseline doc at $BASELINE_DOC"
  python3 - <<'PY'
import os
from datetime import datetime

baseline_path = os.environ.get("BASELINE_DOC")
commit = os.popen(f"git -C {os.environ.get('ROOT_DIR','.')} rev-parse HEAD").read().strip() or "unknown"
search_hybrid = os.environ.get("SEARCH_HYBRID_METRICS", "")
search_lexical = os.environ.get("SEARCH_LEXICAL_METRICS", "")
search_semantic = os.environ.get("SEARCH_SEMANTIC_METRICS", "")
stats_metrics = os.environ.get("STATS_METRICS", "")

baseline_text = ""
try:
    with open(baseline_path, "r", encoding="utf-8") as f:
        baseline_text = f.read()
except FileNotFoundError:
    baseline_text = ""

def find_baseline(command: str):
    if not baseline_text:
        return None
    for line in baseline_text.splitlines():
        if command in line and line.strip().startswith("| `"):
            parts = [p.strip() for p in line.strip().strip("|").split("|")]
            if len(parts) >= 4:
                try:
                    return tuple(float(x) for x in parts[1:4])
                except ValueError:
                    return None
    return None

def format_delta(new_vals, base_vals):
    if not new_vals or not base_vals:
        return None
    deltas = [n - b for n, b in zip(new_vals, base_vals)]
    return tuple(deltas)

section = f"\n## Run: {datetime.utcnow().isoformat(timespec='seconds')}Z\n\n"
section += f"- Commit: `{commit}`\n"
section += f"- XF_DB: `{os.environ.get('DB_PATH','')}`\n"
section += f"- XF_INDEX: `{os.environ.get('INDEX_PATH','')}`\n\n"
section += "| Command | p50 | p95 | p99 |\n| --- | ---:| ---:| ---:|\n"
delta_rows = []
if search_hybrid:
    p50, p95, p99 = search_hybrid.split()
    section += f"| `xf search \"rust\" --limit 100` | {p50} | {p95} | {p99} |\n"
    baseline = find_baseline("xf search \"rust\" --limit 100")
    deltas = format_delta((float(p50), float(p95), float(p99)), baseline) if baseline else None
    if deltas:
        delta_rows.append(("xf search \"rust\" --limit 100", *deltas))
if search_lexical:
    p50, p95, p99 = search_lexical.split()
    section += f"| `xf search \"machine\" --mode lexical --limit 100` | {p50} | {p95} | {p99} |\n"
    baseline = find_baseline("xf search \"machine\" --mode lexical --limit 100")
    deltas = format_delta((float(p50), float(p95), float(p99)), baseline) if baseline else None
    if deltas:
        delta_rows.append(("xf search \"machine\" --mode lexical --limit 100", *deltas))
if search_semantic:
    p50, p95, p99 = search_semantic.split()
    section += f"| `xf search \"stress\" --mode semantic --limit 100` | {p50} | {p95} | {p99} |\n"
    baseline = find_baseline("xf search \"stress\" --mode semantic --limit 100")
    deltas = format_delta((float(p50), float(p95), float(p99)), baseline) if baseline else None
    if deltas:
        delta_rows.append(("xf search \"stress\" --mode semantic --limit 100", *deltas))
if stats_metrics:
    p50, p95, p99 = stats_metrics.split()
    section += f"| `xf stats` | {p50} | {p95} | {p99} |\n"
    baseline = find_baseline("xf stats")
    deltas = format_delta((float(p50), float(p95), float(p99)), baseline) if baseline else None
    if deltas:
        delta_rows.append(("xf stats", *deltas))

if delta_rows:
    section += "\nDelta vs baseline (ms):\n\n"
    section += "| Command | p50 Δ | p95 Δ | p99 Δ |\n| --- | ---:| ---:| ---:|\n"
    for row in delta_rows:
        cmd, d50, d95, d99 = row
        section += f"| `{cmd}` | {d50:+.3f} | {d95:+.3f} | {d99:+.3f} |\n"

with open(baseline_path, "a", encoding="utf-8") as f:
    f.write(section)

print(section)
PY
fi

if [ $FAIL_COUNT -gt 0 ]; then
  log "Overall: $PASS_COUNT passed / $FAIL_COUNT failed"
  exit 1
fi

log "Overall: $PASS_COUNT passed / $FAIL_COUNT failed"
exit 0
