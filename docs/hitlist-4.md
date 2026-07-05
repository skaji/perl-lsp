# hitlist-4 — triage of veesh's fresh op.c/op.h dogfood notes (hitlist.md)

Probe base: `54ce0e1b` (tip of `spike/cpp-support`), binary `cargo build --release
--features all-langs`, corpus `/home/veesh/personal/perl5`. All coordinates below
are 0-indexed row:col unless marked "1-idx". Probes ran through BOTH lanes:

- **CLI lane**: `perl-lsp --batch <root>` (full synchronous startup — workspace
  index + macro gather complete before the first answer).
- **Server lane**: raw LSP over stdio (scripts in the probe scratchpad:
  `lsp_full_probe.py` / `lsp_warm_probe.py` / `lsp_inlay.py`), didOpen with
  FULL-sync didChange, queries repeated cold (+10s/+25s) and warm (+90s).

The two lanes disagree on half the findings — that disagreement IS the second
family. Three of veesh's six findings are warm-state correct at this base and
reproduce only in the server's first-open window.

---

## Per-finding verdicts

### 1. Bare `op_p->` on its own line → no member completion; peel diagnostic silent

**Repro (completion half).** Insert `    op_p->` after op.c:183 (1-idx) inside
`Perl_op_prune_chain_head`:

- CLI (scratch copy of the tree, line on disk): `--batch` completion at
  183:10 → all 14 `struct op` members. Works.
- Server, warm (+90s), FULL-sync didChange with the same insertion, completion
  183:10 → same 14 members. **Works.**
- Server, inside the first-open window (~10–25s after didOpen op.c): completion
  at a member position returns the **global** item list (`PERL_IN_OP_C`,
  `CALL_PEEP`, …) — exactly veesh's "falls back to global".

**Verdict:** completion half = **family B** (first-open degraded window), not a
sentinel gap. `cursor_sentinel` handles the bare-arrow ERROR shape fine (reduced
fixture + real op.c both complete members); receiver `op_p: OP**` resolves and
DEEP receivers still list members (show-only op_fix, by design).

**Repro (diagnostic half).** Server-lane publishDiagnostics on
`docs/hitlist-4-fixtures/member_op_mismatch.cc`:

- `o.op_type` on `OP* o` → `member-access-operator` WARNING ("use `->` here").
  Mode B machinery works.
- `p->op_type` / `p.` on `OP** p` → **nothing, by design**:
  `expected_member_op` (file_analysis.rs:811) returns `None` for depth ≥ 2 and
  `member_op_mismatches` (file_analysis.rs:4224) skips DEEP receivers ("needs a
  wrap, not a swap" — unit-locked by
  `cpp_member_op_mismatches_drive_off_deref_depth`). So "you need to peel
  `(*op_p)->`" has **no producer anywhere**.
- Additionally: typing a dangling `o->` line made the file's *existing*
  mismatch diagnostic disappear on the next publish (dangling access emits no
  MethodCall ref and the ERROR region degrades the others).
- CLI parity gap: `symbols::pack_diagnostics` is wired only in `backend.rs`
  publishes — `collect_diagnostics`/`--batch diagnostics`/`--check` never
  include Mode B, so CLI probes (and any future gold row) are blind to it.

**Verdict:** diagnostic half = **family D** (DEEP-peel DX gap + mid-edit
suppression + CLI parity).

### 2. op.c:185 (1-idx) `(*op_p)->op_type == |` — no OP enum offered

**Repro.** Line 185 is `        && (   (*op_p)->op_type == OP_NULL`.

- CLI completion at 184:37 (inside `OP_NULL`) → full `opcode` enum, declaration
  order, `detail: "opcode"` — that is `rank_domain_members`, the real domain
  slot. Works.
- Server warm, line truncated to `… == ` via FULL-sync didChange, completion at
  184:35 (empty RHS) → `OP_NULL/OP_STUB/…` ranked first, `detail: "opcode"`,
  n=709. **Works.** Prefix variant (`== OP_`) also works.
