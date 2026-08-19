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

**2,836 of 4,302 answers identical (65.9%); 1,466 divergent.** Of the
identical, 945 were empty on both sides.

## The noise floor comes first

The same binary, run twice over the same positions, disagrees with itself on
**3.8%** of answers:

| shape | self-vs-self |
|---|---|
| `reranked` | 164 |
| `disagree` | 7 |
| everything else | **0** |

Completion ordering is not stable run to run. So the 70 `reranked` rows in
the A/B carry **no information at all** — they are below the floor — while
`only-*`, `subset`, `superset` and `content-differs` can be read at face
value. Without this measurement a reader has no way to tell those cases
apart, and the third of the A/B's shapes that is pure noise looks exactly
like the two thirds that are not.

## Adjudicated

| claim | rows | verdict |
|---|---|---|
| head no longer offers `(anon)` as a completion candidate | 87 `subset` + 1 `only-base` | **fix** — an anonymous sub is not a name anyone can type. 88 of 88 lose `(anon)` and *nothing else*, on two independent runs |
| head resolves goto-def to the declaration line where base returned `0:0` | 110 `disagree` | **fix** — every same-file definition disagreement is this |
| head returns *every* `@INC` provider of a name where base returned one | ~117 `disagree` | **intended** — this is the `(name, inc-root)` candidate relation. `use strict` now answers both `perl/5.38.2/strict.pm` and `perl-base/strict.pm`. Flagged because it is my own change: the sweep is confirming it, not discovering it |
| head answers where base was empty | 263 `only-head` | improvement (99 definition, 87 completion, 77 hover) |
| head answers a superset | 182 `superset` | improvement (155 completion, 27 references) |
| head truncates long completion lists | 27 `capped-head` | **by design** — `MAX_COMPLETION_ITEMS`, with `isIncomplete` set |
| completion ranking moved | 70 `reranked` | **unreadable** — below the noise floor |

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

**526 completion `disagree` rows** where head both gains and loses real
names. Perl builtins explain only 2%; **63% lose sigil'd (lexical or
package) names**. Whether that is scope correctness or lost candidates is
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
