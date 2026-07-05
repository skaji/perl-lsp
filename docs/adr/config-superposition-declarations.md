# ADR: Config superposition over declarations

Status: **decided** (veesh, 2026-07-05); implementation not started.
Spike gate below runs before the main slices.

## Context

The macro model already treats config-variant `#define`s as a
superposition (arm-fold, labeled multi-target gd — see
`docs/adr/macro-handling.md`; `cpp_reparse.rs` computes per-site
condition-term stacks with header-guard suppression). Code itself has no
such model, and round-3 evidence made this the dominant remaining
wrong-answer tier:

- **Case A — directive inside a declaration** (json.hpp:21396, `#if` in
  ctor-initializer position): local misparse PLUS unbounded blast radius
  — class attribution never re-anchors, so ~4400 lines (~80% of
  `basic_json`) lose membership: empty member completion, hover
  cross-file corruption.
- **Case B — config-twin regions** (perl5 op.c): `o->op_slabbed`
  resolves at an unconditional site and is dark inside an
  `#ifdef PERL_DEBUG_READONLY_OPS` region. Mechanism not yet pinned
  (parse damage from region placement vs. our transform blanking an
  arm) — the spike settles it.
- **Positive control**: macro-def superposition works (spdlog
  `SPDLOG_LOGGER_CALL` gd returns both arms, labeled).

## Decision

Two commitments, in order:

1. **The re-anchor invariant (immediate, model-independent).**
   Attribution must re-anchor at the next unambiguous boundary after any
   misparse — no local damage may have unbounded blast radius. Paired
   with a repair-lane extension for declaration-position directives
   (guarded by the usual `structure_count` anti-gaming floor). This is a
   robustness bug fix, not a config-model choice.

2. **Superposition as first-class variants, scoped to DECLARATIONS.**
   Conditional regions become variant spaces: arms parse separately
   (bounded local reparse, existing splice machinery); every fact minted
   inside an arm carries a variant tag = the interned condition-term
   stack `cpp_reparse` already computes. Consumers fold:
   - Navigation **unions, labeled** (the macro-def gd rendering).
   - Typing folds by **agreement through the existing arm-fold
     reducers** — a config arm is a branch arm whose condition is a
     preprocessor expression; the reducer vocabulary does not grow, the
     walker emits arm-tagged witnesses from a new arm kind.
   - **Unified symbol identity across arms** (one symbol, multiple
     tagged def-sites — forced by rename correctness: a rename must edit
     every arm atomically, including configs the user isn't looking at).
   - **Nobody evaluates conditions; ranking may peek** — platform-obvious
     signals / non-`#else` weak prior rank first, all arms always shown
     (ranks-never-prunes).
   - Statement-level twins are OUT of scope initially; all measured pain
     is declaration-granularity (fields, ctor-initializers, defs).

## Rejected

- **Pick-a-config** (clangd-style): against the project's grain —
  inactive-arm code is code developers navigate ("you frequently DO care
  about portability"); library corpora are deliberately all-configs; and
  someone must own config-selection UX. This is the failing status quo
  with ceremony.
- **Flatten both arms into one parse**: produces invalid syntax in
  exactly the hard cases (concatenated ctor-initializer lists, colliding
  `#else`-twin definitions) and pushes contradictory facts onto the same
  witness attachment with no provenance separating them — the
  parallel-truth drift the bag exists to prevent. Named and refused as
  the rule-#10 "smallest diff right now" temptation.

## Spike gate (run FIRST, ~half a day)

1. Pin Case B's mechanism: parse damage vs. transform blanking, on a
   reduced fixture + real op.c coordinates.
2. Stress the linear-cost claim on perl.h: region count × arm count with
   nesting — confirm tagging stays linear (no global cross-product) and
   estimate cache growth before committing the EXTRACT_VERSION bump.

## Costs acknowledged

Serialized-shape change (variant tags on symbols/refs/witnesses →
EXTRACT_VERSION, cache growth); every one-symbol-per-name consumer meets
variant families (macro-variant precedent says they survive); nested
conditions make tags condition SETS.

## Sequencing

Slice 0 spike gate → slice 1 re-anchor invariant + declaration-position
repair (kills Case A's blast radius even before variants land) → slice 2
variant-tagged declarations + arm-fold wiring (Cases A and B fully) →
re-probe json.hpp `basic_json` and op.c config twins as acceptance.
