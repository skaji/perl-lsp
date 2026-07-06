# Symbols relational — the resident floor moves to the store

Implementation brief for the deferred phase named in
`docs/adr/relational-ref-index.md` ("symbols relational + the
register-from-tables warm start"). Same discipline as the refs work: SQL is
retrieval, Rust is semantics; rows and blob are one generation; eviction
strictly follows persistence.

This is also `docs/prompt-storage-engine.md`'s Phase 2 remainder (refs
landed first): symbol rows are the enumeration surface that un-pins the
resident `symbols` bucket. The Surface / freshness arcs (that prompt's
Phases 1+3) build on top and are NOT in scope here — notably, the
`return_type` column that prompt sketches for `symbols` rows is deferred to
the Surface work, where enrichment gets its queryable substrate (see
`docs/open-forks.md`).

## Why now (measured)

Post-refs-relational resident composition — abseil warm, 875 files,
46.1 MB payload:

| bucket | MB | share | fate here |
|---|---|---|---|
| `symbols` | 21.7 | 47.1% | → rows + evict (phase B) |
| `include_closure` | 10.8 | 23.4% | stays (already interned; the vec IS the visibility gate) |
| `rebuilt_indices` | 4.9 | 10.6% | symbol-keyed maps die with symbols |
| `cpp_extras` | 3.7 | 8.1% | stays (query-time fact vectors; next candidate after this) |
| `scopes` | 3.3 | 7.1% | stays |
| `struct_shell` + misc | 1.7 | 3.7% | floor |

Chromium warm floor — 132,662 files, 6,907.5 MB payload:

| bucket | MB | share |
|---|---|---|
| `include_closure` | 2,827.7 | 40.9% |
| `symbols` | 2,489.6 | 36.0% |
| `rebuilt_indices` | 618.2 | 8.9% |
| `scopes` | 370.5 | 5.4% |
| `cpp_extras` | 356.4 | 5.2% |
| `struct_shell` + misc | 245.0 | 3.5% |

Symbols + their derived indices are ~45% of the floor; this phase unpins
them. The warm *wall* (9 m — decoding every blob once to re-register) is
what phase C's register-from-tables kills. `include_closure` is now the
largest single bucket at scale — a REPRESENTATION problem (16-byte
`Arc<str>` per closure entry × deep header closures), not a relocation
one; logged as a fork, out of scope here.

## Consumer matrix (from the code sweep)

Present-day eviction (bag, refs) never touched symbol structure, so every
symbol reader was written against always-resident symbols. Under symbol
eviction they split:

- **(A) Registration-time — safe by construction** (run on the fresh/warm
  copy before the strip): `register_symbols` + `is_linkage_visible` gating,
  `ModuleEdgeIndexes::feed` (`indexable_names`, exports, parents, bridges,
  specs), `register_workspace_resident` / `first_package_name`,
  `record_workspace_projections` (bag, not symbols), resolver-thread feeds.
- **(B) Whole-tree sweeps — must go rows-first** (rehydrating everything
  per query is the 9-minute disaster): the `workspace/symbol` handler +
  `--workspace-symbol` (reads name/kind/selection_span only — exactly the
  row shape). Heatmap + `--refs-parity` enumeration sweeps are CLI-wide
  but already take present views per file; they upgrade to the whole view.
- **(C) Bounded candidate reads — rehydrate via the LRU** (per-candidate,
  same policy as refs): goto-def (`member_def_location`,
  `pack_symbol_def_location`, `dispatch_handler_locations`), the
  `collect_from_analysis` declaration-site matcher (already receives a
  present view; upgrade), `complete_methods_for_class` cross-file arm,
  `collect_class_fields`, `imported_sub_keys`, hover's cached-file scan,
  `has_sub_in_package` / `package_var_def_line` (CachedModule methods —
  route callers), `for_each_entity_bridged_to` (`symbols.get(idx)`),
  `implementations_of` / specialization walks, `sub_info_view`
  (symbols + bag — the two-axis reader), unregister paths (rare; whole).

Fields that stay resident and are NOT evicted here: `export`/`export_ok`,
`package_parents`, `plugin_namespaces`, `specializes`, `include_closure`,
`scopes`, `cpp_extras` — their readers are untouched.

## Shape of the change

### Phase A — write side (dark)

