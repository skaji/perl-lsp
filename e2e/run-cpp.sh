#!/usr/bin/env bash
# C++ e2e: builds a --features all-langs release and drives cpp-lsp in headless
# nvim (reuses the same lua harness as the Perl suites). Separate from
# run.sh because it needs the cpp-feature binary.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

# Reap our own servers on exit — see e2e/run.sh for the rationale. A hung/killed
# nvim orphans its perl-lsp child (reparented to init, same process group). The
# parent-liveness monitor self-exits it within ~10s; this trap is immediate.
our_pgid=$(ps -o pgid= -p $$ | tr -d ' ')
declare -A _preexisting_lsp=()
for _p in $(pgrep -x perl-lsp 2>/dev/null || true); do _preexisting_lsp[$_p]=1; done
reap_servers() {
  local pid pgid
  for pid in $(pgrep -x perl-lsp 2>/dev/null || true); do
    [[ -n "${_preexisting_lsp[$pid]:-}" ]] && continue
    pgid=$(ps -o pgid= -p "$pid" 2>/dev/null | tr -d ' ')
    [[ "$pgid" == "$our_pgid" ]] && kill -TERM "$pid" 2>/dev/null || true
  done
}
trap reap_servers EXIT

echo "building --features all-langs release..."
cargo build --release --features all-langs >/dev/null 2>&1
PERL_LSP_BIN="$PWD/target/release/perl-lsp" \
  nvim --headless --clean -u e2e/init_cpp.lua -l e2e/cpp.lua
  PERL_LSP_BIN="$PWD/target/release/perl-lsp" \
  nvim --headless --clean -u e2e/init_cpp.lua -l e2e/cpp_members.lua
  nvim --headless --clean -u e2e/init_cpp.lua -l e2e/cpp_locals.lua
  nvim --headless --clean -u e2e/init_cpp.lua -l e2e/cpp_macro_calls.lua
  nvim --headless --clean -u e2e/init_cpp.lua -l e2e/cpp_labels.lua
  PERL_LSP_BIN="$PWD/target/release/perl-lsp" \
  nvim --headless --clean -u e2e/init_cpp.lua -l e2e/cpp_member_op.lua
  PERL_LSP_BIN="$PWD/target/release/perl-lsp" \
  nvim --headless --clean -u e2e/init_cpp.lua -l e2e/cpp_header_edit.lua

PERL_LSP_BIN="$PWD/target/release/perl-lsp" \
  nvim --headless --clean -u e2e/init_python.lua -l e2e/python_members.lua
