# PARKED — the single deferred-work ledger

THE one place. Session summaries and hitlists may narrate; this file is
the source of truth for what's deliberately not done. Each entry: what,
why parked, what unblocks it. Prune on landing.

## Design-debt tier (candidates for a tightening round)

- **Two include-BFS walkers + two `file_stamp` fns** (cpp_reparse vs
  module_cache): twice examined, twice left (different contracts/layers);
  verdicts recorded so sweeps don't re-litigate. Merge only with a reason.
- **Two C-comment strippers in cpp_reparse** (`strip_c_comments` vs
  `blank_comments_in_range`): distinct contracts — the former COLLAPSES
  whitespace to produce clean body text, the latter is length-preserving
  (spaces over comment bytes, newlines kept) so byte offsets stay in
  original coordinates for member positioning. Not a merge target.
- **Two "enclosing class" notions in `emit_return_fuel`**: the implicit-
  field half reads the ref's own `scope.package`; the sibling-CALL half
  walks up to the enclosing method SYMBOL's package (so out-of-line bodies,
  whose body scope carries no package, still resolve). Deliberately
  different robustness; unifying is a behavior change, not a cleanup.
- **Two domain/type completion rankers** (`backend::rank_domain_members`
  for pack enum members vs `symbols::rank_candidates_by_expected_type` for
  Perl scope vars): different item types (`CompletionItem` vs
  `CompletionCandidate`) and semantics (enum members verbatim, front-loaded
  vs. type-matching locals kept at `PRIORITY_LOCAL`). No shared gatherer to
  factor.
- **Fork-ratification decruft items (veesh, 2026-07-05)** — all forks in
  `docs/open-forks.md` are now ratified/resolved; two carry queued
  cleanups: (a) rename `implicit_field_reads` to a member-scoped name (it
  gates sibling CALLS too); (b) generalize `Slot::ModulePath.in_use` into
  a generic "which detector arm fired" fact carried by every Slot, not a
  ModulePath-only bool. Both are pure renames/reshapes, next sweep.

## Feature tier (each is a fireable slice)

- **Perl domain typing** — needs a constant-group / Type::Tiny enum-domain
  model (`docs/adr/field-projections.md`).
- **Type-constrained completion** — the cpp domain slot (`op_type == |` →
  `OP_*` ranked first) and the Perl ArgPosition consumer LANDED on the
  `Slot::expected_type` seam (`docs/adr/cursor-slots.md`). Residual: the
  switch-`case |:` position (needs the switch-condition climb) and the
  Perl-side domain source, which still wants the constant-group /
  Type::Tiny enum-domain model above.
- **Template rungs**: dependent types (`T::value_type`), value-arg
  deduction, template-template params.
- **Flag-set domains** (`op_flags`/`OPf_*` — subset-of vs one-of).
- **Use-after-move** — the DECIDABLE subset is WIRED (opt-in
  `initializationOptions.diagnostics.useAfterMove` / CLI `--use-after-move`,
  off by default; `docs/adr/use-after-move.md`). Flags only a straight-line,
  in-function, LOCAL moved-then-used, behind three honesty gates
  (`use_after_move_reads`): B (in a function body — kills member-init /
  delegating-ctor floods), C (straight-line — no conditional/loop/switch/
  ternary/preproc between move and read), E (locals only — a moved parameter
  is a forwarding/subobject idiom). Verified 0 FP over the spdlog/fmt/onednn
  headers (was ~17 with the naive check). STILL PARKED, needs true
  path-sensitivity + subobject/interprocedural analysis: a use in a different
  branch arm, a loop-carried move, a `x.member` sibling-read after a
  base-subobject move (`operator=`/move-ctor), and a by-mutable-ref reset
  (`reset(x); x.use()`). Those stay silent by design — the gates trade recall
  for zero false positives.
- **PR #100** re-extraction onto `projection.rs` (user closes or reworks).
  - i think this will just be closed; anyways it didn't look like it did the intended PPP,
    which is to have mojo helpers which mint dynamic helpers show their definitions;
    literally no reason to punt on a conrete impl. this branch is leaning towards prod, so
    no reason to duplicate there
- **PR #105** heatmap-viz refresh (pre-rebased on local `tmp-viz-trial`).
  - yalla, let's clean that up; the core of the PR is just the html
- **Per-toolchain global system-header cache**; the cross-language
  "system root" generalization (perl=@INC, python=probe).
- **Instance brands** (per-object dispatch scoping) — downstream of the
  long-distance value-provenance tier (`prompt-type-inference-residual.md`).

## Residual-bug tier (pinned, xfail'd where reducible)

