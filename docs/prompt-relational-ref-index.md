# Implementation brief — the relational ref index

Design + measurements: `docs/adr/relational-ref-index.md`. Lifecycle template:
Slice 2 (`docs/adr/memory-slice-2-lru.md` — landed; copy its shapes, don't
reinvent). **Storage/lifecycle + retrieval-path change only — zero semantics
change.** Every `references`/`rename`/`heatmap` answer must be identical to
today's; only where ref bytes live and how candidate rows are found changes.

## The one-line thesis

`refs_to`'s iteration space shrinks from "every ref in every resident file"
to "rows already matching the name key, fetched from an indexed SQLite table
that is written at the same seam as the blob" — so resident refs can be
evicted like the witness bag, and the ~65%-of-resident refs bucket goes to
zero for non-open files.

## Order of work

Land in three separately-testable steps, each green on the full net:

1. **Write side** — shred+persist rows (nothing reads them yet). Ships dark.
2. **Read side** — `refs_to` two-phase driver behind an env flag
   (`PERL_LSP_REF_ROWS=1`), parity harness comparing both paths.
3. **Eviction** — `evict_refs()` at the register seams; flag flips to
   default-on; `PERL_LSP_NO_EVICT=1` keeps the resident path as escape hatch.

## File-by-file change list

### 1. `src/module_cache.rs` — schema + shred + retrieval primitives

- Add the `files` / `strings` / `refs` tables + both indexes to
  `init_schema` via `CREATE TABLE IF NOT EXISTS` (the `deps_stamp` ALTER
  precedent — additive, NOT a `SCHEMA_VERSION` bump; a bump nukes valid blob
  caches).
- `pub fn shred_refs(conn, path, fa: &FileAnalysis)` — inside the caller's
  transaction: upsert `files` row, `DELETE FROM refs WHERE file_id = ?`,
  intern strings (`INSERT OR IGNORE` + id lookup, memoized per connection),
  insert one row per ref with the column mapping from the ADR table.
- `pub fn delete_ref_rows(conn, path)` — the removal half for deleted files
  (wire wherever `purge_module`/file-deletion invalidation runs).
- `pub struct RefRow { ... }` mirror of the columns +
  `pub fn refs_named(conn, name: &str) -> Vec<(PathBuf, Vec<RefRow>)>` —
  the ONE retrieval entry point: joins `strings`/`files`, groups by file.
  Also `pub fn ref_count_named(conn, name) -> u64` (the count-first surface
  for hot-name capping) and a `GROUP BY name_id` batched variant for the
  heatmap's fan-in pass.
- Transaction batching for the bulk drain: `save_blob_to_db` and
  `shred_refs` for one file must share a txn; wrap N files per `BEGIN` in
  the drain loop (today each statement autocommits — measured fine, but the
  txn is for blob↔rows atomicity, not speed).

### 2. `src/file_analysis.rs` — eviction + row view

- `#[serde(skip, default)] refs_evicted: bool`; `pub fn evict_refs(&mut
  self)` clears `refs` + the rebuilt `refs_by_name`/`refs_by_target`/
  `call_ref_by_start` maps, sets the flag; `pub fn refs_are_evicted(&self)`.
  Pinned fields untouched. Mirror `evict_witness_bag` exactly.
- The row view: `pub trait RefLike` (or a lightweight enum) exposing the
  fields the matcher reads — kind discriminant, match name, qual pair,
  access, span, flags, arg_count — implemented by `&Ref` and by
  `module_cache::RefRow`. This is what lets ONE matcher serve both sources.
- Guard direct `self.refs` readers: audit every consumer that can see a
  non-open file (`applicable_dispatches`' dedupe set, `dispatch_at`,
  `find_references`/`collect_refs_for_target` when invoked on index copies)
  — each either (a) reads rows via the retrieval seam, or (b) goes through
  the `refs_present` accessor (below). A silent read of an evicted-empty
  `refs` vec is the R1-class bug of this slice.

### 3. `src/module_index.rs` — refs_present + row-store handle

