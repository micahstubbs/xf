#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOG_DIR="$ROOT_DIR/test-output"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/dm_context_e2e_$(date +%Y%m%d_%H%M%S).log"

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

log "=== dm_context_test.sh starting ==="

# Create test archive with DM conversations
mkdir -p "$ARCHIVE_DIR/data"

# Create DMs test file with two conversations
cat <<'JSON' > "$ARCHIVE_DIR/data/direct-messages.js"
window.YTD.direct_messages.part0 = [
  {
    "dmConversation": {
      "conversationId": "conv_alice_bob",
      "messages": [
        {
          "messageCreate": {
            "id": "dm001",
            "senderId": "alice123",
            "recipientId": "bob456",
            "createdAt": "2025-01-08T10:00:00.000Z",
            "text": "Hello Bob! How are you?"
          }
        },
        {
          "messageCreate": {
            "id": "dm002",
            "senderId": "bob456",
            "recipientId": "alice123",
            "createdAt": "2025-01-08T10:05:00.000Z",
            "text": "Hi Alice! I am doing great, thanks for asking!"
          }
        },
        {
          "messageCreate": {
            "id": "dm003",
            "senderId": "alice123",
            "recipientId": "bob456",
            "createdAt": "2025-01-08T10:10:00.000Z",
            "text": "That is wonderful to hear. Let us meet for coffee sometime."
          }
        }
      ]
    }
  },
  {
    "dmConversation": {
      "conversationId": "conv_alice_charlie",
      "messages": [
        {
          "messageCreate": {
            "id": "dm004",
            "senderId": "charlie789",
            "recipientId": "alice123",
            "createdAt": "2025-01-09T14:00:00.000Z",
            "text": "Hey Alice! Did you see the Rust news?"
          }
        },
        {
          "messageCreate": {
            "id": "dm005",
            "senderId": "alice123",
            "recipientId": "charlie789",
            "createdAt": "2025-01-09T14:30:00.000Z",
            "text": "Yes! Rust 2024 edition is coming. I am excited about it."
          }
        }
      ]
    }
  }
]
JSON

# Index the archive
run_cmd "index" "$XF_BIN" index "$ARCHIVE_DIR" --db "$DB_PATH" --index "$INDEX_PATH"
if [ $LAST_STATUS -ne 0 ]; then
  fail "index command failed"
fi
pass "index command succeeded"

# Test 1: Search DMs without --context (should work normally)
run_cmd "dm_search_basic" "$XF_BIN" search "Alice" --types dm --db "$DB_PATH" --index "$INDEX_PATH"
if [ $LAST_STATUS -ne 0 ]; then
  fail "basic DM search failed"
fi
pass "basic DM search works"

# Test 2: Search DMs with --context (should show conversation context)
run_cmd "dm_context_text" "$XF_BIN" search "Alice" --types dm --context --db "$DB_PATH" --index "$INDEX_PATH"
if [ $LAST_STATUS -ne 0 ]; then
  fail "DM context search failed"
fi
# Check that output contains conversation header
if ! grep -q "Conversation" "$LAST_STDOUT"; then
  fail "DM context output missing conversation header"
fi
pass "DM context text output works"

# Test 3: Search DMs with --context and JSON output
run_cmd "dm_context_json" "$XF_BIN" search "Alice" --types dm --context --format json --db "$DB_PATH" --index "$INDEX_PATH"
if [ $LAST_STATUS -ne 0 ]; then
  fail "DM context JSON search failed"
fi
# Check JSON structure
if ! grep -q "conversation_id" "$LAST_STDOUT"; then
  fail "DM context JSON missing conversation_id field"
fi
if ! grep -q "is_match" "$LAST_STDOUT"; then
  fail "DM context JSON missing is_match field"
fi
pass "DM context JSON output works"

# Test 4: --context without --types dm should fail with clear error
run_cmd "dm_context_wrong_type" "$XF_BIN" search "test" --context --db "$DB_PATH" --index "$INDEX_PATH"
if [ $LAST_STATUS -eq 0 ]; then
  fail "--context without --types dm should fail"
fi
if ! grep -qi "dm" "$LAST_STDERR"; then
  fail "--context error message should mention DM"
fi
pass "--context without DM types correctly fails"

# Test 5: --context with CSV should fail (not supported yet)
run_cmd "dm_context_csv" "$XF_BIN" search "Alice" --types dm --context --format csv --db "$DB_PATH" --index "$INDEX_PATH"
if [ $LAST_STATUS -eq 0 ]; then
  fail "--context with CSV should fail (not yet supported)"
fi
pass "--context with CSV correctly reports unsupported"

# Test 6: JSON schema validation (if jq is available)
if command -v jq &> /dev/null; then
  run_cmd "dm_context_json_validate" "$XF_BIN" search "Alice" --types dm --context --format json --db "$DB_PATH" --index "$INDEX_PATH"
  if ! jq -e '.[0].conversation_id' "$LAST_STDOUT" > /dev/null 2>&1; then
    fail "JSON schema: missing conversation_id at top level"
  fi
  if ! jq -e '.[0].messages[0].id' "$LAST_STDOUT" > /dev/null 2>&1; then
    fail "JSON schema: missing message id"
  fi
  if ! jq -e '.[0].messages[0].is_match' "$LAST_STDOUT" > /dev/null 2>&1; then
    fail "JSON schema: missing is_match flag"
  fi
  pass "JSON schema validation passed"
else
  log "SKIP: jq not available for JSON schema validation"
fi

# Test 7: Multiple matches in same conversation
run_cmd "dm_context_multi_match" "$XF_BIN" search "alice" --types dm --context --format json --db "$DB_PATH" --index "$INDEX_PATH"
if [ $LAST_STATUS -ne 0 ]; then
  fail "DM context multi-match search failed"
fi
# There should be messages with is_match=true
if ! grep -q '"is_match":true' "$LAST_STDOUT"; then
  fail "No matched messages found in output"
fi
pass "DM context multi-match works"

log "=== dm_context_test.sh completed ==="
log "All tests passed!"
