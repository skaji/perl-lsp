# Design brief — the storage engine arc: span-free surface, relational shred, incremental freshness

Status: design prompt for a follow-up agent. Nothing here is landed; this is
the forward design that `docs/chromium-scale-analysis.md` ends by demanding
("SQLite as a query engine … the load-bearing decision") fused with the
incremental-framework eval on branch `claude/salsa-incremental-eval-1bmv23`
(`docs/eval-salsa-incremental.md` @ `721f378` — read it first; its numbers are
not repeated in full here).

Base: `spike/cpp-support` @ `3520b2e`, `EXTRACT_VERSION 163` (post
Memory Slice 2 — witness-bag eviction + `pack_bag_cache` LRU are landed and
this design builds on them, not around them).

## The one-line thesis

One new artifact — a **span-free, per-file cross-file Surface** — is
simultaneously (a) the row source for the relational SQLite shred that removes
the 0.51 MB/file resident wall, (b) the early-cutoff boundary that makes
incremental invalidation actually cut, and (c) the thing "always enriched"
enriches. Build the artifact once; all three arcs ride it.

## Why now: two independent walls, one shared missing piece

- **The memory wall** (`docs/chromium-scale-analysis.md`, MEASURED): after
  Slice 2, the pinned resident set — `refs`, `symbols`, parents, include
  closure — costs 0.51 MB/file, dead-linear, projecting whole-tree Chromium at
  ~67 GB. The fix named there: stop holding the reverse-reference substrate
  resident; shred it into indexed SQLite tables and let B-trees answer
  `references`/workspace-symbol from disk.
- **The freshness wall** (`docs/eval-salsa-incremental.md`, MEASURED): cross-
  file enrichment is open-documents-only, not because it's expensive
  (~0.1 ms/file) but because there is **no consumer→dependency edge** — when
  file B changes, nothing knows which files' enrichment is now stale. Open
  docs brute-force re-enrich on every resolver tick; everything else drifts
  until the query-time structural walk (`query_rec` + `parents_of`) papers
  over it.

These meet at edit time. A relational store you can't invalidate precisely is
a store you must re-shred wholesale per edit — at 131K files that's the naive
"rebuild the world" the eval measured at ~370 ms *per 340 files* and growing
linearly (≈2+ min extrapolated to Chromium, per keystroke-adjacent event).
With dependency tracking and a span-free cutoff boundary, the eval's spike
measured a body-only edit at the root of a 339-descendant inheritance tree at
**~1 ms, zero dependents recomputed, flat across workspace size** — a
~160–200× gap that widens with scale. The shred makes always-enriched
*storable*; the surface makes it *cheap to keep true*.

## The Surface (the keystone type)

A position-independent projection of one file's cross-file-visible facts:

```
Surface {
    packages: [ {
        name,
        parents,          // resolved isa/roles/bridges, post-fold
        methods: [ { name, kind, arity_shape, return: InferredType,
                     hash_keys, provenance } ],
    } ],
    imports, exports, reexports,
    plugin_bridges, app_surface_consumers,
}
```

Contract (each clause is load-bearing):

