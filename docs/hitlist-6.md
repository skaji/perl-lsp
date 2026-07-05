# Hitlist 6 — perl5 C guts dogfood probe

Probe-only round (root-cause phase). Target: the perl interpreter's C source
(`/home/veesh/personal/perl5`) — `op.c`, `sv.c`, `pp*.c`, `regcomp.c` and the
headers `op.h`, `sv.h`, `hv.h`, `perl.h`, `cop.h`, `opnames.h`, `embedvar.h`.
Base: `spike/cpp-support` tip (`5f84a53f`). Binary: `cargo build --release
--features all-langs`.

## Method / harness

A real developer opens the whole perl5 tree, but that path is the
KNOWN-ASSIGNED cold-start timeout, so probing used a **bounded scratch root**
of ~32 real perl5 C files (the targets above + their headers) copied into a
temp dir. Cold index ~10.6 s / ~795 MB RSS; warm (SQLite) ~0.26 s. All coords
0-indexed, computed from the source line before every query (`scratchpad/col.py`).
Warmup query discarded per fresh root. Two black-box "bugs" turned out to be
coordinate/bounded-root artifacts and were dismissed after re-probing — see
"Dismissed" below. The one primary finding is confirmed by a **self-contained
reduced fixture** (no perl headers), so it is not a harness artifact.

---

## FAMILY A — member-block macros with an anonymous union lose their struct edge  ·  HIGH

**The single most important finding.** All `SV` struct-member navigation in
perl's guts is dark.

### Symptoms (real corpus, bounded root)

| Query | Coord (0-idx) | Expected | Got |
|-------|--------------|----------|-----|
| gd `sv->sv_flags` (read) | `sv.c 390:28` | `sv.h:211` field | **No definition found** |
| hover `sv->sv_flags` | `sv.c 390:28` | `sv_flags: U32` | **No hover info** |
| gd `sv->sv_refcnt` | `sv.c 6646:8` | `sv.h:210` | **No definition found** |
| completion `sv->` | `sv.c 390:28` | sv_any/sv_refcnt/sv_flags/sv_u | **garbage** (macros `PERL_IN_SV_C`, `SV_COW_THRESHOLD`, subs `S_destroy`…) |
| gd `struct STRUCT_SV *s; s->sv_flags` (bypasses the `SV` typedef) | probe file | `sv.h:211` | **No definition found** |
| — control — gd `o->op_next` (OP) | `op.c 324:7` | `op.h:51:10` | `op.h:51:10` ✓ |
| — control — completion `o->` (OP) | `op.c 324:7` | BASEOP fields | full list ✓ |

`SvFLAGS(sv)` (the *macro* wrapper) still resolves and hovers fine — only the
raw `->sv_flags` field drill is dark. So the daily-driver blast radius is
"navigate a struct member directly", which is pervasive in `sv.c`/`pp*.c`.

### Root cause (confirmed by reduced fixtures)

Perl builds `struct STRUCT_SV` by pasting **stacked** member-block macros:
`_SV_HEAD(void*)` (plain fields incl. `sv_flags`) **and** `_SV_HEAD_UNION`
(whose body is `union { char* svu_pv; … } sv_u`). The synthetic base for
`_SV_HEAD` is minted correctly (its `sv_flags`/`sv_refcnt` Field symbols exist
in the index), but the **`package_parents` edge `(STRUCT_SV → _SV_HEAD)` is
never emitted**, so the members are orphaned — unreachable from the struct.
Even a receiver typed *directly* `struct STRUCT_SV *` fails, proving the break
is the struct→member-block edge, not the `SV` typedef and not cross-file
resolution (a plain cross-file typedef+member-block split resolves fine).

Bisection (all reduced, self-contained — see `hitlist-6-fixtures/`):

| Fixture | Shape | gd member |
|---------|-------|-----------|
| `A-PASS-single-memberblock-control.c` | one `_SV_HEAD(void*)` member-block | ✓ resolves |
| `A-PASS-no-union-control.c` | **two** stacked plain member-blocks, no union | ✓ resolves |
| `A-FAIL-union-memberblock.c` | `_SV_HEAD(void*)` + `_SV_HEAD_UNION` (anonymous **union** field), multi-line `\`-continued bodies | ✗ **dark** |

The differential: a member-block macro whose body contains an **anonymous
union field**, when stacked in a struct, defeats the blank-and-reparse in
`plan_member_blocks` — the blanked struct body evidently doesn't reparse as a
clean `struct_specifier`, so `enclosing_aggregate_name` returns nothing at the
`_SV_HEAD` paste byte and no `(struct → macro)` edge is emitted for the whole
struct. (Two plain member-blocks reparse clean and both link.)

### Owning seam

`src/cpp_reparse.rs::plan_member_blocks` — the per-candidate blank + damage
gate (≈3339-3402) and `enclosing_aggregate_name` (3677). The blank/reparse
must survive a union-bearing member-block so the enclosing struct name is
recoverable at each paste site; consumed by
`src/language_driver.rs::inject_member_blocks`.

### Suggested slice

Slice A (HIGH, opus): make member-block edge attachment robust to a
union-bearing (and, generally, brace-bearing) member-block body. Likely the
blank should leave a parse-clean placeholder for the union field, or
`enclosing_aggregate_name` should locate the struct from the *original* tree
(where the tag is intact) rather than the blanked reparse. Encode
`A-FAIL-union-memberblock.c` as a RED xfail gold row; it XPASSes when the edge
forms.

---

## FAMILY B — gd on a field returns the field decl **plus** its inferred type-def  ·  LOW (likely intended)

- gd `(*op_p)->op_type` (`op.c 184:24`) → **two** targets: `op.h:55:21` (the
  `PERL_BITFIELD16 op_type:9` field, correct) **and** `opnames.h:434:3` (the
  `typedef enum … opcode;` name).
- hover `op_type` → `op_type: opcode` (not `PERL_BITFIELD16`).

`op_type`'s type is *inferred* as the `opcode` enum from its assignments
(`o->op_type = OP_CONST`, enum members), which is arguably more useful than the
declared bitfield type. gd then folds goto-type-definition into goto-def,
returning a second target. Not a correctness bug — a UX call on whether plain
goto-def should return only the declaration. Contrast `op_next` (no enum
inference) → single correct target.

**Seam:** witness-bag field-type inference + the gd type-def folding in
`resolve.rs`. **Slice:** low priority / won't-fix pending a product decision.

---

## FAMILY D — references/gr undercounts uses inside `#define` bodies  ·  MEDIUM-LOW

