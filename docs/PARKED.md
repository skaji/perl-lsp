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
- **Open forks awaiting ratification** (`docs/open-forks.md`):
  `Slot::ModulePath.in_use` field; ref-type deref snippets as projection
  policy vs candidate data. Both cheap to undo.

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
- **Use-after-move re-wire** — needs path-sensitivity (function + test
  kept, unwired).
- **PR #100** re-extraction onto `projection.rs` (user closes or reworks).
- **PR #105** heatmap-viz refresh (pre-rebased on local `tmp-viz-trial`).
- **Per-toolchain global system-header cache**; the cross-language
  "system root" generalization (perl=@INC, python=probe).
- **Instance brands** (per-object dispatch scoping) — downstream of the
  long-distance value-provenance tier (`prompt-type-inference-residual.md`).

## Residual-bug tier (pinned, xfail'd where reducible)

- **Ctor-convention heuristic misfires on uppercase macro/function-style
  calls** (`RCPVx(pv)` mints `ClassName("RCPVx")` as a flow witness —
  root seed of the hitlist-3 #1 bug). The annotation-priority fix shields
  ANNOTATED receivers; an `auto`/annotation-less local initialized from
  an uppercase call still mistypes. Wants the heuristic gated on "callee
  resolves to a known type/ctor", not name case alone. (The round-3
  braced-init fix generalized the annotation-dominates axis to every
  `InferredType` flavor, not just `ClassName` — but `ClassName` was already
  shielded, so this residual's exposure is UNCHANGED: the auto-less case has
  no annotation witness to dominate the bogus flow class.)
- **C struct-field member resolution through a call-expression receiver**
  (`mkStruct()->field` where the callee's declared return is a struct
  pointer) — dark; distinct from the landed cpp method chain roots
  (methodchain works, C Field refs don't). Noted by the depth-B agent as
  pre-existing; lives in the `expr_type_at_span` path. Unassigned.
- **json.hpp attribution stops mid-class** at a `#if`-conditional
  ctor-initializer — the config-superposition tier (a `#if` inside a
  class body forks the member list; wants the superposition model applied
  to declarations).
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
- **M6/L3 session determinism** (cold-open None→warm flip; debounce
  staleness) — KNOWN-GAPS "LSP session determinism".
- **Enum value as template argument** not a ref (`MakeError<StatusCode::
  kNotFound>`) — hitlist-2 residual, unassigned.
- **Refs inside another macro's `#define` body aren't indexed** — a use of
  `FLAGS` inside `IS_OK`'s body, redis `OBJ_ENCODING_EMBSTR` in
  `sdsEncodedObject`, perl5 `SvFLAGS` (190/347 grep-real) / `SvANY`
  (111/200). Macro definition bodies are preproc-excluded from ref minting;
  gd THROUGH the same nested sites works, so this is index-population only.
  Pinned `cpp-macro-nested-ref-in-macro-body` (xfail); unassigned.
- **`fmt::` qualified-path completion** unfiltered (the completion half
  of namespace participation — gd/gr half landed in slice B).

## Cross-references
- Gap shapes behind open xfails: `gold-corpus/KNOWN-GAPS.md`
- Fix-run narrative: `docs/hitlist-2.md`, `docs/session-2026-07-03-summary.md`
- Architectural forks: `docs/open-forks.md`