- Server inside the first-open window → global fallback.

**Verdict:** **family B**. (Veesh's session may additionally have run a server
binary predating `8f8cdff1` — the domain-ranking feature was ~10h old — but at
this base the slot answers correctly once the window heals.)

Probe caveat worth keeping: a too-small fixture does NOT exercise this slot —
`field_domain`'s owner-gated vote returns `None` in a 2-file reduction (traced
via temporary instrumentation), and what *looks* like domain ranking in a
reduction is often just prefix-filtered closure completion (`detail:
"opcode — op_mini.h"` vs the slot's bare `detail: "opcode"`). Assert on the
detail string.

### 3. gd on `OP` (the type) → lands on a random macro  — **top pain, real bug**

**Repro (deterministic, both lanes, warm and cold).** gd at op.c 179:25
(`Perl_op_prune_chain_head(OP** op_p)`), and even at the typedef's own site
perl.h 3217:18:

```
/home/veesh/personal/perl5/regcomp.h:485:9  (if ! defined(PERL_REGCOMP_H_) && …)
```

— the sole candidate is `#define OP(p) ((p)->head.data.type)` (regcomp.h:485,
preceded by `#undef OP` at :478 — a deliberately scoped regex-internal macro).
Hover on the token shows the macro too. The right answer, `typedef struct op
OP;` (perl.h:3218, extracted as SymKind::Class — visible in `--outline
perl.h`), **never enters the candidate set**. Controls: gd on sibling typedefs
with no shadowing macro — `LOGOP` (op.c 1735:4) → perl.h:3224, `UNOP` (op.c
2352:12) → perl.h:3220 — both perfect. So the typedef lane works; the macro
lane pre-empts it.

**Mechanism (resolve.rs).** For a `RefKind::Variable` read with no local
resolution, the bare-name lane at resolve.rs ~579:

```rust
None if names_visible_macro(&r.target_name, analysis, module_index) => Some(None)
```

mints `TargetKind::FileScopeValue` (the macro identity) and **returns early**
— any visible `#define` of the name, anywhere, claims the token. Two
compounding shape-blindnesses:

- `names_visible_macro` (resolve.rs:3569) is name-keyed only. It never asks
  whether the macro is *function-like* and the use *parenless*. C's own rule
  answers this exactly: a function-like macro expands **only** when the name is
  followed by `(` — `OP**` in type position cannot be that macro, which is why
  the real preprocessor compiles this code happily.
- `pack_def_paths` (resolve.rs:3615) joins `#define` candidates
  **unconditionally across the closure gate** (the win32/unix config-variant
  rationale). Here regcomp.h happens to be genuinely in op.c's closure
  (op.c:166 includes it), so the closure gate wouldn't have saved op.c — but
  the unconditional join means every OTHER TU in the workspace also gds to the
  regex macro.

Veesh's `#undef` hint checks out as signal: regcomp.h `#undef OP` →
`#define OP(p)` marks a deliberately re-scoped internal vocabulary; the def-site
guard annotation already printed in the gd output shows region tracking exists
to hang a rank penalty on. But the load-bearing fix is the shape gate, not the
rank tweak.

**Sub-finding:** `workspace/symbol "OP"` never surfaces the typedef either
(returns `opcode`, `OP_IS_*` fuzzies; exact-name `OP` absent) — same slice
should check exact-name ranking for short type names.

**Verdict:** **family A**. NOT the in-flight `#ifdef`-exclusion (Case B) work —
the macro def is region-*active* in its own file and the typedef sits in no
conditional region; this is candidate-lane arbitration, not region visibility.

### 4. `op_p->op_next` hovers as `OP`, really `OP*`

**Repro.** CLI hover op.c 189:27 → ` op_next: OP ` *field*. Same via warm
server. Inlay hints on op.h 50–52 also render `: OP` (no star).

**Layer answer: extraction loss, not rendering.** `Symbol::display_type`
(file_analysis.rs:969) is the single "name: type" projection and appends
`deref_stack` stars — hover/inlay/sig-help all route through it. Control
fixture (`docs/hitlist-4-fixtures/pointer_hover.c`, plain `struct op { OP*
op_next; … }`): hover shows `op_next: OP*` at BOTH def and use. In perl5 the
field is declared inside the `#define BASEOP` body, and that lane mints the
symbol with an **empty `deref_stack`**. Related degradations on the same
symbols (same lane): kind is `Variable` with `package: "BASEOP"` (not `Field`
of `op`; visible in `--outline op.h`), and the def-site hover reads
`op_type: U16TYPE` / `*variable*` while the use-site reads `op_type: opcode` /
`*field* (stored as uint16_t)`.

**Verdict:** **family C** — the macro-body member lane. **LANDED** (see family C).

### 5. Enum variants: hover / inlay / references

- **Hover shows value but not type** (veesh): inverted at this base. CLI+server
  hover on `OP_NULL` (def and use) → ` OP_NULL: opcode ` *enumerator* — the
  TYPE now, but the **value (`= 0`) no longer shown**. Residual polish: render
  both (`OP_NULL = 0: opcode`).
- **Spurious inlay hints in opnames.h**: NOT reproduced at base. Server
  `textDocument/inlayHint` over opnames.h 15–30 → `null`. Enumerators extract
  as `SymKind::Enumerator` (outline confirms) and `symbols::inlay_hints` hints
  only `Variable` decls. Veesh's session predates the kind split
  (`3419a21a`, Jul 3) reaching his running server — stale-build observation.
- **"No references other than their own def"**: reproduced ONLY in the
  first-open window (server refs on opnames.h 16:3 → count=1, the def itself;
  same query warm → dozens across class.c/dump.c/op.c…; CLI → same dozens).
  **Family B.**

### 6. op.h:55 `op_type` — unnecessary inlay hint + no references

- **Inlay: REPRODUCED.** Server inlayHint op.h 45–65 → every BASEOP member
  hinted with its own declared type: `: OP`, `: PADOFFSET`,
  `: PERL_BITFIELD16` (op_type, line 54), `: U8`. The suppression rule exists
  and works on the plain-struct control (mini fixture → no hints): inlay skips
  a Variable whose `Variable{name, scope}` attachment carries an
  `ANNOT_SOURCE` witness (symbols.rs:2224). The macro-body lane's members
  don't satisfy that lookup (witness missing or attached under a different
  scope) — same lane as finding 4's deref_stack loss. **Family C.**
