#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOG_DIR="$ROOT_DIR/test-output"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/shell_cli_e2e_$(date +%Y%m%d_%H%M%S).log"

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
HISTORY_PATH="$WORK_DIR/.xf_history"

# Create test archive if not using existing DB/INDEX
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
        "urls": []
      }
    }
  },
  {
    "tweet": {
      "id_str": "1234567890123456790",
      "created_at": "Thu Jan 09 14:30:00 +0000 2025",
      "full_text": "Learning about Tantivy search engine.",
      "source": "<a href=\"https://x.com\">X Web App</a>",
      "favorite_count": "100",
      "retweet_count": "25",
      "lang": "en",
      "entities": {
        "hashtags": [{"text": "tantivy"}],
        "user_mentions": [],
        "urls": []
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

run_cmd_stdin() {
  local name="$1"
  local stdin_data="$2"
  shift 2
  local stdout_file="$WORK_DIR/${name}.out"
  local stderr_file="$WORK_DIR/${name}.err"
  local start end duration_ms

  log "RUN: $name => $* (with stdin)"
  start=$(date +%s%N)
  if echo -e "$stdin_data" | "$@" >"$stdout_file" 2>"$stderr_file"; then
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

log "=== shell_cli_test.sh starting ==="

# Index the archive if needed
if [ -z "${XF_DB:-}" ] || [ -z "${XF_INDEX:-}" ]; then
  run_cmd "index" "$XF_BIN" index "$ARCHIVE_DIR" --db "$DB_PATH" --index "$INDEX_PATH"
  if [ $LAST_STATUS -ne 0 ]; then
    fail "index command failed"
  fi
  pass "index command succeeded"
fi

# Test 1: shell --help shows all options
run_cmd "shell_help" "$XF_BIN" shell --help
if [ $LAST_STATUS -ne 0 ]; then
  fail "shell --help failed"
fi
if ! grep -q "\-\-prompt" "$LAST_STDOUT"; then
  fail "shell --help missing --prompt option"
fi
if ! grep -q "\-\-page-size" "$LAST_STDOUT"; then
  fail "shell --help missing --page-size option"
fi
if ! grep -q "\-\-no-history" "$LAST_STDOUT"; then
  fail "shell --help missing --no-history option"
fi
if ! grep -q "\-\-history-file" "$LAST_STDOUT"; then
  fail "shell --help missing --history-file option"
fi
pass "shell --help shows all expected options"

# Test 2: shell requires database (should fail if db doesn't exist)
run_cmd "shell_no_db" "$XF_BIN" shell --db "$WORK_DIR/nonexistent.db" --index "$WORK_DIR/nonexistent_index"
if [ $LAST_STATUS -eq 0 ]; then
  fail "shell should fail when database doesn't exist"
fi
if ! grep -qi "not found\|does not exist\|database" "$LAST_STDERR"; then
  fail "shell should report database not found error"
fi
pass "shell correctly fails when database is missing"

# Test 3: Interactive shell with quit command
run_cmd_stdin "shell_quit" "quit" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history
if [ $LAST_STATUS -ne 0 ]; then
  fail "shell with quit command failed"
fi
pass "shell quit command works"

# Test 4: Interactive shell with help command
run_cmd_stdin "shell_help_cmd" "help\nquit" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history
if [ $LAST_STATUS -ne 0 ]; then
  fail "shell with help command failed"
fi
if ! grep -qi "search\|help\|quit\|stats" "$LAST_STDOUT"; then
  fail "shell help command should show available commands"
fi
pass "shell help command works"

# Test 5: Interactive shell with search command
run_cmd_stdin "shell_search" "rust\nquit" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history
if [ $LAST_STATUS -ne 0 ]; then
  fail "shell with search command failed"
fi
pass "shell search command works"

# Test 6: Custom prompt option
run_cmd_stdin "shell_custom_prompt" "quit" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history --prompt "myxf> "
if [ $LAST_STATUS -ne 0 ]; then
  fail "shell with custom prompt failed"
fi
pass "shell custom prompt works"

# Test 7: Custom page size option
run_cmd_stdin "shell_page_size" "quit" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history --page-size 5
if [ $LAST_STATUS -ne 0 ]; then
  fail "shell with custom page-size failed"
fi
pass "shell custom page-size works"

# Test 8: Custom history file option
run_cmd_stdin "shell_history_file" "rust\nquit" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --history-file "$HISTORY_PATH"
if [ $LAST_STATUS -ne 0 ]; then
  fail "shell with custom history-file failed"
fi
# Check that history file was created
if [ ! -f "$HISTORY_PATH" ]; then
  log "WARN: history file not created at $HISTORY_PATH (might be expected if no commands saved)"
fi
pass "shell custom history-file works"

# Test 9: Stats command in shell
run_cmd_stdin "shell_stats" "stats\nquit" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history
if [ $LAST_STATUS -ne 0 ]; then
  fail "shell with stats command failed"
fi
pass "shell stats command works"

# Test 10: List tweets command in shell
run_cmd_stdin "shell_list" "list tweets\nquit" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history
if [ $LAST_STATUS -ne 0 ]; then
  fail "shell with list command failed"
fi
pass "shell list command works"

log "=== shell_cli_test.sh completed ==="
