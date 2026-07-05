# Hitlist 3 — depth-round intake (2026-07-05)

Live findings queue for round 3. Same discipline as hitlist-2: every
reducible finding gets a RED xfail row before its fix; root causes get
CONFIRMED by experiment, not guessed.

## Round-3 dogfood synthesis (folly/spdlog/abseil/json/perl5/redis, 2026-07-05)

Raw reports: three probes, ~30 findings, deduped into FOUR families +
small fry. gr/reverse-index was correct in every family-Q case — the rot
is all in forward gd/hover.

### Family Q — qualifier/owner-blind forward resolution (5 symptoms, likely 1 root)

gd/hover fall through to a bare-name global search that ignores both the
`X::` qualifier and the receiver's class; gr at the same coordinates is
correct every time.
- folly `case dynamic::STRING:` (dynamic-inl.h:1408) → gd lands on an
  UNRELATED `#define STRING` in FBStringBenchmark.cpp (wrong-answer).
- folly `dynamic::OBJECT` (dynamic.cpp:98) → dark (same gap, no collider).
- spdlog `level::info` (3 sites) → `dragonbox::float_info<Float>::info`
  type alias in bundled fmt (wrong-answer).
- spdlog member calls `logger.log/.info/.trace` → same-named FREE
  functions in spdlog.h win over the class members; receiver typing is
  fine (set_level resolves; completion correctly scoped) — the collision
  priority is the bug.
- fmt hover on `native_formatter::format` decl → color.h's free
  `format` (hover-only; gr at same span correct → hover shares the
  qualifier-blind path).
FIX SLICE Q (owner-anchored forward resolution). Assigned.

### Family M — macro-body extraction fidelity (perl5 core types dark)

- **`_SV_HEAD` comment-truncation, ROOT-CAUSED**: tree-sitter-cpp's
  `preproc_arg` truncates a backslash-continued macro body at the first
  trailing `/* comment */`; `Macro.body` keeps only the first field, so
  SV — THE central Perl type — has zero member intelligence
  (`sv->sv_flags` dark everywhere; `sv->` completion junk). Fix scoped by
  the probe's sub-agent: re-derive the body from raw source bytes across
  continuations (strip comments) instead of trusting the CST span
  (cpp_reparse.rs: MACRO_DEF_QUERY / clean_body / plan_member_blocks).
- **BASEOP per-field synthesis drops 4/14 fields** (op_targ, op_opt,
  op_slabbed, op_flags dark; 9 siblings + completion fine) — DISTINCT
  bug (body parses in full, no comments involved) in the per-field
  span-splitting (`synth_base`). Field position doesn't predict failure.
- **gr misses macro-nested refs at scale**: SvFLAGS 190 vs 347 grep-real,
  SvANY 111 vs 200 — refs inside OTHER macros' bodies aren't indexed
  (generalizes the known redis `OBJ_ENCODING_EMBSTR` gap from
  one-site-curio to 45% undercount on core symbols). gd through the same
  nested sites WORKS — index-population-only.
FIX SLICE M. Assigned.

### Family A+I — cpp local-intelligence gaps

- **Braced-init flow misinference beats the declared type** (abseil):
  `flat_hash_map<int,int> m = {{1,7},{2,9}};` hovers `m: Numeric`,
  completion falls to junk; the no-init control is perfect. The C++ twin
  of the landed annotation-priority fix, on an axis it didn't cover
  (either the annot witness isn't minted for this decl shape, or the
  braced-init flow witness outruns it).
- **Implicit-`this` sibling method CALLS dark** (folly, template AND
  plain classes): bare `reserveSmall(...)` inside an out-of-line member
  body, `isNull()` inside `dynamic::empty()` — gd nothing. Qualified
  `Class::method` resolution works; the sibling-call link back is
  missing. The sibling-FIELD reads landed (emit_return_fuel); calls are
  the unfinished half.
FIX SLICE AI. Assigned.

### Ledgered small fry (not sliced this round)

- json single-header attribution break: SHARPENED — trigger is the `#if`
  in ctor-initializer position at json.hpp:21396; attribution never
  recovers for ~4400 lines (~80% of basic_json: completion empty, hover
  cross-file corrupted). Plus spurious Method re-emission of two call
  statements at the break point. Still the config-superposition-on-
  declarations tier (PARKED); now with exact blast radius.
- Dual-vs-single gd targets on typed fields inconsistent (`op_type` →
  field + enum def; `op_next` → field only) — normalize deliberately.
- redis `extern struct redisCommand redisCommandTable[]` → defining decl
  behind `#include "commands.def"` not linked (single site).