- **References: window artifact.** Server in-window refs op.h 54:22 → count=1
  (self). Warm → count=587. CLI → hundreds (class.c:593 etc.). **Family B.**

---

## Family synthesis

### A — bare-name macro lane out-claims type names (resolve.rs)  → finding 3  — **LANDED** (`fix/macro-type-arbitration`)

**Mechanism:** `resolve()`'s pack backward/forward lanes treat "any visible
`#define` of this name" as total identity for a bare token
(`names_visible_macro` early-returns `FileScopeValue`), before any
typedef/class candidate is considered; `pack_def_paths` additionally joins
macro defs across the closure gate unconditionally. A **function-like** macro
thus claims **parenless type tokens** it could never expand at (the C rule:
fn-like macros expand only at call shape `NAME(`).
**Owning seam:** `src/resolve.rs` — the `RefKind::Variable` bare-name lane
(~579), `names_visible_macro` (3569), `pack_def_paths` (3615).
**Slice A (top pain):** carry the macro's fn-like/object-like shape into the
claim (`names_visible_macro` → "names a macro *usable at this token shape*"),
let parenless type tokens fall through to a type-name candidate lane
(typedef/class SymKind from def_candidates — LOGOP/UNOP controls pin the
expected behavior), keep both in the CandidateSet with ranking rather than
early-return; optional rank signal: `#undef`-preceded redefinition = scoped
internal. Include the ws-symbol exact-name check. Fixture: gd on `OP` at op.c
179:25 must answer perl.h:3218 (gold row candidate); gd on `OP(...)` call-shaped
uses in regexec.c must still answer the macro.

