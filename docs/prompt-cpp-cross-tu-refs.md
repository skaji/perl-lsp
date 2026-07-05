# C/C++ cross-TU `references` — investigation & verdict

**Status: NO BUG. The reported undercount was a coordinate off-by-one in the
CLI repro (1-based positions fed to a 0-based CLI). Cross-translation-unit
`references()` works — including test and benchmark TUs, the differentiator
we claim over clangd.**

## The report

A warm-CLI measurement claimed cross-TU references undercounts for C/C++:

- `StrCat` (free fn) at `str_cat.h:581:34` → 25 refs across only 2 files
  (str_cat.h + str_cat.cc), missing ~91 textual users (43 `_test.cc`).
- `Mutex` (class) at its decl `mutex.h:163:48` → **0 refs**.

The tension: the user's *live editor* session on the same corpus DID surface
test + benchmark usages. So the CLI and the editor disagreed.

## Root cause — none of (a)–(e); a measurement artifact

`perl-lsp --references <root> <file> <line> <col>` takes **0-based** input
where **`col` is a byte offset** (only the printed output is 1-based). This is
documented in `--help` ("Positions: `<line> <col>` input is 0-based") and is
the convention every gold fixture row is authored in (rows carry `"line": 0`).
The repro coordinates were 1-based (`581:34`, `163:48`), so every query landed
one row below the intended token:

- `str_cat.h` line 581 (1-based) is the **1-arg** `StrCat(const AlphaNum&)`
  overload; row 581 read as 0-based is line 582 (blank / a different overload).
- `mutex.h` line 163 (1-based) is `class … Mutex {`; row 163 read as 0-based is
  line 164 — inside the class body, off the `Mutex` token → nothing resolves →
  **0 refs**.

The editor sends the real 0-based cursor position, so the live session
resolved correctly and surfaced the whole tree. **CLI and LSP never diverged**
— the analysis path is identical (`resolve::resolve(cursor) → CandidateSet`,
`references()` projection, `refs_to` with `RoleMask::VISIBLE` over the
workspace-walked pack index, include-closure visibility gate). The only
difference was the hand-typed coordinate base.

### Measured with correct 0-based coordinates (abseil-cpp, warm)

| symbol | position (0-based) | refs | distinct files | test files |
|---|---|---|---|---|
| `Mutex` (class) | `mutex.h` 162:47 | 230 | 37 | 11 (+1 benchmark) |
| `StrCat` (free fn) | `str_cat.h` 580:33 | 348 | 70 | 41 |

Textual `grep -w` counts: Mutex 48 files (12 `_test.cc`); StrCat 93 files
(43 `_test.cc`). The LSP reaches 37/48 and 70/93 respectively — and **every**
covered file is a real code reference. Both StrCat overload decls (1-arg,
2-arg) return the identical 348/70 set: references is name+owner-keyed, it does
NOT narrow per signature.

### The residual (textual-minus-found) is comment noise, not a gap

The 11 textual-only Mutex files name `Mutex` exclusively in **comments** or
prose — e.g. `const_init.h` (`//   ABSL_CONST_INIT absl::Mutex global_mutex`),
`barrier.cc` (`// … released the Mutex …`), `thread_identity.h`
(`// Used by the implementation of absl::Mutex`). The LSP correctly excludes
comment mentions; it is *more* precise than textual grep, not undercounting.

## Why (a)–(e) were all ruled out

- **(a) def-anchored gr not reaching non-open workspace files** — refuted:
  `refs_to`'s `RoleMask::VISIBLE` walk covers OPEN ∪ WORKSPACE ∪ DEPENDENCY;
  pack workspace files ride the DEPENDENCY role via `for_each_cached_file`
  (`CandidateSet::pack_routed` → `target_visibility` = VISIBLE).
- **(b) reverse-ref index not populated for pack langs** — N/A: there is no
  separate reverse-ref index; `refs_to` sweeps every cached file per role and
  matches with the include-closure gate (`file_sees_target`).
- **(c) enrichment gating** — irrelevant to references: identity + the
  cross-TU walk don't depend on OPEN-only enrichment.
- **(d) name-only vs qualified identity mismatch** — refuted: namespaced free
  fns (`pkg::Combine`), classes (`pkg::Mutex`), and members (`Mutex::Lock`) all
  resolve and gr symmetrically from decl and use anchors.
- **(e) the CLI path specifically** — refuted: CLI and LSP share the exact
  resolution/projection code; only the typed coordinate base differed.

## Regression net

`gold-corpus/fixtures/cpp-cross-tu-refs.json` (6 `gold` rows) over the hermetic
multi-TU fixture `gold-corpus/cpp-fixture/multitu/` (`strjoin.h` declaring a
namespaced free fn + class + method; `strjoin.cc` impl; `app.cc`/`other.cc`
consumers; `strjoin_test.cc` a test TU). The rows lock, from BOTH decl and use
anchors:

- free-fn / class / method references reach every including TU **incl. the
  test TU** (the clangd differentiator);
- the class/method sets **exclude `other.cc`**, which includes the header but
  never uses `Mutex`/`Lock` (visibility gate admits the closure; the matcher
  keys on the name — precision, not just recall).

No source change: nothing in the analysis was wrong. The one real wart is the
CLI's 0-based-input / 1-based-output split (help-documented, fixture-pinned);
unifying it is deliberately out of scope here since the fixtures pin both forms.
