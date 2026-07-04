# Hitlist 3 — depth-round intake (2026-07-05)

Live findings queue for round 3. Same discipline as hitlist-2: every
reducible finding gets a RED xfail row before its fix; root causes get
CONFIRMED by experiment, not guessed.

## 1. Context-param macro chains break member resolution (perl5 pTHX_)

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