**LANDED.** The gd hijack lived in the `definitions()` **forward** macro lane
(`ranked_macro_variants`), not the backward `resolve_symbol_scoped` bare-name
lane the triage first fingered. Fix = a **site-shape** gate: `token_is_call_shaped(source, point)`
(is the token followed by `(`, skipping whitespace — the C-preprocessor rule
for when a fn-like macro expands) meets the **candidate-shape** `MacroDef.params.is_some()`;
a variant survives iff `call_shaped || m.params.is_none()`. At a parenless site
the fn-like `#define OP(p)` drops out and the token falls through to the typedef
lane LOGOP/UNOP already used; at `OP(node)` it stays. Both directions verified
on perl5 (op.c:180 `OP`→perl.h:3218 typedef; regexec.c `OP(ST.me)`→regcomp.h:485
macro) and in the reduced fixture `gold-corpus/cpp-fixture/macroarb/` (gold rows
`cpp-macro-type-arbitration.json`; RED-confirmed: pre-fix both parenless sites
land on the macro). ws-symbol sub-finding also landed: pack symbols live in the
per-language `module_index` sub-indexes, invisible to the FileStore-only
`workspace/symbol` sweep — added `ModuleIndex::for_each_pack_registered_file`
and taught both the server handler and the CLI `--batch` handler to sweep it, so
a C typedef/class/free-function surfaces in workspace search (gold
`cpp-workspace-symbol.json`; the `OP` typedef Class now appears).
**Residuals (PARKED):** (1) the `#undef`-preceded-redefinition rank signal was
not needed and not added (the shape gate alone resolves the arbitration). (2) gr
symmetry on the typedef token: `resolve_symbol_scoped`'s backward `RefKind::Variable`
lane has no source access, so it can't run `token_is_call_shaped`; gr from a
parenless `OP` still mints the macro `FileScopeValue` target (pre-existing). A
first attempt to gate that lane on `MacroDef.params` alone broke gr for
function-like decl-position macros (`int x ABSL_GUARDED_BY(mu)` — call-shaped in
source but a parenless-looking Variable/Sub phantom in the CST), so it was
reverted; a correct fix needs source threaded into that lane. Left as follow-up.

### B — the first-open degraded window (backend open/index path) → findings 1c, 2, 5c, 6b

**Mechanism:** `did_open` analyzes pack files with `set_gather_cached_only`
(degraded macro gather) and only then lazily kicks
`ensure_workspace_indexed(language)` + a background gather refresh. On perl5
the healed state arrives ~60–90s after the first open; until then, **on both
attempt 1 and attempt 2** (this is not the one-query cold discard): member
completion → global fallback, hover on members → null, gd from uses → null,
references from an open def-site → the def itself only, Mode-B diagnostics
absent. Everything silently self-heals; nothing re-publishes or signals.
An interactive user's first minute in a file — exactly when dogfood notes get
written — sees all of it. CLI probes can never see it (full synchronous
startup), which is why past triage lanes kept "failing to reproduce" these.
**Owning seam:** `src/backend.rs` — `did_open` (945), `spawn_pack_gather_refresh`,
`ensure_workspace_indexed`.
**Slice B:** (i) re-publish/refresh open docs when the gather+index heal lands
(diagnostics already have a refresh hook — extend to a re-analyze of open pack
docs); (ii) prioritize the opened file's include closure in the lazy index;
(iii) surface the window (workDoneProgress or a one-shot info diagnostic), so
degraded answers are at least labeled. Verify by scripting the LSP lane
(probe scripts kept in scratchpad; consider promoting one into e2e).

### C — the `#define`-body member lane is second-class (extraction) → findings 4, 6a, 5-def-hover — **LANDED**

