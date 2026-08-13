#!/usr/bin/env bash
set -euo pipefail

# Integration test suite for via.
# Requires: via and teetty on PATH.
# Usage: test/run.sh [path-to-via]

VIA="${1:-via}"
DIR="$(cd "$(dirname "$0")" && pwd)"
MOCK="$DIR/mock-repl.sh"
export REPLS_DIR="${REPLS_DIR:-$(mktemp -d)}"
PASS=0
FAIL=0
ERRORS=()

cleanup() { rm -rf "$REPLS_DIR"; }
trap cleanup EXIT

# ── helpers ──────────────────────────────────────────────────────────

pass() { PASS=$((PASS + 1)); printf '  \033[32mok\033[0m  %s\n' "$1"; }
fail() { FAIL=$((FAIL + 1)); ERRORS+=("$1: $2"); printf '  \033[31mFAIL\033[0m %s: %s\n' "$1" "$2"; }

# Assert that a command succeeds
assert_ok() {
  local name="$1"; shift
  if output=$("$@" 2>&1); then
    pass "$name"
  else
    fail "$name" "exit $? — $output"
  fi
}

# Assert that stdout contains a substring
assert_contains() {
  local name="$1" expected="$2"; shift 2
  local output
  if output=$("$@" 2>&1); then
    if [[ "$output" == *"$expected"* ]]; then
      pass "$name"
    else
      fail "$name" "expected '$expected' in: $output"
    fi
  else
    fail "$name" "exit $? — $output"
  fi
}

# Assert that stderr contains a substring (command may fail)
assert_stderr_contains() {
  local name="$1" expected="$2"; shift 2
  local output
  output=$("$@" 2>&1) || true
  if [[ "$output" == *"$expected"* ]]; then
    pass "$name"
  else
    fail "$name" "expected '$expected' in: $output"
  fi
}

# Assert command fails
assert_fails() {
  local name="$1"; shift
  if output=$("$@" 2>&1); then
    fail "$name" "expected failure but got success — $output"
  else
    pass "$name"
  fi
}

# Start a session in the background, wait for prompt
start_session() {
  local name="$1" delim="$2"
  "$VIA" "$name" run --delim "$delim" --bg -- bash "$MOCK" "$delim"
  "$VIA" "$name" wait --timeout 10 2>/dev/null
}

stop_session() {
  local name="$1"
  echo ':quit' | "$VIA" "$name" write 2>/dev/null || true
  sleep 0.5
}

# ── tests ────────────────────────────────────────────────────────────

echo "=== via integration tests ==="
echo "  VIA=$VIA"
echo "  REPLS_DIR=$REPLS_DIR"
echo ""

# ── run & delim storage ──────────────────────────────────────────────
echo "# run & delim storage"

start_session test-01 'mock>'

assert_contains "delim file written" "mock>" cat "$REPLS_DIR/test-01/delim"
assert_contains "command file written" "mock-repl.sh" cat "$REPLS_DIR/test-01/command"

# ── wait (stored delim) ─────────────────────────────────────────────
echo "# wait"

assert_stderr_contains "wait (bare)" "ready" "$VIA" test-01 wait --timeout 5
assert_stderr_contains "wait --until (bare)" "ready" "$VIA" test-01 wait --until --timeout 5
assert_stderr_contains "wait --until (explicit)" "ready" "$VIA" test-01 wait --until 'mock>' --timeout 5

# ── shorthand ────────────────────────────────────────────────────────
echo "# shorthand"

assert_contains "shorthand sends input" "=> hello" "$VIA" test-01 hello
assert_contains "shorthand with --timeout" "=> world" "$VIA" test-01 --timeout 10 world
assert_contains "piped stdin" "=> piped" sh -c "echo piped | $VIA test-01"

# ── tail bare flags ──────────────────────────────────────────────────
echo "# tail bare flags"

assert_contains "tail --since (bare)" "mock>" "$VIA" test-01 tail --since
assert_ok "tail --until (bare)" "$VIA" test-01 tail --until --timeout 5
assert_ok "tail --delim (bare)" "$VIA" test-01 tail --delim
assert_ok "tail --since --until (bare)" "$VIA" test-01 tail --since --until --timeout 5

# ── tail with explicit values ────────────────────────────────────────
echo "# tail explicit flags"

assert_contains "tail --since (explicit)" "mock>" "$VIA" test-01 tail --since 'mock>'
assert_ok "tail --until (explicit)" "$VIA" test-01 tail --until 'mock>' --timeout 5
assert_ok "tail --delim (explicit)" "$VIA" test-01 tail --delim 'mock>'

# ── session listing ──────────────────────────────────────────────────
echo "# session listing"