- `CrossFileLookup::refs_present(&cached) -> Arc<FileAnalysis>` — identical
  shape to `bag_present` (`module_index.rs:1285`): resident-if-not-evicted,
  else rehydrate the full blob through the **same** `PackBagCache` (rename
  it `PackBlobCache` — it already caches whole `FileAnalysis`es, not bags;
  one LRU serves both the bag consumers and the refs fallback, one byte cap,
  no second cache).
- A per-language row-store handle (wraps "open read conn to
  `modules-{lang}.db`") exposed to `resolve.rs` the same way `bag_cache`
  is wired (`with_bag_cache` precedent). Read conns are cheap to open;
  thread-local or per-query both fine — measure, don't pool prematurely.

### 4. `src/resolve.rs` — the two-phase `refs_to`

- Extract `collect_from_analysis`'s per-ref match into
  `ref_matches_target(row: &impl RefLike, target, ctx) -> Option<RefLocation>`
  minus the arms that need full analysis context; those arms return a
  `NeedsRehydrate` verdict instead of an answer.
- `refs_to`: open arm unchanged (resident refs through the `RefLike` view).
  Workspace/dependency arms: `refs_named(name)` per in-scope language DB →
  per file: existing role/gate checks (`file_sees_target` reads the resident
  `include_closure` — unchanged) → per row: `ref_matches_target` →
  `NeedsRehydrate` rows batch per file → `refs_present` → today's full-`Ref`
  matcher on just those. Symbol/declaration sites: keep the resident-symbols
  half, but drive candidate files from the name reverse index
  (`modules_with_symbol` / `all_defs`) instead of the all-files sweep.
- `group_refs` rides the same driver (it is the other backward walk).
- Env flag routing (step 2): `PERL_LSP_REF_ROWS` picks scan vs rows; the
  parity harness runs both.

### 5. `src/module_resolver.rs` — the write + strip seams

- Fresh path (par_iter body): workers already `encode_analysis` + push to
  `fresh`; ALSO build the row batch there (pure CPU, no SQLite in workers)
  and push it alongside the blob. Drain loop writes blob + rows in one txn
  per file (batched N files per BEGIN). Then `evict_refs()` on the resident
  copy at the same point `evict_witness_bag()` runs (after persist, before
  register) — all three existing seams (~L893, L946, L1095).
- Warm path: blobs are decoded during warm anyway → if a file has no rows
  (first run after this lands), shred from the decoded analysis before the
  strip. This is the entire migration story — no separate backfill pass.
- `pack_file_changed`: re-analyze already re-persists; ensure the new txn
  includes row replacement, and `PackBlobCache::invalidate` still fires.
- Perl @INC path (the dedicated `module-resolver` thread, `save_to_db`
  ~L181): same shred call inside the same write. Perl *workspace* files stay
  resident-refs (no persistence path yet — phase 3), so `index_workspace_
  with_index` is untouched.

### 6. `src/backend.rs` — blocking-I/O routing

- `references`, `rename` (and any handler whose projection now hits SQLite):
  wrap the resolve+projection in `spawn_blocking` with `Arc` snapshots of
  the stores. Everything the projections touch is `Send + Sync` (DashMaps,
  Arcs); the open-doc guard discipline from the CandidateSet ADR already
  permits holding only read guards — verify none is held across the
  `spawn_blocking` boundary.
- CLI mirrors (`--references`, `--rename`, `--heatmap`) are already
  synchronous — they just work.

### 7. `src/layering_tests.rs` + docs

- Any new file (`ref_rows.rs` if the row view doesn't fit `file_analysis.rs`)
  gets a `layer_map` entry. `RefRow`/`RefLike` are model-layer; the SQL
  lives in `module_cache.rs` (storage); `resolve.rs` only sees the typed
  retrieval API — no `rusqlite` import outside the cache module.
- CLAUDE.md: refs-residency note in the architecture section; update the
  `module_cache.rs` file-map line.

## Seam signatures (the contract)

```rust
// module_cache.rs
pub fn shred_refs(conn: &Connection, path: &str, fa: &FileAnalysis) -> Result<()>;
pub fn delete_ref_rows(conn: &Connection, path: &str) -> Result<()>;
pub fn refs_named(conn: &Connection, name: &str) -> Vec<(PathBuf, Vec<RefRow>)>;
pub fn ref_count_named(conn: &Connection, name: &str) -> u64;

// file_analysis.rs
impl FileAnalysis {
    pub fn evict_refs(&mut self);
    pub fn refs_are_evicted(&self) -> bool;
}
pub trait RefLike { /* kind, match_name, qual, access, span, flags, arg_count */ }

// module_index.rs (CrossFileLookup)
fn refs_present(&self, cached: &CachedModule) -> Arc<FileAnalysis>;  // bag_present twin
fn ref_rows_named(&self, name: &str) -> Vec<(PathBuf, Vec<RefRow>)>; // routes to the right lang DB(s)
```

## Test + measurement plan

**Parity (the load-bearing gate):** on abseil, for every symbol the heatmap
enumerates, run `refs_to` with `PERL_LSP_REF_ROWS=0` and `=1`; assert
identical `(file, span, access, rewritable)` sets. Also the Slice-1/2
completeness anchor: `--references` on the `_test.cc`-fan-in symbol —
identical count + files with refs evicted.

**Unit:** shred→`refs_named` round-trip preserves every column per RefKind;
`evict_refs` clears refs + maps, pinned fields survive; blob↔rows txn
atomicity (kill between = both old); warm-path backfill shreds row-less
files; `RefLike` parity between `&Ref` and `RefRow` on a constructed
analysis.

**Footprint:** `PERL_LSP_HEAP_DUMP=1` cold abseil — `refs` + `rebuilt_indices`
buckets ≈ 0 (were 157.5 + 20.3 MB = 72% of 246 MB payload); peak RSS drops
accordingly. Then the real gate: re-run the Chromium stress corpus
(`docs/chromium-scale-analysis.md` wrapper) — the whole-tree index must
complete inside the 20 GB guard that previously killed it at ~38K files.

**Latency:** e2e references round-trip on a warm workspace stays inside the
interactive budget; add a timing assertion around the hot-name path using
`ref_count_named` capping.

**Regression gates:** `cargo test` green under default AND
`--features all-langs`; gold + e2e nets green; `./e2e/run.sh` where nvim
exists, CI otherwise.

## Risk list

- **R1 — silent empty-refs reads.** Any consumer reading `self.refs` on an
  evicted index copy sees `[]`, not an error. Audit is the mitigation
  (change #2); the parity harness catches the reachable cases; grep-level
  audit for `\.refs\b` catches the rest. This is the exact Slice-2 R1 shape.
- **R2 — matcher-arm fidelity.** The `RefLike` extraction must not quietly
  drop an arm's input (e.g. `MethodCall` fallback to bag-routed invocant
  resolution, alias-spelled rename refusal). Rule: any arm that reads
  anything beyond the row → `NeedsRehydrate`, never a guess. Parity harness
  + per-kind round-trip unit tests are the net.
- **R3 — blocking I/O leaking onto the async loop.** The retrieval seam is
  synchronous SQLite; it must only run under `spawn_blocking` (handlers) or
  on already-blocking threads (CLI, resolver). Don't add an async wrapper —
  route callers, like Slice 2's rehydration rule.
- **R4 — mid-cold-index queries.** Rows for not-yet-indexed files simply
  aren't there yet; a query mid-index answers over what's persisted (same
  partial-answer semantics as today's mid-index resident scan). WAL keeps
  readers/writer concurrent. No special case.
- **R5 — hot-name materialization.** 10⁵–10⁶-row identifiers cost seconds
  to fetch fully. `ref_count_named` first; the LSP adapter may cap/stream
  raw `references` fan-outs, rename stays exhaustive (a genuinely
  million-site rename is pathological input regardless of storage).
- **R6 — double residency during migration.** Until eviction flips on
  (step 3), rows + resident refs coexist — disk grows (~5.3 GB at Chromium
  scale, measured) but RAM is unchanged. Fine; don't flip eviction before
  the parity gate is green.
- **R7 — `PackBagCache` rename/unification.** Reusing one blob LRU for bag
  + refs rehydration changes that cache's traffic pattern; the byte cap is
  shared. If refs-fallback traffic evicts hot bags in practice, split caps —
  but measure first.
