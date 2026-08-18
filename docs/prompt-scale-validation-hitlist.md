# Scale-validation hitlist — what 122× found, and what it schedules

The validation pass of 2026-08-17. Every performance number this project had
before it came from `crm` — 1,136 Perl files — while the stated target is
monorepos two orders of magnitude larger. This pass measured the gap.

Three instruments ran: a **4.65 h soak** (hour-scale behaviour, previously
unmeasured — the longest prior measurement was 240 s), **Koha** (3.1×), and a
**5,000-dist CPAN sample** (122× — genuinely the target rung). The
**differential sweep** is the one track still owed.

## Status board

One line per item. `CLOSED` = fixed and verified, or closed as not-a-bug.
`PARTIAL` = the half that mattered most is fixed, named residual still open.
Detail for each is in its section below.

### Tier 1
| # | item | status |
|---|---|---|
| 1 | Post-cold-index availability hole | **PARTIAL** — "no answers" fixed `d9053e4f`; writer-drain diagnostics blackout still open, gate split adjudicated in PR #121 |
| 2 | Fatal stack overflow on deep CSTs (P0) | **CLOSED** — depth gate `fed8ac00`, then the recursion class itself removed (PR #123): descent is queued, cap 500 → 100,000, measured |
| 3 | `references` terminal at scale | **OPEN** — cpan5k still never returns after every fix; peak RSS 7.16 → 4.55 GB across three of them. Bottleneck now measured as enrichment fan-out, not the walk (66 candidate views) |
| 4 | Completion payload unbounded | **CLOSED** `b6312ea2` — 7.29 MB/236 ms → 55.9 KB/4 ms |
| 5 | Every CLI verb hangs at 122x | **CLOSED** PR #125 — **confirmed at 138k: DNF → exit 0 in 350 s** with a real answer. Finite, not fast; the residual is a new row (#6) |
| 6 | Pack indexing dominates startup on a Perl corpus | **OPEN** — new; `cli::index_pack` is 188 s of a 350 s Perl query |

### Tier 2
| item | status |
|---|---|
| `pod.rs` multibyte panic | **CLOSED** `f47c002b` |
| Fold-64 non-convergence | **CLOSED** `fed8ac00` |
| `query_rec` 512-depth cap | **OPEN** — seen again during the #3 probe |
| hover empty on a module-name token | **OPEN** |
| `epoch.gen_stamp_missing = 1074` | **CLOSED** — explained, never a bug |

### Tier 3 / Tier 4
| item | status |
|---|---|
| `@INC` single-provider tier | **CLOSED** (C) — all four stages, PR #122 |
| ~10 bookkeeping `get_cached` sites | **OPEN** — deliberate |
| `cursor_slot.rs:205` | **OPEN** — deferred, reducible |
| Merge the two index families | **OPEN** |
| Iterative builder walk | **CLOSED** PR #123 — both walks byte-agree over the whole suite |
| Grammar-kind tripwire | **IN FLIGHT** (B) — must accept DECLARED future kinds or it eats the forward-compat arms |

### Found en route (not original rows)
| item | status |
|---|---|
| Qualified calls binding to same-named local subs | **CLOSED** `98bf42da` |
| `RUST_LOG` / ghost stats never reached CLI verbs | **CLOSED** `336fc624` |
| `resync_bytes` had no alarm | **CLOSED** `5cf44dfd`, corrected in PR #121 (fired on a designed state) |
| `ResolveQueue` lost wakeup — resolver sleeps for the session | **CLOSED** PR #121 — priority push had no ordering against the drain; an `EXTRACT_VERSION` bump triggered it |
| `PackInvalidator::swap` strips against an unchecked persist | **IN FLIGHT** (A) — with its blocker, a test-mode `open_cache_db` |
| Gold silently skips 22 rows on Debian/Ubuntu arch, still exits 0 | **IN FLIGHT** (B) — hit independently by two sessions |
| 3 fold nondeterminism bugs (unstable cached blob) | **CLOSED** PR #123 — witnesses landed in `HashMap` order, so the same file built twice differed |
| A stale cache hides a fix as readily as it hid the crash | **OPEN** — see below; cost one false gold FAIL during integration |

### Validation
| item | status |
|---|---|
| Narrow seam review | **DONE** — PR #121, nine findings |
| Pack-language soak | **DONE** — 3h20m clean; caveat: ran pre-rows-lane |
| T1 #1 + #3 combination | **DONE** — negative; the row does not close |
| Cold cpan5k with every fix in | **OWED** |
| Differential sweep (main vs branch) | **OWED** |
| Profile the 150 s of refs CPU | **IN PROGRESS** — running locally now |
| Re-soak `PackBagCache` on current tip | **OWED** |


## Corpora

Durable, in `/home/veesh/perl-corpora/`:

| corpus | Perl files | note |
|---|---|---|
| `koha/` | 3,554 (732 KLOC) | 3.1×; the only corpus hitting DBIC **and** Mojo plugin paths together — the right regression corpus, minutes per round |
| `cpan5k/` | 138,822 | 122×; 5,000 random dists from the 44,223 index (list + sample preserved) |
| `pnx-two/` | 2 | the P0 crash repro |
| `quarantine/` | 2 | see caveat |

**Caveat on every CPAN-5k number: two files quarantined** — both XML
documents shipped with Perl extensions, found by a first-char-`<` scan. Rate:
2 in 138,824 files, 2 dists in 5,000.

## Results

| metric | crm (1,136) | Koha (3,554) | CPAN-5k (138,822) |
|---|---|---|---|
| LSP warm ready | 0.81 s | 1.58 s | **1.06 s** |
| post-ready RSS (warm) | ~297 MB | 170 MB | **255 MB** |
| cold bulk index | — | ~9 s | ~10.5 min (4.5 ms/file vs 3.0) |
| `modules.db` | — | 80 MB (22.5 KB/file) | 1.73 GB (**13.9 KB/file — fell**) |
| db rows | — | 656k refs / 123k syms | 12.86M refs / 3.53M syms |
| open (warm) | — | 1.6 ms | 0.9 ms |
| hover/def (warm) | — | 0.2 ms | 402 ms |
| completion (warm) | — | 7 ms / 24 KB | 188 ms / **7.8 MB** |
| references, hot name | ~15 ms | 5.6 s | **120 s TIMEOUT** |
| diagnostics after edit | — | 330 ms | **never (60 s)** |
| CLI one-shot (warm) | 1.33 s | 1.90 s | **DNF, killed 42:32 @ 7.11 GB** |

Soak (crm, 4.65 h, 125 edit bursts): RSS slope **+0.7 kB/h past t=2h** —
h2→h4 byte-identical, `VmHWM` = plateau, no latency drift, perl-hub 37.6 M
lookups at 99.99% hit with zero capacity evictions. **Perl-only**, so
`PackBagCache` was compiled in but never exercised.

## Verdict — the axes point opposite ways

**Storage and startup hold.** Warm ready is scale-free, post-ready RSS is
flat, the bulk walk is near-linear, and per-file db cost *fell* at 39× the
corpus. The FileStore / row-store / eviction architecture does exactly what
it was designed for. This is the tier that makes the target market possible.

**Query paths break**, each for its own reason, and the CLI's one-shot
"act like the LSP just started" semantics are O(corpus) in time and RAM —
which bounds `--check` / `--heatmap` / `--workspace-symbol` / batch as
workspace-scale tools.

---

# Landed against this hitlist

Newest last. Every row was base-verified — the test fails (or the binary
crashes) on the commit before its fix, not just passes after.

| commit | row | what changed |
|---|---|---|
| `f47c002b` | T2 POD panic | char-boundary truncation, shared with `for_path_sniffed` |
| `336fc624` | (found en route) | `RUST_LOG` + ghost stats now reach CLI verbs at all |
| `9d5e1cc0` | T2 `gen_stamp_missing` | closed as explained; not a bug |
| `98bf42da` | (found en route) | qualified calls stop binding to same-named local subs |
| `fed8ac00` | **T1 #2** + T2 fold-64 | depth gate before the recursion; monotone propagator repair |
| `b6312ea2` | **T1 #3** + **T1 #4** | `refs_present` axis reader + rows lane; completion capped at 200 |
| `d9053e4f` | **T1 #1** (the "no answers" half) | no synchronous CPU in a handler — they share one task |
| `fc863769` | — | Tier 1 rows rewritten to measured outcomes |
| (this) | pack soak | `resync_bytes` alarm made permanent |

Verified at the full bar with the cpp feature on, after each integration:
1,511 unit · 491 gold (0 FAIL / 0 XPASS / 0 CRASH) · e2e 113/0 · e2e-cpp 0.

**All four Tier 1 rows have moved.** #2, #3 and #4 are closed; #1 is closed
for the half that made it worst-in-class (no answers) with the writer-drain
diagnostics blackout characterized and deliberately deferred.

**Corpus-scale corroboration.** The 138k cold walk on the fixed binary
produced ZERO `pod.rs` multibyte panics and ZERO fold-64 bails, against 2 pod
panics and 3+ fold bails on every baseline walk. `f47c002b` and `fed8ac00`
ship with single-file tests; this run is the only thing that exercises them at
122x, which makes it the stronger evidence of the two.

Two notes worth keeping:

- **The box was loaded** (four agents, one an hour-scale cpp soak) and both
  e2e suites showed a one-off flake under it — a perl run reporting 113
  passed / 0 failed while exiting 1, and a cpp member-completion race
  returning empty labels. Both clean on rerun, three consecutive times for
  the perl one. Harness timing under load, not a test failure, but it is the
  kind of thing that reads as a regression at 2am.
- **The gold canary for the depth crash needs a cold cache — and it cuts both
  ways.** A warm module cache serves the stored blob and never re-walks the
  tree, so the crash hides and the row passes for the wrong reason. The mirror
  image bit during integration: after the cap moved 500 → 100,000, gold FAILED
  with *two limits live at once* — one file's diagnostic said "the 100000
  limit" and another's said "the 500 limit", while the source held only one
  gate. The second was a cached diagnostic from before the change. Clearing
  that root made it green. **Any diagnostics-shaped assertion is suspect until
  you have run `perl-lsp --clear-cache <root>`**, whether you are trying to see
  a bug or trying to see its fix.

# Tier 1 — blocks the target market

### 1. Post-cold-index availability hole — **"no answers" FIXED `d9053e4f`**
After the bulk walk, a ~10-minute resolve/enrichment phase **wedges every
verb** — opens, hovers, completions all hit the 120 s timeout — at 7.6–8.0 GB
RSS. A warm **restart of the identical state is ready in 1 s at 255 MB**:
restarting currently beats staying up.

Worst-in-class because every other finding is a *slow answer* and this one is
*no answers*, during a first-time user's first ten minutes. Invisible below
~10k files. Same family as the warm-open cascade fixed in `7343ae59`
(background resolution starving the request path) but an order of magnitude
larger and not addressed by that batch's batching gate.

Memory is all anonymous heap (`RssAnon` 7.65/7.69 GB); work during the window
added +286 MB — mild live growth, not clean reuse. The live-vs-allocator
split was **not** cleanly separable because the background phase never went
quiet; the availability hole is the sharper finding.

**Root cause — and it is a property of the runtime, not of `did_open`.**
tower-lsp 0.20's `serve()` polls the stdin reader and every handler future
inside ONE joined task (`buffer_unordered` is concurrency *within* the task,
not across threads). Any synchronous CPU in any handler therefore stalls every
other handler and the message reader until it yields. `did_open` ran
`enrich_open` synchronously in its handler: 344 s of CPU for one `Dancer2.pm`
against the 138k index, answering nothing meanwhile. Smoking gun: `def-d2`
returned at the *exact millisecond* Dancer2's diagnostics published, having
had no work of its own to do. Lock contention was ruled out — busy threads were
R-state in decode, idle workers parked in futex normally. The standing rule
(**no synchronous CPU in a handler future, ever**) now lives in
`src/lsp/backend/query.rs`'s module doc.

Measured, cpan5k cold: hover 120,000 ms TIMEOUT → 0.7 ms; def 44,498 ms →
0.3 ms; recovery instant vs +1,471 s. Baseline at load 1–8, after at 7–32 on
20 cores — only categorical results banked; the 691 s → 612 s gate-open delta
is **not** claimed.

**Still open — the writer-drain window.** Diagnostics stay deferred for the
~7–9 min the single-threaded persist writer drains its unbounded backlog
(~7 GB RSS), and Complete verbs still wait their 120 s cap in it. Now
honestly announced ("Saving index to cache…") instead of a silent 100%.
The gate cannot simply open at walk end: stripped fresh copies register only
post-commit, so an evicted copy without a committed blob rehydrates to
wrong-empty and rows-based queries would be silently partial. The fix is a
`attached` / `durable` gate split plus worker-time registration behind a
pending-blob overlay — it touches exactly the residency seams the narrow-seam
review still owes, so it waits for that review rather than being rushed here.

**Also handed off** (resolve/enrichment lane, not availability): three heavy
opens cost 279 enrichments / ~1.77M blob decodes / 730k `cycle_declines`.
Roots are the package→SET-of-files candidate relation at 122x (5–12 declaring
files for a common name), transitive overlay fan-out, and Perl's still-empty
`ScopedLookup` slot (T3). This is why one doc's diagnostics take 68 s — real
cost, but background cost now.

### 2. Fatal stack overflow on deep CSTs — P0 — **FIXED `fed8ac00`**
The builder's `visit_node` → `visit_children` → `visit_function_call` walk
recurses once per CST level. A 50 KB XML-as-`.pm` yields ~2,200 levels and
overflows a 2 MB rayon worker stack: **fatal abort of the whole server, and
`catch_unwind` cannot catch a stack overflow** — the per-file safety net has
a hole exactly here. One copy survives on the 8 MB main stack; two crash, so
in the wild it is scheduling-dependent and will present as flaky.

P(a corpus contains one generated/XML/deep `.pm`) → 1 with size.

Repro: `~/perl-corpora/pnx-two/`. Fix: **a parse-depth gate before build**
(must run before the recursion, since unwinding cannot help); an iterative
walk removes the class but is a core-traversal rewrite and belongs in its own
arc. A gold fixture of the two-copy dir is a free crash canary — `run.pl`
already hard-fails aborts.

### 3. `references` terminal at scale — **OPEN.** Koha fixed (`b6312ea2`); cpan5k still DNF
120 s DNF at 122×; 5.6 s at Koha. Root-caused by controlled A/B:
`PERL_LSP_NO_EVICT=1` collapses the walk 5,613 → 1,357 ms (repeat
3,647 → 842 ms). **~4× of the cost is blob decode of evicted candidates, not
matching** — true match cost is 0.85 s for 585 candidates / 1,660 sites.
Candidates scale with corpus for common names (`store` = 585, a rare name = 1).

The readers are `bag_present`, `symbols_present`, `whole_present` — **there is
no refs-axis reader**, so the backward walk takes the all-axes gate and pays a
full decode per candidate. This is `814bc0dc`'s `symbols_present` fix one axis
over (that one took decodes 29,988 → 182).

NO_EVICT is *not* the fix: whole-copy residency was 977 MB at 3.5k files,
≈28 GB at 100k.

**What landed, and the correction.** `refs_present` serves resident when refs
and symbols survived and rehydrates otherwise, through a rows lane that
retains bag-stripped copies (the bag is 52% of a Koha analysis's heap, so the
same 128 MiB budget caches ~2x denser). Koha `store`: 5,493 → 3,362 ms cold,
~8,000 → ~4,300 decodes, RSS 852 → 657 MB, answers byte-identical.

A naive bag-strip **loses 106 sites** at Koha, because a chained
`->set(..)->store` re-derives its invocant through the candidate's own bag.
The matcher therefore runs on a view that upgrades per file to whole when a
ref's verdict isn't baked — over-approximating on purpose, since a wrong
upgrade costs one decode and never an answer.

**The cpan5k attribution above was wrong.** Decode-per-candidate was the real
root cause at Koha, but at 122x the walk is already **181 ms** (91 candidates)
and the 120 s DNF is not the walk.

**MEASURED, and my prediction was wrong: the combination does NOT close it.**
With both `b6312ea2` and `d9053e4f` in, `references` on a hot name at warm
cpan5k **still never returns.** The probe deliberately set a 150,000 ms client
timeout — *above* the server's own 120,000 ms cap — so a cap expiry would be
distinguishable from a true non-answer. Nothing came back, six times. So this
was never only the wait policy expiring.

It is not a deadlock either: ~294% CPU throughout, RSS climbing 97 MB → 3.4 GB
and plateauing. Real work that never finishes. Repeats are not cheap — each of
five repeats burned the full 150 s, the first three adding ~650 MB each, so the
"memory grows and buys nothing" characterisation stands unchanged.

**The Koha control is what makes this trustworthy**: same binary, driver,
protocol and coordinates gave 3,328 ms and a **byte-exact 284,617-byte** answer
against the prior 3,362 ms / 284,617 B. The rig is good; the cpan5k DNF is a
property of cpan5k.

So the residual is neither the refs axis nor the wait policy. It points at the
same place row #1 handed off: the candidate explosion and enrichment fan-out at
122x — the package→SET-of-files relation returning 5–12 declaring files for a
common name, transitive overlay enrichment, and Perl's still-empty
`ScopedLookup` slot (T3). **Row #3 stays open**, now with a well-posed next
step: profile where those 150 s of CPU go, LSP path, warm, hot name.

Related, same root: **repeat refs never cache-hit** — RSS plateaus
(566→635 MB over 6 identical queries, bounded, not a leak) while latency stays
~3.4 s. Capacity thrash; memory grows and buys nothing. `refs_present` makes
it moot — no decode, nothing to cache.

### 4. Completion payload unbounded — **FIXED `b6312ea2`**
7.8 MB / ~50k items per keystroke (21.3 MB in the post-cold state). The
workspace/in-scope tier has no scale cap. Broken at any size; invisible below
~10k files.

Capped at 200: narrowed by the typed prefix first, then ranked by the client's
own sort key before the cut, so the in-scope and imported tiers survive and
the auto-import firehose is what goes; `isIncomplete` makes the client
re-query as the prefix grows. **7,289,367 B / 236 ms → 55,853 B / 4 ms.**
Under the cap nothing changes at all — an ordinary file's list is untouched.

### 6. Pack indexing dominates startup on a Perl corpus — NEW

Found confirming #5. `PERL_LSP_PHASE_TIMING` on a warm `--definition` at 138k:

```
  cli::index_pack        188,173 ms   <- 54% of the run
  cli::index_workspace   145,934 ms
  cli::resolve_imports    12,098 ms
  cpp.transform           32,083 ms   (single worst file; many more in the 5-12 s band)
```

The corpus is "Perl", but CPAN dists ship XS: **10,834 C/C++/XS files** (4,680
`.c`, 4,301 `.h`, 755 `.xs`, 551 `.cc`, 357 `.cpp`, 190 `.hpp`) against 138,822
Perl files. So 7.8% of the files take 54% of the time — C++ costs ~17 ms/file
against Perl's ~1 ms, because header expansion (`cpp.transform`) is expensive.

**For a Perl query none of that work can affect the answer.** Perl analysis
never consults pack data; the XS `.c` beside a `.pm` is not what `--definition`
on a Perl symbol resolves through. The server defers workspace indexing to the
first `didOpen`; the CLI eagerly indexes *every* language it serves.

The fix should not branch on "is this a Perl query" (rule #10). The property
wanted is "index the languages this query can consult" — or match the server
and make pack indexing lazy until a pack file is actually opened.

### 5. `cli_full_startup` never reaches queryable state at 122x — CLOSED
Found while probing row #3. Every CLI verb hangs at 138k files: `--references`,
`--definition` and a rare-name query (8 occurrences) all DNF at exit 124, zero
bytes, ~100% CPU on ONE thread, 1.5–2.0 GB. Verb-independent and
candidate-count-independent, so it is stuck **before** the query runs.

**The 793 ms comparison does not mean what it looks like.** The LSP server
and the CLI run the SAME warm lane and the SAME indexer — `initialize`
simply does neither. It sets the workspace root (waking the resolver
thread) and returns capabilities; the workspace index is lazy, fired by the
first `didOpen` onto `spawn_blocking`, and `@INC` resolution is on-demand
on the resolver thread. So 793 ms is "accepting requests", while
`cli_full_startup` returning is "queryable" — the CLI pays synchronously
what the server defers, and a verb cannot answer early because there is no
partial answer to give.

What made that synchronous cost unpayable was two quadratic terms in the
index build, neither CLI-specific — **the server pays them too, in the
background, and its index takes just as long to become complete.** The CLI
is what made them visible.

1. **`push_unique`'s bucket scan** (`ModuleEdgeIndexes::feed`). Reverse-index
   buckets were `Vec<String>` with a linear membership scan per insert, so a
   bulk feed cost O(bucket²). The worst case is also the common one: `new` is
   declared by every module in the workspace, so its bucket IS the workspace.
2. **`purge_module`'s full sweep.** Removing one module's edges scans every
   bucket of every map, and `rebuild_name_registration` calls it once per
   registered package name — O(names × buckets) over a bulk index.

Measured on a synthetic workspace (`--definition`, warm, per-phase via
`PERL_LSP_PHASE_TIMING`; every other startup phase stays under 40 ms):

| files | `index_workspace` | `rebuild_reverse_index` | total |
|---|---|---|---|
| 1,000 | 927 → 877 ms | 22 → 15 ms | 1.00 → 0.95 s |
| 2,000 | 1,477 → 1,108 ms | 83 → 31 ms | 1.66 → 1.20 s |
| 4,000 | 2,682 → 1,637 ms | 560 → 90 ms | — → 1.82 s |
| 8,000 | 8,853 → 2,763 ms | 4,108 → 236 ms | 15.41 → **3.03 s** |

`rebuild_reverse_index` grew as ~n^2.5 (190× for 8× the files) and is now
linear; the whole startup is 5.1× faster at 8k and no longer superlinear.
Each term was isolated by ablation before being fixed — removing the
uniqueness check alone took 8k from 4,108 → 137 ms, and skipping the purge
alone took `index_workspace` from 9,840 → 2,601 ms.

Not verified at 138k (no corpus here). Linear extrapolation from 8k puts the
two phases in the tens of seconds rather than the hours the old curve implies,
but the claim that matters is the exponent, not the constant.

Until confirmed at scale, `--check` / `--heatmap` / `--workspace-symbol` /
`--dump-package` should still be treated as unproven at workspace scale, and
**the CLI is not yet a validated measurement fallback there** — a trap for
anyone reaching for the cheap probe.

(The probe's first write-up blamed a CPU grind in the refs walk; the rare-name
and `--definition` controls refuted that and the retraction is in its log. The
row #3 conclusion rests only on the LSP measurement.)

# Tier 2 — cheap, and now debuggable

Each of these was anonymous until `3fef0120` added breadcrumbs; all now have
named inputs.

- ~~**`src/build/pod.rs:20`**~~ — **fixed, `f47c002b`.** `result[..2000]`
  byte-sliced inside a multibyte char. Victims:
  `Test-BDD-Cucumber-Definitions-0.38/-0.39 lib/.../Base/Ru.pm` (Russian POD).
  Caught per-file, so the file's analysis vanished silently — not a crash, a
  disappearance. The rule now lives once, in
  `util::text::truncate_on_char_boundary`, shared with `for_path_sniffed`
  (which had the correct spelling all along; two spellings of one rule is how
  the wrong one survives). The regression test sweeps a byte-shift: the first
  version of it passed with the bug fully present, because whether the cap
  straddles a character depends on alignment.
- ~~**Fold-64 non-convergence**~~ — **fixed, `fed8ac00`.** All three offenders
  (`Module-Generic`, `Config-Universal`, `File-stat-Extra`) were period-2
  oscillations on tag `call_binding`. Root cause worth remembering:
  **clear-and-emit is only sound when re-derivation does not depend on the
  pass's own output.** In a recursive cluster the propagator's own published
  witness is what resolves the recursive return arm, so clearing it
  un-resolved the arm, dropped the answer, and prevented the re-push — flip
  forever. CLAUDE.md's worklist invariant states clear-and-emit as
  unconditional; it now has exactly one known exception, and this is it.
- **`query_rec` 512-depth cap hit** on `MethodOnClass` — cross-dist
  class-name collisions make merged ancestry pathological at corpus scale.
  This is the package-identity candidate relation meeting the real world, and
  it argues for filling the `ScopedLookup` visibility slot Perl still passes
  empty.
- **hover empty on a `Koha::Database` module-name token** where goto-def works
  at the same position — adjacent to the require/hover family fixed in
  `5e97516b`.
- ~~**`epoch.gen_stamp_missing = 1074`**~~ — **closed, not a bug.** It counts
  warm @INC providers that needed a registration generation, stamped once at
  resolver startup. Measured on crm: 1,151 warm entries → 1,080 distinct paths
  (71 rows are name-aliases sharing a file) → 1,073 stamped (7 already carried
  a generation from the concurrent workspace front door). The run-to-run ±1 is
  that race, and `or_insert` makes it benign by design — the front-door
  generation wins, exactly as the function's doc comment says. Each stamp also
  bumps `gen_counter`, a leg of `enrichment_epoch`, so no memo taken during the
  window survives it.

# Tier 3 — correctness debt

- ~~**@INC tier is single-provider.**~~ CLOSED. All four stages landed: the
  `(name, inc-root)` relation, the `modules` PK migration to
  `(module_name, path)` at schema v10, `CandidateSet::scoped` filled from
  the asker's `@INC` via `VisibilityAxis`, and the substrate-tier
  `incdual-*` rows whose twins live outside the workspace. The residual is
  ACQUISITION — a `use lib` root outside the workspace that is not on the
  process `@INC` is ranked but never scanned; see `gold-corpus/KNOWN-GAPS.md`.
- **~10 bookkeeping `get_cached` sites** left on the derived winner
  deliberately (existence checks equivalent by construction, last-resort
  fallbacks, CLI).
- **`cursor_slot.rs:205`** — driver-owned slot detection; recorded as
  reducible-but-deferred, not irreducible.

# Tier 4 — structural, own arcs

- **Merge the two index families.** The last genuinely irreducible seam site;
  the package-identity work weakened its main justification (the keyspaces no
  longer differ in shape, only in acquisition).
- **Iterative builder walk** — removes the stack-overflow class rather than
  gating it.
- **Grammar-kind tripwire** — must accept DECLARED future kinds or it fails on
  the intentional `parenthesized_expression` forward-compat arms and invites
  exactly the harmful deletion. See `PARKED.md`.

# Not scheduled, with reasons

- **The full 44k CPAN rung.** The 5k sample already saturates every curve:
  startup/storage proven linear-or-better, every broken query path already
  terminal. 8× more corpus measures the same walls at 8× cost. Re-dial after
  Tier 1 lands; Koha is the regression corpus meanwhile.
- **Code lens** — clients poll it per open/change, so N subs means N workspace
  reference walks per edit. Needs bulk counts off the relational `refs` rows,
  which is its own design.
- **`workspace/fileOperations`** (rename a file → rewrite `package` + every
  consumer). Wanted, but subtle: a file rename must **not** imply a package
  rename, because name ≠ path in general — only propagate when path and
  package currently correspond.

# Validation still owed

- ~~**The T1 #1 + T1 #3 combination on cpan5k**~~ — **measured; it does not
  close the row.** Details in row #3 above. What it bought was a sharper
  question: the remaining cost is real CPU that never terminates, not a wait
  expiring, so **the next measurement is a profile of that 150 s** — and it is
  now well-posed (LSP path, warm, hot name, ~294% CPU, 97 MB → 3.4 GB).
- **Cold cpan5k with every fix in.** Deliberately not started when it could not
  fit the window; the walk alone is ~10 min plus ~9 min of writer drain.
- **Differential sweep** — main vs branch over thousands of positions, turning
  "review 130k lines" into "adjudicate a divergence list".
- ~~**Pack-language soak**~~ — **run, 3h20m on abseil (873 files), clean.**
  `resync_bytes` **fired zero times** across 297,268 `pack-cpp` lookups and
  296,852 capacity evictions, with `peak_bytes` pinned at exactly the cap
  (134,217,696 B) for the whole run — the byte-accounting invariant behind the
  13.9 GB ratchet holds under sustained churn, which is the one thing a
  Perl-only soak could never show. Zero bytes on stderr; no latency drift
  (references got *faster*, 755 → 478 ms median).

  Honest residuals:
  - **RSS did not fully flatten**: 497 → 963 MB, decelerating hard (+195,
    +132, +66, +39, +15, +19 MB per 30-min bucket), tail slope ~35–37 MB/h and
    still converging. Both 10-minute idle windows were *perfectly* flat
    (byte-identical across consecutive samples), so the growth is edit-driven
    cache/index fill, not a background leak — but "converging" is not
    "converged", and 3h20m did not reach the asymptote.
  - **The 963 MB is not fully decomposed.** Bounded caches account for ~235 MB
    and the post-ready baseline ~190 MB; the remainder is *inferred* to be
    workspace-index residency for the corpus and was not heap-profiled.
    Recorded as inferred, not measured.
  - **It ran on `737b3cc8`, before `b6312ea2` added the rows lane to
    `PackBagCache`.** The clean bill covers the cache as it was, not as it is.
    A short re-soak on the current tip is owed before treating that file as
    hour-scale-proven again.

  The missing alarm the run needed is now permanent: `resync_bytes` increments
  `pack_bag_cache.resync_bytes_fired`, so the next drift is visible instead of
  silently self-healed.
- **Narrow seam review** — the few hundred lines where a bug is silent and
  catastrophic (cache accounting, residency, invalidation, the enrichment
  writer, `IndexCore` shared state).
