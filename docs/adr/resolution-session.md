# ADR: The resolution session — one memo across a walk's consults

`QueryState` dedups within ONE `ReducerRegistry::query`. A backward
reference walk issues one such query per candidate call site, and each
re-derives the same `MethodOnClass{class, method}` lattice from scratch.
At 138k files that re-derivation is combinatorial — 5–12 files declare a
common package name, mutual imports are the norm, and the chase recurses
through both — and the verb does not return. Measured: one `references`
request performed **10.7M cross-file consults in 15 minutes** and never
reached the projection, with the blob caches sized so large that
rehydration hit ~100% and the query still did not finish. Cache sizing
moves that wall; it cannot remove it.

The session is the outer memo, plus the bound that catches whatever the
memo does not.

## Shape

`ResolutionSession::enter(index)` is an RAII guard, thread-local, opened
once around a walk (`refs_to`). While it is open the `MethodOnClass` /
`SlotType` cross-file hops answer from a memo of **candidate
contributions**: "what does the file at `path` contribute to this query".
It also shares `visible_def_candidates` (a clone + sort of the whole
candidate vec per call, asked millions of times per walk) and carries the
walk's consult budget.

Keyed on the candidate's **path**, never on a bag address. A bag-keyed
memo that outlives one query has an ABA hazard the moment an evicted
analysis is dropped and a rehydrated one lands at the same address — and
avoiding it would mean pinning every consulted analysis for the walk
(gigabytes at this scale). A path is stable and free.

## The four soundness gates

**One visibility scope.** Entries are used only under the same
`&dyn CrossFileLookup` the session was opened with. A pack file's
`ScopedLookup` is a different object, so a closure-narrowed candidate view
never reads a memo minted under the unscoped index, nor writes one. Pack
behaviour is therefore unchanged by construction.

**The epoch.** Validity rides `CrossFileLookup::resolution_epoch()` — the
same additive counter (`gen_counter` + `shape_bumps` + freshness writes)
the enrichment-key memo validates against. Any index mutation moves it and
the session drops memo, candidates and interned paths wholesale. A new
mutation path must move one of the counter's legs; it never needs to know
this memo exists.

**Complete values only.** A value the cycle guard fed by cutting a key
*above* the evaluation's own root is path-dependent — reusing it elsewhere
serves a truncated answer. `QueryState` keys its visited set by path
DEPTH, so a cut names the frame it closed on; `QueryState::scoped` reports
whether an evaluation's cuts were all internal, and the memo declines to
store when they were not. A cut wholly inside the candidate's own subtree
is self-contained and reusable.

**Full query identity in the key.** Attachment, receiver identity, arity
hint, point and framework all ride the key, for the same reason they ride
`QueryState`'s: two queries differing in any of them resolve differently.
Receiver is the whole structural identity, not a variant tag —
`ReturnExpr::Receiver` substitutes the receiver, so `ClassName("Foo")` and
`ClassName("Bar")` must not share a slot.

## The self-skip

Hop (1) skips a candidate whose bag IS the querying bag: the reducers
above already ran on it, and re-entering would recurse. A memo HIT can't
make that check — it has no bag in hand. It doesn't need to: the stored
value for candidate `X` is `query_rec(X.bag, q, ctx_X)`, which is exactly
what a query already running on `X.bag` with the same key computes. Serving
it early is the fixpoint, not a different answer. Self-skipped candidates
are never STORED, so an entry always describes a full evaluation.

## The consult budget

Even memoized, some query at some scale exceeds any bound, and degrading
honestly beats running forever. Each candidate evaluation spends one unit;
at zero the cross-file hops stop and `ResolutionSession::degraded()` says
the walk under-answered. `PERL_LSP_RESOLVE_FUEL` sets it (`0` =
unbounded). The default is sized two orders above a healthy
workspace-scale walk (Koha's `references` on `store`: ~5k consults), so it
bounds the pathological query without touching a real one.

## Where policy goes

A new cross-cutting axis belongs in the session or in CandidateSet
construction — never in a handler. The session is per-walk state, so it is
the natural home for anything a walk must budget or remember across its
consults.
