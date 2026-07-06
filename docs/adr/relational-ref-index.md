# ADR: The relational ref index — SQLite is the reverse index

Status: design (measured — synthetic scale benchmarks + abseil calibration).
Implementation brief: `docs/prompt-relational-ref-index.md`. Motivating
measurement: `docs/chromium-scale-analysis.md`. Prior residency work:
`docs/adr/memory-slice-2-lru.md` (the lifecycle template this design reuses),
`docs/prompt-bounded-memory.md` (the completeness invariant).

## Context: refs are the wall, and the scan is the reason they're pinned

Post-Slice-2, every indexed file's `FileAnalysis` still pins its `refs`
(`Vec<Ref>`, 1–3 heap strings per ref) plus the rebuilt `refs_by_name` /
`refs_by_target` maps, resident forever — ~65% of the remaining
0.51 MB/file that projects whole-tree Chromium to ~67 GB. They are pinned
because of how the one whole-tree backward walk works:

- `resolve.rs::refs_to` — the driver behind the `references()` /
  `rename_edits()` projections and therefore `--heatmap`'s fan-in — visits
  **every** `FileAnalysis` in all three tiers (open / workspace / cached) and
  runs `collect_from_analysis`, which is a **linear loop over that file's
  `refs` with a per-ref name compare**. There is no cross-file ref index of
  any kind; the per-file `refs_by_target` map only accelerates the
  *same-file* walk. Answering "who references X" over 131K files means
  touching 131K ref vectors, so all of them must be resident.

Meanwhile SQLite already holds every one of those refs — inside the opaque
bincode+zstd `FileAnalysis` blob (`modules-{lang}.db`). The storage layer has
the data; it just can't see into it.

The queries themselves are narrow: of everything that reads refs cross-file,
only **references**, **rename**, and **heatmap** exist, and all three ask the
same question — *rows matching one name, then a predicate*. Goto-def,
workspace/symbol, and implementations are symbols-side and already name-keyed
(`all_defs` / `ModuleEdgeIndexes.names`). The whole-tree linear scan exists to
serve a lookup that is name-keyed by nature. That is a B-tree's job.

## Decision

