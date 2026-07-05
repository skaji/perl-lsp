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

## Implicit-`this` capability: one flag for fields AND calls — 2026-07-05 — OPEN
- **Context:** hitlist-3 Family A+I slice. The implicit-field-read pass is
  gated by the pack's `implicit_field_reads` capability. The sibling-CALL
  half (a bare `foo()` inside a method body meaning `this->foo()`) needed a
  gate too — same fork the task flagged: reuse the flag, or add a sibling
  one.
- **Options:** A — reuse `implicit_field_reads` for both halves. B — add a
  parallel `implicit_method_calls` capability.
- **Picked:** A. "Can a bare name resolve through an implicit `this->`" is a
  SINGLE language fact — C/C++ elide the receiver for both members and
  methods; Python/R make it mandatory for both. There is no language where
  fields elide but methods don't (or vice-versa), so a second flag would be
  a distinction with no possible producer. The flag's NAME is now
  field-specific and slightly under-describes its scope; a future rename to
  `implicit_this_members` is the loose cleanup, deferred to avoid churn
  across the pack definitions.
- **Undo cost:** trivial — split into two bools and thread the second
  through `emit_return_fuel`; the sibling-call pass already stands alone as
  its own block, so it just reads a different flag.

## Sibling-call vs. same-named free function ranking — 2026-07-05 — OPEN
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
- **Picked:** A for this slice — `resolve.rs` is explicitly owned by a
  sibling worktree this slice must not touch. Logged for that owner. The
  reduced-fixture row `cpp-sibling-call-shadows-free` is PROVISIONAL
  (asserts the sibling link is offered; does not gate on the free being
  absent) so the residual is tracked without a false-green.
- **Undo cost:** small and localized to `resolve.rs::overload_arity_definitions`
  — gate the family gather on `pkg_agrees(relative=false, …)` (exact
  package) when the call carries a member-pinned `resolved_package`.

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

## Type-constrained completion — carried expected type vs new Slot variant — 2026-07-05 — OPEN
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
