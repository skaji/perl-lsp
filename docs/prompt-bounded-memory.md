# Bounded memory: the abseil 4 GB investigation + design

Status: design + measurements. No LRU shipped in this slice. The only code
change landed here is an **env-gated diagnostic** (`PERL_LSP_MEM_REPORT`, inert
by default) that reports the resident size of the four cpp gather caches.

## TL;DR — the surprise

The 4 GB peak on abseil is **NOT** what the original brief (or my own first
hypothesis) assumed. It is not 875 fat `FileAnalysis`es sitting resident, and it
is **not** transient parallel-expansion state. Two measurements kill both
hypotheses:

1. **Peak is independent of parallelism.** Cold `--references` peak RSS:
   - `RAYON_NUM_THREADS=1` → **4.09 GB**
   - `RAYON_NUM_THREADS=4` → **4.11 GB**
   - default (12) → **4.24 GB**

   If the peak were concurrent per-file transients, 1 thread would be ~12× lower.
   It is flat. **Bounding index parallelism buys nothing.**

2. **The dominant consumer is a set of unbounded, per-file, process-global macro
   caches in `cpp_reparse.rs`** — `macro_table_cache` + `pre_expanded_cache` —
   that accumulate monotonically during the cold include-closure gather and are
   never evicted for the rest of the session. Measured directly (heap-payload
   estimate, `PERL_LSP_MEM_REPORT=1`, cold abseil):

   ```
   header_cache:       1092 headers,    2.1 MB   (shared across files — deduped, tiny)
   macro_table_cache:   877 files,    596.6 MB   (raw merged table, Arc-shared w/ pre_expanded)
   pre_expanded_cache:  877 files,    961.9 MB   (full+alias expanded variants, ON TOP of raw)
   include_closure:     877 files,     20.8 MB
   TOTAL cpp gather caches:          1581.4 MB
   ```

   This is a **heap-payload** estimate (String/Vec capacity only); real RSS cost
   is higher once BTreeMap node overhead, HashMap load factor, and glibc arena
   retention are counted — consistent with the ~3.3 GB gap between the warm
   resident (0.89 GB) and the cold peak (4.24 GB).

### The root cause in one sentence

`header_cache` proves the transitive header universe dedupes to **2.1 MB for
1092 headers**. But `gather_included_macros` then **flattens each file's closure
into a private per-file `BTreeMap`, cloning every `Macro`** — so the same
`std::`/`absl::` header macros are duplicated into **877 separate merged
tables**, and `pre_expanded_cache` stores a **second, expanded copy** of each.
The sharing `header_cache` achieves is thrown away at the merge. That
duplication — ~1.58 GB measured, more in RSS — is the 4 GB.

## Measured breakdown

| Metric | abseil (875 files: 488 `.cc` + 387 `.h`) |
|---|---|
| Cold `--references` peak RSS | **4.24 GB** (12 threads), 4.09 GB (1 thread) |
| Cold wall | 10.5 s (12t) / 60 s (1t) / 126 s user |
| Warm `--references` peak RSS | **0.89 GB** |
| Warm wall | 1.5 s |
| clangd (compile_commands.json) | ~0.32 GB, **159 files** (build only) |

### Top 3 resident consumers at cold peak

1. **`pre_expanded_cache` — ~962 MB+ measured payload.** Per-file
   full+alias *expanded* external macro variants (`ExpandedVariant::of(&raw)`),
   one `PreExpandedExternal` per source file, keyed by file path, never evicted.
2. **`macro_table_cache` — ~597 MB measured payload.** Per-file *raw* merged
   transitive macro table (`Arc<BTreeMap<String, Macro>>`). Arc-shared with
   pre_expanded's `.raw`, so counted once. Never evicted.
3. **877 resident `Arc<FileAnalysis>` — ~0.89 GB** (this is the whole *warm*
   number; it is the floor, not the spike). Held in the pack `ModuleIndex`
   (`all_files` / `all_defs`) plus, transiently during index, the `fresh`
   `Mutex<Vec<…>>` accumulator (same Arcs, not doubled).

`header_cache` (2.1 MB) and `include_closure_cache` (20.8 MB) are negligible —
the shared-header memoize is doing its job; only the *merged/expanded* per-file
tables are the leak-shaped growth.

