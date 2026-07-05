# Implementation brief — Memory Slice 2 (evict the witness bag)

Design + measurement: `docs/adr/memory-slice-2-lru.md`. This is the concrete
change list for the follow-up implementation agent. **Storage/lifecycle only —
zero semantics change.** No analysis result may differ; only where the witness
bag bytes live changes (on disk + LRU-on-demand instead of resident-always).

Base: `spike/cpp-support` (0fce3796, `EXTRACT_VERSION 162`). The measurement
support (`heap_estimate`, `WitnessBag::heap_bytes_estimate`, the
`PERL_LSP_HEAP_DUMP` gate) is ALREADY LANDED on `design/memory-slice-2` — build
on that, don't re-add it.

## The one-line thesis

The witness bag is 71.5% of abseil's resident payload (613 MB of 857 MB) and is
a build-time inference scaffold whose conclusions are already baked into
`FileAnalysis` fields. Drop it from resident pack-workspace analyses after the
fold; rehydrate the exact file's bag from the existing 26 MB SQLite blob into a
byte-capped LRU only when a *type* query needs an evicted file.

## File-by-file change list

### 1. `src/file_analysis.rs` — the strip + the evicted flag

- Add `#[serde(skip, default)] bag_evicted: bool` to `FileAnalysis`.
- Add `pub fn evict_witness_bag(&mut self)`:
  ```rust
  self.witnesses = crate::witnesses::WitnessBag::default();
  self.bag_evicted = true;
  ```
  Clears both the `Vec<Witness>` and its rebuilt index; touches no pinned field.
- Add `pub fn bag_is_evicted(&self) -> bool { self.bag_evicted }`.
- **Guard the type-query entry points** so a bag-less file does not silently
  answer "no type": `inferred_type_via_bag`, `sub_return_type_at_arity`,
  `method_call_return_type_via_bag`, `expr_type_at_span`,
  `find_method_return_type`, `mutated_keys_on_class` must, when
  `bag_is_evicted()`, route through the rehydration hook (below) rather than
  reading the empty bag. The cleanest shape: these already take `module_index`
  (or can); add an internal "resolve my live bag" step that asks the pack index
  to hand back a bag-present `&FileAnalysis` for `self`'s path. See risk R3 for
  the alternative if threading `self`'s path down is ugly.

### 2. `src/module_index.rs` — the `PackBagCache` + rehydration

- New struct `PackBagCache` (own file `src/pack_bag_cache.rs` or inline):
  ```
  entries: DashMap<PathBuf, Arc<FileAnalysis>>   // full, bag-present
  clock: AtomicU64                                // recency source
  recency: DashMap<PathBuf, u64>                  // last-touch stamp
  bytes: AtomicUsize                              // current resident estimate
  cap_bytes: usize                                // maxCacheMb * 1MiB
  conn: <pack SQLite handle or a loader closure>
  ```
- `pub fn bag_for(&self, path, decode: impl Fn(&Path) -> Option<FileAnalysis>) -> Option<Arc<FileAnalysis>>`:
  hit → touch recency, return; miss → `decode(path)` (keyed SELECT + decode) →
  `Arc::new` → insert, add `heap_estimate().total()` to `bytes`, evict
  lowest-recency entries until `bytes <= cap_bytes`. `cap_bytes == 0` → return
  the Arc without inserting (rehydrate-and-drop).
- Wire it onto the pack `ModuleIndex` (each `pack_index` gets one, or the hub
  keys by lang). The pack index already owns the per-lang SQLite conn path via
  `open_cache_db(cache_key, lang)`.

### 3. `src/module_cache.rs` — keyed single-file decode

- Add `pub fn load_one(conn, path: &str) -> Option<FileAnalysis>`:
  `SELECT analysis FROM modules WHERE path = ?1` → `decode_analysis(blob)`
  (existing private fn — reuse it; it already zstd→bincode→`after_deserialize`).
  This is the rehydration primitive `PackBagCache::bag_for` calls.

### 4. `src/module_resolver.rs` — strip at the register seam

`index_pack_languages`, both feed paths (the two `register_symbols` call sites):

- **Fresh path** (`par_iter` body, ~L876): currently
  `let arc = Arc::new(analysis); pack_index.register_symbols(path, arc.clone()); fresh.push((canon, arc));`
  The `fresh` Vec is what gets `save_to_db`'d later (full blob — good, must stay
  full). Split the resident copy from the persisted copy:
  ```
  let full = analysis;                       // -> disk, keep the bag
  let mut resident = full.clone();           // or encode-then-strip to avoid clone
  resident.evict_witness_bag();
  let arc = Arc::new(resident);
  pack_index.register_symbols(path.clone(), arc);
  fresh.lock().push((canon, Arc::new(full))); // persisted WITH bag
  ```
  A full `FileAnalysis::clone` per file is wasteful; prefer: serialize the blob
  now (`module_cache::encode_analysis(&full)`), push the *bytes* to `fresh`, then
  `full.evict_witness_bag(); Arc::new(full)` for the resident register — one
  struct, no clone. Adjust the later `save_to_db` loop to write pre-encoded
  bytes (add a `save_blob_to_db(conn, name, path, blob)` beside `save_to_db`).
- **Warm path** (~L847): `pack_index.register_symbols(cached.path, cached.analysis.clone())`.
  `cached.analysis` came from `decode_analysis` (bag present). Strip before
  register: build a bag-less `Arc<FileAnalysis>` from it. `CachedModule.analysis`
  is `Arc<FileAnalysis>` — either make a bag-less clone here, or (better) have
  `warm_cache`/the temp map hand back an already-stripped analysis for the
  register feed while `PackBagCache` reloads full on demand.
