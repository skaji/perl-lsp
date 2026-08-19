# Differential answer sweep

A large rework cannot be reviewed by reading it. This converts the diff into
a **divergence list**: run two binaries over thousands of cursor positions on
a real corpus, compare their answers, and group what differs into claims a
person can adjudicate one at a time — *intended improvement*, *regression*,
or *wash*.

## Running it

```sh
# 1. positions — binary-independent, so both sides answer the same questions
bench/sweep/sweep.py positions --root <corpus> --out pos.jsonl \
    --per-file 8 --max-files 600 --seed v1

# 2. one warm server per side, in its own cache dir
bench/sweep/sweep.py run --bin <base-binary> --root <corpus> \
    --positions pos.jsonl --out ans-base.jsonl --side base \
    --cache-dir /tmp/sweep-cache-base
bench/sweep/sweep.py run --bin <head-binary> --root <corpus> \
    --positions pos.jsonl --out ans-head.jsonl --side head \
    --cache-dir /tmp/sweep-cache-head

# 3. the report
bench/sweep/sweep.py diff --base ans-base.jsonl --head ans-head.jsonl \
    --out report.md
```

The three steps are separate on purpose: a corpus run costs hours, and the
diff is where the thinking happens. Re-diffing with different grouping must
never mean re-sweeping.

## The decisions that make it trustworthy

**Positions come from the source text, never from a binary.** A
binary-derived sample (semantic tokens, say) lets one side choose where it is
tested, and a divergence stops being attributable: did the answer change, or
did the question? The tokenizer is in `positions.py` and is deliberately
conservative — every heuristic fails toward emitting fewer positions.

**Token-driven, with per-kind quotas.** Random offsets land on whitespace. An
unweighted token sample is ~71% `variable` on real CPAN code, which is the
case both sides are most likely to agree on, so an unweighted sweep spends
its budget confirming null results. The per-file cap is round-robin across
kinds, because a flat cap re-imposes the very distribution the weights
corrected.

**The server path, and the report says so.** The CLI and the server answer
differently for the same query — 284,617 vs 193,725 bytes on Koha
`references` — because they reach different readiness states. Neither is
wrong; mixing them produces a divergence list that is mostly harness.

**Readiness is a cross-file gate.** Non-empty is not ready: goto-def on a
local `sub` answers from the open document with no index at all. Measured
here, the base passed a non-empty probe in 12 ms where the head took 1,309 ms
— sweeping on that would have compared a cold server to a warm one and
reported the difference as regressions. The gate requires a definition that
lands in a *different file*, on both sides, and a side that never achieves it
is flagged in the report rather than silently swept.

**A verb one side does not implement is a capability difference.** The base
here does not serve `typeDefinition`; left in, that alone put an `error-base`
on every position. Capabilities are read from `initialize` and the asymmetry
is subtracted into its own line.

**Timeouts are never folded into "empty".** A slow side that lost a race is
not a side that lost an answer, and conflating them is the most misleading
thing this tool could report.

**A hang cluster is distinguished from a dead server.** The base binary has
`definition` requests that never return — ten clusters over 1,458 positions.
The first version of this called that a wedge and respawned the process,
which was wrong: checked outside this driver, a `definition` that times out
at 30 s is followed by a `documentSymbol` on the same open file answering in
milliseconds. The server is alive; individual requests hang, and they
cluster.

So consecutive timeouts now trigger a liveness probe — `documentSymbol` on an
already-open file, which needs no index and no cross-file work — and a server
that answers it keeps its warm index. Only a process that fails the probe is
respawned. Either way the event is recorded, because "this verb hangs here"
is a finding whether or not it killed the server.

**Each side gets its own `XDG_CACHE_HOME`.** Both key `~/.cache/perl-lsp` off
the workspace path, and their `EXTRACT_VERSION` and plugin fingerprints
differ — sharing one makes each side hard-clear the other's cache on every
startup.

## Reading the report

Shapes are ordered by what a reviewer must look at first. `only-base` and
`subset` are the regression candidates: the base answered and the head did
not, or answered less. `only-head` and `superset` are the improvement
candidates. `disagree` is the residual that always needs a human.

`n` is positions; **`distinct` is claims** — the number of different (base
answer, head answer) pairs behind them. One generated data file can
contribute sixty positions that disagree identically. Adjudicate `distinct`.

## What it does not do

It does not decide whether a divergence is a regression. That is the point:
it produces the list, a person rules on each row, and the ruling becomes a
gold fixture (`gold-corpus/run.pl --emit`) so the answer is pinned from then
on.