- `struct interpreter`/PERLVAR token-pasting invisible — fundamental
  no-preprocessing limitation, recorded as expected behavior (PL_curcop
  resolves one hop, `Icurcop` doesn't exist as text anywhere).

### Verified green in round 3 (the re-probe column)

Substitute arity (all 4 arities, independently), join() arity both ways,
GUARDED_BY 56/56 grep-exact, format_to ~90 grep-consistent, domain
completion ranking OP_* at two real op.c sites in declaration order,
pTHX_ hover/locals/members across 5 functions, embed.h alias dual-target
gd (macro def + real fn), memory_buffer completion, OBJ_ENCODING_EMBSTR
20/22 with both misses accounted for, commands.def navigation, robj*
member gd, OP_PADSV gr 100% comment-exact.

## 1. Context-param macro chains break member resolution (perl5 pTHX_) — RESOLVED

**Resolution (2026-07-05):** the pTHX_ chain was a RED HERRING. Bisection
showed the trigger is a SECOND top-level declaration in the header combined
with a receiver typed through a macro call: `RCPV *rcpv = RCPVx(pv)`. The
uppercase macro-call `RCPVx(pv)` mis-fires the ctor-convention
`ClassName("RCPVx")` witness (query_extract), which — as a `flow`-sourced
class assertion — *shadowed* the explicit `skeleton-annot` declared type
`RCPV` in the Variable reducer (equal source priority → latest-wins → flow
wins). Member resolution then dispatched on the bogus class `RCPVx`, whose
"parents" only resolved by luck through `primary_package_parents`'s
`len()==1` single-entry fallback — which stops firing the moment the header
holds a second `package_parents`-bearing decl (hence the "2 typedefs" /
"extra struct" / "extra global" triggers, all equivalent).

Fix: an EXPLICIT type annotation is a higher-confidence class assertion than
an inferred flow type — `WitnessSource::priority` returns 20 for the
`ANNOT_SOURCE` (`skeleton-annot`) tag vs 10 for flow. The declared `RCPV`
now wins, the receiver types `RCPV`, and `parents_cached("RCPV")` resolves
robustly via the exact-name branch regardless of header decl count. General
(rule #10): keyed on the annotation SOURCE, no macro names, no perl5
specifics. Rows `cpp-ctxparam-member-gd-{control,nested}` promoted to gold;
`cpp-ctxparam-member-hover-nested` added. Real-world verified on perl5
op.c:16170 (`rcpv->refcount` → cop.h:574) and a second pTHX_ function
(`o->op_type` in Perl_op_free). NOTE: the op.c CLI queries need one warm-up
query first — the cold-start flake (finding #2) still poisons the first hit.

### Original dossier (pre-resolution, kept for the probe trail)

**Report (veesh):** op.c:16170 `rcpv->refcount++` has no gd — expected the
field on `struct rcpv` (cop.h:574, via `typedef struct rcpv RCPV`,
cop.h:580).

**Probing dossier (2026-07-05):**
- Editor (open-doc path): var `rcpv` typed, `RCPV` gd → typedef works;
  ONLY the member-access gd fails. ← the core symptom.
- CLI (workspace path) on the same coordinates: strictly worse — no hover
  on `Perl_rcpv_copy`/`Perl_op_free` names, no gd on the local var either.
  Open-vs-workspace parity gap; may be a distinct bug (or the CLI gd
  mirror skipping enrichment). Finding #3 below.
- Miniature differential (all warm, cold-flake-guarded):
  - single-level config-superposed param macro (`pCTX_`), alone: WORKS.
  - transitive include, statement macros, macro initializer: WORK.
  - add the faithful perl5 chain to the header — `pTHX_` → `pTHX,` →
    `tTHX my_perl PERL_UNUSED_DECL` + attribute-macro defs +
    `typedef struct interp PerlInterpreter` — and member gd fails for
    EVERY function in the TU, including the single-level control that
    passed before. TU-wide degradation, not per-signature.
  - fn symbols still extract in the fixture (hover works); the typed-local
    → member chain is what dies. Suspect: expansion/splice-map span drift
    poisoning the local's witnesses, or the macro-table lane
    misclassifying the nested chain.
- Pinned: `gold-corpus/cpp-fixture/ctxparam/` +
  `cpp-xfail-ctxparam-member-gd-control` / `-nested`
  (cpp-definition.json).

**Why it matters:** pTHX_ prefixes essentially every function in perl5 —
the flagship C corpus. This shape gates all intelligence inside those
bodies.

## 2. Cold-start flake reconfirmed (M6/L3)

First query against a fresh root returned misses that succeed on rerun
(`Modules: 0 cached` on the failing run). Matches veesh's in-editor blip:
op.c outline empty once with a "too long"-ish error, fine after. Already
parked as "LSP session determinism"; today's repro is another vote to
schedule it — it actively poisons probe evidence (nearly cost us a wrong
root-cause on this very hitlist item).

## 3. Workspace-symbol / CLI parity gaps (pack languages)

- `--workspace-symbol` returns `[]` for EVERYTHING in a C root (struct,
  macro, fn — even symbols gd resolves): pack files aren't in the
  workspace_index (extension type-prune is Perl-only), so workspace/symbol
  has no pack surface.
- `--outline <file>` (single-file CLI) returns `[]` for .c/.cpp files whose
  gd works (returns 1 item for cop.h). The in-server outline is fine —
  CLI-mirror gap only.
- CLI gd/hover appear to run WITHOUT the open-doc enrichment the editor
  gets (stratum split in finding #1). If confirmed, CLI mirrors should run
  the same pipeline `publish_diagnostics` does, or dogfood probes
  systematically under-report.
