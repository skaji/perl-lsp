# Macro-expansion salvage: the scaling wall on dense files

Forward-design note. The problem is a **residual on the densest, most
macro-abusive translation units** (perl5 `op.c` is the canonical case);
the machinery is otherwise correct. Two sibling defects share one fix.

## LANDED: context-free-safe verdict (fixes op.c:633)

The **verdict-cache slice landed**, but the on-the-ground mechanism for
op.c:633 turned out to be a *sibling* of the budget wall this note
predicted, not the budget wall itself. Verified reality on the current
checkout: `pTHX_` resolves to an **empty** body (non-multiplicity config),
salvage **never runs** on op.c, and `Perl_op_refcnt_inc(pTHX_ OP *o)` at
line 630 lives inside `#ifdef PERL_DEBUG_READONLY_OPS` (628–663). The WIDE
expansion raises whole-file damage (a *sibling* macro in the file), so
`preprocess_validated_with` drops to the **re-excluded fallback**
(`expand_region_bodies=false`) — which excludes every conditional-region
body wholesale and strands the clean `pTHX_` there, exactly the "clean
macro dropped with the damaging batch" defect this note describes, but at
the region-exclusion seam rather than the salvage-budget seam.

The fix is the verdict's essence, delivered where it actually bites:
`is_context_free_safe(macro)` classifies an object-like macro with an
empty/whitespace body as **context-independently safe** (its expansion is
pure byte DELETION — it can only remove a token, never introduce a bad
one, in ANY position). Such a macro is:

- **exempt from the conditional-region-body exclusion** (never the hard
  string/comment/directive spans) in `compute_splices_inner`, so `pTHX_`
  survives the re-excluded fallback → `o` types as `OP` → `o->op_slabbed`
  resolves; and
- **pulled out of the salvage bisection** as the always-applied baseline
  (`salvage_splices`), so it costs zero probes and can't lower the recall
  of the ambiguous groups (a deletion only lowers damage).