**Shred refs into relational rows in the existing per-language SQLite DB at
persist time, make `refs_to`'s retrieval an indexed `SELECT ... WHERE
name_id = ?`, and evict resident refs with the exact Slice-2 lifecycle**
(strip after persist, full blob keeps everything, byte-capped LRU rehydrate
for the paths that need the real structs).

Three sub-decisions, each load-bearing:

1. **SQL is retrieval, Rust is semantics.** The SQL layer answers exactly one
   question: "give me the candidate rows for this name (and which files they
   live in)." Everything semantic — the RoleMask arms, the
   `file_sees_target` closure gate, the per-`RefKind` matcher arms, receiver
   gating, rewritability policy — stays in Rust, applied over the returned
   rows. We are not porting `collect_from_analysis` to SQL; we are replacing
   its *iteration space* (every ref in every file) with a pre-narrowed one
   (only rows that already match the name key). No semantic rule moves into
   the database, so no rule can drift between a SQL copy and a Rust copy.

2. **Rows carry the post-fold verdicts.** Shredding happens at the same seam
   where the blob is encoded — *after* `fold_to_fixed_point` has baked
   `resolved_method_target`, `invocant_class`, `resolved_package`, and
   rewritability into the refs. So the columns are decision-ready facts, not
   raw syntax, and the common matcher arms run on rows alone.

3. **The row-can't-decide fallback is single-file rehydration, and it is
   bounded by construction.** Any matcher arm that today consults analysis
   state beyond the ref itself (the bag-routed
   `method_call_invocant_class` fallback for build-unresolved invocants, any
   future arm) rehydrates that one file's full blob through the Slice-2 LRU
   pattern and runs today's exact code on the real `Ref`. Crucially the
   fallback set is *files that contain a name-matching row* — the SQL
   retrieval already narrowed the universe — never "all files". A missing
   column can cost latency on some rows; it cannot cost completeness and it
   cannot re-inflate the tree.

This is the same single-seam shape as the witness bag ("production is push,
consumption is query through the registry"): row production is the shred at
persist time, row consumption is one retrieval seam that every backward-walk
driver (`refs_to`, `group_refs`) calls. Nobody else touches the tables.

## Schema

Additive to the existing per-language DB (`modules.db` / `modules-{lang}.db`),
created with `CREATE TABLE IF NOT EXISTS` — deliberately NOT a
`SCHEMA_VERSION` bump, which would nuke valid blob caches (the `deps_stamp`
ALTER precedent, `module_cache.rs`):

```sql
CREATE TABLE IF NOT EXISTS files(
  file_id INTEGER PRIMARY KEY,
  path    TEXT NOT NULL UNIQUE      -- canonical, same key as modules.path
);
CREATE TABLE IF NOT EXISTS strings(  -- the interner: names, packages, classes
  str_id INTEGER PRIMARY KEY,
  s      TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS refs(
  file_id   INTEGER NOT NULL,       -- REFERENCES files
  name_id   INTEGER NOT NULL,       -- match key: unqualified_target_name(), interned
  full_name_id INTEGER,             -- interned full spelling iff != match key (FQ calls)
  kind      INTEGER NOT NULL,       -- RefKind discriminant
  start_row INTEGER NOT NULL, start_col INTEGER NOT NULL,
  end_row   INTEGER NOT NULL, end_col   INTEGER NOT NULL,
  access    INTEGER NOT NULL,       -- Read / Write / Declaration
  flags     INTEGER NOT NULL,       -- bitfield: rewritable, folded_from-present,
                                    -- resolved (qual is a build-time verdict vs unknown), ...
  qual_kind INTEGER NOT NULL,       -- what qual_id means, per RefKind (see below)
  qual_id   INTEGER,                -- interned qualifier; NULL = none/unresolved
  arg_count INTEGER                 -- NULL when unknown
);
CREATE INDEX IF NOT EXISTS idx_refs_name ON refs(name_id);
CREATE INDEX IF NOT EXISTS idx_refs_file ON refs(file_id);   -- invalidation + per-file ops
```

The `qual_kind`/`qual_id` pair is the one column every kind-specific matcher
arm keys on — the qualifier that arm compares against the target:

| RefKind | qual_kind | qual_id |
|---|---|---|
| `FunctionCall` | `ResolvedPackage` | `resolved_package` |
| `MethodCall` | `InvocantClass` | `resolved_method_target`'s / PostFold `invocant_class`; NULL = build-unresolved (fallback candidate) |
| `DispatchCall` | `Dispatcher` | `dispatcher` |
| `HashKeyAccess` | `HashKeyOwnerSub` / `HashKeyOwnerClass` | owner name |
| `Variable` / `PackageRef` / `ContainerAccess` | `None` | NULL |

One row shape for every kind, no per-kind tables: the matcher arms already
switch on `RefKind`; the row just needs to carry each arm's compare key. Any
arm whose compare key turns out not to fit this shape uses the rehydration
fallback until it earns a column — never a second table, never a
kind-branching schema (rule #10: the shape lives on the value).

Symbol declaration sites (the other half of a `references` answer) stay on
the resident `symbols` + the name-keyed `all_defs`/`names` reverse indexes in
phase 1 — they are 1–2 orders of magnitude smaller than refs. Phase 2 gives
them the same treatment (below).

## The query path

`refs_to` becomes a two-phase driver; `CandidateSet` and every projection
signature are untouched (the ADR-guaranteed property: axes live in
construction, projections inherit):

1. **Retrieval.** Open documents iterate their resident refs exactly as
   today (freshest state, unsaved edits, zero I/O). The workspace/dependency
   arms replace their per-file linear scans with one
   `SELECT ... FROM refs WHERE name_id = ?` per language DB in scope, rows
   grouped by file.
2. **Predicate.** Per file: the RoleMask arm check and the
   `file_sees_target` closure-connectivity gate run exactly as today (the
   gate reads the scanned file's resident `include_closure`). Per row: the
   per-`RefKind` matcher arm compares the row's `qual` columns against the
   target. Rows whose arm needs more than the row carries (NULL
   `InvocantClass` on a `MethodCall`, i.e. today's bag-routed query-time
   invocant resolution) batch into a per-file rehydrate — one blob decode
   per affected file via the LRU — and run the current full-`Ref` matcher.

To keep one matcher instead of two, the per-ref predicate is extracted to
operate on a **row view** — a trait/enum both sources satisfy (`&Ref` from a
resident analysis; `RefRow` from SQL). The open-doc arm and the SQL arm call
the same function; parity is by construction, and the migration's regression
net (below) asserts it empirically.

**Threading.** This makes references/rename/heatmap do disk I/O, which
today's handlers must not (async handlers are zero-I/O). The projection call
moves inside `spawn_blocking` for those verbs — the projections only read
`Arc`'d stores, so snapshotting them across the boundary is safe. Reads use
per-query (or thread-local) read-only connections; WAL keeps readers
concurrent with the index writer, so queries work mid-index. Writes stay
where they are: the sequential drain after the Rayon fan-out (workers still
never touch SQLite), now writing row batches in the same transaction as each
blob.

**Consistency.** Per file, one transaction: `DELETE FROM refs WHERE
file_id = ?` + row inserts + blob `INSERT OR REPLACE`. Rows and blob can
never disagree; a crash leaves both at the previous state. Rows are derived
purely from the same `FileAnalysis` the blob serializes, so
`EXTRACT_VERSION` governs both: a stale blob getting re-resolved rewrites
its rows in the same transaction. Warm start backfills for free: files whose
blobs warm-load but have no rows (first run after this lands) shred from the
already-decoded analysis — no separate migration pass, no re-parse.

## Residency: what leaves, what stays

`FileAnalysis::evict_refs()` mirrors `evict_witness_bag()`: clears `refs`
(and thereby the rebuilt `refs_by_*` maps), sets a `#[serde(skip)]`
`refs_evicted` flag; called at the same three register seams, always after
the blob+rows are persisted. The disk blob keeps full refs — rehydration is
lossless, and single-file consumers (`applicable_dispatches`' dedupe,
`--dump`-style inspectors, anything unforeseen) route through a
`refs_present`-style accessor identical in shape to `bag_present`.

Pinned resident, per file, after this lands: `symbols` (+ name indexes),
`include_closure`, `package_parents` / `specializes`, `return_types`,
`provisional_dispatches`, the small header maps. Open documents keep
everything, always.

| Query | Served by |
|---|---|
| references / rename / heatmap fan-in | SQL retrieval + row matcher (+ bounded rehydrate) |
| goto-def / workspace-symbol / implementations | resident symbols + name indexes (unchanged) |
| documentHighlight, completion, hover | open-doc resident (unchanged) |
| type inference cross-file | witness bag LRU (unchanged, Slice 2) |
| dispatch (`applicable_dispatches`) | resident `provisional_dispatches`; ref-dedupe via rows or rehydrate |

The completeness invariant (`prompt-bounded-memory.md`) holds with the same
proof shape as Slice 2: the rows are shredded from the identical post-fold
refs the blob carries, for every indexed file; the matcher is the same
predicate over the same facts. Residency changes; coverage cannot.

## Measured: SQLite at Chromium row counts

Real per-file volume, measured on abseil (cold index, release build,
`PERL_LSP_HEAP_DUMP`): **875 files, 157.5 MB resident refs payload
(+20.3 MB rebuilt maps) ≈ 650–800 refs/file** (payload-derived estimate)
→ whole-tree Chromium ≈ **85–105M rows**. Synthetic benchmark at the top of
that range — 131K files × 800 refs = **104.8M rows**, power-law name
distribution, Python sqlite3 driver (a *floor*; Rust with prepared
statements does better):

| metric | measured |
|---|---|
| bulk insert, no indexes (cold index cost) | 104.8M rows in 230 s = **455K rows/s**, flat 174 MB RSS |
| `CREATE INDEX` after load (name + file) | 131 s + 52 s ≈ **3 min** |
| bulk insert with both indexes pre-created | **155K rows/s** sustained (16M-row variant) |
| DB size incl both indexes | **5.3 GB** = 50.6 B/row |
| reverse lookup, warm: 4 / 96 / 2K / 29K / 1.14M rows | 0.01 ms / 0.12 ms / **3.3 ms** / 72 ms / 3.0 s |
| reverse lookup, cold page cache (same bands) | 0.04 ms / 0.5 ms / **11.4 ms** / 122 ms / 3.2 s |
| heatmap-shaped `GROUP BY name` over all 105M rows | **13.6 s** |
| per-file invalidation, indexes live (delete + reinsert 800 rows) | ~9 ms + ~210–290 ms |
| process RSS while querying the 5.3 GB DB | **178–186 MB** |

Consequences baked into the design:

- **Retrieval is linear in result-set size** (~1.5–2.5 µs/row warm). A
  median references click is single-digit ms warm; the pathological
  identifier (`size`, `begin` — 10⁵–10⁶ rows tree-wide; abseil's
  `string_view` already fans to 7,737) costs 100s of ms to seconds to
  *fully* materialize. The retrieval seam therefore supports a
  count-first / streamed answer so the LSP adapter can cap raw fan-outs the
  way clients already truncate them — rename (which genuinely needs every
  row) stays exhaustive.
- **Cold-cache first-click** costs ~10–120 ms in the realistic bands
  (random B-tree page faults). Acceptable for a first `references` after
  boot; goto-def and completion never touch this path.
- **Keep the indexes live from the start.** Pre-created indexes cost ~3×
  on insert throughput, but even the indexed floor (155K rows/s) writes
  Chromium's rows in ~11 min of *background writer* time, overlapped with
  the ~18-min Rayon parse that feeds it. In exchange, mid-cold-index
  queries stay indexed and there is no post-pass / no second code path vs
  steady-state incremental writes. Index-after-bulk stays available as a
  cold-build optimization if the writer ever lags the parse.
- **Write cost lands where predicted**: row writes hide inside the parse
  floor, and per-file invalidation is ~300 ms on the background writer —
  imperceptible next to the re-analysis itself.

## What this buys, honestly

- Resident refs (+ their rebuilt maps) go to zero for non-open files. On
  abseil that is 72% of the post-Slice-2 payload (177.8 of 246.1 MB);
  scaling the measured 0.51 MB/file RSS by the payload ratio projects
  **~0.15–0.2 MB/file → whole-tree Chromium ~20–26 GB** (estimate — the
  RSS-overhead share of the evicted bucket is not exactly proportional).
  That clears the 20 GB leash for every corpus we've measured *except*
  whole-tree Chromium itself, which needs the follow-on phases below —
  next up: `include_closure` interning (now 14% → ~50% of what remains,
  the `prompt-bounded-memory.md` Problem-3 item) and relational symbols —
  to fit comfortably on a 32 GB box. This ADR's seam is what makes those
  phases incremental instead of another redesign.
- `references`/`rename` stop being O(tree) per query: today's whole-tree
  sweep touches every file's refs on every invocation; the indexed lookup
  touches only matching rows. At abseil scale the resident scan was never
  the bottleneck — at Chromium scale the scan *is* the query cost, and it
  inverts: the B-tree walk is independent of tree size.
- Heatmap's fan-in — today O(symbols × files × refs) — rides the same
  retrieval seam and gains a batched shape (one `GROUP BY` pass) *inside*
  that seam, preserving the "fan-in is the references projection, never a
  parallel ref walk" contract.
- SQLite's B-tree, page cache, mmap, WAL, and crash safety replace what a
  hand-rolled ref-LRU + parallel reverse index would have had to reinvent.
- String interning becomes structural: `strings` dedupes every
  name/package/class on disk; resident memory only ever holds the rows a
  query actually fetched.

## Rejected alternatives

- **"Slice 3": evict refs to a byte-capped LRU** (the whole-body-LRU sketch
  from `prompt-bounded-memory.md` Problem 2). An LRU answers *keyed* lookups
  cheaply; `references` is an *inverted* lookup. Every query would fan the
  rehydration across all files that might match — for a hot name, that is
  the whole tree through a blob-decode funnel (~1.7 ms/file × 131K files ≈
  minutes, repeatedly). The LRU pattern is right for the bag (keyed by file,
  queried rarely) and wrong for refs (queried by name, across everything).
- **Port the matcher into SQL.** The predicate reads cross-file state SQL
  can't see (closure gates, receiver isa-walks through `parents_cached`,
  RoleMask policy) and would become a second implementation of resolution
  semantics that drifts from the Rust one — the exact N-path disease the
  CandidateSet ADR exists to kill. SQL narrows; Rust decides.