- Attach the `PackBagCache` (with the pack conn + `maxCacheMb`) to the pack index
  before `hub.attach_pack_index`.

### 5. `src/backend.rs` — `maxCacheMb` initialization option

- Read `initializationOptions.maxCacheMb` (default 128) in `initialize`; thread
  it to `index_pack_languages` / the `PackBagCache` constructor. 0 disables
  retention.

### 6. In-session invalidation (`invalidate_pack_file` path, ~L915 onward)

- On a changed/saved pack file: drop its `PackBagCache` entry (stale bag) in
  addition to the existing gather-cache eviction, so the next type query
  rehydrates the fresh bag. The re-analyze already re-registers a bag-less
  resident copy via the same strip seam.

## Seam signatures (the contract)

```rust
// file_analysis.rs
impl FileAnalysis {
    pub fn evict_witness_bag(&mut self);
    pub fn bag_is_evicted(&self) -> bool;
}

// module_cache.rs
pub fn load_one(conn: &Connection, path: &str) -> Option<FileAnalysis>;
pub fn encode_analysis(fa: &FileAnalysis) -> Option<Vec<u8>>;         // may already exist as private; make it callable
pub fn save_blob_to_db(conn: &Connection, module_name: &str, path: &str, blob: &[u8]);

// pack_bag_cache.rs
impl PackBagCache {
    pub fn new(cap_bytes: usize, loader: impl Fn(&Path) -> Option<FileAnalysis> + Send + Sync + 'static) -> Self;
    pub fn bag_for(&self, path: &Path) -> Option<Arc<FileAnalysis>>;   // resident-or-rehydrate
    pub fn invalidate(&self, path: &Path);
}
```

## Test + measurement plan (prove BOTH targets)

**Footprint (~0.5 GB):**
```
perl-lsp --clear-cache <abseil>
PERL_LSP_HEAP_DUMP=1 /usr/bin/time -v perl-lsp --references <abseil> \
  <abseil>/absl/strings/string_view.h 41 15
```
Expect: `witness_vec` + `witness_index` ≈ **0 MB** in the heap dump (index copies
bag-less); peak RSS **≤ ~0.6 GB** cold, lower warm. Compare against the recorded
Slice-1 baseline (1.207 GB RSS / 857 MB payload / bag = 613 MB).

**Completeness (must be byte-identical to Slice-1):**
- `--references` on a cross-TU symbol returns the SAME file+range set as before
  the change (the pinned-refs invariant). Use the Slice-1 completeness anchor:
  an abseil symbol whose refs include a `_test.cc` clangd misses. Assert count
  and the presence of the `_test.cc` file unchanged.
- `--definition` and `--workspace-symbol` unchanged (bag-free projections).
- **Type-query rehydration:** a cross-file method-return-type query that reaches
  into an evicted TU must still resolve. Author a gold/e2e row: a call whose
  receiver type comes from a function/method defined in a NOT-open abseil file;
  assert hover/type resolves identically with the LRU on and with `maxCacheMb=0`
  (rehydrate-and-drop) — both must match the pre-change answer.

**Unit:**
- `evict_witness_bag` clears the bag and sets the flag; pinned fields
  (`refs`, `symbols`, `return_types`, `resolved_method_target`) survive.
- `load_one` round-trips a saved analysis (bag present after decode).
- `PackBagCache` evicts LRU tail over cap; `cap==0` never retains.

**Regression gates (unchanged bar):** `cargo test` green under BOTH `cargo build
--release` and `--features all-langs`; do NOT run gold/e2e nets (coordinator's
gate). The measurement probe already compiles under both.

## Risk list (weigh before/while implementing)

- **R1 — cross-file type resolution rides the target file's bag.**
  `find_method_return_type`'s `MethodOnClass` walk + `expr_type_at_span` read the
  *target* file's witnesses, not just the querier's. So evicting bags on all
  workspace files WILL make these return `None` unless the rehydration hook
  (change #1 guard) is wired for every such entry point. This is the correctness
  crux — verify each type entry point routes through `bag_for` before shipping.
  A missed entry point = a silent cross-file type regression (references/goto
  stay fine; only type inference degrades). Mitigate with the type-query gold row
  above.
- **R2 — the warm path double-holds.** `warm_cache` loads full blobs into a temp
  map; if you strip only at register but the temp map lingers, you keep full bags
  transiently. Ensure the full decoded analysis is dropped after the strip +
  persist feed (don't retain it in the temp map past registration).
- **R3 — threading `self`'s path to the type entry points.** `FileAnalysis`
  doesn't carry its own path; the rehydration hook needs `(path, pack_index)`.
  Either pass the path into the query methods (several call sites) or have the
  pack index resolve "which path is this `&FileAnalysis`" — the cleaner shape is
  the caller (which already has the `CachedModule`/path) passing it in. Audit the
  call sites; this is the main plumbing cost.
- **R4 — open-doc double copy.** An open cpp file lives in `FileStore::open`
  (full, with bag) AND as a bag-less pack-index copy. Correct (hover reads the
  open copy), but confirm the open-doc analysis is NEVER the one stripped — the
  strip is only in the pack-index register feed.
- **R5 — LRU cap sizing for cpp.** Bags average ~700 KB/file on abseil (10–100×
  a Perl module). A cap tuned for Perl would hold too few cpp bags; 128 MB
  default ≈ 180 abseil bags is the starting point — validate no thrash on a
  realistic type-heavy cpp session.
- **R6 — `bag_evicted` provenance.** `--dump-package` and any inspector that
  reads the bag must rehydrate first (or honestly report "bag evicted, N MB on
  disk") rather than printing an empty bag as "no type facts".
