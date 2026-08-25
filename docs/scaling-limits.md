# Scaling limits: what we know, measured

Two shapes cost far more than their size suggests. Both are documented rather
than fixed, because in each case the fix is large and the population affected is
small and identifiable.

Everything here was measured on 28 large open-source Perl codebases plus two
private applications (`corpus/README.md` reconstructs the public set).

## The healthy baseline, so the outliers mean something

Across 28 real Perl codebases, cold `--check`:

- conclusion-cache hit rate **96–99.9%**, every corpus
- provider-fetch **attempts** 8–206 per file for 25 of 28
- 4–36 ms per file
- memory flat and bounded

That band is what normal looks like. Two things fall outside it.

## 1. `package main` monocultures (FHEM shape) — PATHOLOGICAL, not fixed

[FHEM](https://github.com/fhem/fhem-mirror) — 991k LOC, 973 Perl files, zero
vendored — does not complete `--check` on a machine with 31 GB of RAM.

**Scope, established by using it rather than by measuring it: this is a
BATCH-VERB limit, not a server limit.** FHEM opens fine in an editor. Startup is
slow, and after that it behaves. The difference is structural rather than
lucky — `--check` runs the enriched-diagnostic sweep across *every* workspace
file, while the server indexes nothing at `initialize` and enriches only the
documents you actually open. The crest below is 20 rayon workers each holding a
per-file working set; an editor session has no such sweep to run.

So: `--check`, `--heatmap` and the other whole-workspace verbs are the affected
surface. Interactive editing of a `main`-monoculture codebase is not, and this
file said otherwise until someone opened FHEM in nvim and found it usable.

**The shape:** 503 of its 614 `.pm` files declare `package main` explicitly, and
31 more declare nothing. **534 of 614 files (87%) provide one package name.**
This is not a mistake in FHEM: `fhem.pl` `do`-loads all of them into a single
interpreter, and 361 of them call `readingsSingleUpdate` from `fhem.pl`'s main.
They genuinely share one stash. **The 534-wide provider relation is correct.**

**The cost:** every `main` lookup consults a 534-member candidate set. `main` is
27% of package lookups and **94% of provider fetches**. Per-worker sweep working
sets reach 633 MB, and ~20 rayon workers multiply that into the crest.

**What does not explain it, each ruled out by a control rather than an argument:**

| suspect | ruled out by |
|---|---|
| walk residency | index-only run holds 0.98 GB — *leaner than release's entire check* |
| overlay clones | 0.76 GB cumulative; the overlay cache is capped at 133 MB |
| the sweep path memo | disabling it moves peak 0.7% — and costs 55% wall, it is load-bearing |
| allocator arena count | `MALLOC_ARENA_MAX=2` gives ≤5%, inside run variance |
| the diagnostics channel | 300 KB high-water against 10 GB |
| per-file memo byte cap | engaged correctly (19,929 evictions) and made peak **worse** (+15%) and wall **worse** (+51%) — reverted |
| source-byte admission gate | never engages: needs 3+ giants concurrently in flight to reach its budget |

**What does explain it, in two parts:**

- **glibc brk retention** owns the *sustained* figure. 1.6 MB analyses churn
  decode-and-drop and freed chunks never return.
  `MALLOC_MMAP_THRESHOLD_=65536` cuts sustained RSS **53%** (10.02 → 4.68 GB)
  and turns a monotone climb into a sawtooth that actually returns memory. Peak
  falls only 16%, so this helps a long-lived server and barely helps a one-shot
  CLI.
- **Per-worker in-flight sets** own the *crest*. `RAYON_NUM_THREADS=4` cuts peak
  **67%** (9.83 → 3.20 GB) for **4.9%** wall — the sweep is not CPU-bound at 20
  workers, so 16 of 20 buy almost nothing in time while costing 6.6 GB.

**Workaround for this shape today:**

```
RAYON_NUM_THREADS=4 MALLOC_MMAP_THRESHOLD_=65536 perl-lsp --check <root>
```

**Why it is not fixed.** The relation is semantically correct, so narrowing it
would be wrong. The remaining honest fix is deduplicating provider decoding
across a sweep (~13,456 rehydrates for ~500 distinct providers is ~27x redundant
work held in up to 20 overlapping per-file memos). That is a real change to the
sweep's memory model and it has not been built.

**Who this affects:** codebases where one package name has hundreds of
providers. That is `do`-loaded plugin frameworks. It is not ordinary
application code, and it is not `.t`-heavy repositories — test files declaring
`main` implicitly do **not** reproduce it.

## 2. Vendored dependency piles — MISLEADING, not slow

A tree of many unrelated distributions (a CPAN mirror, a large `thirdparty/`)
has no locality: each file consults a different module set, so the conclusion
cache thrashes. A 551-distribution pile took **137x more cache misses** than
real application code and produced a "cliff" — 65x slowdown for 1.84x the files
— that **no real codebase reproduces**.

If you are benchmarking, this is the trap: unresolvable imports cost the
resolver nothing to answer, so a dependency pile both thrashes the cache *and*
understates resolution work. Measure applications, and install their
dependencies onto `@INC` rather than into the workspace (`corpus/README.md`).

## Reporting a scaling problem

Useful report: `PERL_LSP_GHOST_STATS=/tmp/g.txt perl-lsp --check <root>` plus
`/usr/bin/time -v`, and the count of files declaring the same package name
(`grep -rhc '^package ' --include='*.pm' . | sort | uniq -c | sort -rn | head`).
Those three tell us within minutes which of the shapes above you have, or that
you have a new one.

## 3. Dependencies are not free — every historical number here understates load

Measured on the eight-repo corpus, cold `--check`, same binary both arms, deps
on `@INC` via `PERL5LIB` (never in the workspace — see `corpus/README.md`):

| repo | wall nodeps → deps | Δ | RSS nodeps → deps |
|---|---|---:|---|
| BMO | 7.82 → 13.76 s | **+76%** | 0.48 → 0.62 GB |
| openfoodfacts | 9.51 → 12.76 s | +34% | 0.37 → 0.47 GB |
| WeBWorK | 7.40 → 9.89 s | +34% | 0.30 → 0.36 GB |
| FHEM | 41.06 → 54.23 s | +32% | both killed |
| Evergreen | 13.37 → 17.18 s | +29% | 0.53 → 0.65 GB |
| Foswiki | 10.80 → 12.83 s | +19% | 0.48 → 0.53 GB |
| Webmin | 9.26 → 8.76 s | −5% | flat |
| Znuny | 85.30 → 79.07 s | −7% | flat |

**+19–76% wall, +2–29% RSS** where it bites. The two flat rows are controls
rather than noise: Webmin uses path-based `require` and barely touches CPAN, and
Znuny vendors 723 `cpan-lib` modules *inside* its workspace so its imports
already resolved.

Wall and fetch counts do **not** track each other — BMO gains 76% wall on 16%
more fetches, openfoodfacts 34% on 168% more. The expense is what a *successful*
resolution pulls in, not the lookup count.

**Consequence:** any benchmark run without dependencies installed understates
real resolution load by roughly a third. State which shape a number was measured
on.

## 4. Memory and fan-out are independent axes

`Znuny` is the correction to an earlier framing here. It has the **lowest**
provider fan-out of 28 corpora (8 attempts/file) and is entirely healthy on that
axis — and it still peaks at **8.15 GB** on 3,078 files, second only to FHEM.
Low fan-out does not imply low memory; file count does not predict either. A
corpus needs both measured.

## 5. `--heatmap` is a batch verb, not an interactive one

| corpus | `--check` | `--heatmap` | ratio | max fan-in |
|---|---:|---:|---:|---:|
| WeBWorK (225 files) | 3.80 s | 5.00 s | 1.3x | 81 |
| Webmin (1,333) | 4.76 s | 31.07 s | 6.5x | 199 |
| BMO (739) | 5.39 s | 91.69 s | **17x** | 340 |

Cost tracks **fan-in**, not file count — BMO is smaller than Webmin and costs
3x more. That follows from what the verb does: it mints the `references()`
projection at every declaration, so the work is declarations x their fan-in.

**It also runs on one core.** Measured at 104-105% CPU throughout, where
`--check`'s diagnostics sweep parallelises across all of them. So the ratios
above are two effects compounding — more work per declaration, done serially —
and the serial half looks addressable with the same `par_iter` + channel shape
the diagnostics sweep already uses. Not attempted; recorded as the obvious next
step for anyone who needs `--heatmap` to be faster.

Memory is mild by comparison (Webmin 0.47 → 0.95 GB, BMO 0.49 → 0.70 GB), so
this is a wall cost, not a memory one.

## 6. Single huge files: `build()` goes cubic past ~20k lines

Found by opening FHEM's biggest file in an editor and waiting 30 seconds for
semantic tokens. The server was responsive throughout — it just had no analysis
to answer from yet.

Measured through the LSP, real workspace, `@INC` live (`PERL_LSP_PHASE_TIMING=1`):

| lines | parse | `build()` |
|---:|---:|---:|
| 1,760 | 10 ms | 71 ms |
| 3,268 | 20 ms | 188 ms |
| 5,607 | 30 ms | 276 ms |
| 20,669 | 160 ms | 2,374 ms |
| **46,522** | **318 ms** | **35,304 ms** |

```
parse   ~ lines^1.05                      (linear; the parser is not the problem)
build() ~ lines^1.58 → 0.71 → 1.65 → 3.33 (exponent RISES with size)
```

**The knee is between 20k and 46k lines.** Under ~6k, build is sub-300 ms.
At 20k it is 2.4 s. At 46k it is 35 s, and the file is unusable interactively
until it finishes.

### Where the time goes

`build()` is 33.2 s of a 33.6 s open, and inside it:

```
build::fold_to_fixed_point   30,680 ms    <- 92%
build::pattern_dispatch         913 ms
build::walk                     740 ms
build::finalize_post_walk       442 ms
```

The CST walk is 0.74 s. **The worklist fold is the whole cost**, and the
counters say what it is folding:

```
hop.edge            1,838,748
hop.OBSERVATION       503,918
hop.fact              271,500
hop.inferred_type     232,585
build.fold_bag_len     57,228   <- witnesses in the bag
```

A 57k-witness bag re-walked to a fixed point at 1.8M edge hops. `Expr(span)`
witnesses are emitted per meaningful expression, so bag size tracks expression
count, and the fold is superlinear in bag size — which is why the exponent
climbs rather than holding.

### What this is and is not

**Not** the `--check` memory story above; that is whole-workspace sweep
residency, this is one file's analysis, and they share no mechanism. **Not**
`@INC` size either, though `@INC` is required to see it: the same file with an
empty module universe builds in **0.39 s**, so a single-file reproduction
without a real workspace measures nothing. That trap cost an hour here.

**Interactive symptom, precisely:** verbs answer *fast* and return empty
(`null`) while the build runs, rather than blocking. The client renders nothing
and does not retry, so it looks like a dead server until the build finishes and
the server sends `semanticTokens/refresh` — at which point everything appears
at once. Answering "not ready" in a way clients retry on would mask the whole
delay without making the build any faster.

**Who hits it:** anyone with a single module past ~20k lines. Rare, but not
FHEM-exclusive — a few generated or accreted modules that size exist in plenty
of long-lived codebases.
