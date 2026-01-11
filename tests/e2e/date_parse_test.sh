#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOG_DIR="$ROOT_DIR/test-output"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/date_parse_e2e_$(date +%Y%m%d_%H%M%S).log"

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

  log "EXIT: $LAST_STATUS (duration: ${duration_ms}ms)"
  log "STDOUT: $(cat "$stdout_file")"
  log "STDERR: $(cat "$stderr_file")"
}

log "=== date_parse_test.sh starting ==="

run_cmd "index" "$XF_BIN" index "$ARCHIVE_DIR" --db "$DB_PATH" --index "$INDEX_PATH"
if [ $LAST_STATUS -ne 0 ]; then
  fail "index command failed"
fi
pass "index command succeeded"

run_cmd "search_named_period" "$XF_BIN" search "rust" --since "Jan 2025" --until "Jan 2025" --verbose --db "$DB_PATH" --index "$INDEX_PATH"
if [ $LAST_STATUS -ne 0 ]; then
  fail "search with named period failed"
fi
if ! grep -qi "rust" "$LAST_STDOUT"; then
  fail "expected search output to contain 'rust'"
fi
if ! grep -q "Parsed --since 'Jan 2025' as" "$LAST_STDERR"; then
  fail "expected verbose parse output for --since"
fi
pass "search with named period succeeded"

run_cmd "search_invalid" "$XF_BIN" search "rust" --since "notadate" --db "$DB_PATH" --index "$INDEX_PATH"
if [ $LAST_STATUS -eq 0 ]; then
  fail "expected failure for invalid date expression"
fi
if ! grep -q "could not be parsed" "$LAST_STDERR"; then
  fail "expected parse error message"
fi
pass "invalid date expression fails as expected"

log "=== date_parse_test.sh completed ==="