**Status: LANDED** (`fix/macro-member-payload`, EXTRACT_VERSION 157). The
member-block synth lane now mints each member with the SAME payload a plainly-
declared struct field carries: `SymKind::Field`, the pointer `deref_stack`
(peeled by the shared `query_extract::peel` + `C_FIELD_DECL_PEEL` — one deref
walker for both lanes, rule #10), and the `ANNOT_SOURCE` type witness. The
existing renderers then fixed findings 4 + 6a with no adapter change:
`op_p->op_next` hovers `OP*` at both def (op.h:51) and cross-file use (op.c:190),
the def-site reads `*field*` (was `*variable*`), and inlay emits no hint on
BASEOP members. PARKED residual: the def-site type still displays the immediate
alias leaf (`op_type: PERL_BITFIELD16`) rather than chasing to `unsigned short`
in single-file context — same alias-chase gap as any plain field, not lane-
specific.

**Mechanism:** members declared inside a multi-line `#define` (BASEOP) and
remapped back into the macro body get: `SymKind::Variable` with
`package: <MACRO>` (not `Field` of the including struct), **empty
`deref_stack`** (hover/inlay drop `*`), **no working `ANNOT_SOURCE`
suppression** (inlay echoes the literal declared type on every member), and a
degraded def-site hover (`U16TYPE`/*variable* vs the use-site's
`opcode`/*field* + storage overlay). Use-site behavior (refs, gd, member
completion, domain typing) is healthy — only the def-side symbol payload is
lossy.
**Owning seam:** the cpp expansion+remap extraction (`src/cpp_reparse.rs` ↔
`src/query_extract.rs` annot/flow capture — the plain-field query path mints
all three payloads; the remapped lane misses them).
**Slice C:** make the remapped member symbols carry the same payload as plain
fields (deref_stack, ANNOT witness on the symbol's own scope, Field kind /
struct package or at least both packages), then the existing renderers fix
findings 4 + 6a with no adapter changes (`display_type` already renders stars;
inlay suppression already keys ANNOT). Controls in
`docs/hitlist-4-fixtures/` pin the plain-struct behavior.

### D — member-op DX residuals (peel + parity) → finding 1d

**Mechanism:** three verified gaps: (i) DEEP receivers are excluded from Mode B
by design (`expected_member_op` → None), so the one case veesh actually hit
(`OP**`) can never produce the "you need to peel" hint — nothing suggests
`(*op_p)->`; (ii) a dangling `->`/`.` line mid-edit erases the file's other
mismatch diagnostics on the next publish (no MethodCall ref in the ERROR
region); (iii) `pack_diagnostics` is not part of `collect_diagnostics`, so
`--batch`/`--check`/gold cannot observe Mode B at all.
**Owning seam:** `file_analysis.rs::expected_member_op`/`member_op_mismatches`,
`symbols.rs::pack_diagnostics` wiring, and (for the mid-edit erasure) the
extraction's behavior on ERROR-region member refs.
**Slice D (small):** extend the mismatch model with a DEEP verdict carrying the
wrap spelling (`(*p)->`) — show-only diagnostic, no auto-fix, mirroring Mode
A's show-only stance; wire `pack_diagnostics` into the CLI diagnostics path
(parity + makes it gold-testable); keep the mid-edit erasure as a known
degradation unless cheap.

**LANDED** (`fix/member-op-dx2`):
- (i) **peel hint** — `deref_peel(stack, receiver)` (sibling of
  `expected_member_op`, `file_analysis.rs`) computes the wrapped spelling for a
  DEEP pointer chain (`(` + `*`×(pointers-1) + name + `)`), driven off the
  pointer count in the deref stack (rule #10 — the composition, not a name,
  decides; reference-mixed shapes stay silent). `FileAnalysis::member_op_deep_accesses`
  projects it; `member_op_mismatches` + the new query share one
  `for_each_member_access` walk (swap vs peel = disjoint partitions of the flagged
  accesses). `symbols::pack_member_op_peel_diagnostics` emits a show-only WARNING
  (code `member-access-peel`, no `data.operator` → no quick-fix). E2E:
  `deep-peel-diag` in `cpp_member_op.lua`.
- (iii) **CLI parity** — `pack_diagnostics` now runs on every pack-language file
  in the CLI whole-tree pass (`main.rs::enriched_tree_diagnostics` sweeps
  `ModuleIndex::for_each_pack_index` → `for_each_registered_file`), mirroring the
  backend's per-language dispatch. `--batch diagnostics` / `--check` / gold now
  see Mode B (swap + peel). Gold: `fixtures/cpp-member-op-dx.json` (2 rows over
  `gold-corpus/cpp-diag-fixture/memberop.cc`).

**PARKED** (residual):
- (ii) **mid-edit erasure** — the provable subset already survives: a mismatch on
  a prior line in the same function, or in any later scope anchored by an
  intervening `}`, is kept (locked by `cpp_dangling_arrow_keeps_provable_mismatches`).
  The only loss is a mismatch whose receiver **declaration** the dangling
  expression greedily consumes (`q->` eats the following `Box* p;`, so `p` has no
  type in the recovered tree) — its type is genuinely gone, so it is left out
  rather than guessed (per "don't publish wrong diagnostics to keep count"). No
  cheap diagnostics-layer fix: it's tree-sitter-cpp recovery, not a wholesale
  bail. Left as a known degradation.

**EXTRACT_VERSION:** unchanged at 157 — no serialized `FileAnalysis` field added
(`MemberOpPeel` is a transient query result, not cached).

### Polish (fold into A/C or a fifth micro-slice)

- Enumerator hover: show value AND type (`OP_NULL = 0: opcode`). (5a)
  **LEDGERED (still open):** distinct seam from family C — the enum-value
  literal (`= 0`) is not captured on the `Enumerator` symbol at extraction, and
  the hover render branch is the generic `name: type` path, not the macro-member
  Field lane. Needs (i) capturing the enumerator value at extraction (store on
  the symbol/detail), (ii) threading it into the `Enumerator` arm of
  `render_symbol_hover`. Not taken with the family-C payload fix (different seam).
- ws-symbol exact short-name ranking (`OP`). (3 sub-finding)

## In-flight overlap

The parallel branch narrowing the macro-expansion exclusion over `#ifdef`
bodies (Case B) covers **none** of these: finding 3's macro def is
region-active in its own header and the typedef sits in no conditional region
(mechanism = candidate-lane arbitration); families B/C/D are open-path,
extraction-payload, and DX issues respectively. No double-scheduling.

## Proposed slice order (by user pain)

1. ~~**Slice A** — gd-on-`OP` wrong answer (deterministic lie on a core type;
   every navigation from it is poisoned).~~ **LANDED** (`fix/macro-type-arbitration`).
2. **Slice B** — first-open window (the first minute of every session lies
   about references/hover/completion, silently).
3. **Slice C** — macro-body member fidelity (`OP*` hover, inlay noise on
   real-world `#define`-composed structs — perl5's whole OP hierarchy).
4. **Slice D** — peel DX + Mode-B CLI parity (small, unlocks gold coverage).
5. Polish: enumerator hover value; ws-symbol exact-name.

## Probe-method notes (for the next round's briefs)

- The server advertises **FULL** text sync; an incremental-range didChange
  replaces the whole doc with the fragment and darkens every query. Probe
  scripts must send full text (this burned ~one round of false "whole-doc
  darkness" findings before being caught with a control).
- Reduced fixtures under-power the domain-comparison slot (`field_domain`'s
  owner-gated vote needs real usage mass) — and prefix-filtered closure
  completion cosplays as domain ranking. Distinguish by `detail`: slot ranking
  = bare enum name; closure completion = `enum — file.h`.
- CLI diagnostics now include Mode B (slice D iii landed) — `--batch
  diagnostics` / `--check` surface member-op swap + peel for pack files.
