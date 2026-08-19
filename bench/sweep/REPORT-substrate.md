# Differential sweep: `main` (0.6.1) vs the rework (0.7.0)

A worked run, kept so the tool has a reference output and so the
adjudication below can be re-checked rather than believed.

- **Corpus** — `gold-corpus/local/lib/perl5`, the pinned CPAN substrate:
  2,265 modules, 200 sampled files, **1,458 positions**, **4,302
  (position, verb) answers** compared.
- **Path** — server, LSP over stdio. Both sides built with **default
  features**: `main` predates the `cpp` feature, so the comparison covers
  Perl only.
- **Verbs** — completion, definition, hover, references (10% sample),
  documentSymbol. `typeDefinition` excluded: `main` does not serve it, which
  is a capability difference, not a divergence.
- Raw report: `report-substrate-raw.md`.

**2,747 of 4,302 answers identical (63.9%); 1,555 divergent.** Of the
identical, 895 were empty on both sides.

## The noise floor comes first

> **Re-measured end to end with the guarded harness.** Every figure below
> comes from one verified set: five runs (base + four head) on one position
> set, each renamed from `.partial` only on clean completion, each carrying
> its position-set hash and a completion marker, and cross-checked by
> `diff` before use. The earlier figures in this section were published
> twice and wrong twice — first from a floor measured over a different
> population, then from noise files belonging to the *previous* invocation.
> Neither error was visible in the output.

The same binary, run twice, disagrees with itself — but **the floor is not a
number, it is a distribution.** Four head runs give six pairs, and on
identical inputs they disagree by nearly 2× in one shape:

| shape | across 6 pairs | reported floor (worst) |
|---|---|---|
| `reranked` | 158, 163, 164, 164, 167, 168 | **168** |
| `disagree` | 14, 15, 17, 19, 25, 25 | **25** |
| everything else | all zero | **0** |

A two-run floor is one draw from that. Whichever pair happened to run would
have been quoted as "the floor", and for `disagree` that is anywhere from 14
to 25 — so a block sitting in that band cannot be called signal on a two-run
measurement. The reported floor is the **worst** pair, because a shape earns
"signal" by clearing the worst self-disagreement observed, not the luckiest.
`--noise` takes as many runs as you give it.

All of the `disagree` floor is on completion; `definition` contributes zero
in all six pairs. The groups table carries a per-verb floor for that reason.

Completion ordering is not stable run to run. So the `reranked` block carries
**no information at all** — it sits below the floor — while `only-base`,
`subset`, `superset` and `content-differs` can be read at face value. Without this measurement a reader has no way to tell those cases
apart, and the third of the A/B's shapes that is pure noise looks exactly
like the two thirds that are not.

## Adjudicated

| claim | rows | verdict |
|---|---|---|
| head no longer offers `(anon)` as a completion candidate | 52 `subset` + 1 `only-base` | **fix** — an anonymous sub is not a name anyone can type. 53 of 53 lose `(anon)` and *nothing else*, and this has now reproduced on four independent runs |
| head resolves goto-def to the declaration line where base returned `0:0` | 56 `disagree` | **fix** — every same-file definition disagreement is this, in every run |
| head returns *every* `@INC` provider of a name where base returned one | ~117 `disagree` | **intended** — this is the `(name, inc-root)` candidate relation. `use strict` now answers both `perl/5.38.2/strict.pm` and `perl-base/strict.pm`. Flagged because it is my own change: the sweep is confirming it, not discovering it |
| head answers where base was empty | 417 `only-head` | improvement |
| head answers a superset | 254 `superset` | improvement |
| head truncates long completion lists | 16 `capped-head` | **by design** — `MAX_COMPLETION_ITEMS`, with `isIncomplete` set |
| completion ranking moved | 41 `reranked` | **unreadable** — floor is 168 |

## Needs a ruling

**One regression candidate**, and it is specific:

- `x86_64-linux-gnu-thread-multi/Moose.pm:200:38` — completion after
  `$metaclass->initialize`. Base offers 13 items, head offers none.
  Reproduces on the CLI, so it is not a warm-up artefact. Base's 13 are all
  subs defined *in Moose.pm itself* — `after`, `around`, `augment`,
  `_get_caller` — i.e. a same-file fallback when the receiver cannot be
  resolved, not real metaclass methods. So the honest question is whether
  head returning nothing is better than base returning plausible-but-wrong
  candidates. It is the only `only-base` row in the sweep that is not the
  `(anon)` fix.

**531 completion `disagree` rows** where head both gains and loses real
names (of 560 completion disagreements). Perl builtins explain only ~2% of
what head gains; **341 — 64% — lose sigil'd (lexical or package) names**,
against a `disagree`/completion floor of 25. Whether that is scope correctness or lost candidates is
the one bucket this sweep cannot rule on.

## Two things about `main` itself

**`textDocument/definition` hangs.** Over three runs `main` accumulated 4,
5 and 10 unrecoverable stalls at different files each time — cache-state
dependent, not file-specific. `0.7.0` had none in ~9,000 positions.

The first diagnosis was wrong and the correction matters: the server is
**not** wedged by a single hanging request. Checked with a client that is
not this harness, a `definition` that times out at 30 s is followed by a
`documentSymbol` on the same open file answering in milliseconds. What kills
it is a *cluster*; the sweep now probes liveness before respawning, and in
the final run all four probes found the process genuinely unresponsive.

**Wall clock.** 1,458 positions: `main` 247 s, `0.7.0` 54 s. Base's figure
swings 247–608 s across runs because of the stalls, so treat it as an
order-of-magnitude statement. The two sides also ran sequentially here;
`bench/lsp_bench.py` is the tool for latency, not this one.
