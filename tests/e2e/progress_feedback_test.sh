#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOG_DIR="$ROOT_DIR/test-output"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/progress_feedback_e2e_$(date +%Y%m%d_%H%M%S).log"

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
trap 'log "Preserving work dir: $WORK_DIR"' EXIT

ARCHIVE_DIR="$WORK_DIR/archive"
DB_PATH="$WORK_DIR/xf.db"
INDEX_PATH="$WORK_DIR/index"

mkdir -p "$ARCHIVE_DIR/data"
cat <<'JSON' > "$ARCHIVE_DIR/data/tweets.js"
window.YTD.tweets.part0 = [
  {
    "tweet": {
      "id_str": "1234567890123456789",
      "created_at": "Wed Jan 08 12:00:00 +0000 2025",
      "full_text": "Hello world! This is a test tweet about Rust programming.",
      "source": "<a href=\"https://x.com\">X Web App</a>",
      "favorite_count": "42",
      "retweet_count": "7",
      "lang": "en",
      "entities": {"hashtags": [], "user_mentions": [], "urls": []}
    }
  }
]
JSON
cat <<'JSON' > "$ARCHIVE_DIR/data/manifest.js"
window.YTD.manifest.part0 = {
  "userInfo": {
    "accountId": "999999999",
    "userName": "test_user",
    "displayName": "Test User"
  },
  "archiveInfo": {
    "sizeBytes": "1234",
    "generationDate": "2025-01-01T00:00:00Z",
    "isPartialArchive": false
  }
}
JSON

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

log "=== progress_feedback_test.sh starting ==="
log "ENV: XF_BIN=$XF_BIN NO_COLOR=1"

run_cmd "index" env NO_COLOR=1 "$XF_BIN" index "$ARCHIVE_DIR" --db "$DB_PATH" --index "$INDEX_PATH"
if [ $LAST_STATUS -ne 0 ]; then
  fail "index command failed"
fi
if ! grep -q "Indexing complete in" "$LAST_STDOUT"; then
  fail "index output missing completion timing"
fi
if grep -q $'\x1b' "$LAST_STDOUT"; then
  fail "expected no ANSI color codes with NO_COLOR=1 during index"
fi
pass "index timing output present and NO_COLOR honored"

run_cmd "search" env NO_COLOR=1 "$XF_BIN" search "rust" --db "$DB_PATH" --index "$INDEX_PATH"
if [ $LAST_STATUS -ne 0 ]; then
  fail "search command failed"
fi
if ! grep -q "Found" "$LAST_STDOUT"; then
  fail "search output missing Found line"
fi
if ! grep -q " in " "$LAST_STDOUT"; then
  fail "search output missing timing"
fi
if grep -q $'\x1b' "$LAST_STDOUT"; then
  fail "expected no ANSI color codes with NO_COLOR=1 during search"
fi
pass "search timing output present and NO_COLOR honored"

log "=== progress_feedback_test.sh completed ==="
