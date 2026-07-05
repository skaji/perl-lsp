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
- **json.hpp attribution stops mid-class** at a `#if`-conditional
  ctor-initializer — DECIDED arc: see
  `docs/adr/config-superposition-declarations.md` (re-anchor invariant +
  declaration-scoped variants; spike gate first). Blast radius detail in
  hitlist-3. BASEOP config-twin darkness (op.c) is the same tier.
  - the design is settled to solve this guy
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
- **M6/L3 session determinism — cold-open degraded window** (residual; the
  DEADLOCK half is FIXED — see below). The on-open analyze is cached-only and
  the pack index attaches after the lazy background walk, so a query in that
  window can see a degraded answer (pack completion falls back to the Perl hub
  → `@INC` flood; cross-file gd/hover `None`) with no client re-request signal
  for the pull verbs (completion self-heals via `isIncomplete`). Normally the
  window closes in <500ms; under heavy load (a cold cache + the Perl cpanfile
  resolver storm competing for CPU) it stretches past the e2e's 500ms settle
  and a fast burst of queries can race it. Wants a completion signal on BOTH
  `spawn_pack_gather_refresh` AND `ensure_workspace_indexed` (its latch marks
  KICKOFF, not completion) plus a bounded wait in the pull handlers — deliberate
  design gap. A cheap partial unblock: coalesce the `on_refresh`
  diagnostics-refresh callback (it fires once PER resolved module — ~45× in a
  400ms burst on a mixed repo, each a full `for_each_open_mut` + publish),
  shrinking both the CPU pressure and the stdout flood that widen the window.
  **The deadlock that used to MASK this window is fixed** (`Document::analysis`
  is now `Arc`; handlers snapshot + drop the `get_open` read guard before
  `resolve()` re-locks the open shards — the reentrant-read-behind-a-queued-
  `for_each_open_mut`-writer deadlock). Repro lock: `e2e/cold-start-repro.sh`
  (pre-fix ~7.5% cold-run failure, post-fix 0). Also: debounce-window staleness
  (mid-typing `doc.analysis` describes prior text). KNOWN-GAPS "LSP session
  determinism".
- **Enum value as template argument** not a ref (`MakeError<StatusCode::
  kNotFound>`) — hitlist-2 residual, unassigned.
  - looks easy enough to close
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
- **`fmt::` qualified-path completion** unfiltered (the completion half
  of namespace participation — gd/gr half landed in slice B).
  - this should be closed

## Cross-references
- Gap shapes behind open xfails: `gold-corpus/KNOWN-GAPS.md`
- Fix-run narrative: `docs/hitlist-2.md`, `docs/session-2026-07-03-summary.md`
- Architectural forks: `docs/open-forks.md`
