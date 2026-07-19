---
name: edit-bench
description: Realistic-editing LSP benchmark rounds against real projects — cold/warm startup, per-verb latency, diagnostics push, RSS — for every language perl-lsp serves. Use to measure a change's real-editor cost, add a project to the corpus, or refresh bench/RESULTS.md.
---

# edit-bench: measure what an editor user feels

The harness: `bench/lsp_bench.py` (LSP-over-stdio playback driver; scenario
schema documented in its docstring), committed scenarios in
`bench/scenarios/*.json`, and the running ledger `bench/RESULTS.md`
(append one section per round; never rewrite old rounds — trends are the
point).

A **round** = for each project: `--clear-cache <root>` → COLD run → WARM
run (fresh server per run), strictly sequential on a QUIET box, then
append results + findings to the ledger.

## Protocol

1. **Build the binary first**: `cargo build --release --features cpp`.
   (`cargo test --release` without the flag silently rebuilds perl-only —
   verify with `perl-lsp --languages`.)
2. **Clone projects** (shallow) into scratchpad, one dir per project.
   Existing corpus: bugzilla (Perl), abseil (C++), redis (C — root at
   `src/`, NOT repo root; deps/ would bloat the index). Each scenario's
   `project` key records repo + root_subdir.
3. **Re-anchor before running**: upstream HEADs drift. For every scenario,
   verify each probe answers NON-EMPTY via the CLI mirrors at the
   scenario's coordinates, e.g.
   `perl-lsp --definition <root> <abs-file> <line> <col>`.
   **The positional CLI is 0-BASED (line and byte col), same as the
   scenario JSON — coordinates transfer verbatim.** (Only `--at f:l:c` is
   1-based.) If a probe dead-ends, re-locate the anchor line by content
   (grep the token) and update the scenario coords in a commit.
4. **Run**, sequentially, nothing else on the box:
   ```
   perl-lsp --clear-cache <root>
   python3 bench/lsp_bench.py --bin target/release/perl-lsp --root <root> \
     --scenario bench/scenarios/<p>.json --out <out>/cold.json --label cold
   python3 bench/lsp_bench.py ... --out <out>/warm.json --label warm
   ```
5. **Append to `bench/RESULTS.md`**: the round header (date, commit,
   machine cores/RAM), the summary table, per-project anomalies, and new
   findings (each with the metric that evidences it). Commit.

## Adding a project

Spawn a prep subagent per project (they parallelize; measurement never
does). The agent: clones, reads real code, authors a ~15-20 step scenario
telling a realistic story — open a busy file; hover/def/references (pick
a symbol with MANY call sites — the honest expensive case) / member
completion; two body edits + revert (each `await_diagnostics`); a
contract edit (new sub / header prototype — the freshness-invalidation
path) + revert; a cross-file consumer open + nav; `rss` checkpoints
between phases — then **CLI-validates every probe non-empty** and writes
`bench/scenarios/<project>.json`. Prep agents report misbehavior they
notice (wrong/empty answers) as findings — they fix nothing.

## Reading the numbers honestly (traps that already bit)

- **`result_size` is the honesty column.** A fast cold answer with a
  small size next to a big warm size = the index served a PARTIAL result
  that looked complete (seen: abseil cold references 3.6 KB vs warm
  12.5 KB). Report it as a finding, not a win.
- **~400 ms plateaus are the bounded cold-wait quantum** (`await_open_ready`
  / `await_index_ready` caps), not intrinsic verb cost.
- **`open` result_size=4 (`null`)** = documentSymbol missed the bounded
  wait — the outline never landed in-response (editors heal via the
  refresh nudge; the miss is still worth recording, especially WARM).
- **cpp/c "ready" is first-file interactivity**, not whole-workspace
  index completion — the open doc resolves into its headers while the
  bulk index still runs; RSS keeps growing after ready.
- **`diagnostics_ms: null`** = no publishDiagnostics arrived in 60 s —
  a finding (which language tier stayed silent?).
- Scratchpad clones and metrics die with container restarts — anything
  worth keeping goes into `bench/RESULTS.md` (or the scenario files)
  the same session it's measured.

## Fix loop

Findings accumulate in the ledger's per-round "Findings" list with
status tags (NEW / KNOWN / FIXED-BY). When a fixing round lands, re-run
the affected scenarios and tag the finding FIXED-BY <commit> with the
before/after numbers — the ledger doubles as the regression net for
editor-feel, the tier `cargo test` and gold can't see.