The damage-never-rises invariant is preserved throughout by the existing
whole-file validation gate. Classification is a pure syntactic property
(fix #4 below), free and stable, so cross-open reuse is automatic via the
already-cached `PreExpandedExternal` — no SQLite blob field, no
`EXTRACT_VERSION`-persisted verdict (the bump to 161 is only because cpp
analysis OUTPUT changed, invalidating stale blobs).

**Still open (localization, fix #1):** a NON-empty, position-DEPENDENT
macro with genuinely mixed good/bad uses. `pTHX_ → PerlInterpreter
*my_perl,` (a multiplicity config) is NOT context-free (the trailing comma
is safe only in a param list), so it stays on the normal
exclusion/salvage path. Mapping the first-parse ERROR range back through
the splice map to the covering group would keep clean uses of such a
macro without probing the whole name set.

## The machinery (as it stands)

Macro expansion turns every macro *use* into a **splice** (a byte-range
replacement). `preprocess_validated_with` (`src/cpp_reparse.rs`) applies
all splices, parses once, and keeps the lot if `parse_damage` did not
rise over baseline — the cheap path most files take.

When the full expansion **raises** damage (a macro whose body the single
pass mis-models — nested CALLs, `##` token-paste, X-macro tables),
`salvage_splices` bisects for a subset that validates:

- Splices are grouped **by macro name** (a broken body breaks every use
  of that name → the name is the validation unit; keeps reparses
  O(names) not O(uses)).
- `salvage_groups` binary-searches: try all groups → if damage rises,
  split in half, recurse L, recurse R, then test the two survivors
  combined (paired BEGIN/END couple through whole-file validation). A
  group that can't be kept is retried as length-preserving **blanks**;
  if the blank also fails, the group is dropped.
- **Invariant:** damage never rises — an unvalidated splice is never
  kept (`salvage_validates` returns false on budget exhaustion).

Each `salvage_validates` probe is **one full parse of the whole file**.
`SALVAGE_PARSE_BUDGET = 48` caps total probes. The cap is a deliberate
latency guard: op.c is ~16k lines and its first-open was dragged 50s→s;
an uncapped bisection (hundreds of probes × a 16k-line parse) walks
straight back onto that cliff.

## The two defects (siblings)

1. **Per-name granularity** (already ledgered, `PARKED.md`): a name with
   *mixed* good and bad uses is kept-or-blanked *wholesale* — the unit is
   the name, so one bad use taints every good use of that name.

2. **Budget-exhaustion scaling** (this note — previously unrecorded): the
   bisection is **blind**. It doesn't know *which* splice caused the
   damage, so it binary-searches the whole name set. op.c has ~3959
   splices across a large, diverse macro-name set (`pTHX_`, `aTHX_`,
   dozens of `SV*`/`OP*`/`PL_*`), with the damage scattered — so the
   search burns all 48 probes before it isolates the culprits. The
   instant the budget hits zero, every not-yet-validated group is dropped
   wholesale. **`pTHX_`/`aTHX_`, whose expansions are perfectly clean,
   sit in that unprocessed tail and die as collateral.**

Downstream symptom: `op.c:633` `Perl_op_refcnt_inc(pTHX_ OP *o)` keeps
`pTHX_` literal → tree-sitter types `o` as `pTHX_` → `o->op_slabbed` is
dark. This is NOT the conditional-region exclusion (slice 1 fixed that);
it is a second, independent wall — the salvage budget. Verified via
`PERL_LSP_SALVAGE_DEBUG` (budget exhausts, `pTHX_`/`aTHX_` in the dropped
set).

## Fixes, ranked

1. **Damage localization instead of blind bisection** (the principled
   one, kills defect #2 and shrinks #1). The first full-expansion parse
   already surfaces the ERROR node's byte range. Map that range back
   through the splice map to the covering macro-name group → the culprit
   is known in O(1), no search. Probe/drop only *that* group; `pTHX_`,
   which never touches the damaged region, is never probed and never
   dropped. Turns O(names) blind probes into O(bad-groups) targeted ones,
   and 48 stops binding on op.c.

2. **Per-name expansion-verdict cache** (cheapest durability win, and it
   addresses BOTH defects — user-endorsed). A macro name's
   expansion-safety is *stable*: `pTHX_` expands to a clean declarator
   fragment and is safe/blankable **everywhere**; the offending X-macro
   is broken everywhere; perl.h is fixed across opens. Classify each name
   once — `{clean-expand | blank | drop}` — and persist the verdict keyed
   by header-set/toolchain (it rides the SQLite blob machinery already).
   `pTHX_` gets classified blankable on the first open and is **never
   bisected again** — no probe spent on it, so the budget goes only to
   genuinely-ambiguous names. Note: a name-level verdict also resolves
   defect #1 for the common case (a name is uniformly clean or uniformly
   broken); a name with *genuinely* position-dependent expansion is the
   rare residual that still wants #1's localization.

3. **Incremental reparse per probe** (the general lever, most invasive).
   tree-sitter can reparse just the edited subtree given the prior tree.
   If each probe were incremental rather than a fresh 16k-line parse, the
   per-probe cost collapses and the budget could rise 10–100× for nearly
   free. Attacks cost-per-probe rather than probe-count.

4. **Syntactic pre-filter** (cheap partial) — **LANDED** as
   `is_context_free_safe` (narrowed to the provably context-independent
   class: empty/whitespace-body deletions). A single-token declarator
   macro (`pTHX_`) is structurally safe; the dangerous shapes contain
   `##` or produce unbalanced braces. A one-pass filter exempts the
   obviously-safe majority from salvage, shrinking the bisection input.

**Recommended order:** #2 (verdict cache) is the smallest diff with the
biggest op.c payoff — `pTHX_`, used across the whole codebase, stops
costing anything after the first classification. #1 (localization) is the
principled follow-up that also covers position-dependent names. #3/#4 are
levers if either proves insufficient.

## Scope (honest)

Densest-file residual, not a general break. op.c mostly works; only the
macros stranded in the dropped salvage tail — unluckily the `pTHX_`-
threaded functions — go dark. It bites the hardest TU in a famously
macro-abusive codebase.