Calibration vs grep (over the same 32-file set):

| Symbol | tool refs | grep truth | note |
|--------|-----------|-----------|------|
| `op_next` (field) | 85 | 134 (`->op_next`) | ~37% miss |
| `SvFLAGS` (macro) | 278 | 296 (`\bSvFLAGS\b`) | ~6% miss |

The systematic miss is uses that occur **inside `#define` macro bodies** —
tree-sitter models a macro body as one opaque `preproc_arg`, so a
`->op_next` / `SvFLAGS(...)` buried in another macro's definition isn't seen by
the code parser (hence the much larger op_next gap: field drills are heavy in
macro bodies like `OP_NAME`/`cUNOPx`). The nested-macro scan in `cpp_reparse.rs`
(`collect_macro_body_uses`) already recovers macro-**name** uses inside bodies;
it does not recover field/identifier member uses.

**Seam:** `src/cpp_reparse.rs` macro-body use scan. **Slice:** medium-low —
counts already find the vast majority; the miss is macro-internal only.

---

## Dismissed (coordinate / bounded-root artifacts — recorded so the next round doesn't re-chase)

- **PL_* globals "dark" — FALSE ALARM.** `PL_curcop`/`PL_op` first looked
  unresolvable, but that was because `embedvar.h` wasn't in the bounded root.
  With it indexed, `gd PL_curcop` (`op.c 1401:8`) → `embedvar.h:61` and hover
  shows `#define PL_curcop (vTHX->Icurcop)`. Per-interpreter PL_ vars resolve
  fine. Minor real note: gd lands on the `#if defined(MULTIPLICITY)` variant
  only. `PL_ppaddr` still didn't resolve, but its (generated) declaration file
  wasn't in the root — unconfirmed, not filed.
- **`op_flags` "No definition found" at `op.c 892:7`** — the coordinate is
  inside a `=cut` apidoc **comment** block (example code, lines 883-897). Real
  `o->op_flags` reads (`op.c 948:20`, `1096:17`) resolve to `op.h:63` fine.
- **`sv_flags` RHS "wrong target"** — cursor was on the receiver `nsv`, which
  correctly returned `nsv`'s parameter declaration. Not a field bug.

## What works well (baseline, so a regression here is a real loss)

OP struct member gd/hover/completion (`op_next`, `op_ppaddr`, `op_type`,
`op_private`, full `o->` completion); macro gd/hover (`SvFLAGS`); enum members
(`OP_NULL`, `OP_CONST` → `opnames.h`); `embed.h` function-delegation gd
(`op_free` → `Perl_op_free` + the `embed.h` `#define`); per-interpreter PL_
globals via `embedvar.h`.

---

## Matches assigned work (do not re-file)

- **op.c cold-start / first-open TIMEOUT** — not hit; probing used a bounded
  32-file root (cold ~10.6 s, warm ~0.26 s). Not re-filed.
- **Memory footprint** — peak RSS ~795 MB indexing 32 C files; consistent with
  the assigned footprint item. Noted, not re-filed.
- **op.c:633 pTHX_ salvage / macro-salvage localization #1 residual** — not
  exercised directly. Family A (member-block edge for union-bearing macros) is
  *macro-salvage-adjacent* but a distinct mechanism (edge attachment, not the
  localization residual); flagged here for the owner to de-dup, filed as its
  own slice.

## Fix-slice breakdown (by daily-driver pain)

1. **Slice A (HIGH, opus)** — member-block edge attachment robust to
   union/brace-bearing member-block bodies. Fixes all SV/body-struct member
   navigation. `cpp_reparse.rs::plan_member_blocks`. Repro:
   `hitlist-6-fixtures/A-FAIL-union-memberblock.c`.
2. **Slice D (MEDIUM-LOW)** — references coverage of field/member uses inside
   `#define` bodies. `cpp_reparse.rs` macro-body scan.
3. **Slice B (LOW / product decision)** — whether gd-on-field should return
   only the declaration (drop the folded type-def target).
