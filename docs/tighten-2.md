# Tighten round 2 — hitlist

Generators: rule-audit (opus), arc-diff audit (sonnet, `0653955..1d8134d`),
design-debt drain (opus). Round 1 (warnings→0, dead code) landed as `1d8134d`.
Identity gate per skill: suites unchanged counts, gold tallies byte-identical,
no new allowlist entries.

## Slices this round

### T2-A — ancestry-walk consolidation + family-walk dedup (S/M)
- `class_isa` (fa:3032) / `class_isa_prefix` (fa:3082): copy-pasted DFS, only
  the terminal predicate differs; the prefix walker's doc comment FALSELY
  claims "walked in one place". One predicate-parameterized walker.
- `class_is_dbic_result` (fa:10531): a THIRD hand-rolled DFS — cap 40 vs the
  siblings' 200 vs the MRO seam's 20, parents_cached-only seam (deliberate,
  per debt-drain: cross-file-only polarity gate), Core/Row-vs-Schema/ResultSet
  name enumeration in the Model layer (rule #10 HIGH; the name-set stays for
  now — moving it to the dbic plugin manifest is a follow-on with the
  role_makers precedent). Route through the shared walker with (seam,
  predicate, budget) params; caps stay per-call-site (identity).
- `method_override_family` / `owned_accessor_family` (fa:10994/11028):
  byte-identical descendant-walk tails → one `descendant_family` helper.
- Moniker tie-break comparator calls `for_each_cached` inside sort_by
  (fa:10509) — precompute family sizes once (frozen: same ordering).
- Long-term home note: GraphView's lazy walk (graph.rs untouched all arc) —
  the consolidated walker should sit ON GraphView or carry a comment naming
  it as the collapse target.

### T2-B — gated-emission residency compliance + comment/dangling fixes (M)
- `materialize_gated_emissions` (mi:1217-1236) bypasses `insert_cache`:
  direct all_files/cache inserts skip `edges.feed` + loader-shape recording,
  and pin a fully-resident whole copy INVISIBLE to
  `whole_copy_registration_sites_are_allowlisted` (calls none of the tracked
  names). Route through the canonical seam (or a documented variant), make
  the pin tripwire-visible with a residency bound. If edge-feeding changes
  any answer (it may make MORE resolvable), STOP and report — that converts
  the slice from tighten to a dogfood fix row.
- Comment drift: query_extract.rs:1956 past-tense capture comment;
  class_isa_prefix false shared-seam claim (fixed by T2-A).
- Dangling refs: hitlist-7.md:82 stale `#[ignore]`/line citation;
  prompt-enrichment-inheritance-residual.md stale line numbers (52-55).
- gated_emissions residency note in docs/prompt-storage-residuals.md
  (rule-audit finding 6: rides eviction unstriped; sparse by construction).

## Coordinator-applied at close (no agent)
- PARKED.md: bump all 7 re-ratified verdict dates; REWRITE entry 7
  (load_components prefix — old blocker obsolete: EmitAction::PackageParent
  exists; new rationale: generic mixin machinery, plugin is ClassIsa-gated);
  record new candidates A-D (ancestry-walk family w/ GraphView target,
  DBIC-result polarity gate, RESOLVE_MEMO-vs-PackBagCache contracts, three
  deliberate CLI dedup keys).
- CLAUDE.md residency bullet: drop `refs_present` from the single-axis-reader
  enumeration (stale doc — seam is deliberately dead for symmetry).
- Fixture org notes (nested-fixture dual-purpose; posmacro/outofline_defs
  naming coincidence) — recorded, not acted on.

## Deferred to round 3
- `language == "cpp"` include-token gates (backend.rs:1899/:2029) →
  Slot-taxonomy dispatch (M, pre-existing).
- refs_present + refs_are_evicted deletion vs keep-for-symmetry —
  HUMAN CALL for veesh (code comment documents deliberate retention;
  YAGNI says delete ~40 LOC + trait surface).

## Round-1 carry (landed `1d8134d`)
Warnings 11→0, dead `refresh_diagnostics`/`ClosureList::len` deleted,
test-DCE discrepancy explained (rule-audit finding 4: pub-masked test-dead,
no perl/cpp asymmetry, no fix needed).
