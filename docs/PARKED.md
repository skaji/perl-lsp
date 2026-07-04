# PARKED — the single deferred-work ledger

THE one place. Session summaries and hitlists may narrate; this file is
the source of truth for what's deliberately not done. Each entry: what,
why parked, what unblocks it. Prune on landing.

## Design-debt tier (candidates for a tightening round)

- **Cursor-context split**: Perl has `cursor_context.rs`, pack languages
  have `cursor_sentinel.rs` — two slot-detection systems with no shared
  vocabulary. The CandidateSet ADR's honest boundary leaves slot detection
  outside the seam (correctly), but the two implementations answer the
  same question ("what kind of hole is the cursor in") per-language with
  no common shape. Wants: one slot taxonomy (method-position / member /
  key / import / type-position / …), per-language detectors behind it —
  the LanguagePack pattern applied to cursor context. Slice-E's access
  filter already had to thread through cursor_sentinel awkwardly.
- **Entity-content completion sources** still adapter-side (methods-on-
  class, hash keys, dispatch handlers, `use Foo qw(|)`) — deliberately
  outside the set (they ride MethodOnClass/ReceiverGated), but the pack
  in-scope/member sources note from THE MIGRATION says candidate-level
  pack gathering is the mechanical next step.
- **language_driver post-extraction pipeline** grew organically (remap →
  member-block injection → domain sites → …): wants the builder.rs
  treatment — named phases, ordering documented.
- **Two include-BFS walkers + two `file_stamp` fns** (cpp_reparse vs
  module_cache): twice examined, twice left (different contracts/layers);
  verdicts recorded so sweeps don't re-litigate. Merge only with a reason.
- **Open forks awaiting ratification** (`docs/open-forks.md`): hover
  presentation payload (bare RefLocation vs candidate payload);
  Function-lane def_paths minting location. Both cheap to undo.

## Feature tier (each is a fireable slice)

- **Perl domain typing** — needs a constant-group / Type::Tiny enum-domain
  model (`docs/adr/field-projections.md`).
- **Type-constrained completion** at domain slots (`op_type == |` → `OP_*`)
  — needs cursor-context work (couples to the slot-taxonomy item above).
- **Overload arity ranking** (hitlist-2 #3) — needs extraction-minted
  arg/param counts first; evaluated in slice A, not taken.
- **Template rungs**: dependent types (`T::value_type`), value-arg
  deduction, template-template params.
- **Flag-set domains** (`op_flags`/`OPf_*` — subset-of vs one-of).
- **Parametric macro return** (`#define ID(x) (x)` → arg's type).
- **Use-after-move re-wire** — needs path-sensitivity (function + test
  kept, unwired).
- **PR #100** re-extraction onto `projection.rs` (user closes or reworks).
- **PR #105** heatmap-viz refresh (pre-rebased on local `tmp-viz-trial`).
- **bool → Numeric** — needs `InferredType::Bool` across the ~12-variant
  lattice + reducers.
- **Per-toolchain global system-header cache**; the cross-language
  "system root" generalization (perl=@INC, python=probe).
- **Instance brands** (per-object dispatch scoping) — downstream of the
  long-distance value-provenance tier (`prompt-type-inference-residual.md`).

## Residual-bug tier (pinned, xfail'd where reducible)

- **Implicit-`this->field` reads apply to ALL pack languages**
  (`language_driver.rs::emit_return_fuel`'s second pass): "bare unresolved
  identifier naming an enclosing class's field = member read" is cpp
  semantics — false for Python (bare name never means `self.field`).
  Wants a LangPack capability flag (NOT a language-name branch — rule #10);
  directed at the next round-close sweep. Trigger conditions are narrow
  (unresolved Variable ref + class-scoped + name-matching Field), so
  exposure is low today.
- **Call-root chain arm hardcodes `arity_hint = Some(0)`**
  (`file_analysis.rs::expr_type_at_span`): wrong hint for a multi-arg call
  root on an arity-discriminated callee. Harmless while cpp overloads are
  parked; the depth round's overload-arity slice mints real arg counts —
  fix it there with the same fuel.
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
- **Ref inside another macro's body** (`OBJ_ENCODING_EMBSTR` in
  `sdsEncodedObject`) — hitlist-2 residual, unassigned.
- **`fmt::` qualified-path completion** unfiltered (the completion half
  of namespace participation — gd/gr half landed in slice B).

## Cross-references
- Gap shapes behind open xfails: `gold-corpus/KNOWN-GAPS.md`
- Fix-run narrative: `docs/hitlist-2.md`, `docs/session-2026-07-03-summary.md`
- Architectural forks: `docs/open-forks.md`