### On-disk cache vs resident ratio

| Store | abseil size |
|---|---|
| `modules-cpp.db` (bincode+zstd `FileAnalysis` blobs) | 26 MB |
| `macros/` (877 persisted merged macro tables, bincode) | 74 MB |
| **Total on disk** | **100 MB** |
| Warm resident | 0.89 GB → **9× disk** |
| Cold peak | 4.24 GB → **42× disk** |

The disk backing store is already complete and 42× smaller than the cold peak.
The rehydrate headroom is enormous: everything resident is reconstructable from
100 MB on disk (or cheaply re-gathered from the 2.1 MB `header_cache`).

## First-class invariant: completeness is preserved; only residency is bounded

A live clangd side-by-side is load-bearing here. On abseil, clangd's
`compile_commands.json` indexes **159 files (the build only)** and is
structurally **blind to 306 `_test.cc` / `benchmark.cc` files** — so
`find references` in the editor shows test+benchmark usages clangd **misses**.
That whole-tree, build-independent completeness is exactly what our ~0.89 GB
buys, and it is our differentiator, not just a cost.

**Therefore the design bounds only the RAM working set. It never reduces which
files are indexed, and never makes references / workspace-symbol incomplete.**

Concretely:
- The **on-disk SQLite index stays complete for the whole tree.** Every `.cc`
  and `.h` is analyzed and persisted, exactly as today.
- A `find references` / `workspace symbol` **must still return hits from every
  indexed file**, resident or not. If a file's heavy `FileAnalysis` was evicted,
  the query rehydrates it (or reads a resident lightweight projection — below).
- The knob is **residency**, not coverage. `maxCacheMb` changes how much RAM we
  hold, never how much of the repo we know about.

This is the opposite of the clangd trade: clangd bounds memory by indexing
*less*; we bound memory by keeping *less resident* while indexing *everything*.

## Design

Two independent problems, addressed in priority order.

### Problem 1 (the 4 GB): the per-file macro caches — evict after build

**These caches are pure recompute-on-demand, and after a file's `FileAnalysis`
is built they are dead weight for the rest of a bulk index.** They exist to (a)
build the file's analysis (a one-shot during index), (b) re-analyze on edit, (c)
serve on-open. During a bulk workspace index answering one query, (a) happens
once and (b)/(c) never — yet the tables live forever.

They are also **fully backed on disk** (`macros/*.bin`, 74 MB) and cheaply
re-derivable from the resident 2.1 MB `header_cache`. So dropping them is nearly
free to reverse.

**Slice 1 (smallest, highest impact): evict the per-file gather-cache entry for
a file the moment its analysis is built during bulk index.** — **LANDED.** In
`index_pack_languages`' `par_iter` body, after `analyze_with_path` returns and
the `Arc<FileAnalysis>` is registered + queued for persist, call
`cpp_reparse::evict_gather_caches_keep_headers(&{this file})`. This retains-out
`macro_table_cache`, `pre_expanded_cache`, `include_closure_cache` and **leaves
`header_cache` intact** — the pre-existing `evict_analysis_caches` (the on-change
seam) drops the header entry too, which is right for a content edit but wrong for
a residency evict, so slice 1 got its own keep-headers variant (both share the
`evict_gather_caches` core). The file's raw+expanded tables die with the
iteration; `header_cache` (the shared truth) stays warm so a later on-edit
re-gather of any single file is a 2.1 MB-scale BFS, not a cold gather.

**Measured (abseil, `--references`, cold):**

| | before | after |
|---|---|---|
| Peak RSS | 4.02 GB (4221364 KB) | **1.22 GB (1274300 KB)** |
| `macro_table_cache` | 596.6 MB / 877 files | **0.0 MB / 0 files** |
| `pre_expanded_cache` | 961.9 MB / 877 files | **0.0 MB / 0 files** |
| `include_closure` | 20.8 MB / 877 files | **0.0 MB / 0 files** |
| `header_cache` | 2.1 MB / 1092 headers | 2.1 MB / 1092 headers (kept warm) |
| gather-cache TOTAL | 1581.4 MB | **2.1 MB** |
| Cold index wall | 11.52 s | 11.19 s (no cliff) |

