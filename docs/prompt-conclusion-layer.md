# Specification: the conclusion layer

**Status: specified, closes, staged; stage 2 gated on a post-stage-1
re-measurement (verdict at the end).**

A **conclusion** is the registry chase partially evaluated over what one file's
bag knows, with the three query binders (`ReducerQuery.point` / `.receiver` /
`.arity_hint`, `reducers.rs:12-38`) and the cross-file world left free. Free
binders residualize as syntax; the cross-file world residualizes only as a
`Link`. A per-file `ConclusionMap` persists next to the blob and serves the
cross-file consult path without decoding the bag; the bag remains the
derivation of record and the fallback.

```rust
/// Portable, cross-file-enterable key. Strings only — see "Keys" below.
pub enum ConclusionKey {
    MethodOnClass { class: String, name: String },
    SubByName(String),
    SlotType { class: String, key: String },
    TypeName(String),
}

/// One key's partially-evaluated answer.
pub enum Conclusion {
    Value(InferredType),
    ReturnOf(ReturnExpr),
    Timeline(Vec<TimelineSegment>),          // λ point — bake-internal, never persisted
    Link { target: ConclusionKey, arity: Option<u32>, receiver: ReceiverRule },
    Project { base: Box<Conclusion>, step: ProjectionStep },
    OpenNone,
}

pub struct TimelineSegment { pub span: Span, pub value: InferredType }

pub enum ReceiverRule {
    /// Pass the evaluating query's receiver through unchanged (inheritance hop).
    Thread,
    /// `fresh_dispatch_receiver(incoming, class)` (registry.rs:92-107): keep
    /// the incoming receiver iff its class IS `class` or a subclass of it;
    /// otherwise substitute `ClassName(class)`.
    Dispatch(String),
}

/// Per-file, persisted with the blob, invalidated with it (same stamp),
/// versioned by CONCLUSION_VERSION in the EXTRACT_VERSION gate family
/// (`schema.rs:13`).
pub struct ConclusionMap(HashMap<ConclusionKey, Conclusion>);
```

**Evaluation contract.** A conclusion query is `(key, receiver:
Option<InferredType>, arity: Option<u32>, args: Vec<InferredType>)` — there is
deliberately no `point` (see `Timeline`). The evaluator mirrors
`query_rec_body`'s consult ladder (`registry.rs:462-769`): per visible def
candidate, look the key up in that file's map; on a form, evaluate it; then
the inheritance walk over `PackageFacts::parents` (pinned fields, not the
bag); then bridges. Cycle guard and depth cap mirror `VisitedKey`
(`registry.rs:25`: attachment + receiver identity + arity, here `(file, key,
receiver, arity)`). Three outcomes per lookup:

- **a form** — evaluate per the sections below; no bag decode.
- **`OpenNone`** — this key is unbakeable here; decode the blob, rebuild
  `BagContext`, run the real registry chase (`registry.rs:273`). Full cost,
  paid for this key only.
- **absent** — the bag deterministically answers `None` for this key; serve
  `None` with no decode and let the ladder move on (parents, next candidate) —
  exactly what a local-reducer miss does today (`registry.rs:417-427` falls
  through to the fallbacks).

The absent-means-None split is the sharpest knife in the design: it is sound
only if the bake enumerates **every key the bag could answer** — derivable
from the bag's attachment index (`mod.rs:44-48`, keyed by attachment), the
`SymbolTable` names, and the file's declared bridges. A key-production site
the enumeration misses turns "the bag would answer" into a silent `None`.

## Keys: `ConclusionKey` vs internal ids

The four variants are exactly the four measured cross-file consult shapes:
`MethodOnClass` 106,533 (96.0%, counter `registry.rs:532`), `SlotType` 4,367
(`registry.rs:681`), bridged 42 (`registry.rs:619`), `imported_sub_return` 5
(`query.rs:143`). Every variant is strings-only: mintable by an asker holding
nothing but names, stable across any edit of the provider.

