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

## `Slot::ModulePath`'s `in_use` field — 2026-07-05 — OPEN
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

## Ref-type deref snippets — candidate data vs projection policy — 2026-07-05 — OPEN
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