Peak beat the ~2.0–2.5 GB estimate — the 1.58 GB of per-file duplicates never
accumulate, so the remaining resident is the ~0.89 GB FileAnalysis floor plus
glibc arena. Completeness verified warm-to-warm: the same cross-file references
query (`ascii.cc` → `AsciiStrToLower`) returns **13 refs across 6 files
(incl. `ascii_test.cc`, `charset_test.cc` — the tests clangd misses)** identical
before and after. No `EXTRACT_VERSION` bump (eviction timing, no serialized-shape
change; stays 162).

- **Expected peak after slice 1:** the ~1.58 GB measured payload (and its
  larger RSS shadow) collapses to at most one file's tables per live worker
  (~12 × a few MB) plus `header_cache` (2.1 MB). Peak should fall from **4.24 GB
  toward ~2.0–2.5 GB** — bounded by the FileAnalysis accumulation (Problem 2)
  and glibc arena retention, not the macro tables. This is a handful-of-lines
  change, no new data structure, no LRU.
- **Correctness:** transparent. The evicted table is never read again this run;
  on edit, `included_macros_pre_expanded` re-derives it (disk `load_persisted`
  or `header_cache` BFS) exactly as it does today on a cold miss. Query answers
  are identical.
- **Concurrency:** `evict_analysis_caches` takes each cache's `Mutex` briefly
  and `retain`s — no guard held across the `analyze` call, no await involved
  (this path is the synchronous Rayon pool, off the tokio loop). No new hazard.

**Slice 1b (alternative / complement): a bulk-index mode that never inserts into
the global caches at all.** A thread-local "batch gather" flag makes
`included_macros_inner` / `included_macros_pre_expanded` skip the
`cache.insert(…)` (still consulting `header_cache` for the parse memoize). Avoids
the insert-then-evict churn. Slightly more invasive (touches the cache read/write
sites); slice 1 is the safer first cut.

**What stays:** `header_cache` is the one gather cache worth keeping resident —
it is the deduped header universe (2.1 MB), it is what makes on-edit re-gather
cheap, and it is shared, not per-file. Do **not** bound it.

### Problem 2 (the 0.89 GB warm floor): bound the resident FileAnalysis set

