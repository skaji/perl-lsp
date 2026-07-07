# Open architectural forks — for discussion

Convention (standing order, 2026-07-03): when autonomous work hits a genuine
architectural fork, we (a) pick the LOOSELY-COUPLED option — reversible,
behind a seam, no serialized-format lock-in where avoidable — (b) implement
it, and (c) log the fork here with the options, what was picked, why, and
what undoing it would cost. The user reviews this ledger; entries get
resolved (ratified or reversed) explicitly.

Format per entry:

## <fork name> — <date> — <status: OPEN / ratified / reversed>
- **Context:** where it came up (slice, finding).
- **Options:** A / B (/ C), one line each.
- **Picked:** which, and the loose-coupling story (how it stays undoable).
- **Undo cost:** what reversing takes.
- **Discussion needed:** the question for the user.

---

## Freshness engine: hand-rolled reverse-dep vs Salsa — 2026-07-06 — RATIFIED (veesh, 2026-07-07)
- **Context:** storage-engine mission phase 3 (docs/prompt-storage-engine.md;
  eval on claude/salsa-incremental-eval-1bmv23). The Surface boundary makes
  the engine choice reversible.
- **Options:** A — hand-rolled `FreshnessIndex` (surface records +
  name-keyed reverse-dep + seen-set dirty walk, ~150 lines, no deps).
  B — Salsa 0.27 (revision machinery, durability, auto edges; `'db`
  virality, memory-tuning burden, pre-1.0). C — comemo (lighter, no
  revision machinery).
- **Picked:** A, per the eval's own recommendation: we already own the
  reverse-index discipline, and the engine sits entirely behind
  `Surface`/`SurfaceVerdict` — swapping in Salsa later touches the
  recording sites, not the consumers.
- **Undo cost:** moderate — reimplement `record`/`dirty_consumers` over
  salsa inputs/tracked fns; call sites unchanged.
- **Discussion needed:** when the query graph deepens (materialized
  workspace enrichment, phase 4 SQL views), revisit whether the dirty-set
  still suffices or Salsa's cancellation/durability earns its costs.

## Unregister inverse under symbol eviction: recorded name list vs self-healing candidates — 2026-07-06 — RATIFIED (veesh, 2026-07-07)
- **Context:** symbols-relational phase B. `unregister_file` walked
  `old.analysis.symbols` to remove name registrations; under symbol
  eviction that vec is empty, and rehydrating after an edit persists
  fetches the NEW generation's names (wrong inverse).
- **Options:** A — record the registered (name, is-class) pairs per path
  at registration time (a side map, ~pointers + names). B — make
  `all_defs`/`cache` self-healing at read (validate candidates against
  `all_files` membership/Arc identity; registration appends only). C —
  read the syms rows for the OLD generation before shredding the new one
  (couples unregister ordering to persistence).
- **Picked:** A (`ModuleIndex.registered_names`). Exact inverse by
  construction, no read-path cost, and it doubles as the class-rank
  source for the cache-slot tie-break (which also read evicted symbols).
  Cost: one Vec<(String, bool)> per registered file — ~tens of MB at
  chromium scale, bounded and measured into the floor.
- **Undo cost:** small — the map is private to `module_index.rs`; B can
  replace it without touching call sites outside registration/unregister.
- **Discussion needed:** if the floor budget tightens further, B removes
  the per-file name copies entirely at the price of lazy consistency
  (stale candidates until next read).

---

## include_closure representation: interned path pointers vs file-id arrays — 2026-07-06 — RATIFIED (veesh, 2026-07-07)
- **Context:** chromium warm heap dump post-refs/symbols eviction:
  `include_closure` is the largest resident bucket — 2,827 MB / 41% at
  132K files (16-byte `Arc<str>` per closure entry × deep header
  closures). It is read on hot paths (the refs_to visibility gate, per
  candidate) so relocation to rows would thrash; this is a REPRESENTATION
  problem.
- **Options:** A — global path table + per-file sorted `Arc<[u32]>`
  (4 bytes/entry, ~4× smaller; gate becomes a binary search over ids).
  B — dedupe identical closure SETS across files (headers within one
  subtree share suffixes; measure hit rate first). C — roaring bitmaps
  over the global file table (best compression, new dep).