- **Name→file posting list only (no verdict columns), rehydrate for the
  predicate.** Strictly less schema, but every `MethodCall` row would force
  a blob decode of its file; a hot method name spanning thousands of files
  degenerates to the Slice-3 failure mode. The verdict columns keep the
  common case row-only; the fallback stays the exception.
- **A dedicated inverted-index store** (tantivy / custom mmap index /
  clangd-style RIFF index files). Real engines, but each adds a second
  storage system with its own invalidation, crash-safety, and versioning
  lifecycle next to SQLite — which we already ship, already key by file,
  already invalidate correctly, and which measures fast enough (above).

## Phases (this ADR decides phase 1; the seam anticipates 2–3)

1. **Refs relational** — pack languages + Perl @INC deps (every tier that
   already persists blobs). Perl *workspace* files are RAM-only today
   (never written to SQLite) and small in practice; they keep resident refs
   until phase 3 gives them a persistence path. The scan arms they ride are
   untouched, so this is purely "some arms got faster/lighter".
2. **Symbols relational** — the same shred for `symbols`/`all_defs`, making
   goto-def candidate discovery and workspace/symbol a query, and unpinning
   the second-largest resident bucket.
3. **Header-only residency** — with refs and symbols relational, the
   resident per-file floor drops to a header (path, roles, closure ids,
   parents, return-type map). Warm start stops decoding 131K blobs to
   re-register them (today's warm path decodes everything it warms);
   `files`/`strings` become the registration source and Perl workspace
   files join the persistence path. That is the "fits anywhere" end state.

## Migration net

- **Parity harness (the load-bearing gate):** on a real corpus (abseil),
  run `refs_to` both ways — resident scan vs SQL retrieval — for every
  symbol the heatmap enumerates; assert identical `(file, span, access,
  rewritable)` sets. This is executable because phase 1 keeps the resident
  path compilable behind the eviction flag (`PERL_LSP_NO_EVICT=1` retains
  refs, exactly as Slice 2's escape hatch retains bags).
- The Slice-1/2 completeness anchor stays: `--references` on the abseil
  symbol whose fan-in includes `_test.cc` files clangd misses — same count,
  same files, refs evicted.
- Gold + e2e nets unchanged and green; `cargo test` under default and
  `--features all-langs`.
- RSS regression: `PERL_LSP_HEAP_DUMP` on abseil must show the `refs` and
  `rebuilt_indices` buckets ≈ 0 for index copies; peak RSS target on the
  Chromium stress corpus re-run: complete the whole-tree index inside the
  20 GB guard that killed it at 38K files.