Still ~2.8× clangd. This is the LRU the original brief scoped. Lower priority
than Problem 1 (it's the floor, not the spike), but it's what gets us toward
clangd's 320 MB.

**Separate the cheap-complete name index from the heavy evictable analysis.**
This separation is the core of preserving the completeness invariant while
bounding residency:

- **Resident + complete (never evicted): a lightweight name→file reverse
  index.** `ModuleIndex` already keeps a `func → modules` reverse index and
  `all_defs`; the target shape is a compact `{ symbol_name → [file_path], kind }`
  table covering **every** indexed file, holding *no* per-file `Vec<Ref>` /
  `WitnessBag` / index maps. This is what answers "which files could reference X"
  and powers `workspace/symbol` without rehydrating anything. Sized in the
  low tens of MB for 875 files (names + paths only).
- **Evictable (LRU-bounded): the heavy `Arc<FileAnalysis>`.** The full analysis
  (refs, witnesses, rebuilt HashMap indices) is what a `find references` needs to
  produce *exact ranges* in a given file. Bound this map by a byte cap
  (`initializationOptions.maxCacheMb`, default e.g. 256 MB) with LRU eviction.

- **Pin rules (never evict):**
  1. **Open documents** — a doc the user has open carries its `Document` (tree +
     text + analysis) and is never a workspace/LRU entry; untouched.
  2. **The lightweight name index** — always complete, always resident.
  3. Optionally the N most-recently-queried files (the LRU's natural retention).

- **Rehydrate path:** on a query needing an evicted file's full analysis, load
  the bincode+zstd blob from `modules-cpp.db` (already there) and rebuild the
  serde-skip indices (`rebuild_all_indices`). **Measured rehydrate cost proxy:**
  the warm run reads+registers all 877 blobs and rebuilds indices in ~1.5 s wall
  total → ~1.7 ms/file amortized for the decode+index-rebuild. A single-file
  rehydrate on one interactive query is sub-millisecond-to-low-single-digit-ms —
  well within an interactive budget. (Author a focused single-blob
  decode+rebuild micro-benchmark to confirm before shipping.)

- **Concurrency / guard safety:** the LRU must not reintroduce the
  guard-across-await hazard (see `filestore-guard-discipline`). Handlers
  snapshot `Arc::clone` the analysis out of the map and **drop the map guard
  before** any `resolve()` / `.await`. The LRU bookkeeping (recency touch,
  eviction) happens under a short lock that is never held across an await. A
  `DashMap` + an atomic-clock recency stamp (evict the lowest-stamp entries when
  over cap) avoids a global lock entirely; a sharded `lru`/`clru` crate behind a
  `parking_lot::Mutex` is the simpler alternative if per-entry-cost accounting is
  wanted.

- **Transparency invariant:** a rehydrated `FileAnalysis` is byte-identical to
  the resident one (same blob → same struct → same `rebuild_all_indices`). No
  query answer changes. The cache is invisible to every projection.

### Problem 3 (complementary): per-file fat-trim

Cheaper wins that shrink both the resident floor and the disk blob:

- **Intern the `include_closure` strings.** `include_closure: Vec<String>` and
  `include_directives` hold canonical header paths, **duplicated across every
  file that includes them** (abseil files share ~90% of their closure). 20.8 MB
  resident for the closure cache alone, plus a copy inside each of 877
  `FileAnalysis`es. A process-global path interner (`Arc<str>` / a `u32` id into
  a shared path table) collapses this to one copy per unique header.
- **Drop the witness bag for non-open (workspace) files after fold.** The
  `WitnessBag` is a *type-inference build scaffold*. For cpp workspace files it
  is largely consumed by the time the skeleton is assembled; a rehydrated
  workspace file needs refs+symbols for references/symbols, not the full bag.
  Investigate `#[serde(skip)]`-ing the bag for the workspace role (keep it for
  open docs where hover/completion re-query it), rebuilding lazily only if an
  inference query hits an evicted file. Bounds both disk blob and rehydrated
  resident.
- **The serde-skip index maps** (`symbols_by_name`, `refs_by_target`,
  `call_ref_by_start`, …) are rebuilt on load — good, they cost nothing on disk,
  but they *are* resident fat. They only need rebuilding for a file that is
  actually queried; a rehydrate-on-demand model rebuilds them lazily anyway.

## Sequence (smallest first)

| Slice | Change | Expected abseil peak | Risk |
|---|---|---|---|
| **1** ✅ | Evict per-file gather caches after each file's analysis in `index_pack_languages` (`evict_gather_caches_keep_headers`). | **4.02 → 1.22 GB** (measured) | Very low — transparent, tiny diff |
| 2 | Split lightweight name index (complete, pinned) from heavy `FileAnalysis`; LRU-bound the heavy map + rehydrate from SQLite; pin open docs. | ~2.0 → **~0.4–0.6 GB** | Medium — new residency layer; guard discipline |
| 3 | Intern `include_closure`/path strings; drop witness bag for workspace role post-fold. | shaves 0.1–0.3 GB off floor + disk | Low–medium |

Deferred / out of scope for the first slice: an actual `maxCacheMb`
configuration surface (slice 2 wires it), a lazy on-demand *index* (clangd
doesn't full-index — but we deliberately do, for the completeness differentiator,
so lazy-indexing is explicitly **rejected**, not deferred).

## Why not just bound parallelism / go lazy-index

- **Bound parallelism:** measured no-op (peak flat 4.09 → 4.24 GB across
  1→12 threads). The caches accumulate regardless of concurrency.
- **Lazy/on-demand index (clangd-style):** would delete the whole-tree
  references/symbol completeness that is our differentiator (clangd misses 306
  test/bench files on abseil). Rejected by the completeness invariant.

## Measurement instrumentation (landed, inert by default)

`cpp_reparse::cache_size_report()` + a `PERL_LSP_MEM_REPORT`-gated `eprintln!`
at the end of `index_pack_languages`. Reports entry counts + heap-payload MB for
the four gather caches. Behind an env gate, zero cost when unset; kept as a
regression check for slice 1 (after the evict, the report should show near-zero
`macro_table_cache` / `pre_expanded_cache` at index end).

Reproduce:

```
perl-lsp --clear-cache <abseil-root>
PERL_LSP_MEM_REPORT=1 /usr/bin/time -v \
  perl-lsp --references <abseil-root> <abseil>/absl/strings/string_view.h 41 15
```
