# Forward design: a dependent conclusion layer for the witness bag

**Status: designed, closes, NOT recommended for build yet.** Staged, with
stage 1 cheap and semantics-free and stage 2 gated on a re-measurement.

The bag is the DERIVATION. It is 41.5% of a stored `FileAnalysis` by
compressed bytes and 52.9% of the bincode payload. The question was whether
we can persist CONCLUSIONS and load the bag only on demand.

## Measurements this rests on

All from `--check` over the 2,265-module substrate; counters are in-tree
behind `PERL_LSP_GHOST_STATS`, probes are `#[ignore]`d in
`index/module_cache/blob.rs`.

Cross-file consult shapes: `MethodOnClass` 106,533 (**96.0%**), `slot_type`
4,367, `bridged` 42, `imported_sub_return` 5.

A FLAT conclusion layer (one resolved return per sub) does not close it:
85,952 `MethodOnClass` queries (77.3%) answer without entering an
`Expr(span)`; 25,201 (22.7%) do. That layer is 534,938 bincode / 276,551
zstd — 0.9% / 4.1% of the bag.

What the chase reads at `Expr` attachments: `Edge` 446,478, `InferredType`
180,515, `Projected` 44,901, `ReturnExpr` 25,402, **zero `Observation`**.

What it reads at EVERY attachment: `Edge` 2,166,944 (47.5%), **`Observation`
1,298,645 (28.5%)**, `InferredType` 554,032, `Fact` 163,457, `CallReturn`
137,682, `Projected` 134,578, `ReturnExpr` 62,344, `QualifiedCallReturn`
10,484.

## Why it closes

A conclusion is `query()` partially evaluated over what the file knows,
leaving free only what it cannot: the query POINT, the RECEIVER, the ARITY,
and the state of the cross-file world. The first three are bounded and
residualize as syntax — a dependent conclusion. The fourth is unbounded and
residualizes only as a LINK. That is the bag's own "edges, not values" rule
lifted one layer up.

The observations are the crux, and they close for a measured reason. They
live at `Variable` attachments, where `FrameworkAwareTypeFold` gates them
temporally (`reducers.rs:166-181`, `:832`) — so a variable's conclusion is
`λ point. InferredType`, a step function with a breakpoint per witness span.

**Timelines cannot cross a file boundary, and the code enforces it.** Every
cross-file entry builds a FRESH query with `point: None` against a context
rebuilt from the provider's own `scopes`/`packages` (all five sites in
`witnesses/query.rs`), entering at a point-free `Symbol(sid)`. The asker's
point is never threaded across the boundary. The single `point: Some(..)`
construction (`registry.rs:1034`) is the scope-chain walk INSIDE one bag, and
the `point: q.point` threading is likewise intra-walk.

So an intra-file edge does reference a timeline — that is where the 1.3M
observation reads are — but no INBOUND edge from another file can name one.
The portable key set is point-free by design and the reset makes it point-free
by enforcement, independently. The apparent counterexample is not one:
enrichment's MCB bridge pushes `Variable → Edge(MethodOnClass)`, a
consumer-local variable pointing OUTWARD at a foreign symbol. Nothing outside
ever names that variable.

That is a stronger property than the composition argument below, and it is the
one that matters for thrash: the only consumer of an UNAPPLIED timeline is
`inferred_type_via_bag(var, point)` — hover / inlay / completion on a file the
user has open, where the bag is resident anyway. Leaving timelines to the full
bag decode therefore costs nothing, because the on-demand decode never fires
for timeline reasons on a closed provider.

**And the binder is always applied at bake.** Every hop that enters a
`Variable` fixes the point AT the hop, from bake-time constants —
`registry.rs:799-801` uses `Expr(span).start` for edges chased from
expressions and `scope_point(scope)` otherwise. So composition through a
variable selects ONE segment, and no cross-file key ever needs an unapplied
timeline. **Timelines exist in the algebra and never on disk.** That is what
keeps the layer at conclusion size: persisting them instead would be
~9-18 MB, 16-32% of the bag, most of the way back to keeping it.

The forms: `Value`, `ReturnOf(ReturnExpr)` (reusing `eval_return_expr`
verbatim — `ReturnExpr` is ALREADY a dependent conclusion),
`Timeline` (bake-internal), `Link{target, arity, receiver}` (subsuming
`Edge`, `CallReturn`, `QualifiedCallReturn`), `Project{base, step}`, and
`OpenNone`. Estimated ~1.5-2.5 MB bincode, 3-5x the flat map, ~20x under the
bag — reasoned, not measured; verify before committing a schema.

## The residue, which is real

1. **Rename transport and `--dump-package`** consume the bag as a data
   structure, not through the registry. No conclusion form serves them. The
   honest claim is "the TYPE CHASE never decodes the bag", never "nothing
   does".
2. **`Custom` payloads / non-default reducers** are unbakeable by
   construction. The plugin fingerprint's hard-clear must cover the
   conclusion column, and that is a discipline rather than a shape.
3. **Enrichment-overlay retries still decode and enrich the whole provider
   bag**, because enrichment is bag surgery. The layer makes the raw-tier hop
   cheap and cannot touch the enriched-tier hop.

## Traps

- `MethodSurface::ret` is a PRE-ENRICHMENT local conclusion
  (`surface.rs:62-66`): two providers with different enriched returns project
  byte-identical Surfaces. So only post-fold, post-finalize conclusions
  persist; invalidation rides the blob axis, never the Surface axis; enriched
  answers are never written back.
- The chase survives — hops get cheaper, not fewer. An answer still moves
  when a provider resolves, so the persisted map must never hold a
  materialized cross-file value.
- Any reducer change now changes SEMANTICS without changing shape, so it must
  bump `EXTRACT_VERSION` (or a sibling `CONCLUSION_VERSION` in the same gate
  family).
- Never bake through a degradation: a `QUERY_REC_DEPTH_CAP` truncation or a
  `degraded` analysis must persist `Link`/`OpenNone`, not the short answer.

## Verdict

**Stage 1 — worth building.** Store the bag in its own blob column. One
schema bump, zero new semantics, and it captures the rows-axis share
(69.3% of decodes discard the bag; 27.0% of decode time) while making
`--dump-package`/rename's full decode an explicit second read.

**Stage 2 — the conclusion layer — only on evidence.** The ceiling is small:
decode is ~13% of the tail, so eliminating bag decode entirely is worth ~7%,
and stage 1 already reaches most of what stage 2 would.

One argument for stage 2 needs correcting before anyone leans on it. The
design cites the provider chase at 61.6% of an enrichment build — that figure
is superseded. `docs/adr/enrichment-build-cost.md` records the resident-copy
export gate taking the chase 1,541 -> 240 ms; recomputed, the chase is now
**~20% of a build, not 61.6%**. The direction of the argument survives (a
conclusion map makes raw-tier consults near-free, and that share is still
rehydration-dominated) but its magnitude is three times smaller than stated.
Re-measure the chase post-stage-1 and let that number decide.
