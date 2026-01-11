#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOG_DIR="$ROOT_DIR/test-output"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/help_text_e2e_$(date +%Y%m%d_%H%M%S).log"

XF_BIN="${XF_BIN:-$ROOT_DIR/target/debug/xf}"

log() {
  local msg="$*"
  local ts
  ts="$(date '+%Y-%m-%d %H:%M:%S.%3N')"
  echo "[$ts] $msg" | tee -a "$LOG_FILE"
}

fail() {
  log "FAIL: $*"
  exit 1
}

pass() {
  log "PASS: $*"
}

if [ ! -x "$XF_BIN" ]; then
  log "xf binary not found at $XF_BIN; building"
  (cd "$ROOT_DIR" && cargo build)
fi

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

LAST_STDOUT=""
LAST_STDERR=""
LAST_STATUS=0
LAST_DURATION=0

run_cmd() {
  local name="$1"
  shift
  local stdout_file="$WORK_DIR/${name}.out"
  local stderr_file="$WORK_DIR/${name}.err"
  local start end duration_ms

  log "RUN: $name => $*"
  start=$(date +%s%N)
  if "$@" >"$stdout_file" 2>"$stderr_file"; then
    LAST_STATUS=0
  else
    LAST_STATUS=$?
  fi
  end=$(date +%s%N)
  duration_ms=$(( (end - start) / 1000000 ))

  LAST_STDOUT="$stdout_file"
  LAST_STDERR="$stderr_file"
  LAST_DURATION=$duration_ms

  log "EXIT: $LAST_STATUS (duration: ${duration_ms}ms)"
  log "STDOUT: $(cat "$stdout_file")"
  log "STDERR: $(cat "$stderr_file")"
}

log "=== help_text_test.sh starting ==="
log "ENV: XF_BIN=$XF_BIN NO_COLOR=${NO_COLOR:-}"

run_cmd "help_root" "$XF_BIN" --help
if [ $LAST_STATUS -ne 0 ]; then
  fail "xf --help failed"
fi
if ! grep -q "Common tasks:" "$LAST_STDOUT"; then
  fail "xf --help missing Common tasks section"
fi
pass "xf --help includes Common tasks"

run_cmd "help_search" "$XF_BIN" search --help
if [ $LAST_STATUS -ne 0 ]; then
  fail "xf search --help failed"
fi
if ! grep -q "Examples:" "$LAST_STDOUT"; then
  fail "xf search --help missing Examples section"
fi
if ! grep -q "Formats:" "$LAST_STDOUT"; then
  fail "xf search --help missing date format guidance"
fi
if grep -q "follower" "$LAST_STDOUT"; then
  fail "xf search --help should not list follower as a type"
fi
if grep -q "following" "$LAST_STDOUT"; then
  fail "xf search --help should not list following as a type"
fi
if grep -q "block" "$LAST_STDOUT"; then
  fail "xf search --help should not list block as a type"
fi
if grep -q "mute" "$LAST_STDOUT"; then
  fail "xf search --help should not list mute as a type"
fi
pass "xf search --help includes examples and searchable types only"

log "=== help_text_test.sh completed ==="
