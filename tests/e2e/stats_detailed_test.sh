#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOG_DIR="$ROOT_DIR/test-output"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/stats_detailed_e2e_$(date +%Y%m%d_%H%M%S).log"

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
DB_PATH="${XF_DB:-$WORK_DIR/xf.db}"
INDEX_PATH="${XF_INDEX:-$WORK_DIR/index}"

if [ -z "${XF_DB:-}" ] || [ -z "${XF_INDEX:-}" ]; then
  mkdir -p "$ARCHIVE_DIR/data"
  cat <<'JSON' > "$ARCHIVE_DIR/data/tweets.js"
window.YTD.tweets.part0 = [
  {
    "tweet": {
      "id_str": "1234567890123456789",
      "created_at": "Wed Jan 08 12:00:00 +0000 2025",
      "full_text": "Hello world! This is a test tweet about Rust programming. #rust",
      "source": "<a href=\"https://x.com\">X Web App</a>",
      "favorite_count": "42",
      "retweet_count": "7",
      "lang": "en",
      "entities": {
        "hashtags": [{"text": "rust"}],
        "user_mentions": [],
        "urls": [{"expanded_url": "https://example.com"}]
      }
    }
  },
  {
    "tweet": {
      "id_str": "1234567890123456790",
      "created_at": "Thu Jan 09 14:30:00 +0000 2025",
      "full_text": "Learning about Tantivy search engine. Mentioning @alice for visibility.",
      "source": "<a href=\"https://x.com\">X Web App</a>",
      "favorite_count": "100",
      "retweet_count": "25",
      "lang": "en",
      "entities": {
        "hashtags": [],
        "user_mentions": [{"screen_name": "alice", "id_str": "42"}],
        "urls": []
      }
    }
  },
  {
    "tweet": {
      "id_str": "1234567890123456791",
      "created_at": "Fri Jan 10 09:15:00 +0000 2025",
      "full_text": "SQLite is an amazing embedded database.",
      "source": "<a href=\"https://x.com\">X Web App</a>",
      "favorite_count": "55",
      "retweet_count": "12",
      "lang": "en",
      "entities": {
        "hashtags": [{"text": "sqlite"}],
        "user_mentions": [],
        "urls": [],
        "media": [
          {"id_str": "media111", "media_url_https": "https://pbs.twimg.com/media/test.jpg"}
        ]
      }
    }
  }
]
JSON
fi

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

log "=== stats_detailed_test.sh starting ==="

if [ -z "${XF_DB:-}" ] || [ -z "${XF_INDEX:-}" ]; then
  run_cmd "index" "$XF_BIN" index "$ARCHIVE_DIR" --db "$DB_PATH" --index "$INDEX_PATH"
  if [ $LAST_STATUS -ne 0 ]; then
    fail "index command failed"
  fi
  pass "index command succeeded"
fi

run_cmd "stats_text" "$XF_BIN" stats --detailed --db "$DB_PATH" --index "$INDEX_PATH"
if [ $LAST_STATUS -ne 0 ]; then
  fail "stats --detailed text failed"
fi
if ! grep -q "Temporal Patterns" "$LAST_STDOUT"; then
  fail "missing Temporal Patterns section"
fi
if ! grep -q "Engagement Analytics" "$LAST_STDOUT"; then
  fail "missing Engagement Analytics section"
fi
if ! grep -q "Content Analysis" "$LAST_STDOUT"; then
  fail "missing Content Analysis section"
fi
if [ $LAST_DURATION -gt 2000 ]; then
  log "WARN: stats --detailed took ${LAST_DURATION}ms (over 2000ms)"
fi
pass "stats --detailed text output contains required sections"

run_cmd "stats_json" "$XF_BIN" stats --detailed --format json --db "$DB_PATH" --index "$INDEX_PATH"
if [ $LAST_STATUS -ne 0 ]; then
  fail "stats --detailed json failed"
fi
if ! jq . "$LAST_STDOUT" >/dev/null 2>&1; then
  fail "stats --detailed json output is invalid"
fi
if ! jq -e '.temporal and .engagement and .content' "$LAST_STDOUT" >/dev/null 2>&1; then
  fail "stats --detailed json missing temporal/engagement/content keys"
fi
pass "stats --detailed json output validated"

log "=== stats_detailed_test.sh completed ==="