The internal ids — `SymbolId`, `ScopeId`, `RefIdx`, `Span` — are per-`FileAnalysis`
and shift on unrelated edits. They never appear in a key. The code already
enforces the boundary: every cross-file entry today lands on a strings-only
attachment (`MethodOnClass` at `registry.rs:462-549`, `SlotType` at
`registry.rs:640-694`, `TypeName` at `registry.rs:730-758`) or resolves a
**name** to a local `Symbol(sid)` on the provider's side (`query.rs:91-106` —
`SubByName` is that lookup given a key spelling). The one exception is the
bridged hop, which enters at `Symbol(sym.id)` because "per-FA SymbolIds can't
be portably edge-encoded" (`registry.rs:444-447`); the bake closes it from the
other side — the bridging file knows its own bridges, so it writes those
entries under `MethodOnClass{bridged_class, name}`.

Internal ids MAY appear *inside* a persisted `Conclusion`'s `InferredType`
(e.g. `CodeRef { return_edge: Some(Expr(span)) }`, `types.rs` of
`file_analysis`, lines 26-49): unlike the Surface, the map is co-persisted and
co-invalidated with the blob whose bag those ids index, so they are
stamp-consistent — but *interpreting* one later requires the bag (see the
`Value` break below).

The in-RAM precedent already exists: the resolution session's `CandidateKey`
(`session.rs:48-58`) memoizes cross-file consult answers keyed `(path,
attachment, receiver, arity, point, framework)` for one walk. The map is the
persisted, point-free, epoch-independent projection of that key.

---

## `Value(InferredType)`

**Type.** The `InferredType` enum of `model/file_analysis/types.rs:17` —
`ClassName`, `HashWithKeys`, `Parametric(..)`, `Sequence`, etc. — stored
verbatim, constant in all three binders.

**Real Perl.**

```perl
package My::Client;
sub _build_ua {
    my $self = shift;
    return LWP::UserAgent->new( timeout => 10 );
}
```

