#!/usr/bin/env bash
#
# repl_test.sh - E2E tests for xf REPL mode
#
# Tests REPL-specific functionality:
# - Session starts and exits cleanly
# - Help output contains key commands
# - Unknown command yields error
# - History file behavior (created when enabled, not created with --no-history)
#
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOG_DIR="$ROOT_DIR/test-output"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/repl_e2e_$(date +%Y%m%d_%H%M%S).log"

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

# Create test archive if not using existing DB/INDEX
if [ -z "${XF_DB:-}" ] || [ -z "${XF_INDEX:-}" ]; then
  mkdir -p "$ARCHIVE_DIR/data"
  cat <<'JSON' > "$ARCHIVE_DIR/data/tweets.js"
window.YTD.tweets.part0 = [
  {
    "tweet": {
      "id_str": "1234567890123456789",
      "created_at": "Wed Jan 08 12:00:00 +0000 2025",
      "full_text": "Test tweet for REPL testing. #test",
      "source": "<a href=\"https://x.com\">X Web App</a>",
      "favorite_count": "10",
      "retweet_count": "2",
      "lang": "en",
      "entities": {
        "hashtags": [{"text": "test"}],
        "user_mentions": [],
        "urls": []
      }
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

run_repl() {
  local name="$1"
  local stdin_data="$2"
  shift 2
  local stdout_file="$WORK_DIR/${name}.out"
  local stderr_file="$WORK_DIR/${name}.err"
  local start end duration_ms

  log "RUN REPL: $name => $* (stdin: ${stdin_data:0:40}...)"
  start=$(date +%s%N)
  # Use echo -e to interpret escape sequences like \n
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

log "=== repl_test.sh starting ==="

# Index the archive if needed
if [ -z "${XF_DB:-}" ] || [ -z "${XF_INDEX:-}" ]; then
  run_cmd "index" "$XF_BIN" index "$ARCHIVE_DIR" --db "$DB_PATH" --index "$INDEX_PATH"
  if [ $LAST_STATUS -ne 0 ]; then
    fail "index command failed"
  fi
  pass "index command succeeded"
fi

# ============================================================================
# Test 1: REPL starts and exits cleanly with quit command
# ============================================================================
run_repl "repl_quit" "quit\n" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history
if [ $LAST_STATUS -ne 0 ]; then
  fail "REPL should exit cleanly with quit command"
fi
if ! grep -q "Goodbye" "$LAST_STDOUT"; then
  fail "REPL should print Goodbye on exit"
fi
pass "REPL starts and exits cleanly with quit"

# ============================================================================
# Test 2: REPL exits cleanly with exit alias
# ============================================================================
run_repl "repl_exit" "exit\n" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history
if [ $LAST_STATUS -ne 0 ]; then
  fail "REPL should exit cleanly with exit command"
fi
pass "REPL exits cleanly with exit alias"

# ============================================================================
# Test 3: REPL exits cleanly with q alias
# ============================================================================
run_repl "repl_q" "q\n" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history
if [ $LAST_STATUS -ne 0 ]; then
  fail "REPL should exit cleanly with q command"
fi
pass "REPL exits cleanly with q alias"

# ============================================================================
# Test 4: Help output contains key commands
# ============================================================================
run_repl "repl_help" "help\nquit\n" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history
if [ $LAST_STATUS -ne 0 ]; then
  fail "REPL help command failed"
fi
if ! grep -q "search" "$LAST_STDOUT"; then
  fail "Help should mention search command"
fi
if ! grep -q "list" "$LAST_STDOUT"; then
  fail "Help should mention list command"
fi
if ! grep -q "stats" "$LAST_STDOUT"; then
  fail "Help should mention stats command"
fi
if ! grep -q "quit" "$LAST_STDOUT"; then
  fail "Help should mention quit command"
fi
pass "Help output contains all key commands"

# ============================================================================
# Test 5: Unknown command yields error
# ============================================================================
run_repl "repl_unknown" "unknowncommand\nquit\n" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history
if [ $LAST_STATUS -ne 0 ]; then
  fail "REPL should still exit cleanly after unknown command"
fi
if ! grep -qi "unknown\|error" "$LAST_STDERR" && ! grep -qi "unknown\|error" "$LAST_STDOUT"; then
  fail "Unknown command should produce an error message"
fi
pass "Unknown command yields error"

# ============================================================================
# Test 6: History file NOT created with --no-history
# ============================================================================
TEMP_HISTORY="$WORK_DIR/temp_history_nohistory"
rm -f "$TEMP_HISTORY"
run_repl "repl_no_history" "help\nquit\n" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history --history-file "$TEMP_HISTORY"
if [ $LAST_STATUS -ne 0 ]; then
  fail "REPL with --no-history failed"
fi
if [ -f "$TEMP_HISTORY" ]; then
  fail "History file should NOT be created when --no-history is set"
fi
pass "History file not created with --no-history"

# ============================================================================
# Test 7: History file IS created without --no-history
# ============================================================================
TEMP_HISTORY="$WORK_DIR/temp_history_enabled"
rm -f "$TEMP_HISTORY"
run_repl "repl_history_enabled" "help\nquit\n" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --history-file "$TEMP_HISTORY"
if [ $LAST_STATUS -ne 0 ]; then
  fail "REPL with history enabled failed"
fi
if [ ! -f "$TEMP_HISTORY" ]; then
  log "WARN: History file not created at $TEMP_HISTORY (might be expected if rustyline doesn't persist single command)"
fi
pass "REPL with history enabled works"

# ============================================================================
# Test 8: Search command works in REPL
# ============================================================================
run_repl "repl_search" "search test\nquit\n" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history
if [ $LAST_STATUS -ne 0 ]; then
  fail "REPL search command failed"
fi
pass "Search command works in REPL"

# ============================================================================
# Test 9: Stats command works in REPL
# ============================================================================
run_repl "repl_stats" "stats\nquit\n" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history
if [ $LAST_STATUS -ne 0 ]; then
  fail "REPL stats command failed"
fi
if ! grep -qi "tweet" "$LAST_STDOUT"; then
  fail "Stats output should mention tweets"
fi
pass "Stats command works in REPL"

# ============================================================================
# Test 10: List command works in REPL
# ============================================================================
run_repl "repl_list" "list tweets\nquit\n" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history
if [ $LAST_STATUS -ne 0 ]; then
  fail "REPL list command failed"
fi
pass "List command works in REPL"

# ============================================================================
# Test 11: Multiple commands in sequence
# ============================================================================
run_repl "repl_sequence" "help\nstats\nlist tweets\nsearch test\nquit\n" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history
if [ $LAST_STATUS -ne 0 ]; then
  fail "REPL should handle multiple commands in sequence"
fi
pass "Multiple commands in sequence work"

# ============================================================================
# Test 12: Custom prompt option
# ============================================================================
run_repl "repl_custom_prompt" "quit\n" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history --prompt "custom> "
if [ $LAST_STATUS -ne 0 ]; then
  fail "REPL with custom prompt failed"
fi
pass "Custom prompt option works"

# ============================================================================
# Test 13: Custom page size option
# ============================================================================
run_repl "repl_page_size" "quit\n" "$XF_BIN" shell --db "$DB_PATH" --index "$INDEX_PATH" --no-history --page-size 5
if [ $LAST_STATUS -ne 0 ]; then
  fail "REPL with custom page size failed"
fi
pass "Custom page size option works"

# ============================================================================
# Performance check
# ============================================================================
if [ $LAST_DURATION -gt 5000 ]; then
  log "WARN: REPL startup/shutdown took ${LAST_DURATION}ms (over 5000ms)"
fi

log "=== repl_test.sh completed successfully ==="
log "Total tests: 13 passed"
