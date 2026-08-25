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
