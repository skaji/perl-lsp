#!/usr/bin/env bash
# Server-lifecycle repro: proves the LSP-spec parent-liveness monitor reaps a
# server whose editor died HARD, plus the two clean exit paths. The load-bearing
# case (A) is the one the old stdin-EOF fix could NOT cover: stdin stays open
# (so no EOF fires) and we SIGKILL the process whose `processId` we sent at
# `initialize` — only the independent liveness timer can exit the server.
#
# Exit 0 = all cases pass. Run from anywhere; needs a built binary.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

bin="${PERL_LSP_BIN:-./target/release/perl-lsp}"
if [[ ! -x "$bin" ]]; then
  echo "orphan-repro: no binary at $bin (build release first)" >&2
  exit 2
fi

tmp="$(mktemp -d)"
pids=()
cleanup() {
  # Reap everything we spawned so a failed assertion never leaks a server.
  for p in "${pids[@]}"; do kill -9 "$p" 2>/dev/null || true; done
  rm -rf "$tmp"
}
trap cleanup EXIT

# A process is "running" iff /proc/<pid> exists AND it isn't a zombie (an
# unreaped child of this script shows state Z but is functionally dead). State
# is the first char after the final ")" in /proc/<pid>/stat (comm may hold
# spaces/parens, so split on ")").
running() {
  local line state
  line=$(cat "/proc/$1/stat" 2>/dev/null) || return 1
  state=${line##*) }
  [[ "${state:0:1}" != "Z" ]]
}

# Poll until $1 stops running or $2 seconds elapse. Echoes elapsed seconds.
wait_gone() {
  local pid=$1 timeout=$2 elapsed=0
  while running "$pid"; do
    if (( elapsed >= timeout )); then echo "$elapsed"; return 1; fi
    sleep 1; elapsed=$((elapsed + 1))
  done
  echo "$elapsed"; return 0
}

frame() { # $1 = json body → Content-Length framed on stdout
  printf 'Content-Length: %d\r\n\r\n%s' "${#1}" "$1"
}

fail=0

# ── Case A: hard SIGKILL of the parent (processId) — the leak scenario ──
echo "── Case A: parent SIGKILL, stdin held open ──"
sleep 600 & parent=$!; pids+=("$parent")
fifo="$tmp/stdin_a"; mkfifo "$fifo"
"$bin" < "$fifo" > "$tmp/out_a" 2> "$tmp/err_a" & server=$!; pids+=("$server")
exec 4> "$fifo"   # hold stdin open so no EOF can fire
frame "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"processId\":$parent,\"rootUri\":null,\"capabilities\":{}}}" >&4
sleep 2           # let initialize run + the liveness monitor spawn
if ! running "$server"; then
  echo "  ✗ server exited before parent was killed (unexpected)"; fail=1
else
  kill -9 "$parent"; wait "$parent" 2>/dev/null || true   # reap → /proc/<parent> vanishes
  if secs=$(wait_gone "$server" 20); then
    echo "  ✓ server exited ${secs}s after parent SIGKILL (poll cadence 10s + margin)"
  else
    echo "  ✗ server STILL ALIVE ${secs}s after parent SIGKILL — leak not fixed"; fail=1
  fi
fi
exec 4>&-         # release stdin
rm -f "$fifo"

# ── Case B: stdin EOF → clean exit ──
echo "── Case B: stdin EOF ──"
# processId=null disables the monitor; only the EOF path can exit here.
frame '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"rootUri":null,"capabilities":{}}}' \
  | "$bin" > "$tmp/out_b" 2> "$tmp/err_b" & server=$!; pids+=("$server")
if secs=$(wait_gone "$server" 15); then
  echo "  ✓ server exited ${secs}s after stdin closed"
else
  echo "  ✗ server did not exit on EOF (${secs}s)"; fail=1
fi

# ── Case C: the graceful sequence — shutdown, exit, close connection ──
# This is exactly what a spec-compliant client does (shutdown request → exit
# notification → close its end of the pipe). tower-lsp's exit layer flags the
# server Exited; the read loop then unwinds on the connection close (EOF). We
# send all three and close stdin, mirroring a real editor's teardown.
echo "── Case C: shutdown, exit, close connection ──"
{
  frame '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"rootUri":null,"capabilities":{}}}'
  frame '{"jsonrpc":"2.0","id":2,"method":"shutdown"}'
  frame '{"jsonrpc":"2.0","method":"exit"}'
} | "$bin" > "$tmp/out_c" 2> "$tmp/err_c" & server=$!; pids+=("$server")
if secs=$(wait_gone "$server" 15); then
  echo "  ✓ server exited ${secs}s after shutdown+exit+close"
else
  echo "  ✗ server did not exit on shutdown+exit+close (${secs}s)"; fail=1
fi

echo "════════════════════════════════════════════"
if (( fail == 0 )); then
  echo "orphan-repro: ALL CASES PASS"; exit 0
else
  echo "orphan-repro: FAILURES ABOVE"; exit 1
fi