- **Picked:** A, landed same day (`path_intern::ClosureList` — sorted
  `Arc<[u32]>` over a process-global path-id table; serde keeps the
  `Vec<String>` blob shape so no EXTRACT_VERSION bump). Membership became
  id binary-search (`contains`), set/save consumers go through
  `iter_strs`. One subtlety worth remembering: `closure_stamp` had to
  SORT before hashing — id order is global mint order, nondeterministic
  across sessions, and an order-sensitive hash would have silently
  invalidated every warm row every run. Measured on abseil: closure
  bucket 10.8 → 1.8 MB (residual is `include_directives` strings), whole
  payload 20.2 → 11.2 MB; table 1,123 paths / 0.1 MB. Chromium
  projection: 2,827 MB → roughly 300–500 MB + a ~150 MB one-time table.
- **Undo cost:** small — representation is private to `ClosureList`;
  swapping to dedup'd sets (B) or bitmaps (C) touches only the type.
- **Discussion needed:** whether `include_directives` (now the residual)
  should ride the same table; and whether B (closure-set dedup) stacks
  worthwhile savings on top at chromium depth.

## Implicit-`this` capability: one flag for fields AND calls — 2026-07-05 — RATIFIED + RENAME LANDED (veesh, 2026-07-05)
- **Context:** hitlist-3 Family A+I slice. The implicit-member pass is
  gated by the pack's `implicit_this_members` capability. The sibling-CALL
  half (a bare `foo()` inside a method body meaning `this->foo()`) needed a
  gate too — same fork the task flagged: reuse the flag, or add a sibling
  one.
- **Options:** A — reuse the one capability for both halves. B — add a
  parallel `implicit_method_calls` capability.
- **Picked:** A. "Can a bare name resolve through an implicit `this->`" is a
  SINGLE language fact — C/C++ elide the receiver for both members and
  methods; Python/R make it mandatory for both. There is no language where
  fields elide but methods don't (or vice-versa), so a second flag would be
  a distinction with no possible producer. The flag's NAME is now
  field-specific and slightly under-describes its scope; the rename to
  `implicit_this_members` (member-scoped, covers fields AND sibling calls)
  landed in the round-close sweep.
- **Undo cost:** trivial — split into two bools and thread the second
  through `emit_return_fuel`; the sibling-call pass already stands alone as
  its own block, so it just reads a different flag.
- **Ratification (veesh):** one flag for both, confirmed; the rename to a
  member-scoped name landed (`implicit_this_members`).

## Sibling-call vs. same-named free function ranking — 2026-07-05 — RESOLVED (Family Q, 2026-07-05)
- **Context:** same slice. When a method body calls `foo()` and BOTH a
  sibling method `Class::foo` and a free `foo()` exist, C++ name lookup says
  the member hides the free function. The model tier correctly MINTS the
  sibling link (pins the call's `resolved_package` to the class, so
  `find_definition` lands on the member). But goto-def's set projection runs
  through `overload_arity_definitions` in `resolve.rs`, whose `pkg_agrees`
  admits a package-less (free) function into a class-scoped overload family
  (`_ => true`), so the free decl still surfaces — and its earlier source
  row sorts it FIRST.
- **Options:** A — leave it: the sibling link is present, the ranking
  residual is a resolve.rs overload-family concern. B — teach
  `overload_arity` that a member call (pinned `resolved_package`, class
  origin) excludes package-less free functions from the family.
- **Picked:** B, landed by the Family Q slice that owns `resolve.rs`. This
  ranking residual is the exact symptom-2 of Family Q (owner/qualifier-blind
  forward resolution): `overload_arity_definitions` now ranks candidates by
  **owner match** first — a candidate whose package genuinely agrees with the
  call's anchored owner (both sides carry a package, tails agree) sorts above
  one admitted only by `pkg_agrees`' recall bias (a package-`None` free
  function). The family is never pruned: the free decl stays in the set,
  ranked below the sibling member. The pinned `resolved_package` = the class
  origin makes the sibling member owner-matched, so it wins. General rule (no
  member-vs-free branch): the owner match IS the key, shared with the
  `dynamic::STRING` / `logger.info` / `level::info` cases.
- **Outcome:** `cpp-sibling-call-shadows-free` promoted PROVISIONAL → gold,
  now asserting the sibling member ranks FIRST (`none: ["\nsibling_call.cpp:25:10"]`).