`symbol_rows` joins the derived-table generation: shredded in the same
transaction as the blob + ref rows, erased through the same
`invalidate_generation` seam, tier-tagged the same way, governed by the same
`REF_ROWS_VERSION` (bump → DROP+recreate + re-shred from blobs on next warm;
the version check's shape probe extends to the new table).

Schema (names interned through the existing `strings` table):

```sql
CREATE TABLE syms (
    file_id   INTEGER NOT NULL,   -- files.file_id, same lifecycle as refs
    name_id   INTEGER NOT NULL,   -- strings.str_id
    kind      INTEGER NOT NULL,   -- SymKind discriminant
    start_row INTEGER NOT NULL,   -- selection_span (what workspace/symbol
    start_col INTEGER NOT NULL,   --   and goto-def landing sites report)
    end_row   INTEGER NOT NULL,
    end_col   INTEGER NOT NULL,
    container_id INTEGER,         -- package / class (strings), NULL for free
    flags     INTEGER NOT NULL    -- bit 0: linkage-visible (baked at shred
                                  --   time — scope-kind gate pre-applied)
);
CREATE INDEX idx_syms_name ON syms(name_id);
CREATE INDEX idx_syms_file ON syms(file_id);
```

Row contents follow the consumer matrix: what a query answers *without*
rehydration must be a column (workspace/symbol: name, kind, selection
span; phase-C registration: name, kind, linkage flag); everything else
(SymbolDetail, deref stacks, attributes, full span) stays blob-only and
rehydrates. `SymbolId` is NOT a column — row order within a file is the
symbol order, and identity across the boundary is (file, name, kind,
span), same as the refs rows.

### Phase B — eviction + present-view routing

`evict_symbols()` mirrors `evict_refs()`: strip `symbols` + the
symbol-derived rebuilt indices from index copies once their rows are known
present; `symbols_evicted` flag; consumers that read symbol detail from a
`CachedModule` route through a present view (the existing
`rehydrate_or_resident` LRU body — one miss policy for bag / refs /
symbols). `whole_present` grows the third axis.

### Phase C — register-from-tables warm start (PARKED → the Surface)

Parked deliberately, not dropped: the no-decode warm start needs a
per-file registration seed (names + kinds + linkage + specializes +
closure + the Perl projections), and that seed IS the span-free Surface
`docs/prompt-storage-engine.md` Phase 1 builds — building it twice
against two different shapes is the wasteful path. When the Surface
lands (persisted as its own small column), warm registration decodes
Surfaces only and the 9-minute chromium decode-everything wall falls out
of that work. The original sketch below is kept for the record.

Warm start today decodes every blob once (streaming, one resident at a
time) to (a) re-register name→file feeds and (b) re-shred missing rows.
With symbol rows always present, registration feeds come from one indexed
scan of `syms` ⋈ `strings` ⋈ `files` — no blob decode for unqueried files.
The resident copy for a warm, unopened file becomes a stub `FileAnalysis`
(evicted on all three axes) that rehydrates on first query.

Honest scope: registration is more than name maps — the Perl hub's
`record_workspace_projections` reads the BAG (loader shapes, plugin loads)
at registration. Those projections either (a) persist as their own derived
rows at parse time, or (b) keep a decode-on-warm path for the Perl tier
only. Decide from the consumer matrix; do NOT guess.

## Consumer matrix

<!-- filled from the code sweep: (A) registration-time readers (safe under
eviction), (B) query-time readers needing full Symbol structs (rehydrate),
(C) query-time readers needing name/kind/span only (rows). -->

## Nets

- `cargo test` default + `--features cpp`; layering tests (new table code
  stays in module_cache; eviction flags in file_analysis).
- Gold + e2e unchanged and green, **verified on cold AND warm AND migrated
  caches** (the lesson of the diagnostics round: a degradation that removes
  answers can read as a fix; identical-across-cache-states is the bar).
- `--refs-parity` still 0 mismatches (its symbol enumeration must not
  change answers under symbol eviction).
- workspace/symbol answers byte-identical resident vs rows on bugzilla +
  abseil.
- RSS: heap dump shows `symbols` ≈ 0 and `rebuilt_indices` ≈ small for
  index copies; chromium warm floor and wall both drop (target: floor
  ≲ 3.5 GB, wall well under the 9 m decode-everything pass).

## Phase B — landed, measured

Eviction is REGISTRATION-OWNED (`register_symbols_stripping` /
`register_workspace_stripping`): the name/edge feeds, the class-rank
record, and the unregister inverse list all extract from the WHOLE
analysis, then the axes evict, then the stripped arc is stored — a
caller-side strip would feed registration from an emptied `symbols`
(exactly the ordering bug the first cut hit). The cache-slot tie-break's
class rank moved onto the recorded `registered_names` pairs because
`module_defines_class` on a stripped occupant misjudged every existing
Class as a value.

The routing fan-out was the predicted risk and the gold net caught what
the unit suite couldn't: 18 cpp rows regressed on the first eviction flip.
The recurring miss was MIXED-VIEW expressions — a scan routed to the whole
view whose inner predicate (`symbol_is_class_content`, resolving the
owning container through `symbols_named`) still ran on the evicted copy.
The fixed sites beyond the planned list: the `has_member` ancestor gate,
`visible_defs_with_prefix`'s detail projection, `type_def_location`'s
word-resolve tail, the use-site target-minting lane in `resolve()`
(enum-constant references/rename), `preferred_definitions`' candidate
scan, and the two closure gathers. Verified: gold 413/16/0/0/0 identical
cold + warm (cpp build), `--refs-parity` 0 mismatches (bugzilla exhaustive
5,854 + abseil sampled), abseil `string_view` references byte-identical
(7,737), workspace/symbol rows-vs-resident set-identical (order differs —
rows append after the resident sweeps).

Measured (same box, same method):

| | before (PR #108 tip) | phase B |
|---|---|---|
| abseil resident payload | 46.1 MB (symbols 21.7, rebuilt 4.9) | **20.2 MB** (symbols 0.0, rebuilt 0.7) |
| abseil cold / warm RSS | 179 / 69 MB | 158 / **46 MB** |
| bugzilla cold / warm RSS | 124 / 83 MB | 110 / **72 MB** |
| references (7,737 sites) | 3.4 s | 5.1 s (rehydrates symbols too; LRU-bound) |

`include_closure` is now 53% of the abseil remainder — the representation
fork (see `docs/open-forks.md`).

## Risks

- **Fan-out of symbol readers.** Symbols have far more consumers than refs
  (completion, goto-def, hover, workspace/symbol, heatmap, enrichment,
  inheritance walks). The present-view routing must be mechanical and
  compiler-checked where possible (take fields private / rename, let the
  compiler enumerate readers) — not a grep-and-hope pass.
- **Perl hub registration projections read the bag** (above) — phase C's
  no-decode warm must not silently skip them.
- **workspace/symbol matching semantics** (fuzzy/substring) must produce
  the same result set from `strings` as from resident sweeps.
- **Synthetic symbols** (Moo `has`, DBIC accessors, plugin-emitted) ride
  the blob like everything else — rows are shredded from the built
  analysis, so they come along by construction; the net asserts it.
