# System-Level Rework Hitlist

Synthesis of an eight-lens architecture audit (layer placement, shape branches,
parallel structures, perl/pack duality, split quality, contract debt,
ownership/concurrency, data-model shape) with adversarial verification of every
finding. 41 findings audited: **27 confirmed**, 11 already owned by the ledger
(listed at the end), 3 rejected on re-read and dropped.

Ranking is leverage-per-effort: what unblocks or de-risks the most future work
per unit of migration cost. Effort tags: S (hours), M (days), L (a slice),
XL (an arc).

---

## Theme A — One resolution spine: projections, not parallel resolvers

The CandidateSet ADR's promise is "identity minted exactly once." Four
confirmed findings show verbs and tiers that still mint their own.

### A1. LANDED — `method_call_invocant_type` is THE invocant ladder; `method_call_invocant_class` is its dispatch projection

`method_call_invocant_type` (`src/model/file_analysis/resolution.rs`) is the
one invocant ladder — token-blind receiver-VALUE resolution (bridged /
positional / flow-narrowed place / function-call receiver / exact-span read
incl. Parametric-intact chain receivers / cross-file chain fallback / variable
/ bareword; a rung answers only when its type carries a dispatch class, so
classless answers fall through). `method_call_invocant_class` is its dispatch
projection: the SUPER/qualified method-token arm (a token overrides where
lookup STARTS, never what the receiver IS) + `dispatch_class_of` + the DBIC
source-moniker resolve. `method_call_invocant_class_raw` is deleted;
`fix_chain_receiver_hash_key_owners` asks `method_arg_owner` with the
unqualified method name, so `$rs->SUPER::search({k => 1})` now fills the
row-class key owner (pinned by
`super_qualified_search_still_fills_hash_key_owner`). The three-text-resolver
collapse (docs/prompt-cst-migration.md item 3) lands on this seam.

### A2. documentHighlight and linkedEditingRange bypass the CandidateSet through a second identity implementation — and the in-file family has already drifted three ways — **high leverage / L**

**The wrong embedding.** Both verbs route `symbols::document_highlights` /
`linked_editing_ranges` (`src/lsp/symbols/navigate.rs:179-208`) into
`FileAnalysis::find_highlights` / `find_references`, which sit on
`resolve_target_at` (`src/model/file_analysis/resolution.rs:10-76`) — a full
second identity minting (variable resolution, package-scoped callables,
MethodCall invocant typing + ancestor dispatch) parallel to
`resolve_symbol_scoped`. gd/references/rename construct the set
(`src/lsp/backend/server.rs:508/661/782`); these two never do
(`server.rs:968-985, 1371-1390`; CLI mirror `src/lsp/cli/query.rs:608`).
Lockstep is comment-enforced (`index/resolve/identity.rs:99-101`) and has
failed: `find_references` pre-claims field groups and lexical hash keys
(`cursor_queries.rs:164-173`) which `find_highlights` lacks (`:231`), while
`find_highlights` carries a cross-file same-class grouping fallback
(`:243-285`) neither sibling shares — three sibling verbs, three occurrence
sets at the same cursor. `index/resolve/target.rs:339-342` even carries the
smoking gun: `RefLocation.access` is `#[allow(dead_code)]` with a comment
saying documentHighlight "will migrate to refs_to in a follow-up" — the
follow-up that never happened.

**Why it is wrong at the system level.** Every future CandidateSet axis
(visibility, delegation aliases, ranking) silently skips these two verbs —
the exact C1/C2 disease class the ADR was written to end. The drift is
user-visible today on Moo attribute groups and lexical hash keys.

**Target shape.** Two projections on the set: `highlights()` = the
origin-file-narrowed image of `references()` carrying the already-minted
`RefLocation.access` (drop its dead-code allowance); `linked_editing_ranges()`
= the origin-file rewritable spans of `rename_edits()`, so the co-edit set
equals the rename image by construction. `find_highlights`' cross-file
grouping fallback moves into set construction as an origin-narrowed lane;
`resolve_target_at` shrinks to the Local-arm helper the set calls; the
lockstep comment dies because the discipline becomes structural.

**Migration order.** After A1 (the set's Local arm then rides the unified
ladder). Add the projections, flip navigate.rs and the CLI mirror, collapse
the in-file family.