## Hover presentation payload — 2026-07-03 — RATIFIED (veesh, 2026-07-03)
- **Context:** hitlist-2 slice D (#14): hover became a CandidateSet
  projection (`hover_candidate()` = the top-ranked `definitions()`
  candidate; `symbols::pack_hover_markdown` presents it).
- **Options:** A — the projection returns a bare `RefLocation`; the adapter
  materializes (file → analysis → symbol at span) and renders (member
  drill-downs stay a cursor-side adapter lane over the same invocant
  resolution). B — candidates carry a presentation payload (symbol
  identity, kind, member facts) minted inside `definitions()`, so the
  adapter never re-looks-up.
- **Picked:** A. No widening of `RefLocation` for one consumer, zero new
  serialized shapes; identity/ranking stays single-sourced (the invariant
  that matters) while presentation lookups read through the same scoped
  index the set resolved with. The member drill-down (domain headline /
  storage leaf / template substitution) keeps its landed adapter-side home.
- **Undo cost:** small — introduce a `HoverCandidate` payload struct and
  move the adapter's symbol-at-span lookup into `definitions()`'s lanes;
  the adapter shrinks to a pure renderer.
- **Discussion needed:** if a second presentation consumer appears (e.g.
  CLI gd wanting signatures beside locations), promote to B then.

## Function-lane def_paths minted at the set, not identity minting — 2026-07-03 — RATIFIED (veesh, 2026-07-03)
- **Context:** slice D3 re-activated the def-candidates visibility gate for
  plain function (Sub) targets. Every other def_paths mint sits in
  `resolve_symbol_scoped` behind a structural pack-only fact (macro_defs,
  sigil-less class content); a Sub cursor is language-neutral (a Perl `sub`
  mints the same `RenameKind::Function`), so minting there would gate Perl
  subs — whose visibility is package-keyed, never closure-keyed — off their
  own workspaces (Perl closures are empty).
- **Options:** A — mint in `CandidateSet::resolution()` under the
  caller-declared `pack_routed()` fact. B — add a language/pack tag to
  `FileAnalysis` and gate inside `from_rename_kind`.
- **Picked:** A. The ADR already blesses pack routing as a set-level axis
  with set-owned consequences (VISIBLE widening, rename full-or-refuse);
  the visibility gate is precisely such a consequence, and A adds no
  persisted field.
- **Undo cost:** trivial — move ~10 lines if a `FileAnalysis` language tag
  ever lands for other reasons.
- **Discussion needed:** none urgent; fold into B if/when a language tag
  exists.

## `Slot::ModulePath`'s `in_use` field — 2026-07-05 — RATIFIED + GENERALIZED (veesh, 2026-07-05)
- **Context:** cursor Slot taxonomy (`docs/adr/cursor-slots.md`), migrating
  completion's context match onto `Slot`. The ADR sketches `ModulePath {
  prefix: String }` covering BOTH `use |` (typing the module name —
  `complete_module_names`, loadable-module labels) and `Foo::|` (an
  in-file qualified-path drill — `qualified_path_completions`, sub +
  sub-package labels). The two behaviors are genuinely different renders
  over the same CandidateSet (full module name vs. bare suffix), and
  `prefix`'s text alone can't distinguish them — `Mojo::Ut` is a valid
  partial spelling under either. Folding them into one slot with only
  `prefix` and picking ONE render would silently change completion output
  for whichever case lost, breaking the migration's byte-identical
  requirement.
- **Options:** A — add `in_use: bool` to `ModulePath`, set at detection
  time from which `CursorContext` arm fired (`UseStatement` vs
  `QualifiedPath`); the consumer's `if in_use {..} else {..}` exactly
  reconstructs today's two code paths. B — split into two Slot variants
  (`ModulePath` for the drill, a new `UseModule` for the bare `use` case),
  matching the ADR's 7-variant count more loosely.
- **Picked:** A. Keeps the ADR's closed 7-variant vocabulary intact (the
  field is additive, not a new variant), stays a straight decode of which
  detector fired — no shape re-derivation from tree/text — and the two
  render functions (`complete_module_names` / `qualified_path_completions`)
  are untouched, called exactly as before.
- **Undo cost:** trivial — drop the field and hardcode one render, or
  promote to option B (split variant) if a future consumer wants to
  match on it structurally instead of a bool.
- **Discussion needed:** none urgent; the field is documented at its
  definition (`src/cursor_slot.rs`) and locked by
  `cursor_slot_tests.rs::detect_slot_perl_use_module_name_is_module_path`.
  (The two renders now live set-side as
  `CandidateSet::complete_module_candidates` / `complete_qualified_path`.)
- **Ratification (veesh):** keep, but GENERALIZE — done in the round-close
  sweep. `in_use` is gone; every detected slot now carries a
  `DetectorArm` (`cursor_slot.rs`) — the generic "which detector fired"
  fact. `detect_slot`/`detect_call_slot` return `DetectedSlot { slot, arm }`;
  the `ModulePath` consumer asks `arm == UseModule` (module-name render) vs
  `QualifiedPath` (drill), no per-variant bool. Any future folded-slot
  consumer reads the same arm.

## Ref-type deref snippets — candidate data vs projection policy — 2026-07-05 — RATIFIED (veesh, 2026-07-05)
- **Context:** the entity-content candidate-level migration (PARKED
  "Entity-content completion sources"). Every entity-content source now
  yields `CompletionCandidate` through one adapter projection
  (`candidate_to_completion_item`). Two Member-slot extras don't fit the
  candidate mould: the pack `.`→`->` operator-swap edit (`op_fix`) and the
  Perl ref-type deref snippets (`[index]` / `{key}` / `(args)` offered when
  the `->` receiver is an ArrayRef/HashRef/CodeRef).
- **Options:** A — keep them as they are: `op_fix` rides the existing
  `CompletionCandidate.additional_edits` (candidate DATA — the receiver's
  pointer depth is a fact about the member candidate), and the ref snippets
  stay adapter-appended `CompletionItem`s (projection POLICY — they are
  syntactic templates for a ref receiver, not members of any entity, and
  need `InsertTextFormat::SNIPPET` which the candidate vocabulary doesn't
  model). B — add a snippet-format field to `CompletionCandidate` so the
  ref snippets become candidates too, folding the last Member extra into
  the vocabulary.
- **Picked:** A. `op_fix` was already candidate data and stays there. The
  ref snippets are a fixed 1-item-per-ref-kind template with no gathering
  to unify — making them candidates would add a SNIPPET-only field to the
  struct that every other candidate carries as `None`, bloat for zero
  dedup/provenance benefit. They're the same shape as the import-list
  "still indexing" placeholder: a slot affordance, not a resolved entity,
  so the adapter builds them directly.
- **Undo cost:** trivial — add the snippet field + move
  `ref_type_snippet_completions` into a gatherer if a second snippet source
  ever appears; today there's exactly one.
- **Discussion needed:** none urgent; revisit only if type-constrained
  completion wants snippet candidates from a shared source.
- **Ratification (veesh):** punt is fine — snippets likely push into the
  candidate vocabulary eventually, but blast radius is low; the standing
  condition is that layering rules hold (snippets stay adapter-side, no
  analysis decisions leak into the projection).

## Type-constrained completion — carried expected type vs new Slot variant — 2026-07-05 — RATIFIED (veesh, 2026-07-05)
- **Context:** the type-constrained-completion slice needed a slot for the
  pack domain comparison (`o->op_type == |` → the field's enum DOMAIN). The
  `Slot::expected_type` seam already existed; the question was how the pack
  detector hands its EAGERLY-resolved domain type to that seam.
- **Options:** A — reuse `ArgPosition`, adding an `expected: Option<InferredType>`
  field the detector fills when it already knows the type (Perl call-arg
  slots leave it `None` and resolve the callee's param lazily). B — mint a
  new `Slot::Comparison { expected }` variant.
- **Picked:** A. The ADR already grouped `x == |` under `ArgPosition`
  ("wants sig-help AND type-constrained candidates. Carries the slot's
  EXPECTED TYPE when derivable"), so the field is the shape the doc
  reserved; a comparison and a call-arg answer the same `expected_type`
  question with the same consumer. A new variant would fork the vocabulary
  for one producer with no distinct consumer. `Slot` is ephemeral
  (no serde), so no EXTRACT_VERSION cost either way.
- **Undo cost:** trivial — the field defaults conceptually to `None`; drop
  it and re-inline if a comparison ever needs consumer behavior a call-arg
  doesn't share.
- **Also parked here:** switch-`case |:` domain completion (the ADR's
  "if cheap" half) — SKIPPED. It needs a distinct probe (climb to the
  `switch_statement`, resolve the CONDITION field's domain) rather than the
  `==`/`!=` binary the landed probe reads; not cheap enough to fold in now.
- **Perl ranking tier:** the ArgPosition consumer boosts type-matching
  scope vars by keeping them at `PRIORITY_LOCAL` and nudging the non-matching
  locals they lead to `PRIORITY_LOCAL + 1` (0 is the priority floor, so a
  sub-LOCAL tier isn't expressible; demoting the complement is the minimal
  sort_text-visible reorder). Revisit if a second sub-LOCAL ranking axis
  appears and the two need a shared ordering.
- **Ratification (veesh):** leave as is — "ArgPosition is a drop of a
  lie, but Slot::Comparison would be sprawl." A friendly comment on the
  variant acknowledges the stretch (landed in `src/cursor_slot.rs`).

## Warm stubs — separate table vs. blob column — 2026-07-06 — RATIFIED (veesh, 2026-07-07)

- **Context:** register-from-Surface warm start persists a per-file stub
  (registration feed + specialization edges + projected Surface + the
  stripped skeleton) so warm scans never decode full analysis blobs.
  Where does the stub live?
- **Options:** A — `ALTER TABLE modules ADD COLUMN stub BLOB` (the
  `deps_stamp` precedent). B — a separate `stubs(path PRIMARY KEY, stub)`
  table joined at warm.
- **Picked:** B. SQLite reads a record left-to-right; a column appended
  AFTER the `analysis` BLOB sits past its overflow-page chain, so every
  stub read would drag the full blob off disk — the exact cost the lane
  exists to skip. The separate table also gets its own `stub_version`
  meta wipe without touching blob validity.
- **Undo cost:** low — the stub is a pure cache derived from the blob in
  the same txn; dropping the table (or the version gate wiping it)
  degrades to the full-decode lane and backfills on the next warm.
- **Tradeoffs accepted:** (a) every modules-row rewrite must DELETE the
  path's stub or a stale skeleton pairs with a fresh stamp — enforced
  inside `save_to_db`/`save_blob_to_db_stamped` so writers can't forget;
  (b) `pack_file_changed` deletes but doesn't rewrite stubs, so edited
  files take the full lane on the next warm (bounded: only edited files);
  (c) NO_EVICT bypasses stubs entirely (skeletons are stripped by
  construction — a whole-copy session can't register them);
  (d) stub-lane files whose derived rows vanished (REF_ROWS_VERSION
  wipe) decline to the full lane because re-shredding needs the whole
  analysis.
- **Discussion needed:** whether the Perl workspace tier should get the
  same lane (its warm is milliseconds on bugzilla today; chromium-scale
  Perl trees don't exist in the corpus set).

## Mission-2 hardening round — deferred findings — 2026-07-06 — OPEN (Claude)

Fixed in the round (for the record): free callables were absent from the
Surface (a C free-function signature change read as Unchanged — the
firewall's worst failure mode); the dirty walk missed consumers of
RENAMED-away packages (`stale_provided` now seeds one walk); the deleted
pack file's own reparse caches leaked (deletion was semantically
invisible to re-analyzed consumers); open-doc surfaces were recorded
POST-enrichment (verdict flapping against pre-enrichment indexer
records); the Unchanged gate skipped consumer deps-stamp refresh (the
restart cold storm); watcher re-registration dropped the verdict (stale
open consumers after git pull); stub-lane rows with an unrehydratable
blob (NULL/empty) no longer register; deferred stub backfill is
stamp-guarded against racing edits; `remove_surface` parent-canonicalizes
deleted paths; method dedup collapses only FULLY-equal duplicates.

Deferred, in rough priority order:

- **Concurrent surface writers (buffer vs disk)**: a bulk index or watcher
  tick re-records a DISK build over an open doc's BUFFER record; an edit
  reverting the buffer to the disk state then reads Unchanged against the
  wrong baseline and skips a consumer refresh. Needs record provenance
  (open-doc records outrank background ones while the doc is open) or a
  doc-open guard on background recording. Rare (requires an unsaved
  contract change raced by a background re-record), silent when hit.
- **Verdict-policy seam**: the record→gate→act sequence is spelled in
  three places now (open-doc edits, the watcher, `pack_file_changed`).
  A `FreshnessIndex`-owned policy hook (or a `record_and_dirty` that
  returns the closure) would make the next registration path inherit the
  gate by construction instead of by remembering.
- **Probe serialization in `pack_file_changed`** (Changed case): the
  changed file's probe runs serially before the parallel consumer fan-out
  (~one header-analysis of added latency per save while actively editing
  a widely-included header whose surface DID change). Speculative
  consumer re-analysis concurrent with the probe would restore the old
  wall clock at the cost of wasted work on Unchanged — measure before
  building.
- **`warm_pack_stream_with_stubs` two-closure API**: the shared-state
  RefCell dance at the call site wants a single
  `FnMut(path, WarmPayload) -> Directive` shape.
- **Pack provided-names vocabulary**: `SurfaceRecord.provided` is
  packages-only; a future pack-tier NAME-keyed dirty walk (cpp uses
  include-closure consumers today, so nothing reads it) would
  under-invalidate for free-function headers. If that walk ever lands,
  feed `provided` from the linkage feed, not `packages`.
- **Declined micro-optimizations**: per-registration `fs::canonicalize`
  in `record_surface_value` stays (correctness guard; ~µs against per-file
  analysis costs); cold-path stub encoding stays (measured cold wall
  unchanged, and it buys the FIRST warm, not the second).

## Session review round — duplication + structural residency — 2026-07-07 — TRIAGED (veesh, 2026-07-07)

Landed: `evict_axes` / `prepare_pack_parts` / `prepare_workspace_parts` are
the only spellers of the reads-whole-before-evict strip; the warm scans
share `classify_row_generation` + `write_in_chunks` + a single-callback
`WarmPayload` API (RefCell dance gone; dead rows rejected pre-decode);
`register_symbols` feeds through `prepare_pack_feed`; the watcher and
editor paths share `republish_open_docs_in`; the writers' PANIC arms now
drop stale LRU pins like their commit-fail arms (live bug, both tiers);
the enrichment overlay is byte-capped (128 MiB + 64 entries, per-entry
`heap_estimate` stored); `whole_copy_registration_sites_are_allowlisted`
(layering_tests) makes every whole-copy registration call site declare its
residency bound; a post-bulk-index residency tripwire counts
fully-resident pack copies against the deliberate whole-copy sites and
`log::error` + debug-asserts on unexplained pins.

Deferred, with designs:

- **@INC/'import' tier is never stripped** — the largest remaining
  unbounded residency (a Moose/Mojo/DBIC dep closure is 1500+ whole
  modules for the session). The lanes exist (blobs persisted, hub bag
  LRU, `whole_present`, DEP rows shredded); the work is routing
  `insert_into_cache` through a registration-owned strip after
  `save_module_generation` commits, then a gold cold/warm round — the
  import tier feeds enrichment and inheritance walks, so this needs its
  own verification pass.
- **Watcher re-registration never re-strips** — whole copies pinned until
  restart; a big `git pull` is an unbounded resident delta. Design:
  persist (blob+rows) in the watcher's blocking task, then
  `register_workspace_stripping` on commit, whole-copy fallback only on
  persist failure.
- **Writer fallback budget** — a persistently failing writer (disk full)
  falls back to whole copies for the ENTIRE tree; the tripwire now makes
  it visible but nothing bounds it. Design: byte-accounted fallback
  budget shared per index run.
- **Rows-missing re-strip after backfill** — after a REF_ROWS_VERSION
  bump, refs+symbols stay resident for one session (self-healing at next
  restart; never trips the fully-resident wire). Re-registering post-
  backfill needs a surface-PRESERVING residency-only lane (re-projecting
  from a bag-evicted copy would corrupt the freshness record) — build it
  on `register_workspace_residency`/`register_symbols_inner` with the
  original parts, not the stripped copy.
- **Writer-thread harness dedup** — WsFresh and FreshEntry writers share
  the whole chunk/txn/fallback scaffold shape; a generic harness would
  make fixes land in both by construction. Moderate refactor; the panic
  fix above is the drift it would have prevented.
- **Stamp-capture helper** — the stamp-before-read + re-stat-after-parse
  protocol is spelled in both fresh workers.
- **Parts-token-only inner registration** — make `register_symbols_inner`
  / `register_workspace_residency` accept only the parts structs (private
  fields, constructible solely via the prepare_* choke points) so a
  feed-from-stripped-copy or whole-arc hookup fails to compile. The
  allowlist test covers the gap until then.


## Triage (veesh, 2026-07-07)

- The four architecture picks above: **ratified as-is**, revisit triggers
  stand as written.
- Next arcs, in order: **R4 server consumers** (always-enriched closed
  files through the overlay), then **@INC tier stripping** — both full
  auto with hardening rounds.
- Backlog now: **writer-harness + stamp-capture dedup**. Staying
  laddered: parts-token inner APIs (allowlist test covers the gap),
  pack provided-names vocabulary (fix when a name-keyed pack dirty walk
  lands), writer fallback budget (tripwire keeps it visible), watcher
  persist+re-strip, buffer-vs-disk record provenance, probe
  serialization (measure first), phase-4 SQL views. Declined micro-opts
  stay declined.