assert_contains "list shows session" "test-01" "$VIA"
assert_contains "list --simple" "test-01" "$VIA" --simple

# ── help ─────────────────────────────────────────────────────────────
echo "# help"

assert_contains "global help" "--delim" "$VIA" help
assert_contains "session help" "--delim" "$VIA" test-01 help

# ── auto-generated session name ──────────────────────────────────────
echo "# auto-generated name"

"$VIA" run --delim 'mock>' --bg -- bash "$MOCK" 'mock>'
assert_contains "auto name listed" "bash-00" "$VIA" --simple
echo ':quit' | "$VIA" bash-00 write 2>/dev/null || true
sleep 0.5

# ── long output ──────────────────────────────────────────────────────
echo "# long output"

"$VIA" test-01 --timeout 60 ':long 200' >/dev/null 2>&1
delim_lines=$("$VIA" test-01 tail --delim 2>&1 | wc -l)
# 201 = prompt line + 200 output lines
if [[ "$delim_lines" -eq 201 ]]; then
  pass "tail --delim 200 lines"
else
  fail "tail --delim 200 lines" "expected 201 lines, got $delim_lines"
fi

"$VIA" test-01 --timeout 120 ':long 500' >/dev/null 2>&1
delim_lines=$("$VIA" test-01 tail --delim 2>&1 | wc -l)
if [[ "$delim_lines" -eq 501 ]]; then
  pass "tail --delim 500 lines"
else
  fail "tail --delim 500 lines" "expected 501 lines, got $delim_lines"
fi

# ── cleanup ──────────────────────────────────────────────────────────

stop_session test-01

# ── --bg cleanup ─────────────────────────────────────────────────────
echo "# --bg cleanup"

start_session test-bg 'mock>'
if [ -d "$REPLS_DIR/test-bg" ]; then
  pass "--bg session dir exists while running"
else
  fail "--bg session dir exists while running" "dir missing"
fi
echo ':quit' | "$VIA" test-bg write 2>/dev/null || true
sleep 1
if [ ! -d "$REPLS_DIR/test-bg" ]; then
  pass "--bg session cleaned up after exit"
else
  fail "--bg session cleaned up after exit" "dir still exists"
fi

# ── liveness & stale sessions ────────────────────────────────────────
echo "# liveness & stale sessions"

start_session test-live 'mock>'

assert_ok "pid file written" test -f "$REPLS_DIR/test-live/pid"
assert_ok "recorded pid is running" kill -0 "$(cat "$REPLS_DIR/test-live/pid")"
assert_contains "live session listed as live" "live" "$VIA"

# Fabricate a stale session: valid layout, but its process is gone
sleep 0.1 &
DEAD_PID=$!
wait "$DEAD_PID"
mkdir -p "$REPLS_DIR/test-dead"
printf 'fake-command\n' > "$REPLS_DIR/test-dead/command"
printf 'mock>\n' > "$REPLS_DIR/test-dead/delim"
printf 'mock> ' > "$REPLS_DIR/test-dead/stdout"
printf '%s\n' "$DEAD_PID" > "$REPLS_DIR/test-dead/pid"
mkfifo "$REPLS_DIR/test-dead/stdin"

assert_contains "dead session listed as stale" "stale" "$VIA"
assert_stderr_contains "write to dead session fails fast" "appears to be dead" \
  timeout 5 "$VIA" test-dead hello
rm -rf "$REPLS_DIR/test-dead"

stop_session test-live

# ── SIGTERM cleanup ──────────────────────────────────────────────────
echo "# SIGTERM cleanup"

"$VIA" test-sig run --delim 'mock>' -- bash "$MOCK" 'mock>' >/dev/null 2>&1 &
SUP_PID=$!
for _ in $(seq 50); do [ -f "$REPLS_DIR/test-sig/pid" ] && break; sleep 0.1; done
"$VIA" test-sig wait --timeout 10 >/dev/null 2>&1 || true
TEETTY_PID=$(cat "$REPLS_DIR/test-sig/pid")
kill -TERM "$SUP_PID"
sleep 1
assert_fails "SIGTERM removes session dir" test -e "$REPLS_DIR/test-sig"
assert_fails "SIGTERM kills teetty" kill -0 "$TEETTY_PID"

# ── error cases (no session running) ─────────────────────────────────
echo "# error cases"

assert_fails "wait no session" "$VIA" nonexistent wait
assert_fails "shorthand no session" "$VIA" nonexistent 'hello'

# ── summary ──────────────────────────────────────────────────────────
echo ""
echo "=== $PASS passed, $FAIL failed ==="
if [[ $FAIL -gt 0 ]]; then
  echo "Failures:"
  for e in "${ERRORS[@]}"; do
    echo "  - $e"
  done
  exit 1
fi
