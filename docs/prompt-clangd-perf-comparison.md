# perl-lsp vs clangd — performance & value comparison (teed up)

Forward-work brief. Not yet run. The quantitative arm of the market-survey
thesis (`docs/cpp-lsp-experience-research.md`): we don't have to be
compiler-grade everywhere to be the better daily driver — we need to
KNOW, with numbers, which axes we win and which clangd owns, and not
overclaim in either direction.

## The framing (read before measuring anything)

clangd is compiler-grade: real preprocessing, full semantic analysis,
template instantiation. On DEEP semantic queries — overload resolution,
SFINAE, dependent types — it is more accurate and we will not beat it.
That is fine and must be stated honestly.

Our claim is a different axis bundle:
1. **Time-to-first-value on a fresh clone** — clangd needs
   `compile_commands.json` (a build-system integration step) and builds a
   preamble/index before the first useful answer; we need nothing but the
   source tree. On this box: clangd is not installed, and NONE of the
   cpp-bench corpora ship a `compile_commands.json`. That setup delta is
   itself finding #1 — measure the wall-clock from `git clone` to first
   correct goto-def for each tool, setup included.
2. **Works on un-buildable / partial code** — a file mid-edit, a header
   out of its TU, a project that doesn't configure. clangd degrades hard
   without a valid compile command; we parse anything.
3. **Macro / config-variant navigation** — navigating an INACTIVE `#ifdef`
   arm, a `#define` superposition with labeled multi-target gd. clangd
   picks one config; we show all arms. (The macro arc is the headline DX
   differentiator — quantify the coverage, not just latency.)

So the comparison is NOT "beat clangd on accuracy." It is: on the axes we
claim, quantify the win; on the axes clangd owns, measure the gap so we
know where we stand and can point users at the honest line.

## What to measure

Perf axes (same corpus, same queries, same machine, tool versions pinned):
- **Cold time-to-first-answer** — server start (or file open) → first
  CORRECT goto-def. clangd's weak spot (preamble + index build). Targets:
  op.c (16k), the amalgamated json.hpp (24k), an abseil TU.
- **Warm query latency** — gd / references / hover / completion once
  indexed. clangd's strong spot (full AST). Expect competitive-to-slower;
  the numbers matter, especially references and completion.
- **Memory (peak RSS)** — clangd holds ASTs + preamble (often GB-scale);
  our abseil `--batch` was ~4.2 GB (dogfood note) — measure both on the
  same corpus, don't assume.
- **Full-workspace index time** — abseil / redis end-to-end.
- **Incremental reparse latency** — edit → updated answer (the typing-feel
  metric; ties to the salvage/incremental-reparse work).
- **Setup cost** — generating `compile_commands.json` (CMake
  `-DCMAKE_EXPORT_COMPILE_COMMANDS=ON`) for clangd; zero for us. Wall-clock
  it — it's a real part of the first-value story.

Accuracy axes (so perf numbers aren't read in a vacuum — a fast wrong
answer is not a win): on a fixed query set spanning BOTH the common path
(gd on a local method, references on a free function) and the
differentiator path (gd on a macro, gd across an inactive `#ifdef`, gd in
an unbuildable file), record correct / wrong / no-answer for each tool.
This is where clangd wins the deep cases and we win the macro/partial
cases; the table is the deliverable.

## Prerequisites / setup (all currently ABSENT on this box)
- Install `clangd` (pin the version in the report).
- Install `hyperfine` (repeatable latency runs) or fall back to
  `/usr/bin/time -v` for RSS + wall.
- Generate `compile_commands.json` per corpus (CMake export, or `bear` for
  the make-based ones like redis) — and TIME that step.
- Drive both servers over the LSP protocol with one harness (reuse the
  `e2e/*.lua` nvim driver, or a headless LSP client) firing an identical
  scripted query set at identical positions; our CLI `--batch` path is
  synchronous-startup and will UNDER-report our cold cost vs the real LSP
  session — measure the LSP path, not `--batch`, for the cold numbers.

## Harness shape
A committed script (`e2e/clangd-compare.sh` or similar) that, per corpus:
warms nothing → opens the target file in each server → fires the scripted
query set → records (tool, query, cold/warm, latency, peak RSS, verdict) to
a CSV → emits the comparison table. Repeatable so it can gate our own
perf regressions later, not just a one-shot.

## What "good" looks like (honest expectations)
- We SHOULD win cold-time-to-first-value decisively (no build needed) and
  win/only-we-answer on macro-nav, inactive-config, and unbuildable-file
  queries.
- We will likely LOSE some warm deep-semantic accuracy (overloads,
  templates) and possibly some warm latency on references/completion.
- Memory is genuinely unknown — measure, don't assume either way.
- The win condition for "solid daily driver" is not "beat clangd
  everywhere" — it's "clearly better on the fresh-clone / partial-code /
  macro axes, honestly competitive on the common warm path, with the
  deep-semantic gap named so nobody is surprised."

## Sequencing
Run AFTER the daily-driver push settles (benchmarking while fix agents
saturate CPU poisons the numbers). Not blocking any current slice. A
pointer from `docs/ROADMAP.md` / `docs/cpp-golive-map.md` (ARC 4 / ARC 5
ship-gate) gets added at the next sweep so this is findable.
