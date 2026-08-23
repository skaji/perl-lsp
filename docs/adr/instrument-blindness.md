# Instrument blindness: correct readings of the wrong question

The failure shape is never a wrong reading. It is a CORRECT reading of
an instrument that answers a different question than the one being
asked — so every check of the reading confirms it, and the error
survives scrutiny that would catch any ordinary mistake. Adjacent to
`docs/adr/sibling-forks.md` but distinct, and the boundary runs
through that ADR's "stale agreement" case: a sibling fork is two
answers to one live question; stale agreement is answers to a question
that expired; instrument blindness is one answer to a question nobody
asked. The cures differ — forks collapse into seams, blindness is
cured by evidence discipline — which is why this is its own ADR.

## The worked instances

| instrument | the question it answered | the question asked of it |
|---|---|---|
| gold suite | "does a COLD run hold?" (warm never executes in CI) | "does the server hold?" |
| CI `pull_request` events | "did a covered event fire?" (`edited` missing; `cancel-in-progress` eats the force-push run) | "was this base/tip verified?" |
| `ulimit -v` guard | "did address space exceed the cap?" | "did RSS exceed the cap?" (nondeterministic SIGSEGV then read as a memory profile) |
| `statusCheckRollup` filter | "is any check non-SUCCESS?" (CANCELLED included) | "did CI fail?" |
| scoped `--clear-cache` | "is THIS root cold?" (1 of 17 fixture roots) | "is the run cold?" |
| `ps` grep for `perl-lsp` | "does a process with that NAME exist?" (binaries were `plsp-before`/`after`) | "is the server dead?" |

## The second family: one sample of a trajectory

A single measurement of a quantity that has a TRAJECTORY is the same
blindness on the time axis — a correct reading of the wrong moment. A
"19.9% steady state" that was the trough of a curve climbing to ~46%;
a "10.9% wall win" that settles at ~1.7%. The guards are cheap: take
the LAST emit, not the first; interleave repeats (A/B/A/B/A/B, never
AAA-then-BBB) so drift lands on both sides; and if two samples
disagree, the quantity has a trajectory and one sample was never going
to be an answer.

## The guards

- **Name the instrument's question before trusting its answer.** Every
  row above dissolves the moment the second column is written down.
  The check costs one sentence; each of these cost hours.
- **Ablate the instrument, not just the code.** A gate is only a net if
  it FAILS when the property it guards is broken — break the property
  deliberately and watch the gate trip. The rule earned the hard way:
  a bound-first fixture that passed under BOTH wrong designs, because
  it asserted a true thing about a path the change never touched. An
  un-ablated fixture is an assertion, not a net. (Bound-first without
  ablation is false comfort — the pin discipline in
  `sibling-forks.md` assumes pins that have been shown to fire.)
- **A verdict is trusted only about the tree, moment, and scope it was
  computed on** — the stale-agreement rule generalized from git bases
  to every instrument: re-verify when any of the three moved.
