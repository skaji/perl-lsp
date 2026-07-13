# ADR: Config superposition over declarations

Status: decided. Case B is handled by the macro-expansion exclusion
narrowing (opportunistic widening with a wide-scope fallback). Case A is
handled by the declaration-position directive repair
(`strip_declaration_position_directives`, the isolated ctor-`#if`) plus the
attribution-layer re-anchor (`SkeletonAnalysis::reanchor_truncated_containers`),
which recovers `basic_json`'s member attribution (92 → 763). Variant tags are
scoped ONLY to genuinely superposed declaration twins and remain deferred —
the re-anchor, not variant tags, was Case A's fix. See `docs/PARKED.md`.

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

## Spike findings (2026-07-05)

Fixture: `gold-corpus/cpp-fixture/cfgtwin/cfgtwin.c` (self-contained,
isolates pTHX_-vs-plain × #ifdef-vs-#if/#else × struct-field-in-#ifdef).
Real corpus: perl5 op.c / perl.h / op.h.

### Case B mechanism — TRANSFORM, and MISDIAGNOSED

Case B is **not** a config-superposition problem. It is the macro-
expansion **exclusion over-reaching**. `cpp_reparse::exclusion_spans`
(the `EXCLUDE_QUERY`) captures the WHOLE `preproc_ifdef` / `preproc_if`
node — **body included, not just the directive/condition line** — so
every macro USE between `#ifdef` and `#endif` is skipped by the global
expansion walk. perl5's `pTHX_` context-param macro therefore stays a
**literal token** inside any `#ifdef`-wrapped function; tree-sitter-cpp
then parses `pTHX_ OP *o` as a parameter typed `pTHX_`, the receiver `o`
mistypes as `pTHX_` (confirmed via `--type-at`), and `o->op_slabbed`
goes dark. The struct field itself, its def, and the parse are all fine
— the single broken link is the receiver's type.

Evidence chain (all warm, cold-flake-guarded):
- **Isolation.** `#ifdef` alone is harmless: a *plain*-param function
  inside `#ifdef` (`plain_ifdef`) resolves `o->slabbed` and types `o:
  op`. Only the `pTHX_`-bearing twin (`thx_ifdef`) goes dark. So the
  cause is the macro use, not the region.
- **Transform dump.** The rewritten (post-expansion) text expands
  `pTHX_` at the unconditional site (`void thx_uncond( struct op *o)`)
  but leaves it verbatim inside `#ifdef` (`void thx_ifdef(pTHX_ struct
  op *o)`). Salvage did NOT fire (the rewrite validated) — the token was
  never a candidate, because its byte range sits inside an exclusion
  span.
- **Causal proof.** Deleting `(preproc_ifdef) @x` + `(preproc_if) @x`
  from `EXCLUDE_QUERY` (temporary, reverted) makes every dark case
  resolve with `o: op` and no control regression.
- **Real corpus.** op.c:394 (`o->op_slabbed`, unconditional) → op.h:57
  and `o: OP`. op.c:633 (`Perl_op_refcnt_inc(pTHX_ OP *o)` inside
  `#ifdef PERL_DEBUG_READONLY_OPS`) → "No definition found", `o: pTHX_`.
  Same shape as the fixture's `thx_ifdef`.

### Arm asymmetries — NONE (the exclusion is condition-blind)

`#ifdef` / `#ifndef` / `#if defined(...)` are one mechanism (all are
`preproc_ifdef`/`preproc_if` nodes, all whole-node-excluded). The `#if`
(config-inactive) arm and the `#else` (config-active) arm go dark
*equally* — a macro use in the **config-ACTIVE** arm is dark too
(`ndef_thx` under an always-true `#ifndef`, and the `#else` arm of a
`twin_thx`, both dark). Single-arm `#ifdef` reproduces with no `#else`
at all. Consequences: (a) a config *picker* would not help — the active
config is dark; (b) "config-twin" is a misnomer, no twin is required.

hitlist-3's Family-M note ("op_slabbed goes dark only in config-INACTIVE
`#ifdef` twins") is **wrong on both counts** — active arms are dark too,
and no twin is involved.

### perl.h / op.h numbers (linear-cost stress)

| file  | regions | arms | max depth | distinct stacks | cond lines | sym / ref / witness | bincode | zstd blob |
|-------|--------:|-----:|----------:|----------------:|-----------:|--------------------:|--------:|----------:|
| perl.h| 747 | 1059 | 5 | **946** | 58.5% | 2336 / 4580 / 10581 | 2.12 MB | 286 KB |
| op.h  |  29 |   42 | 2 | 23 | 17.5% | 424 / 483 / 698 | 204 KB | 34 KB |
| op.c  |  78 |  106 | 2 | 36 | 3.6% | 1794 / 22128 / 29023 | 5.5 MB | 807 KB |

(distinct-stack counting mirrors `guard_trail`, with header-guard
suppression; depth is guard-suppressed nesting.)

**Linear, no cross-product.** perl.h's interning table is 946 entries —
≈1.27× the region count, ≈0.89× the arm count. It is bounded by document
structure, NOT by facts×regions (which would be 17497×747 ≈ 13M). Max
depth 5 admits a theoretical 2^5 combos per nest, but realized distinct
stacks stay ~= arm count. The variant tag is one small integer (a u16
index suffices — 946 ≪ 65536) per fact. Tagging is O(facts); the table
is O(distinct stacks). Confirmed.

**Cache delta (perl.h, 17497 facts).** Per-fact tag as `Option<u16>`:
+1 byte (discriminant) on every fact, +2 more on the ~58% inside a
conditional → ≈38 KB added to the 2.12 MB **uncompressed** bincode
(+1.8%; worst case all-tagged u32 = +87 KB / +4.1%). The interning table
itself is a few KB uncompressed. Because tag ids are small and highly
repetitive within a region, zstd crushes them: the **compressed** blob
delta is expected well under 1% of the 286 KB (single-digit KB).
EXTRACT_VERSION must bump regardless.

### Go / no-go

- **Slice 1 (re-anchor + declaration-position repair): GO — and widen
  it.** Case B's actual fix belongs here, not in slice 2: narrow
  `EXCLUDE_QUERY` so it excludes the `preproc_if` **condition** and the
  directive tokens only, leaving the region **body** expandable (capture
  the `condition:` / `name:` field, not the whole node). This is a
  one-query, model-independent robustness fix that clears the entire
  op.c `PERL_DEBUG_READONLY_OPS` darkness tier. (Note: Case B has NO
  unbounded blast radius — the damage is local to one mistyped receiver
  — so the re-anchor *invariant* per se isn't what saves it; the repair-
  lane spirit is.) Caveat: the exclusion presumably exists to avoid
  expanding names on directive lines; keep excluding the condition
  expression, un-exclude only the body.
- **Slice 2 (variant-tagged declarations): GO on cost, but RE-SCOPE.**
  Cost is cheap and linear (above), so the serialized-shape change is
  affordable. HOWEVER the spike shows slice 2 is **not needed for Case
  B** — the slice-1 exclusion narrowing cures it entirely. Variant tags
  remain justified for **Case A** (json.hpp `#if` in ctor-initializer
  position) and for genuinely superposed DECLARATIONS (a field/def whose
  shape differs per config, `#else`-twin functions with different
  bodies) where union-labeled navigation + arm-fold typing are the point.
  Re-justify slice 2 against Case A + true twins, not Case B.

### Surprise that changes the ADR's assumptions

The ADR's Context framed Case B as "config-twin regions … mechanism not
yet pinned (parse damage vs. transform blanking an arm)". **Neither** is
right. It is the transform's macro-expansion exclusion covering
conditional-region bodies, interacting with the `pTHX_` convention;
condition-blind, twin-free, single-arm-reproducible. The load-bearing
consequence: **the expensive slice-2 variant model is not required to
fix the op.c darkness that motivated Case B.** A cheap slice-1 query
narrowing clears it. The config-superposition investment should stand or
fall on Case A and real declaration twins alone.