Witnesses: the walk types the constructor call `ClassName("LWP::UserAgent")`
(the `Foo->new(...) → ClassName(Foo)` convention, `build/builder/chain.rs:17`)
on the call's `Expr(span)`; the return arm pushes `SymbolReturnArm(sid) →
Edge(Expr(call_span))` and `Symbol(sid) → Edge(SymbolReturnArm(sid))`
(`types.rs:54-60`); writeback publishes `MethodOnClass{"My::Client",
"_build_ua"} → Edge(Symbol(sid))` (tag `local_return`, fold step 6). Reducers:
the registry materializes the edge chain (`registry.rs:784-864`),
`SymbolReturnArmFold` (`reducers.rs:426-462`) folds the one arm via
`join_return_arms` (`file_analysis/types.rs:938`).

Baked, under both `MethodOnClass{"My::Client","_build_ua"}` and
`SubByName("_build_ua")`:

```
Value(ClassName("LWP::UserAgent"))
```

**Evaluation.** Hash lookup; return the type unchanged. `receiver`, `arity`,
`point` are all ignored — the bake has proven the answer constant in them.
For the snippet: `ClassName("LWP::UserAgent")`, zero hops, zero decode.

**Where it breaks.** Anything non-constant. Receiver-dependence (a fluent
accessor baked as `Value` would hand the *declaring* class to a subclass call)
must be `ReturnOf`; arity-dependence likewise; cross-file dependence must be
`Link` — a materialized cross-file value would freeze the world, and the
constraint is hops cheaper, never fewer. Second break: an embedded
non-portable attachment. `Value(CodeRef { return_edge: Some(Expr(span)) })`
serves the *type* fine, but a consumer that later invokes the coderef must
chase `Expr(span)` — not a `ConclusionKey` — so that drill falls back to the
full blob decode. The map hands out answers, not the ability to keep deriving.

## `ReturnOf(ReturnExpr)`

**Type.** The existing `ReturnExpr` (`types.rs:270-311`): `Concrete` /
`Receiver` / `ReceiverOr` / `Operator(RowOf | ParamOf | InstanceOf)` /
`UnionOnArgs { branches: Vec<(ArgGuard, ReturnExpr)> }` / `Arg(u32)`.
`ReturnExpr` is ALREADY a dependent conclusion — `(receiver, arity, args) →
InferredType` — and the form stores the payload verbatim; evaluation reuses
`eval_return_expr` (`reducers.rs:867-941`) unchanged.

**Real Perl.**

```perl
package Mojo::UserAgent;
use Mojo::Base -base;
has ioloop => sub { Mojo::IOLoop->new };
```

Witnesses: `visit_has_call` synthesizes the `ioloop` Method symbol and the
Mojo::Base accessor synthesis pushes onto `Symbol(sid)` and
`MethodOnClass{"Mojo::UserAgent","ioloop"}`:

```
ReturnExpr(UnionOnArgs { branches: [
    (Empty,      Concrete(ClassName("Mojo::IOLoop"))),   // getter
    (AtLeast(1), Receiver),                               // fluent writer
]})
```

Reducer: `ReturnExprReducer` (`reducers.rs:797-859`), registered second so
declarative shapes dominate writeback (`registry.rs:222-255`).

Baked: `ReturnOf(<that same UnionOnArgs>)` — literally the witness payload,
moved out of the bag.

**Evaluation.** Run `eval_return_expr` against the query. For
`$ua->ioloop` where `$ua : ClassName("My::UA")` (a subclass — the receiver
survives the dispatch hop via the subclass pass-through,
`registry.rs:97-106`): arity `Some(0)` → guard `Empty` fires →
`ClassName("Mojo::IOLoop")`. For `$ua->ioloop($loop)`: arity `Some(1)` →
`AtLeast(1)` → `Receiver` → `ClassName("My::UA")`. Hint-less introspection:
no `Any` branch → the `Empty` fallback (`reducers.rs:913-938`) →
`ClassName("Mojo::IOLoop")`.

**Where it breaks.** `ReturnExpr` is a *substitution* language, sealed over
`(receiver, args)` — no variant names another attachment or key (deliberate:
`types.rs:255-268`). A default sub that *chases* — `has ua => sub {
shift->build_ua }` — is not a substitution; its answer routes through edges
and must residualize as `Link`. And a `UnionOnArgs` whose branches were
frozen at fold time carries only what the build-time fold resolved: an arm
that chains through an import resolves only under enrichment, which never
persists — the persisted map misses it exactly as the persisted bag does
(the R4 residue, unchanged).

## `Timeline` (λ point)

**Type.** `Vec<TimelineSegment>` — span-scoped verdicts, latest-emitted-wins
within containment, evaluated by picking the fold's verdict at a point. Keyed
by `Variable { name, scope: ScopeId }` — an **internal** id, so a Timeline has
no `ConclusionKey`: it exists in the bake's working set and never on disk.

**Real Perl.**

```perl
my $obj = { name => 'origin' };   # (1)
$obj->{x} = 0;                    # (2)
bless $obj, 'Point';              # (3)
$obj->draw;                       # (4)
```

Witnesses on `Variable{"$obj", scope}`: (1) the TC mirror pushes
`InferredType(HashWithKeys{[("name",_)], open:false})`; (2) the
mutation-extension pass (`query.rs:269-387`) pushes the extended
`HashWithKeys{[name,x],..}` at a zero-width span at the write; (3)
`Observation(ClassAssertion("Point"))` + `Observation(BlessTarget(Hash))`.
Reducer: `FrameworkAwareTypeFold` — Observations are temporal: witnesses past
the query point are skipped (`reducers.rs:169-173`; the `ReturnExpr` sibling
gate is `reducers.rs:832-838`), spans narrow (narrowest containing span,
`reducers.rs:124-141`), and class identity dominates rep
(`reducers.rs:250-266`). So the variable's conclusion is a step function:

```
Timeline([
    { span: (1)..,        value: HashWithKeys{[name]} },
    { span: (2)..(2),     value: HashWithKeys{[name,x]} },
    { span: (3)..,        value: ClassName("Point") },
])
```

The bake stores the *fold's verdict per breakpoint*, not the witnesses — it
runs the fold at each witness start. Segments carry spans, not bare start
points, because narrowing is 2-D: a flow-narrowed guard region
(`docs/adr/flow-narrowing.md`) contributes a verdict that *ends*, which a
breakpoint-only step function cannot express.

**Evaluation.** Given a point: among segments whose span contains it (or, for
zero-width write segments, whose start ≤ it), the latest-starting wins. At
(4): `ClassName("Point")`. Between (2) and (3): `HashWithKeys{[name,x]}`.

**Where it breaks — and why never persisting it is free.** The key is
internal, so no cross-file asker can name a Timeline, and the code enforces
point-freedom at every boundary independently: all five `ReducerQuery`
constructions in `witnesses/query.rs` are `point: None` (lines 39, 68, 109,
199, 309 — each cross-file recursion rebuilds context from the provider's own
`scopes`/`packages`, `query.rs:99-106`); the sole `point: Some(..)` in the
module is `registry.rs:1042`, the intra-bag scope-chain walk. And the binder
is always **applied at the hop**: an edge entering a `Variable` fixes the
point from bake-time constants — `Expr(span).start` when chased from an
expression, `scope_point(scope)` otherwise (`registry.rs:807-809`) — so
composition through a variable selects ONE segment and the Timeline collapses
into the composing conclusion at bake. The only consumer of an *unapplied*
timeline is `inferred_type_via_bag(var, point)` (`queries.rs:281`) — hover /
inlay / completion on an open document, where the bag is resident anyway.
Persisting timelines would cost ~9-18 MB (16-32% of the bag) to serve a path
that never decodes.

## `Link { target, arity, receiver }`

**Type.** As defined at the top: a portable `ConclusionKey` target, an
optional arity that overrides the evaluating query's hint, and a
`ReceiverRule`. It subsumes three payloads and one hop kind:

| today | as Link |
|---|---|
| `Edge(MethodOnClass{c,m})` from a non-`MethodOnClass` attachment (fresh dispatch, `registry.rs:816-836`) | `target: MethodOnClass{c,m}`, `arity: None`, `receiver: Dispatch(c)` |
| `Edge(MethodOnClass{parent,m})` from a `MethodOnClass` (inheritance hop, `registry.rs:822-826`) | `receiver: Thread` |
| `CallReturn { target, arity }` (`types.rs:182`, chased at `registry.rs:865-894`) | `arity: Some(n)`, `receiver: Dispatch(target.class)` |
| `QualifiedCallReturn { method_lookup, receiver_class, arity }` (`types.rs:198-202`, chased at `registry.rs:971-995`) | `target: method_lookup` key, `arity: Some(n)`, `receiver: Dispatch(receiver_class)` |

**Real Perl.**

```perl
package My::Schema::ResultSet::Users;
use base 'DBIx::Class::ResultSet';
sub active {
    my $self = shift;
    return $self->search({ active => 1 });
}
```

Witnesses: `$self` gets `Observation(FirstParamInMethod{..})`; PostFold fills
the call's invocant class; `emit_method_call_return_edges`
(`build/builder/fold.rs:990-1013`) pushes `Expression(ridx) → CallReturn{
MethodOnClass{"My::Schema::ResultSet::Users","search"}, arity: 1 }`; the
return chain is `Symbol(sid) → Edge(SymbolReturnArm(sid)) → Edge(Expr(call
span)) → Edge(Expression(ridx))`; writeback publishes
`MethodOnClass{Users,"active"} → Edge(Symbol(sid))`. The bake (module-index-
free) collapses every internal-id hop in that chain and stops at the first
step that needs the world:

```
MethodOnClass{"My::Schema::ResultSet::Users","active"} ↦
Link { target:   MethodOnClass{"My::Schema::ResultSet::Users","search"},
       arity:    Some(1),
       receiver: Dispatch("My::Schema::ResultSet::Users") }
