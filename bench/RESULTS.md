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
