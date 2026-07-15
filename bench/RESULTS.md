# edit-bench results ledger

Append-only: one section per round, newest last. Protocol and honest-reading
traps: `.claude/skills/edit-bench/SKILL.md`. Driver: `bench/lsp_bench.py`.

## Round 1 — 2026-07-14 — commit 2ad34e8 (v0.7.0 spike tip) — 4 cores / 15 GB

| project | files | cold ready | warm ready | cold peak RSS | warm settled RSS |
|---|---|---|---|---|---|
| bugzilla (Perl) | 194 .pm | 2.5 s | 1.2 s | 274 MB | 183 MB |
| abseil (C++) | 873 .cc/.h | 1.2 s¹ | 1.1 s | 216 MB | 265 MB |
| redis (C) | 216 TUs | 12.8 s | 1.1 s | 490 MB | 315 MB |

¹ cpp/c "ready" = first-file interactivity; the bulk index continues after.

Warm navigation (steady state): hover 0.6 ms (perl, after first) / 2.2 ms
(cpp) / 23 ms (c); cross-file goto-def 0.7 / 4.4 / 31 ms; references
worst-case 96 ms (92 refs) / 1.6 s (54 sites) / 640 ms (~250 sites);
member completion 20 / 8 / 20 ms.

Edit→diagnostics push: body edit ~620 ms (perl, 5.1k-line file) / ~197 ms
(cpp) / ~635 ms (c). Contract/header edit ~790 / ~260 / ~140 ms — redis's
server.h (included by ~every TU) diagnoses in 140 ms and post-invalidation
hover stays 4–6 ms: the Surface freshness gate holding at its worst case.

### Findings

- **NEW P1: cpp first-edit-after-cold-open → 26.2 s to diagnostics.**
  didOpen builds cached-only; the first didChange pays the full cross-file
  gather synchronously (warm: 197 ms). The worst number in the matrix.
- **NEW P1: cold answers silently partial while the index builds.**
  abseil cold references: 3.6 KB result vs 12.5 KB warm at the same
  position — looks complete, isn't. Absence-as-answer's little sibling.
- **NEW P2: documentSymbol null on big files at open** — Bug.pm (5.1k
  lines) even WARM; redis server.h both runs. The 400 ms bounded wait
  expires and the response carries null (editors heal via refresh nudge).
- **NEW P2: C goto-def stops at the header prototype** — never reaches
  the defining TU (lookupKeyReadOrReply → server.h, not db.c). Macro
  goto-def and struct-member hover/completion are correct.
- **NEW P2: `$self->` completion nearly empty on `use base` classes**
  (1 item in Bugzilla::Bug) while bareword `Bugzilla->` completes 46.
- **NEW P3: abseil member-resolution bugs** — private inline static
  (`IsInlined`) → no definition; `ToString` at status.cc:175 → wrong
  definition (lands in the enclosing operator<< signature).
- **NEW P3: perl body-edit diagnostics ~620 ms on a 5.1k-line file** —
  the synchronous per-change rebuild is the typing-responsiveness ceiling
  on big modules.
- Note: perl first-hover cold 3.0 s (on-demand enrichment of the
  receiver's module chain), then sub-ms.

## Rounds 2–4 — 2026-07-14 — commit 2ad34e8 (+bench harness) — 4 cores / 15 GB

Corpus doubled: + mojo (Perl/framework, root=lib, 112 files), fmt (C++
templates, 72 files), curl (C, root=lib, 380 TUs). Three full rounds
(cold+warm × 6 projects); medians below over rounds 2–4.

### Startup + RSS (cold ready min–max across rounds; warm settled median)

| project | cold ready | warm ready | cold peak RSS | warm settled RSS |
|---|---|---|---|---|
| bugzilla | 2.4–6.8 s | ~1.1 s | 248–297 MB | ~188 MB |
| mojo | 1.4–2.7 s | ~0.9 s | 109–121 MB | ~90 MB |
| abseil | ~1.1 s¹ | ~1.0 s | 206–217 MB | ~274 MB |
| fmt | 5.5–7.6 s | ~0.8 s | 292–331 MB | ~266 MB |
| redis | 11.6–12.8 s | ~1.0 s | 472–492 MB | 322–352 MB |
| curl | 14.0–14.2 s | ~1.1 s | 321–408 MB | ~171 MB |

### Warm navigation medians (steady state)

| verb | bugzilla | mojo | abseil | fmt | redis | curl |
|---|---|---|---|---|---|---|
| hover | 333 ms² | 0.3 ms | 2.3 ms | 1.9 ms | 19 ms | 15 ms |
| goto-def x-file | 0.5 ms | 0.4 ms | 4.3 ms | 17 ms | 31 ms | 2.9 ms |
| references | 91 ms | 19 ms | 1.62 s | 202 ms | 634 ms | 112 ms |
| member completion | 16 ms | 14–24 ms | 9 ms | 1.2 ms | 22 ms | 4.8 ms |
| body edit → diags | ~530 ms | ~50 ms | ~193 ms | ~203 ms | ~605 ms | 112–404 ms³ |
| contract/header edit | ~660 ms | ~64 ms | ~253 ms | ~271 ms | ~91 ms | ~217 ms |

¹ first-file interactivity; bulk index continues.
² Bug.pm `Bugzilla->dbh` hover only — on-demand enrichment; mojo's typed
  `has`-accessor hovers are 0.3 ms. See finding below.
³ curl body edits bimodal (first ~110 ms, subsequent ~400 ms).

### Finding updates

- **CONFIRMED P1 (stable): cpp first-edit-after-cold-open ≈ 24 s.**
  abseil body-edit-1 cold: 23.9/24.0/24.7 s across rounds — deterministic,
  not load noise. Warm: 193 ms.
- **CONFIRMED+WIDENED P1: partial/absent answers around index-build and
  enrichment windows.** New instances via the SIZE-VARIES column:
  bugzilla COLD hover 4 B (null!) vs 163 B across rounds; cold completion
  233 B vs 5.5 KB; curl cold references 866 B vs 34 KB; and — notably —
  bugzilla WARM open outline 4 B vs 53 KB and WARM hover 4 B vs 163 B.
  Not exclusively a cold problem.
- **NEW: abseil warm references 1.6 s stable** (54 sites) — vs redis 0.63 s
  for ~250 sites and curl 0.11 s for 155. The cpp references sweep cost is
  not proportional to result count; worth a profile in the fixing round.
- **Framework Perl is the speed king**: mojo `has`-accessor hover/def
  0.3–0.4 ms, `$self->` completion 77 typed items (14 ms), `has` contract
  edit 64 ms. The `$self->` weakness is SPECIFIC to `use base` classes
  (Bugzilla: 1 item) — invocant typing, not completion machinery.
- **NEW: untyped-invocant asymmetry (mojo)** — `$c->render` goto-def
  resolves by name-match but hover on `$c->app` returns nothing.
- **NEW minor: fmt warm header-REVERT 651 ms vs cold 147 ms**; curl
  goto-def→prototype replicated (C-tier pattern; fmt C++ lands on
  definitions, so it's the C path specifically); fmt explicit-instantiation
  template probe (dragonbox) answers empty — knownweak, tracked.