```

**Evaluation.** For `my $rows = $users_rs->active;` (receiver
`ClassName("...Users")`, arity 0): (1) the asker's consult finds the `Link`;
(2) rebind — arity := `Some(1)`, receiver: the `Dispatch` rule sees the
incoming receiver's class equals the target class → passes it through
(`registry.rs:97-106`); (3) re-enter the ladder at the target key: the Users
file's own map is absent for `search` → serve local `None`, walk
`PackageFacts::parents` → `DBIx::Class::ResultSet`, whose (plugin-declared)
entry is `ReturnOf(Operator/Receiver …)` → substitutes the threaded receiver.
The hop count is identical to today's chase — the answer still moves when a
provider resolves — but each hop is a map lookup instead of a bag decode plus
registry recursion. The removable cost is that recursion: `consult.attempt`
2,262 ms over 109,360 calls vs `consult.bag_present` (rehydrate) 621 ms —
78% compute, identical cold vs warm (2,251.7 vs 2,262.2 ms).

**Where it breaks.** (a) The target must be a *static* portable key. A
receiver-dependent target — `$self->$method(@_)` past constant folding,
`ref($x)->new` — has no key to write; the sub's entry is `OpenNone` (today
those sites also pin no edge and count into `dynamic_dispatch_sites`,
`file_analysis/mod.rs:258`). (b) Cycles/depth: the evaluator's visited set
answers `None` on re-entry, mirroring `registry.rs:395-397`; its depth cap
mirrors `QUERY_REC_DEPTH_CAP` (`registry.rs:191-194`). (c) The enriched-tier
hop: when a provider's raw answer dead-ends because the provider's own
imports need enriching, today's fallback-on-miss decodes and enriches the
whole provider bag (`registry.rs:528-543`; symmetric arms at 610-625,
671-691; `query.rs:121-151`). Enrichment is bag surgery; no conclusion form
serves it. The `Link` makes the raw-tier hop near-free and leaves the
enriched-tier hop exactly as expensive as it is.

## `Project { base, step }`

**Type.** `base: Box<Conclusion>` (recursive — the base is itself any form),
`step: ProjectionStep` (`types.rs:250-253`: `HashKey(String)` |
`ArrayIndex(i32)`). Subsumes the `Projected` payload (`types.rs:230-233`) and
its chase arm (`registry.rs:895-970`).

**Real Perl.**

```perl
package My::Worker;
sub _dbh {
    my $self = shift;
    return $self->config->{dbh};
}
```

Witnesses: the drill emits `Expr(drill_span) → Projected{ base:
Expression(r_config), step: HashKey("dbh") }`; `Expression(r_config)` carries
`CallReturn{ MethodOnClass{"My::Worker","config"}, 0 }`; the usual return
chain hangs off `Symbol(sid)`. Baked:

```
MethodOnClass{"My::Worker","_dbh"} ↦
Project { base: Link { target: MethodOnClass{"My::Worker","config"},
                       arity: Some(0), receiver: Dispatch("My::Worker") },
          step: HashKey("dbh") }
