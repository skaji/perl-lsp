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

## Rounds 5–8 — 2026-07-15 — post-fixing-round (tip 2bdf57e) — 4 cores / 15 GB

Four rounds on the fixed binary. FIXED-BY verdicts (medians r5–8 vs r2–4):

| finding | before | after | status |
|---|---|---|---|
| cpp first-edit-after-cold-open | 24.0 s | **195 ms** | FIXED-BY 622361b (cached-only change path + background heal; fork: fast-degraded-now, option B ledgered) |
| abseil warm references | 1.62 s | **45 ms** | FIXED-BY aad409d+2bdf57e (row-narrowed sweeps; PERL_LSP_REFS_NARROW=0 kill-switch; answers byte-identical) |
| bugzilla warm outline null (WaitPolicy) | 403 ms + null | **730 ms + full 53 KB outline, every round** | FIXED-BY f988b52 (Complete wait; honesty costs ~330 ms) |
| rename missing index wait | partial edits possible | Complete wait | FIXED-BY f988b52 |
| C goto-def stops at prototype | header only | **defining TU first + prototype** (redis/curl; +2 gold rows) | FIXED-BY 498d2da (qualified-path residual forked) |
| `$self->` on `use base` | 1 item | **full method surface** (1 → 1574 in `update`) | FIXED-BY e904e7d (identity-over-rep; 2 ctor-gap sites forked) |
| bugzilla warm refs-check | 91 ms | 15 ms | rode the row narrowing |
| curl/mojo warm references | 112 / 19 ms | 10 / 2.4 ms | rode the row narrowing |
| warm settled RSS | 188/274/171 MB (bz/absl/curl) | **159/105/122 MB** | narrowing removed sweep rehydration storms |

### Costs of honesty (designed, fork-reviewable)
- abseil COLD references now ~27 s: `WaitPolicy::Complete` blocks until the
  873-file index lands instead of serving the old 402 ms PARTIAL (3.6 KB)
  answer. The fork's "Discussion needed" now has its concrete price; LSP
  progress reporting for the wait is the obvious follow-up.
- bugzilla open→outline 730 ms warm (was fast-null).

### New characterization: the curl server-context under-answer
Server-mode references on curl answer **4 sites where the CLI answers
155** — warm-deterministic, and it PREDATES the fixing round (rounds 2–4
warm was constant at the same 866 B; only cold occasionally hit the full
34 KB). Eliminated today: NOT row narrowing (identical with it off), NOT
candidate retrieval (17 candidates, byte-same as CLI), NOT rehydration
(strict-residency clean), NOT the relational block's view (whole_present).
Remaining suspect: the OPEN doc's cached-only build mints a weaker pack
target (identity/def_paths) than the CLI's fully-gathered staging, so the
matcher rejects most candidates. Evidence attached to the answer-honesty
fork entry; `PERL_LSP_REFS_DEBUG=1` prints the per-query key/candidate
counts for the next session's repro.

### Residual watch-list
dragonbox template knownweak (unchanged, tracked); fmt warm header-revert
~650 ms asymmetry; redis warm goto-def returns def-only while cold returns
def+prototype (CLI shows both, correct order — wobble, not defect);
bugzilla warm hover still occasionally null under Interactive policy (by
design — the fork's per-verb table is the redirect point).

## Spot check — 2026-07-15 — big-header outline post-WaitPolicy (tip 0485ef9)

Targeted re-verification of the rounds-1–4 "outline null on big headers"
finding, redis `server.h` (fresh shallow clone, quiet box): outline
returns the FULL 752 KB symbol tree in ~30 ms on the first pull, cold
(ready 11.8 s) and warm (ready 1.1 s) — `WaitPolicy::Complete` on
documentSymbol closed the window (bugzilla `Bug.pm` showed the same in
rounds 5–8: 52,882 B every round). Blocking Complete waits now also
surface as LSP work-done progress once they exceed 500 ms
(`bounded_wait_with_progress`), so the honest block is visible in-editor
instead of reading as a hang.

## Residual closed — 2026-07-15 — the ctor-gap 2/60 (tip 900b335)

The invocant fork's residual (`my $self = $class->new(...)` through a
cross-file base ctor → `$self` untyped) was a bug, not a fork: the
receiver-polymorphic ctor machinery existed but the statement/assignment
bless forms never reached it. Fixed (`push_receiver_bless_witness` +
receiver threading through the Variable hop, EXTRACT_VERSION 166).
Verified on real Bugzilla: goto-def on `$self->id` right after
`my $self = $class->new($param)` in `Bug::check` resolves to
`Bugzilla::Object::id` over five same-named decoy `sub id`s, cross-file
through `new` → `new_from_hash` → statement bless. Gold 436/17/0/0/0
(two new substrate rows lock the post-bless hover typing).

## Residual closed — 2026-07-16 — curl server-vs-CLI references (degraded-open window)

The warm-deterministic 4-vs-155 references undercount was the
DEGRADED-OPEN window: did_open's cached-only pack build answers until
the background full-gather heal lands, and the bench's back-to-back
open→references always asked inside it (immediate ask 826 B; the same
ask 15 s later 32,665 B — the full warm answer). Fixed: `degraded_open`
marks the window, `await_open_full` holds references/rename/
implementations (Complete policy) until the heal lands — 280 ms warm on
curl for the byte-identical full answer; cold pays the gather with
work-done progress visible. Outline/hover/completion stay fast-path
(no cross-file read). Server and CLI now agree.
