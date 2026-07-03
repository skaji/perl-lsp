# Session summary — 2026-07-03 (the "get templates done" run)

What landed between "review approved, GET TEMPLATES DONE" and now, plus the
parked ledger. Spike tip: `3efdd9bb`. Main tip: `460419e` (PR #107 merged).

## Landed

1. **PR #107 merged to main** — the resolution CandidateSet: one semantic
   core, features as projections (gd / gr / rename / prepareRename /
   implementations / completion gathering). Includes the completion-noise
   guard (`expect.exact_labels` / `expect.max_items` in the gold harness) —
   which immediately proved #107 byte-clean AND caught a pre-existing
   nested-hash-key completion leak on main (pinned as xfail + a gold
   `max_items` ceiling).
2. **THE MIGRATION** (spike) — main's CandidateSet merged into the spike;
   the cpp axes moved INTO construction: closure visibility (wave-1's C1
   per-path gate DELETED, replaced by the by-construction version),
   delegation / `Specializes` / domain-bridge as declared edges, macro
   variants + reachability as `definitions()`' never-pruned ranking, pack
   rename full-or-refuse on the set. Both review lifts landed (sigil
   stripping behind `conventions.rs` on the set's identity keying;
   completion candidates carry `ImportFact`, the adapter composes edits).
   Cross-language invariant test: one closure fact moves cpp gd AND gr AND
   completion together. Real-perl5 spot checks byte-identical
   (croak_nocontext gr 179/0 .pm, OP_SCOPE 33, op_type `opcode`, ranked
   macro gd, delegated rename refuses). Bonus: a real deadlock found+fixed
   (`for_each_open_mut` under held read guards).
3. **Template slice (b)** — `ParametricType::Instance{base,args}`:
   template instances join their class. `Box<Widget> b; b.size()` gd/gr/
   completion work via `class_name()` projection (zero new resolution
   code); typedef-chase to instances; qualified template bases
   (`: detail::buffer<T>`) inherit; exact-spelling spec dispatch.
4. **Template slice (c) — the finale** — lazy `ParamOf`/`InstanceOf`
   substitution beside `RowOf` (`Box<int> b.get()` → `int`; fmt's
   `iterator_buffer<double*, double>.out()` → the partial spec, `out:
   double`); the fork-4 selection ladder complete (exact > partial-pattern
   with param rebinding > primary, family ranked never pruned); the fork-3
   engine unification (`src/projection.rs::project_fixpoint` — one
   worklist/seen-set/provenance spine under Perl generators (eager) and
   C++ templates (lazy); PR #100 re-extracts onto it). Bonus fixes:
   chained member gd for plain classes, sub-body-local leak in member
   lookup shielded.
5. **Heatmap PR #99** — rebased onto post-#107 main and migrated onto
   `references()` (its parallel walk deleted; 67 rows corrected — class
   subs called as methods no longer read dead). Surfaced a pre-existing
   references gap (Moo rwp writer at decl-token group answer). Schema
   stable; #105's viz stack pre-rebased on local `tmp-viz-trial`.

## Parked / deferred (the ledger)

- **Template rungs**: dependent types (`T::value_type`), value-arg
  deduction, template-template params, macro-prefixed members
  (`FMT_CONSTEXPR auto data()` — macro-damage lane; why fmt's
  `memory_buffer.data()` is still dark).
- **PR #100** re-extraction onto `projection.rs` (user closes or reworks).
- **Perl domain typing** (needs constant-group / Type::Tiny enum-domain
  model — `docs/adr/field-projections.md`).
- **Type-constrained completion** at domain slots (`op_type == |` → `OP_*`).
- **Use-after-move** re-wire (needs path-sensitivity).
- **M6/L3** LSP session determinism (cold-open None→warm flip; debounce
  staleness) — KNOWN-GAPS "LSP session determinism".
- **Per-toolchain global system-header cache**; cross-language "system
  root" generalization (perl=@INC, python=probe).
- **Flag-set domains** (`op_flags`/`OPf_*`), parametric macro return.
- **Completion residuals**: pack in-scope/member sources still
  adapter-side; nested-hash-key level leak (xfail
  `completion-exact-hash-key-slot-no-nested-leak`); Moo rwp
  writer-at-decl gap (prompt-heatmap.md).
- **`extern template` spellings** parse as ERROR in tree-sitter-cpp 0.23.
- **proto.h variadic decls** never register a Sub (`Perl_croak` absent
  from completion/gd).
- Two include-BFS walkers + two `file_stamp` fns in cpp_reparse — merge
  candidates for a calm pass (cleanup #7's "suspicious, not touched").

## In flight (this hour)

Dogfood squadron over `/home/veesh/personal/cpp-bench/` (abseil, fmt,
folly, json, redis) probing outline/gd/gr/gi via the CLI on the spike
binary — findings land in `docs/hitlist-2.md`.