```

**Evaluation** (mirrors `registry.rs:895-970`): evaluate `base`; then narrow.
If `config`'s entry is `Value(HashWithKeys{[("dbh", Some(ClassName("DBI::db"))), …]})`,
`key_value_type("dbh")` answers `ClassName("DBI::db")` directly. If the base
evaluates to a *class-typed* value instead (`ClassName("My::Config")`), the
structural drill can't answer and the evaluator mints a follow-on
`SlotType{"My::Config","dbh"}` lookup — a fresh key into My::Config's map and
up its ancestry, exactly the `SlotType` fallback ladder today
(`registry.rs:929-957` hands off to `registry.rs:640-721`). `ArrayIndex(i)` →
`element_at(i)`.

**Where it breaks.** A base with no structure axis: bare `HashRef` (rep-only
evidence) has no per-key types → honest `None`. A dynamic key `->{$k}` never
emits `Projected` in the first place (constant-folded `$k` does — provenance
rule 9), so there is nothing to bake and the entry is absent-or-`OpenNone`
depending on what else the sub returns. `ArrayIndex` on a non-`Sequence` →
`None` via `element_at`.

## `OpenNone`

**Type.** A unit variant. It means: *this key's local derivation is
unbakeable — the bag may still answer; decode it.* Deliberately payload-free:
the *why* lives in ghost-stats counters, not the map.

**Real Perl.** Any path whose fold consumes what the algebra cannot express:

```perl
package My::App;
use My::DSL;                     # .rhai plugin pushing Custom payloads
resource user => ( ... );        # plugin-synthesized entity
```

If the plugin's emissions put a `Custom { family, json }` payload
(`types.rs:221`) — or a `Fact` of a family no default fold consumes — on the
subgraph reachable from `MethodOnClass{"My::App","user"}`, the bake cannot
reproduce whatever a plugin-registered reducer would do with it. Likewise a
bake whose own chase hits `QUERY_REC_DEPTH_CAP` (`registry.rs:360-379`): the
resulting `None` is a truncation, not a verdict, and baking it — or baking
any answer derived through it — would freeze a degradation. Both cases write:

```
MethodOnClass{"My::App","user"} ↦ OpenNone
```

**Evaluation.** Decode the blob (`blob.rs:115-120`), rebuild `BagContext`
from the rehydrated analysis, run `ReducerRegistry::query`
(`registry.rs:273`) — today's path, paid per-key. Sibling keys in the same
file still answer from the map.

**Where it breaks.** `OpenNone` is honest but blunt: it cannot say *how much*
of the derivation was open, so one `Custom` witness on a hot class's method
subgraph makes every consult of that key pay full price. And the
`OpenNone`-vs-absent boundary is where the bake's soundness lives: mislabel a
should-be-`OpenNone` key as absent and the evaluator serves `None` where the
bag would have answered — silently. The conservative rule: any doubt at bake
time (unrecognized payload family, cap hit, any non-default reducer in play)
writes `OpenNone`, never absence, never a value.

---

## Closure table

Payload kinds (`types.rs:160-247`) against the chase's measured reads
(2,262 ms / 109,360 cross-file consults; per-payload counts from the hop
counters at `registry.rs:316-330`):

| payload | reads (all attachments) | subsumed by |
|---|---|---|
| `Edge` | 2,166,944 (47.5%) | `Link` when the target is portable; **collapsed at bake** when internal (`Expr`/`Expression`/`Symbol`/`Variable` chains fold into the owning key's form) |
| `Observation` | 1,298,645 (28.5%) | `Timeline`, via the fold — bake-internal; binder applied at every hop (`registry.rs:807-809`), so nothing persists. Zero `Observation` reads occur at `Expr` attachments — the fact that makes the expression subgraph collapsible |
| `InferredType` | 554,032 | `Value` |
| `Fact` | 163,457 | folded at bake (`undef_arm` → `join_return_arms`; `mutation` → shape extension) — no residual form; unrecognized family → `OpenNone` |
| `CallReturn` | 137,682 | `Link { arity: Some }` |
| `Projected` | 134,578 | `Project` |
| `ReturnExpr` | 62,344 | `ReturnOf` (payload verbatim, `eval_return_expr` reused) |
| `QualifiedCallReturn` | 10,484 | `Link { receiver: Dispatch(receiver_class), arity: Some }` |
| `Derivation` | — | not a type payload; rename transport walks it in the bag — **unserved residue** |
| `Custom` | — | `OpenNone` |
| `DomainCompare` | — | off the flow chase (`Field` axis, human surfaces only) — not served |

## The bake

Runs post-fold, post-finalize, in the persist path beside blob encode
(`blob.rs:107-110`), on non-degraded analyses only — degraded analyses are
never persisted at all (`file_analysis/mod.rs:269-277`), so the map inherits
that gate. The bake's registry runs with `module_index: None`: no cross-file
value can be materialized, and every fallback that would consult the index
(`registry.rs:457-769`) residualizes as `Link` instead. Key enumeration:
the bag's attachment index + symbol names + declared bridges. Degradation
rules: cap hit or unrecognized payload → `OpenNone`, never the truncated or
guessed answer.

## Constraints (binding on any implementation)

1. **`MethodSurface::ret` is not this layer** (`surface.rs:62-66`): it is a
   pre-enrichment LOCAL conclusion, and two providers with different enriched
   returns project byte-identical Surfaces. Only post-fold conclusions
   persist, and invalidation rides the blob axis (per-file stamp,
   `blob.rs:43-55`) — never Surface equality. Enriched answers are never
   written back.
2. **Hops cheaper, never fewer.** `Link` preserves every hop; the map never
   holds a materialized cross-file value. An answer still moves when a
   provider resolves.
3. **Reducer changes now change semantics without changing shape.** Any edit
   to a reducer, to `eval_return_expr`, or to registration order
   (`registry.rs:218-257`) must bump `EXTRACT_VERSION` (`schema.rs:13`) or a
   sibling `CONCLUSION_VERSION` in the same gate family (as `REF_ROWS_VERSION`
   `schema.rs:19` and `STUB_VERSION` `stubs.rs:22` already are).
4. **Never bake through a degradation** — `QUERY_REC_DEPTH_CAP`, a `degraded`
   analysis, a truncated chase: `OpenNone` or nothing.
5. **Unserved residue, permanent:** rename transport and `--dump-package`
   consume the bag as a data structure, not through the registry — the honest
   claim is "the type chase never decodes the bag", never "nothing does".
   `Custom` payloads and non-default reducers are unbakeable by construction
   (the plugin-fingerprint hard-clear must cover the map). The
   enriched-overlay retry still decodes and enriches whole provider bags.

## Corrections to the prior draft (code wins)

- The sole `point: Some(..)` construction is **`registry.rs:1042`** (was cited
  as :1034).
- The binder-application-at-hop site is **`registry.rs:807-809`** (was
  :799-801).
- The temporal gates: the Observation skip is **`reducers.rs:169-173`**
  inside `FrameworkAwareTypeFold::reduce` (cited as :166-181 — that range now
  also spans unrelated lines), and the `ReturnExpr` sibling is
  **`reducers.rs:832-838`**. The five `point: None` sites in
  `witnesses/query.rs` are lines 39, 68, 109, 199, 309. All verified
  2026-08-22.

## Verdict

**Stage 1 — a separate bag blob column — is worth building now.** One schema
bump, zero new semantics: the bag (56,932,990 bincode / 6,697,721 zstd —
41.5% of stored bytes, 52.9% of bincode payload) moves to its own column,
decoded on demand; `--dump-package` and rename's full read become an explicit
second fetch. It captures the decode-side share by itself and creates the
measurement seam stage 2 needs.

**Stage 2 — this layer — only on post-stage-1 evidence.** The map lands
between the flat one-return-per-sub table (534,938 / 276,551 — which does not
close: 22.7% of `MethodOnClass` queries enter an `Expr`) and the bag;
estimate low-single-digit MB, verify before committing a schema. The target
is the compute half — `consult.attempt`'s 2,262 ms is 78% of the removable
cost and identical cold vs warm — so re-measure that counter once stage 1 and
the session memo have taken their share, and let the number decide.