**Gate.** Extend `candidate_set_visibility_axis_flows_to_every_projection` to
assert highlights narrows with the others; update the ADR projection table
with both verbs. E2e highlight/linked-editing cases at a field-group cursor.

### A3. Perl hover is a parallel resolution stack inside the thin adapter — **medium leverage / L**

**The wrong embedding.** Pack hover is the ratified shape: the set resolves
(`cs.hover_candidate()`), the adapter renders (`src/lsp/symbols/hover.rs:5-11,
113`). Perl hover is a pre-set resolution chain living in the adapter:
builtin-doc lookup, `resolve_imported_function`, `defining_module_cached` +
`bag_present`, and an FQ-call arm reading `RefKind::FunctionCall{resolved_package}`
(`hover.rs:279-386`) — real cross-file resolution in the layer rule 3 says
makes no analysis decisions, re-deriving the import-binding classification
`index/resolve` owns. Consequence: Perl hover CAN disagree with Perl goto-def,
the failure the set exists to prevent. The ADR's carve-out
(docs/adr/resolution-candidate-set.md:76) ratifies a language-specific
*renderer*, not a second *resolver*.

**Target shape.** The builtin / imported-call / FQ-call lanes become questions
asked of the set (or of FileAnalysis queries the set consults); hover renders
`hover_candidate()` plus a resolved-import-binding accessor through a
Perl-specific presenter. Amend the ADR hover row to "set resolves for BOTH
languages; each keeps its presenter."