- **No spans, no `Point`s, no byte offsets, anywhere.** Equality of two
  Surfaces must mean "no cross-file-visible change". A body edit, a
  reformat, a comment, a private-sub rename must yield an **equal** Surface —
  that equality is the early-cutoff firewall (rust-analyzer's "typing in a
  body never invalidates global data"). One smuggled span collapses the
  firewall silently; add a unit test that reformats/comment-pads a fixture
  and asserts Surface equality byte-for-byte.
- **Typed fields, not display strings** (rule #10's lossy-string form).
  `return: InferredType`, never `"returns Foo"`. Consumers project.
- **Derived from the post-fold `FileAnalysis`**, emitted by the builder as a
  sibling output — a pure projection, produced once per build, immediately
  after `finalize_post_walk()`.
- **Language-neutral shape.** Perl packages, C++ classes/namespaces, Python
  classes all fill the same struct; the `language_driver` seam already gives
  each driver its own extraction, and the Surface is the shared vocabulary
  above it. No per-language Surface variants — a consumer switching on
  language is the same rule-#10 bug as switching on shape.

### Surface ≠ outline (don't ride documentSymbol)

Tempting and wrong: the LSP outline (`OutlineSymbol`) looks like "the symbols
in this file" too. The change-sets are orthogonal, and both mismatch
directions bite:

- **Under-invalidation (silent wrongness):** the outline can't see a resolved
  return type change (`return $x` → `return {...}` — a body edit), hash-key
  changes, or `@ISA`/`with` edits that keep the symbol list identical. Keyed
  on outline, dependents keep stale types with no crash to notice.
- **Over-invalidation (perf):** the outline is span-bearing by construction
  (`span`, `selection_span`) and changes on private-helper adds and on every
  sub that moves. Keyed on outline, unrelated edits stampede dependents.

The correct relationship is inverted, rust-analyzer's `ItemTree`/`AstIdMap`
split: the Surface is the **lower**, position-independent layer; the outline
stays a span-bearing *sibling* projection. They share only the stable
symbol-identity spine. The outline's `detail` string should eventually be
*rendered from* Surface data, never parsed as a source of it.

## The relational shred

Per `chromium-scale-analysis.md`'s "Implication" section, verbatim intent:

```sql
CREATE TABLE refs(file, target_name, kind, line, col, invocant_class, ...);
CREATE INDEX idx_refs_target ON refs(target_name);
CREATE TABLE symbols(file, package, name, kind, return_type, ...);
CREATE TABLE parents(class, parent);          -- transitive walks stay in Rust
```

- Rows are written at index time from the build output; `refs`/`symbols` stop
  being pinned resident for pack workspaces (the Perl open-document tier is
  untouched). `references`, workspace-symbol, and goto-def's cold tail become
  indexed SELECTs.
- **Hybrid, permanently:** the witness bag and the fixed-point fold stay
  blob + in-Rust (Slice 2's `pack_bag_cache` already serves that tail);
  recursive walks (ancestry, reducer edges) don't map to single SQL queries.
  Don't attempt "everything in SQL".
- **Latency tiering maps onto the existing priority tiers**: `documents`
  (resident, ns) → `workspace_index` (LRU) → SQLite (µs–ms). goto-def on an
  open file must never wait on a disk query.
- The `refs` row shape wants the same provenance discipline as `Ref` — a row
  that can't say *why* it exists can't power rename safely.

## Freshness: invalidation over the Surface

The dependency graph is small and explicit once the Surface exists: file A's
enrichment depends on `Surface(B)` for each B in A's imports ∪ parent chain ∪
bridges. Two viable engines, decision **deliberately deferred** behind the
Surface boundary (it's reversible once the boundary exists):

- **Hand-rolled** (recommended first cut): a reverse-dep index
  (`Surface-of-B → consumers`) — we already maintain reverse indexes in
  `ModuleIndex` — plus a dirty-set walk on Surface *inequality* after rebuild.
  Equal new Surface → stop (that's the cutoff). A few hundred lines, no new
  deps, no framework risk. Termination/cycles: seen-set on the walk, same as
  every other walker here.
- **Salsa 0.27**: buys the same thing plus durability/revisions/cancellation.
  Real costs measured in the eval: cyclic fixed-point fold must stay inside
  one opaque tracked query (salsa panics on cycles), `'db` lifetime virality,
  memory-tuning burden (RA's post-migration blowups), pre-1.0 churn, and its
  persistence is prototype-grade so SQLite stays regardless. The working
  spike is `src/salsa_bench.rs` on the eval branch (`--features salsa_bench`)
  — port it forward if this path is chosen.

Either way the update pipeline per changed file is: rebuild →
`Surface` equal? → stop | else: upsert that file's shred rows + re-enrich
exactly the dirty consumers. Re-shredding one file's rows is a keyed
DELETE+INSERT — the store never rebuilds wholesale after first index.

## Phase ladder (each lands alone, each exits with a measurement)

1. **Surface extraction + equality tests.** Emit `Surface` from the builder
   (all drivers), serde it, land the reformat/body-edit equality tests and a
   surface-change test (return-type edit → unequal). No consumers yet.
   Exit: Surface stability proven on the gold corpus + one pack fixture.
2. **Shred `refs`/`symbols` for pack workspaces; un-pin them.** SQLite-backed
   `references`/workspace-symbol behind the existing tier order.
   Exit: abseil/folly peak RSS re-measured (expect the 0.51 MB/file wall to
   drop to the bag-LRU + Surface residual); Chromium re-run against the
   20 GB guard — target: **completes**.
3. **Reverse-dep + dirty-set freshness over Surface; enrich the workspace
   tier** (not just open docs). Exit: `bench/gen_corpus.py` (eval branch)
   edit-scenarios reproduced through the real server path — body edit ≈
   O(1 file), surface edit ≈ O(true dependents).
4. **Materialized queryability** — the "interesting data" SQL views (unused
   exports, implementors-of-role, callers-by-arg-type) over the shred, which
   Phase 3 keeps perpetually true. Optionally revisit Salsa here if the query
   graph has deepened past what the dirty-set comfortably serves.

## Honest boundaries / risks

- **R1 — span leakage into Surface** is the whole-design failure mode and it
  fails *silent-wrong*, not loud. The equality tests in Phase 1 are the
  regression net; treat any Surface field addition without an equality test
  as a review reject.
- **R2 — write amplification at index time**: shredding 131K files' refs is a
  bulk insert; batch per file in one transaction, and keep `EXTRACT_VERSION`
  as the schema-evolution gate (row shapes are version-pinned like the blob).
- **R3 — two sources of truth during transition**: while resident `refs` and
  shredded `refs` coexist (Perl tier vs pack tier), route ALL cross-file ref
  queries through `resolve.rs::refs_to` so the tier split lives in exactly
  one place. No handler may know which store answered.
- **R4 — enrichment writes vs shared `Arc`s**: always-enriching the workspace
  tier collides with today's immutable `Arc<FileAnalysis>` sharing. The
  enrichment output should be *derived rows/witnesses keyed by file*, not
  in-place mutation of shared analyses — the truncate-to-baseline dance is an
  open-document-ism that must not be exported to the workspace tier.
- **R5 — Chromium is the honest yardstick**, and it's retained at
  `~/personal/cpp-bench/chromium` with the RSS-guard method documented in the
  scale analysis. A design that only improves abseil hasn't met the wall.

## Pointers

- `docs/chromium-scale-analysis.md` — the wall, the shred sketch, the method.
- `docs/eval-salsa-incremental.md` + `src/salsa_bench.rs` + `bench/gen_corpus.py`
  (branch `claude/salsa-incremental-eval-1bmv23`) — the incremental eval,
  spike code, corpus generator, and the measured cutoff numbers cited here.
- `docs/adr/memory-slice-2-lru.md`, `src/pack_bag_cache.rs` — the landed
  eviction/rehydration tier this composes with.
- `docs/prompt-enrichment-inheritance-residual.md` — what enrichment still
  owes; Phase 3 here is where its per-manifest matrix gets a workspace-wide
  substrate.
- `docs/prompt-bounded-memory.md` — the memory arc's origin story.