- **C free-function return type doesn't propagate cross-file** (the honest
  residual of the call-receiver-field kill). Single-file `mkStruct()->field`
  now resolves — the gap was purely that the pointer-returning and prototype
  (`declaration`) skeleton patterns omitted `@rettype`, so the callee carried
  no return-type witness for `expr_type_at_span`'s member-chain arm to type
  the receiver through; adding rettype-bearing sibling patterns (dedup keeps
  them via `upgrade_ret`) closed it. What REMAINS: `makeGadget()->field` where
  the callee prototype lives in an INCLUDED header. The callee *symbol*
  resolves cross-file, but `query_sub_return_type`'s cross-file arm walks
  `find_exporters`, which filters to Perl `export`/`export_ok` lists that C
  free functions never populate — so the return type never crosses the file
  boundary. Needs pack-language cross-file return typing keyed off the
  resolved call target (not the Perl export model), without over-linking
  same-named free functions. Xfail row `cpp-call-receiver-field-crossfile-call`.
- **Cross-file functional-cast / constructor typing** (callee is NOT a
  local symbol). The name-case ctor heuristic is DEAD: a call's value is now
  the callee's own resolution (`query_extract::into_file_analysis` call-site
  loop → `Expr(call) → Edge(Symbol(callee))`; a `Class` symbol answers
  `ClassName`, a callable its return, an unresolvable name NOTHING —
  `docs/adr/macro-handling.md`). This fixed the `RCPVx(pv)` misfire outright
  (an unresolvable uppercase call leaves an `auto` local honestly untyped;
  gold `ctxparam` + unit `ctor_convention_unresolvable_uppercase_call_no_phantom_class`).
  The residual: a call whose callee resolves ONLY cross-file (Python
  `g = Greeter()` where `Greeter` is a class in another module, or a C++
  functional cast to a header-defined class) types nothing, because the
  callee isn't a local symbol and cross-file classes aren't registered under
  their own name in the module index (Python `Greeter` is registered under
  module `a`). Unblock: index pack classes by name so `get_cached(callee)`
  finds them, then a no-terminal-invent cross-file call-value edge resolves
  at query time (idx present). Xfail-adjacent: unit
  `python_cross_file_method_dispatch_through_mro_walk` now asserts `g` is
  honestly `None` locally (its real subject — cross-file MRO dispatch keyed
  on the class name — is unaffected).
- **json.hpp `basic_json` attribution blast radius** (~4400 lines lose
  membership) — **re-anchor invariant, still open.** Slice 1's
  declaration-position directive repair landed
  (`strip_declaration_position_directives`) and fixes the ISOLATED
  ctor-initializer `#if` (no phantom `start_position`/`end_position`
  members; late members still attribute — gold rows `cpp-ctorif-decl-
  directive-*`). But that is NOT what corrupts json.hpp: **the ctor `#if`
  in isolation causes only LOCAL damage** — a reduced class with the same
  shape still parses and attributes correctly. The real blast radius is
  **deep-error-propagation**: a failure ~4400 lines into `basic_json`
  poisons the whole class node (the class never becomes a
  `class_specifier` — it degrades to ERROR + `function_definition` +
  `compound_statement` soup, so there is no class scope to attribute
  to). The 80-line header parses fine standalone; the trigger is deep and
  unbisected. Two remaining paths, either would bound it:
  (a) an attribution-layer **re-anchor fallback** — positional/textual
  class tracking so members attribute even when the `class_specifier`
  node is corrupted (bounds blast radius for ANY misparse cause, the
  general fix); (b) a deep-construct repair that keeps `basic_json`
  parsing as a class. json.hpp `basic_json` is before == after
  unattributed; commit 2 did not move it. If slice 2 variant tags land,
  they still would NOT help here — the failure is a parse corruption, not
  a config superposition.
- **Slice 2 (config-superposition variant tags) re-scoped** — the spike
  (`docs/adr/config-superposition-declarations.md`, findings 2026-07-05)
  proved slice 2 is NOT needed for Case B (slice-1 exclusion narrowing
  cured it) and does NOT fix Case A's blast radius (a parse corruption,
  above). Variant tags remain justified only for **genuinely superposed
  DECLARATIONS** — a field/def whose SHAPE differs per config, an
  `#else`-twin function with a different body — where the payoff is
  **labeled multi-arm navigation** (gd unions both arms, macro-def
  precedent) and **arm-fold typing on true twins** (a config arm folded
  as a branch arm through the existing reducers). Not motivated by any
  measured darkness after slice 1.