**Migration order.** After A2. The renderer-*placement* half (Perl markdown
assembled in the model at `cursor_queries.rs:401` vs pack's in the adapter) is
owned by the parked multi-language brief — do the resolution half now, the
layer hoist as the opening move of that arc.

**Gate.** Byte-identical gold hover rows, or conscious row promotion. Gold
pins Perl hover output heavily; this is why the item is L, not M.

### A4. The "legacy text-based MCB resolver" is the load-bearing cross-file enrichment typer — **medium leverage / L**

**The wrong embedding.** `pipeline.rs:510-513` labels
`resolve_method_call_types` "the legacy text-based MCB resolver … a fallback,"
but cross-file enrichment runs it WITH the index as the primary mechanism for
imported-method-return propagation (`src/model/file_analysis/enrichment.rs:477`).
It violates the bag doctrine it postdates: text-keyed `MethodCallBinding` rows
(`build/builder/visit_use.rs:1388-1395`) are resolved through the string trio
plus `find_method_return_type`, then MATERIALIZED as `InferredType` TCs
(`enrichment.rs:499-513`) — values, not edges. It is also the sole production
keeper of doc-flagged `inferred_type` (`enrichment.rs:494`), and its hash-key
ownership consumers resolve BY NAME ONLY (`fold.rs:1784-1817`,
`completion.rs:874-881` via `sub_defining_package(mcb.method_name)`) — the
resolution class the stamp contract bans ("no name-only fallback,"
`lifecycle.rs:252-253`). Two stale comments (`queries.rs:285-288`,
`cursor_context.rs:341-343`) claim a bag→legacy fallback that does not exist.

**Target shape.** Slice A (typing): replace the value push with edge pushes —
`Variable → Edge(MethodOnClass{class, name})` under a dedicated `mcb` source
tag, clear-and-emit per run, letting the registry chase lazily with the index
in hand. `inferred_type` becomes honestly test-only. Slice B (ownership):
route `fold.rs:1806-1817` and `completion.rs:874-881` through the invocant
class (post-A1), keying ownership on `{class, method}`. Independent: fix the
two stale comments and retitle `pipeline.rs:510-513` to what the pass IS (the
MCB→bag bridge).

**Gate.** Enrichment idempotency tests (truncate-to-baseline still holds);
gold rows on imported-method variable typing; grep-test that no production
caller of `inferred_type` remains.

---

## Theme B — Analysis belongs to the model; adapters render

### B1. Diagnostic detection logic is semantic analysis embedded in the LSP adapter — **LANDED**

`unknown-hash-key` detection rides two FileAnalysis query seams next to
`closed_shape_is_whole_story` (`queries.rs`): `closed_shape_key_typos`
(the HashKeyAccess refs walk, sigil/write gating, bag query, closed-shape
match, whole-story trust gate) and `projected_key_typos` (the `Projected`
witness enumeration, the base-is-variable exclusion, the
`expr_type_at_span` materialization). Both return `KeyTypoSite` (span, key,
untruncated known_keys, base spelling — `dispatch.rs`, next to the other
diagnostic site structs). `collect_diagnostics` renders sites into
`Diagnostic`s (message wording, five-key elision, severity) and holds no
`crate::model::witnesses` import; the builder-layer helper test asserts on
`resolve_method_in_ancestors` instead of reaching up into lsp/.
docs/adr/narrowing-diagnostics.md names the seams alongside D1-D6.

### B2. The Perl builtin surface is three parallel encodings across three layers — **LANDED**

`model/builtins.rs` is THE Perl builtin surface: one sorted table
`name → (BuiltinKind, return-type, first-arg-type)` sourced from
perlfunc.pod. The adapter's `PERL_BUILTINS`/`is_perl_builtin` are deleted;
diagnostics suppression asks `is_builtin` (plus
`conventions::is_constructor_name` for indirect-object `new`), builtin hover
gates on the same membership, the builder's typed seeding reads the table's
type columns, and the BUILTIN RoleMask tier has its name source: `Function`
rows feed `complete()` as candidates (Perl origins only — the pack arm never
reaches the table). `index/builtins_pod.rs` stays the doc-VALUE store;
`builtins_pod_tests.rs` carries the anti-drift tripwire (perlfunc entries ⊆
table modulo a documented prose-noise set; `Function` rows ⊆ perlfunc). The
realized drift (`exp`, `fc`, `evalbytes`, `lock` typed/documented but
flagged unresolved) is pinned by tests in `model/builtins.rs` and
`diagnostics_tests.rs`.

---

## Theme C — Pack routing and capabilities decided at construction, not per handler

Both items are confirmed now-sized slices consistent with (and shrinking) the
parked full unification in docs/prompt-unify-language-paths.md.

### C1. Pack routing is a construction fact on the CandidateSet — **LANDED**

Pack routing splits into its two real facts, each with one owner. The POLICY
fact (pack semantics on the set: VISIBLE widening, rename full-or-refuse,
pack def_paths) is derived inside `resolve()` from the origin's stamped
`FileAnalysis.language` (`#[serde(default = "perl")]`, stamped by
`PackDriver::analyze_with_path`, read via
`LanguageRegistry::is_pack_language`) — `pack_routed()` is deleted, so no
handler declares or can forget it, and every projection inherits it by
construction. The STORE fact (hub vs pack sub-index) has one speller,
`ModuleIndex::lookup_for(language) -> RoutedIndex` (an owning hub-or-pack
value handlers hold and pass into `resolve()`); the
`pack_store_selection_stays_in_lookup_for` layering tripwire keeps
`pack_index()` out of the LSP layer. All ~13 handler/CLI preambles are
one-line `lookup_for` calls now. `resolve()` cannot take the hub and route
internally because a pack sub-index is an `Arc` out of the hub's registry —
the set borrows, so the caller must own the routed store's lifetime; that is
what `RoutedIndex` is.

### C2. LANDED — driver capabilities are asked of the pack from one shared home

`LanguageRegistry::has_include_tokens` / `has_preprocessor_macros`
(`build/language_driver.rs`, beside the registry) are THE boolean capability
askers; the LSP handlers and their CLI/--batch mirrors gate the include-token
lanes on the same call, so editor and gold answer identically by construction
(the CLI's `lang_id == Some("cpp")` probes are deleted — the server's
asked-never-named spelling was the correct side). Two askers is the recorded
ceiling (docs/PARKED.md): the third collapses to a generic
`pack_cap(lang, sel)`. `capability_askers_answer_by_language_id` pins the
by-id answers; the lifecycle `language != "perl"` policy branches stay owned
by the parked unification.

---

## Theme D — Serving-path ownership: state mutated by its owner, coordination proven once

### D1. LANDED — open-doc enrichment is a derived artifact with one writer

`FileStore::enrich_open(url, idx)` is THE open-doc enrichment writer:
clone-and-enrich off the store lock, ptr-guarded swap, returns the derived
`Arc<FileAnalysis>` for the caller to read. `publish_diagnostics` and the
bulk paths (the resolver `on_refresh` closure and the perl cold-open heal,
both through `refresh_open_diagnostics`) read the returned artifact — no
notification handler mutates a stored analysis, and `for_each_open_mut` is
deleted. The record-surface-BEFORE-publish ordering contract is retired
structurally: freshness records read `Document::baseline_surface` (projected
at every build seam from the pristine analysis, recorded via
`record_and_dirty_value`), so enrichment state cannot reach a surface record
no matter when either runs. Pinned by
`enrich_open_swaps_derived_copy_and_keeps_baseline_surface`.

### D2. LANDED — the resolver thread and ModuleIndex share one owned core (`IndexCore`)

`index/module_index/index_core.rs` owns the shared mutable state — cache,
edge indexes, loader-config shapes, stale/available sets, builtins, resolve
queue/notify, workspace-root channel, generation map + counter, long-lived
flag, bag-cache cell — as ONE struct held via a single `Arc` by `ModuleIndex`
(async side) and the resolver thread (blocking side); the 13-Arc plumbing and
the free-fn twins (`insert_into_cache`, `rebuild_reverse_index`,
`mint_registration_gen`, `stamp_missing_import_gens`) are deleted.
`IndexCore::insert_resolved` is the one spelling of "a resolution landed":
stale-pin clear → generation mint → whole-analysis projections (edge feed +
loader shapes) → registration-owned strip (`strip_import_copy`, core-owned) →
store, with the None-never-clobbers guard; `ModuleIndex::insert_cache` and
the thread both route through it. This fixes the realized drift (an
@INC-resolved plugin-carrying module never fed `loader_config_shapes` on the
thread path — the projection also had to move PRE-strip, since it reads the
witness bag the strip drops). `resolver_loop` is the single loop body,
parameterized by `Option<ServerSession>` (builtins hydrate, warm strip, stale
priority, cpanfile scan, dependency descent, progress are explicit server
gates); the test-loop divergences (no bag-cache stale-pin clear; memoized
None) are unified on the main spelling. Pinned by
`thread_path_resolution_feeds_loader_config_shapes` and
`insert_resolved_none_does_not_clobber_indexed_module`.

### D3. LANDED — pack invalidation is one index-side subsystem (`PackInvalidator`)

`src/index/pack_invalidator.rs` owns the serialization lock, the H9-2
bulk-index coordinator (`PackChangeCoordinator`, relocated), and the H9-1
source-generation guard (`claim_source_gen` + its map, relocated off
`ModuleIndex`). Entry points: `file_changed(root, hub, open_docs, path,
deleted) → InvalidationOutcome { deferred, refresh_open }` and
`begin_bulk_index`/`finish_bulk_index`; the eviction/re-analysis/swap worker
is private, so a new invalidation path cannot compile around lock,
coordinator, or guard. The include-closure consumer rule is ONE predicate
(`is_consumer`) applied to registered files and open docs alike; the
realized drift — Backend refreshed open consumers even on an Unchanged
surface verdict — is unified on the gated spelling (open consumers skip
too; the changed file's own open doc always refreshes), pinned by
`surface_gate_covers_registered_and_open_consumers`. Backend shrinks to
forwarding events and publishing `refresh_open` through the gather
single-flight. The H9 race tests live with the owner
(`pack_invalidator_tests.rs`).

### D4. Blocking-ness of query paths is decided per-verb in handlers; three verbs still do SQLite/fs I/O on the reactor — **medium leverage / M**

**The evidence.** references and rename carry the "real I/O … never the
reactor" rationale and spawn_blocking (`server.rs:649-676, 770-798`);
goto-definition's and hover's raw-word lanes run inline
(`server.rs:535-544, 840-857`) calling `pack_xfile_word_at`
(`lifecycle.rs:400-433` — `fs::read_to_string` + `whole_present`, which on LRU
miss is a SQLite+zstd decode via `registration.rs:641-676`); workspace/symbol
runs its resident sweep + `sym_row_search` SQLite pass inline
(`server.rs:1137-1213`).

**Target shape.** One Backend helper (`run_query`, the single spawn_blocking
hop the references/rename handlers already prototype — factor their duplicated
pack/base_idx re-binding into it) becomes the only way a handler reaches set
construction/projection, `sym_row_search`, or any rehydration reader. Move the
raw-word lanes inside the blocking closure; wrap workspace/symbol. Optional
enforcement: no direct call to `resolve()`/`sym_row_search`/`whole_present`
from an async fn in `server.rs` outside `run_query`.

**Gate.** The grep-test above; bench scenarios (edit-bench) confirming no
latency regression and no reactor stall under cold LRU.

### D5. The bounded-wait lost-wakeup shape is hand-spelled three times, debounce-by-generation twice — **medium leverage / M**

**The evidence.** The "register interest BEFORE the final re-check" discipline
is comment-spelled at three await sites (`indexing.rs:331-334, 363-365, 393`);
the generation-captured debounce exists twice (`spawn_debounced_rebuild`,
`lifecycle.rs:253-297`; the `refresh_gen` dance inline in `Backend::new`'s
on_refresh closure, `lifecycle.rs:170-221`). The repo's own precedent — four
bare gather caches drifting into check-release-compute races before
unification (docs/prompt-storage-residuals.md:44-58) — shows the class rots
when each site proves the invariant independently.

**Target shape.** Two small types in a backend submodule: `ReadyGate`
(latch + Notify + register-before-recheck bounded wait; each caller keeps its
probe closure) replacing the three await bodies, and `DebouncedLatest`
(generation-captured settle-window debounce) replacing both spellings — which
also gets the closure out of `Backend::new`. Leave `GatherRegistry`
(unit-tested single-flight) distinct; `PackChangeCoordinator` and
`claim_source_gen` now live under D3's `PackInvalidator`.

**Gate.** Unit tests on the two primitives (the wakeup proof written once);
existing indexing await tests.

---

## Theme E — The data model carves its own joints

### E1. LANDED — Surface::project's mirror of FileAnalysis is now structural

`FileAnalysis::surface_feed(&self) -> SurfaceFeed`
(`model/file_analysis/surface_feed.rs`) destructures every FileAnalysis field
with no `..` rest pattern — 14 fields bound into the feed, the rest discarded
under grouped why-not-visible comments — so a new field is a compile error
until classified; `Surface::project` reads only through the feed (the
`analysis` handle carries the three derived queries). The two leaks are
projected: `export_tags` (sorted tag → members) and `dbic_source_name`, each
with an equality-net arm in `surface_tests.rs` proving a header-only edit
flips the verdict to Changed. STUB_VERSION bumped (Surface rides the stubs
table). R1 is restated as compiler-enforced in CLAUDE.md and
`docs/adr/storage-engine.md`.

### E2. FileAnalysis is ~50 flat serialized fields from seven fused models, spelled in quadruplicate — **high leverage / XL**

**The wrong embedding.** The struct (`src/model/file_analysis/mod.rs:41-431`)
fuses core tables, per-package facts, export surface, plugin lane, narrowing
lane, pack lane, and eviction bookkeeping as flat siblings; the field list is
re-spelled in `FileAnalysisParts` (`mod.rs:438-482`), `new()`'s exhaustive
destructure (`lifecycle.rs:11-128`), and `heap_estimate`'s hand-enumerated
buckets (`lifecycle.rs:619-866`) — whose bucket names already NAME the natural
sub-structs. Six per-package sibling maps keyed by the same package name
(`mod.rs:94,109,159,277,294,299`) force `Surface::project` to re-join per
package (`surface.rs:144-153`). Each `evict_*` must hand-clear its sibling
index fields (`lifecycle.rs:164-192`) — a fifth parallel spelling.

**Target shape.** Cut along the seams the heap probe names, phased
cheapest-leverage first: (1) `PackageFacts` replacing the six per-package maps
(`parents/uses/framework/requires/is_role/dynamic_parents` — `parents_of` and
Surface read one entry); (2) `RefTable` and `SymbolTable`, each owning its
indices, its `evict()` (index clears by construction), its `heap_estimate()`
arm, and its enrichment baseline; (3) `PackFacts` and `PluginFacts`.
`FileAnalysisParts` collapses to moving sub-structs; `new()` shrinks; serde
field order preserved, one EXTRACT_VERSION bump at the end.

**Migration order.** Phase 1 (PackageFacts) is an M-sized standalone win; do
it early. Phases 2-3 land best AFTER E1 (surface_feed then destructures
sub-structs); E3's `Ref::binding` is already landed and moves into RefTable
with the rest of the refs.

**Gate.** The residency tripwire and eviction tests already net phase 2;
equality-net tests cover PackageFacts' Surface join; heap probe totals
compared before/after.

### E3. LANDED — Ref's resolution outcome lives in one home: `Ref::binding`

`Ref::binding: Option<RefBinding>` (`core_types.rs`) is the one home for
every resolution outcome — `Symbol(SymbolId)`, `Function { package }`,
`Method(MethodTarget)`, `HashKey { owner, sym }`, `Handler { owner, sym }` —
replacing the deleted `resolves_to` / `resolved_method_target` flat columns
and the deleted `FunctionCall.resolved_package` / `HashKeyAccess.owner` /
`DispatchCall.owner` variant payloads (RefKind is pure written shape again;
`GatedRef` carries the same binding). Consumers read through the projection
accessors (`resolved_symbol` / `method_target` / `resolved_package` /
`hash_key_owner` / `handler_owner`) and post-passes stamp through the
`bind_*`/`link_owned_symbol` mutators, so no call site matches `RefBinding`
against `RefKind` itself. `row_seed` derives the same qual columns from the
binding (row format unchanged — no REF_ROWS_VERSION bump); EXTRACT_VERSION
bumped (175→176). The `Function { sym }` slot was dropped — no path mints a
FunctionCall→symbol link today, and a dead field is not a seam. Landed
before E2's RefTable; when RefTable lands it inherits the field as-is.

### E4. Symbol presentation policy is encoded three ways — **low leverage / M**

`hidden_in_outline()` unions an `'include_guard'` attribute probe with a
two-variant detail match (`core_types.rs:587-596`); `Sub` and `Handler` each
re-declare `display`/`hide_in_outline` (`:646-676, :710-717`); `outline_label`
is a third channel (`:215-232`); `SymRowSeed` re-bakes the scattered verdicts
(`:959-974`). Any other kind wanting hiding is silently un-hideable until
someone grows the match. Target: one `presentation: Presentation
{ hide_in_outline, display, label }` (`#[serde(default)]`) minted at symbol
synthesis; genuinely kind-semantic flags (`is_constant`, `opaque_return`,
`lexical`) stay in the detail. EXTRACT_VERSION bump; row flags unchanged.
Gate: outline/workspace-symbol tests; do alongside E2's SymbolTable phase.

---

## Theme F — The tree tells the truth: splits, placement, ledger hygiene

### F1. LANDED — file_analysis's query parts are cut by concern

Each concern lives in ONE part behind the unchanged mod.rs glob surface:
`hover.rs` holds every markdown hover renderer (`hover_info`, `member_hover`,
`format_handler_hover`, the `format_symbol_hover` pair — the file the parked
multi-language hoist lifts from); `ancestry.rs` holds the parent-enumeration
seam (`parents_of`), the bounded isa walkers (`walk_ancestry`, `class_isa`,
`class_isa_prefix`), the include-self MRO walk with its method resolution
(`for_each_ancestor_class`, `resolve_method_in_ancestors`,
`resolve_super_method`, `method_resolution_on_class`,
`class_has_unresolved_ancestor`), and the family/descendant walks (placement
only — the GraphView collapse stays a separate PARKED item); `sym_index.rs`
holds the raw symbol/ref index accessors, with `sym_row_seeds` beside the
Surface classification gate in `surface_feed.rs`; the resolution residue is
`invocants.rs` (target-at-cursor, the invocant/dispatch ladders, role
contracts, class-content predicates), un-shadowing `index/resolve/`.

### F2. module_resolver.rs and module_cache.rs are the two monoliths the restructure skipped — **high leverage / L**

The two largest flat production files (2189 and 1944 lines) against the repo's
own "oversized modules are directories of focused parts" doctrine, and they
are where the parked storage residuals and language-path unification must
land. `index/module_resolver/` → `thread.rs`, `inc.rs`, `index_perl.rs`,
`index_pack.rs`, `persist.rs`, `watch.rs`; `index/module_cache/` → `conn.rs`,
`schema.rs`, `blob.rs`, `rows.rs`, `stubs.rs`, `warm.rs`. The
`index_perl.rs`/`index_pack.rs` sibling cut is the leverage: it turns the
parked convergence into a confrontable two-file diff instead of archaeology
inside one 2.2k-line file. Motion must preserve the ordering invariants
(stub delete inside `save_to_db`; register-after-chunk-commit) — they are
invariants, not file-local details. Sequence after D2 (IndexCore) or accept
one rebase. Gate: pure code motion; residency tripwire + eviction tests.

### F3. LANDED — helpers.rs and infra.rs families live with their sibling seams

`build/builder/` has no grab-drawer: the bless family is `visit_bless.rs`,
POD doc capture is `docs.rs`, AUTOLOAD/__DATA__ synthesis sits beside the
pipeline synthesis passes in `pipeline.rs`, the DBIC resultset-parametric
family lives in `visit_method.rs` (one source file for the parked DBIC
phase-3 lift), and `add_fold_range` is in `visit_decl.rs`; the residue is
`extract.rs` under a tight tree-reading charter. `infra.rs` keeps scope /
symbol-minting / package-range / call-arg infrastructure plus
`coderef_return_edge_for`; flow-edge minting lives in `narrowing.rs`
(docs/adr/flow-narrowing.md maps to exactly one part) and the plugin
`ArgInfo` factory in `plugin_emit.rs`.

### F4. Two unledgered bespoke ancestry walkers, blocked on a missing prune verdict in GraphView — **medium leverage / M**

`trigger_view_at` (`class_queries.rs:903`) hand-rolls an UNCAPPED
transitive-parent BFS (every sibling walk is budgeted/capped);
`unfulfilled_role_requires` (`invocants.rs:1042`) hand-rolls a
role-gated BFS with an idiosyncratic visited-size cap. Neither appears in the
ledger (PARKED.md's four→one entry covers only the three isa DFSes and warns
"not a fifth bespoke helper" — these are the fifth and sixth). The blocker is
real: `GraphView::walk`'s visitor returns `ControlFlow<()>`
(`model/graph.rs:109-121`) — it can stop the whole walk but cannot prune one
node's expansion, which both the role walk and the parked walk_ancestry
collapse need. Target: generalize the visitor to
Continue/PruneChildren/Stop (the WalkVerdict shape `dispatch.rs:200-209`
already proved); `trigger_view_at` becomes `walk(Class(pkg), INHERITS,
idx=None)` with the APP_SURFACE edge masked (an EdgeKindMask bit — also what
the parked collapse waits on); the role gather prunes at non-role nodes,
preserving docs/adr/role-contracts.md's edge semantics. Both inherit MAX_DEPTH
for free. Minimum slice if deferred: add both walkers to the PARKED four→one
entry so the ledger matches the code. Gate: role-contract and trigger-view
tests; a pathological-parent-graph depth test.

### F5. LANDED — small truth-telling fixes

`src/util/` is the neutral leaf tier (std-only, no crate paths — enforced
by `layering_tests::util_tier_is_std_only`) and holds `timings.rs`;
`cpp_obstacle_test_corpus.rs` is recognized as test fixture data by the
layering walk's `_test_corpus` stem predicate; the `refs_present` ghost is
scrubbed — comments, CLAUDE.md, and docs/adr/relational-ref-index.md all
state the real shape (no refs-axis reader; the backward walk goes through
`whole_present`, and a single-axis refs view must not be minted).

---

## Theme G — Plugin-owned vocabulary, not core allowlists

### G1. LANDED — Moo-family `has` semantics are one plugin-declared manifest

`frameworks/moo.rhai`'s `framework_mode_makers()` declares module → flavor
(`"Moo"`/`"Moose"`) + exported keyword surface, and `triggers()` is derived
from it, so the plugin's accessor-option gate and core's native-synthesis
gate share one declaration. The builder bakes `framework_mode_modules:
HashMap<module, (FrameworkMode, keywords)>` at plugin load; `visit_use`
looks consumers up (Mojo::Base stays a structural core arm) and the `option`
keyword is gated per-package via `package_imports_framework_keyword`. Core's
Moo/Moose match arms and the `package_uses_moox_options` module check are
deleted. The realized Mouse drift resolved toward full support (Moose
flavor: native accessor + plugin options), pinned by
`mouse_has_gets_both_native_accessor_and_plugin_predicate`; the open seam is
pinned by `test_plugin_declared_framework_mode_maker_grants_has_semantics`.

### G2. LANDED — name-dispatched action exemptions are per-rule plugin vocabulary

`plugin::ParamType.implicit_action_names` (`#[serde(default)]`) declares the
sub names a framework dispatches by name alone — they pass that rule's
`requires_action_attr` gate; catalyst.rhai declares
`begin`/`end`/`auto`/`default`/`index` on its two attribute-gated wildcard
rules, and core's `collect_param_type_matches` checks per-rule (the
`CATALYST_PRIVATE_ACTIONS` const is deleted). A non-Catalyst rule that
declares no names exempts nothing, pinned by
`attr_gated_rule_without_declared_names_exempts_nothing`.

---

## Suggested sequencing

1. **Cheap truth + drift stoppers (S):** F5 (timings, cpp_obstacle,
   refs_present), G2, C2, F3.
2. **Correctness-bearing seams (M):** E1 (SurfaceFeed + two backfills), A1
   (invocant ladder), B2 (builtins), B1 (diagnostics seams), G1 (Moo gate),
   C1 (pack routing — LANDED), D3 (PackInvalidator — LANDED), F1 (file_analysis
   recut — LANDED), E2 phase 1 (PackageFacts).
3. **Structural slices (L):** D1 (enrichment as derived artifact), D2
   (IndexCore — LANDED), A2 (highlights/linked-editing projections), F2
   (monolith directories, after D2), A3/A4, D4/D5, E3 (LANDED).
4. **The arc:** E2 phases 2-3 (+ E4 alongside).

---

## Known-parked (owned elsewhere — not news)

Verified real, but deliberately owned by the ledger; act on them through their
owning doc, not this list.

- Cursor-context detection split across lsp/ (Perl) and build/ (pack), stitched
  by language branches — CLAUDE.md rule 6 + docs/prompt-unify-language-paths.md.
- Per-verb `language != "perl"` routing across ~12 files —
  docs/prompt-unify-language-paths.md (its three-file seam list is stale;
  amend to the current spread, and C1's `lookup_for` is the doc's own
  opportunistic-convergence slice).
- `universal_methods` DBIC/Moose meta-method allowlist in diagnostics —
  docs/prompt-dbic-as-plugin.md item 2.
- Runtime-exporter recognition module-name allowlist in visit_calls —
  docs/open-problems.md (deferral rationale recorded there).
- Three byte-capped LRU cache cores — docs/PARKED.md (re-examine on a fourth;
  add a cross-link to prompt-storage-residuals.md's R4 dedup residual, which
  is standing split-cost evidence).
- Pack completion lanes bypassing CompletionCandidate / two expected-type
  re-rankers — docs/PARKED.md + docs/prompt-unify-language-paths.md step 3.
- Slot consumers forked above the seam (two disjoint Slot projections) —
  docs/prompt-unify-language-paths.md step 3.
- Two hover renderers at two layers — docs/prompt-multi-language.md (names
  hover rendering as the driver-side move); add a hover row to
  prompt-unify-language-paths.md's table (cheap doc edit).
- Three divergent text→class invocant resolvers —
  docs/prompt-cst-migration.md item 3; sequence immediately after A1 and
  annotate the item with the re-typed-`$self` and missing-index divergences.
- Watcher re-registration pins whole copies (never re-strips) —
  docs/prompt-storage-residuals.md (design sketch recorded there).
- DBIC verb tables hard-coded in `ParametricType` beside plugin-declared
  `column_keyed_verbs` — docs/prompt-dbic-as-plugin.md phase 3; add a one-line
  addendum that both lists are consulted in the SAME builder pass today
  (fold.rs:592/617/644), which is sharper than the brief records.

## Rejected on verification (dropped)

- Mojo route-brand vocabulary in chain.rs — the residence is decided by
  docs/adr/route-branding.md; generalizing now is speculative seam-building.
- PERL_LSP_BENCH as a dead bootleg timer — it has live consumers in
  scripts/qa/ (run-bench.sh, analyze-bench.sh, README.md); migrate-not-delete
  if ever touched.
- Eviction-as-boolean-flags — the read side is policed at the documented
  boundary seam (docs/adr/relational-ref-index.md:198-223); `Evictable<T>`
  would tax hundreds of in-file readers for a hazard they cannot hit; the
  legitimate kernel (axis owns its indices) is delivered by E2.
