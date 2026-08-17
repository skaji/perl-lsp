# Scale-validation hitlist — what 122× found, and what it schedules

The validation pass of 2026-08-17. Every performance number this project had
before it came from `crm` — 1,136 Perl files — while the stated target is
monorepos two orders of magnitude larger. This pass measured the gap.

Three instruments ran: a **4.65 h soak** (hour-scale behaviour, previously
unmeasured — the longest prior measurement was 240 s), **Koha** (3.1×), and a
**5,000-dist CPAN sample** (122× — genuinely the target rung). The
**differential sweep** is the one track still owed.

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

# Tier 1 — blocks the target market

### 1. Post-cold-index availability hole
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

### 2. Fatal stack overflow on deep CSTs — P0
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

### 3. `references` terminal at scale — the missing refs axis reader
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

Related, same root: **repeat refs never cache-hit** — RSS plateaus
(566→635 MB over 6 identical queries, bounded, not a leak) while latency stays
~3.4 s. Capacity thrash; memory grows and buys nothing. `refs_present` makes
it moot — no decode, nothing to cache.

### 4. Completion payload unbounded
7.8 MB / ~50k items per keystroke (21.3 MB in the post-cold state). The
workspace/in-scope tier has no scale cap. Broken at any size; invisible below
~10k files.

# Tier 2 — cheap, and now debuggable

Each of these was anonymous until `3fef0120` added breadcrumbs; all now have
named inputs.

- **`src/build/pod.rs:20`** — `result[..2000]` byte-slices inside a multibyte
  char. Victims: `Test-BDD-Cucumber-Definitions-0.38/-0.39
  lib/.../Base/Ru.pm` (Russian POD). Caught per-file, so the file's analysis
  is silently lost. Minutes to fix.
- **Fold-64 non-convergence** — the debug safety net firing in production;
  the lattice is not reaching a fixed point. Offenders:
  `Module-Generic-v1.7.0/lib/Module/Generic.pm`,
  `Config-Universal-0.5/lib/Config/Universal.pm`,
  `File-stat-Extra-0.010/lib/File/stat/Extra.pm` (3 in 138k).
- **`query_rec` 512-depth cap hit** on `MethodOnClass` — cross-dist
  class-name collisions make merged ancestry pathological at corpus scale.
  This is the package-identity candidate relation meeting the real world, and
  it argues for filling the `ScopedLookup` visibility slot Perl still passes
  empty.
- **hover empty on a `Koha::Database` module-name token** where goto-def works
  at the same position — adjacent to the require/hover family fixed in
  `5e97516b`.
- **`epoch.gen_stamp_missing = 1074`** in the soak's shutdown dump, with no
  workload twin. Correlated with nothing; a counter incrementing a thousand
  times unexplained is worth one look.

# Tier 3 — correctness debt

- **@INC tier is single-provider.** Staged plan in `gold-corpus/KNOWN-GAPS.md`:
  `(name, inc-root)` relation → `modules.module_name` PK migration → fill
  `CandidateSet::scoped` from the asker's @INC → substrate-tier fixture (the
  twin must live outside the workspace or the workspace relation compensates
  and the row passes for the wrong reason).
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

- **Differential sweep** — main vs branch over thousands of positions, turning
  "review 130k lines" into "adjudicate a divergence list".
- **Pack-language soak** — today's was Perl-only, so `PackBagCache`, where the
  13.9 GB ratchet lived, was never exercised at hour scale. A real hole in the
  clean bill.
- **Narrow seam review** — the few hundred lines where a bug is silent and
  catastrophic (cache accounting, residency, invalidation, the enrichment
  writer, `IndexCore` shared state).
