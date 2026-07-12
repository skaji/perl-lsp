# Chromium whole-tree scale analysis (2026-07-06)

What happens when perl-lsp's whole-tree indexer is pointed at Chromium — the
canonical "too big" C++ corpus. Run on `spike/cpp-support` @ `6bd6cd1a`
(EXTRACT_VERSION 163), the state after Memory Slice 2 (per-file witness-bag
eviction) landed. The one-line answer: **at today's resident model whole-tree
Chromium projects to ~67 GB of RAM**, and the per-file cost is dead-linear, so
no amount of scale buys sublinear savings — the fix is a storage-layer change,
not another eviction slice.

## Corpus

Shallow clone (`git clone --depth 1 chromium/src`, no `gclient`, so **no
`third_party`** — v8/skia/angle/etc. live in separate repos):

- 6.9 GB on disk, **500,639 files total**, **131,099 C/C++ files**
  (`*.cc/*.cpp/*.cxx/*.h/*.hpp/*.hxx`).
- This is the *lean* Chromium. A full `gclient sync` checkout balloons past a
  million files and ~30 GB+; the numbers below are a **lower bound** on the
  real thing.

## Method

- `perl-lsp --workspace-symbol <chromium> _NONEXISTENT_` — drives
  `cli_full_startup`, i.e. the full whole-tree pack-language index (the same
  path the LSP server runs at startup), then a symbol query that matches
  nothing (we only care about the indexing cost).
- **RAM guard**: a wrapper polls the process `VmRSS` once a second and
  `kill -9`s it if it crosses a **20 GB** ceiling, so the box (31 GB total,
  ~27 GB available) can never swap-die. Peak RSS + wall time are captured
  whether it completes or is capped.
- Two smaller corpora indexed the same way as calibration points.

## Results

| corpus | C++ files | peak RSS | resident payload | wall time | completed? |
|---|---|---|---|---|---|
| abseil-cpp | 1,222 | 632 MB | 248 MB | ~9 s | yes |
| folly | 2,415 | 1.20 GB | 626 MB | 25 s | yes |
| **chromium** | **131,099** | **20.0 GB (capped)** | — | **~18 min (to cap)** | **no — killed at ceiling** |

The Chromium run tripped the 20 GB guard at **t = 1073 s**. The box recovered
cleanly on the kill: RAM back to 27 GB free, **0 orphan processes**, no swap
thrash — the guard worked exactly as intended.

### MEASURED vs INFERRED

- **MEASURED**: abseil and folly peak RSS + payload + wall time (both ran to
  completion with a `PERL_LSP_HEAP_DUMP`). Chromium peak RSS (20.0 GB) and
  time-to-cap (1073 s).
- **INFERRED**: the Chromium **file count at the cap (~38–39K)** — the run was
  killed mid-index, so the "Indexed N files" completion line never printed.
  38K is `20 GB ÷ 0.51 MB/file`, using the per-file cost calibrated below.
  Treat it as an estimate, not a count.

## The load-bearing finding: per-file resident cost is linear

Peak RSS ÷ files indexed:

- abseil: 647,608 KB ÷ 1,222 = **0.52 MB/file**
- folly: 1,231,856 KB ÷ 2,415 = **0.51 MB/file**

Identical across a 2× file-count spread, and the Chromium RSS curve climbed
**without inflection** from 400 MB straight to 20 GB — no plateau, no
sublinear bend. So the model is trustworthy:

> **whole-tree Chromium ≈ 131,099 × 0.51 MB ≈ 67 GB resident.**

That is ~3.4× the 20 GB leash and ~2.5× the machine's total RAM. Whole-tree
Chromium does not fit at today's resident model, and scale will never rescue
it — every file adds a fixed resident cost that never leaves.

## Why 0.51 MB/file — and what Slice 2 already bought

Memory Slice 2 (`docs/adr/memory-slice-2-lru.md`) evicts each file's **witness
bag** to the SQLite blob *during* indexing (per-file, in the Rayon worker:
`module_resolver.rs` serializes the full bag, then `evict_witness_bag()` strips
the resident copy) and rehydrates on demand into a byte-capped LRU. The bag was
**71.5%** of resident on abseil, so this is not idle:

- **With Slice 2** (this run): 20 GB caps at ~38K files.
- **Without it** (bag kept resident, ~1.8 MB/file): 20 GB would cap at
  **~11K files** — we'd cover 8% of the tree instead of 29%. Slice 2 is
  worth ~3.4×.

But the bag is the *only* thing Slice 2 evicts. What stays **pinned resident,
by design**, is the reverse-reference substrate — `refs`, `symbols`,
`all_defs`, the include closure — because a `references` / goto / rename query
scans it across the **whole tree** (this is our clangd differentiator: we
answer over all 131K files, not just a `compile_commands.json` subset). Post
bag-eviction, `refs` alone is ~65% of the remaining resident. That pinned set
is the 0.51 MB/file, and it scales linearly forever. It is the wall.

## Implication: the fix is the storage layer, not a Slice 3

The obvious next move is "Slice 3: evict refs to a byte-capped LRU too." Don't.
The cleaner answer falls out of a fact we already rely on: **we already use
SQLite as the storage layer** — but today only as a *blob store*. Each file's
`FileAnalysis` is one opaque `bincode+zstd` blob keyed by path; the only query
is `SELECT blob WHERE path = ?` → decompress → deserialize → query in Rust.
SQLite can't see *into* the refs, which is exactly why they must all be
resident to answer "who references `X`."

Make the queryable parts **relational** instead of opaque:

```sql
CREATE TABLE refs(file, target_name, kind, line, col, invocant_class, ...);
CREATE INDEX idx_refs_target ON refs(target_name);
```

Then `references` becomes `SELECT file,line,col FROM refs WHERE target_name = ?`
— SQLite walks a B-tree index on disk and returns only the matching rows; the
131K files' refs never inflate into RAM. This reframes "Slice 3" from *evict
refs to an LRU* into *don't hold refs resident at all — let SQLite be the
reverse-index*, reusing the engine's B-tree/page-cache/mmap/crash-safety
instead of hand-rolling them. Slice 2's rehydration already does keyed
`SELECT ... WHERE path = ?`; this adds indexes on `target_name`/`name`.

Honest boundaries of that redesign (for the eval, not decided here):

- **Shreds well** → `refs` / `symbols` / `parents`: flat rows with clear query
  keys. **Stays blob + in-Rust** → the witness bag and type inference, which
  are recursive graph walks (inheritance chases, reducer edges) that don't map
  to one SQL query; Slice 2's per-file bag rehydration already serves that tail.
  So the end state is a **hybrid**, not "everything in SQL."
- **Latency tiering**: an in-RAM hash lookup is nanoseconds; an indexed SQLite
  query is micros–millis + row deserialization. goto-def must feel instant, so
  the hot path (open documents) stays resident and SQLite serves the cold long
  tail — which maps onto the existing `documents → workspace_index →
  module_index` priority tiers.
- **Cost moves to index time**: shredding + indexing 131K files' refs is more
  write work than one blob each, but it's a one-time bulk insert amortized over
  every query after.
- **String interning** becomes a smaller, local concern: SQLite dedupes on
  disk, and you'd only intern the working set actually pulled into RAM, rather
  than every name across the whole tree.

## Operational takeaway

The 20 GB guard killed a runaway index cleanly with the box fully intact, so
**Chromium is a viable repeatable stress corpus** for whatever the storage
redesign produces — the clone is retained at `~/personal/cpp-bench/chromium`.
Reproduce with a one-second-poll RSS guard around
`perl-lsp --workspace-symbol <root>` and a `PERL_LSP_HEAP_DUMP=1` completion
line (the exact wrapper used here is in the session scratchpad).

Chromium is the corpus that turns "SQLite as a query engine" from a
nice-to-have into the load-bearing decision for whole-tree scale.

**Design:** `docs/adr/relational-ref-index.md` (schema, query path, scale
benchmarks at 105M rows).

**Outcome, measured:** the whole tree (132,659 files) cold-indexes to
completion in 3 h 02 m at **7.3 GB peak** (vs the 20 GB / 38K-file kill
above); a warm start replays the 6.1 GB store in 9 m at 6.7 GB peak.
0.05 MB/file — the ~67 GB projection above becomes ~7 GB actual. Numbers
live in the ADR's phases section.