- **Strip-blanked tokens aren't re-minted as refs** (gr misses blanked
  `NS_BEGIN`-style occurrences; splice-blanked ones ARE re-minted).
- **Per-macro-name salvage granularity** — a macro with both good and bad
  uses is kept/blanked wholesale.
- **One `private:` leak shape** (raw_hash_set.h:3783 — post-declarator
  attribute in a compound misparse the conservative gate doesn't reach).
- **fmt macro-prefixed members** (`FMT_CONSTEXPR auto data()`) don't
  extract — macro-damage lane; why `memory_buffer.data()` is dark.
- **`extern template` spellings** parse as ERROR (tree-sitter-cpp 0.23).
- **proto.h variadic decls** never register a Sub (`Perl_croak` absent
  from completion/gd).
- **Nested-hash-key completion level leak** (Perl, pre-existing — xfail
  `completion-exact-hash-key-slot-no-nested-leak`).
- **Moo rwp writer at decl-token group answer** (prompt-heatmap.md).
- **M6/L3 session determinism — cold-open degraded window** (the DEADLOCK,
  POISONED-PERSIST, and now the HEAL-REPUSH + COALESCE halves are FIXED; only a
  bounded-wait in the pull handlers is LEDGERED — see below). The on-open
  analyze is cached-only and the pack index attaches after the lazy background
  walk, so a query in that window can see a degraded answer (pack completion
  falls back to the Perl hub → `@INC` flood; cross-file gd/hover `None`; refs
  from an open def-site return the def only, e.g. `op_free` count=1 in-window vs
  118 warm). Completion self-heals via `isIncomplete`.
  **The HEAL-REPUSH + COALESCE halves are FIXED** (`fix/degraded-window-heal`).
  Two changes in `backend.rs`:
  - **Completion-signal heal.** `ensure_workspace_indexed`'s latch marked
    KICKOFF, not completion; nothing re-derived an open doc after the index
    landed. Now the end of that background walk calls `Backend::heal_open_docs`,
    which re-analyzes every OPEN doc in the family (pack: full off-lock
    re-analysis via `spawn_pack_doc_refresh`, since the `did_open` gather was
    cached-only and the cross-file index is now warm; perl: enrich + diagnostics
    re-publish) — so the doc-baked degradation self-heals on a server-driven
    event, not on a user re-trigger. `spawn_pack_gather_refresh` already heals
    its own doc on gather completion, so BOTH completion signals now fire a
    heal. Guard discipline held: pack URIs are snapshotted under a read guard
    that drops before any re-analyze; the perl branch enriches under the write
    guard touching only `module_index` and publishes after the guard drops
    (same shape as the resolver `on_refresh`). Verified: `heal_open_docs` logs
    `cold-window heal: index landed for pack family` on op.c open, refs heal
    1→118 (`e2e/cold-window-heal-repro.sh` phase 1).
  - **Coalesced `on_refresh`.** The callback fired once PER resolved module (33
    fires opening a Perl file with 14 `use`s), each a full `for_each_open_mut` +
    publish — CPU + stdout pressure that widens the window. It now bumps a
    `refresh_gen` and debounces 120ms; only the latest fire runs, collapsing the
    burst to ONE execution (measured 33→1, `e2e/cold-window-heal-repro.sh` phase
    2). The final fire always survives the settle, so the fully-resolved state
    is still published.
  **LEDGERED (still open): the bounded wait.** A pull verb (gd/hover/references)
  issued in-window and NOT re-requested still returns the one degraded answer —
  the server has no push channel for pull verbs (only diagnostics + completion
  `isIncomplete` push). A bounded wait in the pull handlers (block a gd/hover
  briefly for an imminent index) would close this, but it risks re-introducing
  the guard-held-across-`resolve()` deadlock family, so it is deliberately NOT
  taken here. The heal-repush shrinks the STICKY surface (doc-baked answers +
  diagnostics now self-heal server-side) and the coalesce narrows the window
  itself; a single in-window pull query under load remains the residual.
  **The deadlock that used to MASK this window is fixed** (`Document::analysis`
  is now `Arc`; handlers snapshot + drop the `get_open` read guard before
  `resolve()` re-locks the open shards — the reentrant-read-behind-a-queued-
  `for_each_open_mut`-writer deadlock). Repro lock: `e2e/cold-start-repro.sh`
  (pre-fix ~7.5% cold-run failure, post-fix 0). Also: debounce-window staleness
  (mid-typing `doc.analysis` describes prior text). KNOWN-GAPS "LSP session
  determinism".
  **The POISONED-PERSIST half is FIXED — the window's damage is non-sticky.**
  The worry was that a degraded cold-run analysis gets frozen into the SQLite
  pack cache behind a `deps_stamp` that self-validates (the stamp is recomputed
  over the STORED closure at load time, so a truncated/empty closure matches
  itself and never re-derives), re-served on every WARM run until `--clear-cache`.
  Two guards close it: `save_to_db` refuses any `degraded` analysis (H8), and
  `PackDriver::register_post_build` now folds closure-INCOMPLETENESS into
  `degraded` — a skipped cached-only gather OR a truncated include closure (a
  header that RESOLVED and exists yet failed to read: non-UTF-8, transient I/O)
  marks the analysis non-persistable, so a complete gather next session
  re-derives it. `cpp_reparse::include_closure` returns `(closure, complete)`
  and only memoizes a complete walk. Verified under heavy CPU load: every
  persisted blob is the correct full-closure analysis, and a warm run heals the
  transient window WITHOUT `--clear-cache` (a genuine poison would fail every
  warm run). Locks: `include_closure_reports_incomplete_on_unreadable_header`
  (unit), `e2e/persist-poison-repro.sh` (cold-load poison → warm heals, no
  clear-cache). What REMAINS of the TRANSIENT window is only the ledgered
  bounded-wait above — a single un-re-requested in-window pull query under load
  still sees the degraded answer for one session; the doc-baked state and
  diagnostics now self-heal server-side, and nothing sticky survives it.
- **Enum value as template argument** — FIXED. The token always had a ref
  (the `@ref.type` catch-all fires in template args; the grammar guesses
  TYPE for value args), so the fix is resolution-side: gd's PackageRef arms
  fall through type space to value space (pack structural gates), and
  `collect_from_analysis` matches `(Method{class}, PackageRef)` under the
  bare-constant hoist gate. gd/gr/rename all reach the site; plain-constant
  (`Buffer<BUF_LIMIT>`) and nested-qualified (`Run<outer::Mode::kSlow>`)
  covered. Pinned in `tmpl_valarg.cpp` gold rows. Honest residual: the
  `StatusCode::` QUALIFIER token is still ref-less (namespace_identifier —
  gd works via the word fallback; gr on the enum type misses qualifier
  positions in ALL positions, not just template args — the namespace-
  participation completion/gr gap below).
- **Nested-macro-body refs — cross-file reach residual** (the core case
  LANDED). A use of macro `A` inside `B`'s `#define` body now mints a ref:
  `macro_body_name_refs` (`cpp_reparse.rs`) lexically scans each opaque
  `preproc_arg` body for identifier tokens naming a KNOWN macro (this file's
  `#define`s ∪ the include closure's) and mints a read at the original span,
  fed into `skel.var_reads` from `enrich_skeleton`. Params + `#`/`##`
  stringify/paste operands + comments/literals are excluded (precision:
  prefer silence over a wrong ref). Gold `cpp-macro-nested-ref-in-macro-body`
  (+`-from-use`) promoted to gold. perl5 gr: `SvFLAGS` 190→320,
  `SvANY` 111→176 (grep-real ~347 / ~200). **Residual:** a body token naming
  a macro defined in a header the file's include closure doesn't reach (perl5
  headers aren't self-contained — `hv.h` uses `SvFLAGS` but may not resolve
  `sv.h`) still goes unminted, hence 320<347. Unblock: a broader TU-level
  macro universe (or a reverse "who-defines" index) so the `known` set spans
  the real translation unit, not just the resolved include graph.
- **`fmt::` qualified-path completion** — CLOSED. A pack `ns::`/`Class::`
  cursor detects as `Slot::ModulePath` via the same `qualifier_at_point`
  goto-def anchors on; `CandidateSet::complete_qualified_path`'s pack lane
  gathers the owner's members (shared `pack_member_of` predicate with
  `member_def_location`, so "offered" = "resolvable"), nested containers,
  and inline-namespace-lifted members ("inline" attribute minted by the
  `@ns.inline` skeleton capture; EXTRACT_VERSION bumped). Empty gather falls
  through to the bare-identifier universe — so real fmt's OWN `fmt::` drill
  (members unattributed behind `FMT_BEGIN_NAMESPACE`) keeps prior behavior
  until the macro-guarded-namespace-open gap closes; `fmt::detail::` filters
  correctly there today. Gold: `cpp-qualified-completion.json` (4 rows).

## Cross-references
- Gap shapes behind open xfails: `gold-corpus/KNOWN-GAPS.md`
- Fix-run narrative: `docs/hitlist-2.md`, `docs/session-2026-07-03-summary.md`
- Architectural forks: `docs/open-forks.md`
