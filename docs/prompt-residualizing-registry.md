# Design brief: a residualizing mode for the reducer registry

**Status: open question, evidence gathered, no implementation.** Written for a
design session. The conclusion layer (`docs/prompt-conclusion-layer.md`) is
built and landed; this is the one thing standing between it and most of its
value.

## The gap, in one number

Over a warm substrate `--check`, the consult path's conclusion lookups split:

| outcome | count | what it costs |
|---|---|---|
| `OpenNone` → decode | **91,525** | full blob decode + full chase |
| absent → proven `None` | 60,947 | nothing |
| `Link` → follow | 4 | — |
| `Value` / `ReturnOf` → answered | 1,301 | a hash lookup |

**57% of lookups land in `OpenNone`.** The layer exists to remove decodes, and
on more than half of its opportunities it declines to.

`Link` — the form whose entire purpose is "the answer is in another file" —
fires **44 times across 2,693 files**. It is, in practice, not implemented.

## Why the bake cannot mint the `Link`

The bake runs with `module_index: None`, deliberately: a materialized
cross-file value would freeze a world that can change without this file
changing (`docs/prompt-conclusion-layer.md`, "Edges, not values"). So a chase
that leaves the file returns `None` at bake time.

The bake then cannot distinguish two cases that look identical from inside:

1. **The bag genuinely has no answer.** The live path also answers `None`.
2. **The chase would have exited cross-file.** The live path has a real answer.

Today both become `OpenNone`, which is sound and expensive.

Treating them both as absent instead is unsound, and measurably so — **56
equivalence breaks per substrate check** under `PERL_LSP_CONCL_EQUIV`, all the
same shape:

```
MethodOnClass{ Log::Log4perl, get_logger }        => ClassName("Log::Log4perl::Logger")
MethodOnClass{ URI, new }                         => ClassName("URI::_foreign")
MethodOnClass{ Dist::Zilla::Role::TextTemplate, fill_in_string } => Optional(String)
MethodOnClass{ Plack::Request, uri }              => ClassName("URI")
```

Each is case 2. Each would have been served as "no answer" forever.

## Why `sole_foreign_edge` does not reach them

The existing `Link` minting looks for an attachment whose sole witness is
`Edge(MethodOnClass{class, name})` with `class` not declared locally. That
catches a direct foreign edge and nothing else — hence 44.

The cases above hold `Edge(Symbol(sid))`: a **local** symbol whose own chase
leaves the file, through its imports. Nothing local names the target, so there
is no key to point a `Link` at without re-deriving the chase — which is the
thing being avoided.

## The question for the session

**Can the registry report where it would have gone, instead of only what it
found?**

Concretely: a mode in which a chase that reaches a point requiring
`module_index` returns not `None` but something like `Residual(ConclusionKey)`
— the portable key it was about to consult. The bake stores that as `Link`;
the consult path follows it into the target file's map instead of decoding.

Sub-questions worth settling there, in rough priority:

1. **Is the exit point always nameable as a `ConclusionKey`?** The four
   observed shapes are `MethodOnClass`, which is portable. Are there exits that
   can only be named by a file-internal attachment (`Expr(span)`,
   `Expression(refidx)`)? Those must stay `OpenNone`, and knowing the ratio
   decides how much of the 57% is actually reachable.

2. **One residual, or several?** A chase may branch — several candidates, an
   inheritance fan-out. `Link` as specified holds one target. Either the form
   grows a set, or a multi-exit chase degrades to `OpenNone` and we measure how
   often that is.

3. **Where does the mode live?** A flag on `ReducerQuery`, a distinct entry
   point beside `query_rec`, or a `BagContext` whose `module_index` is a
   recording stub rather than `None`. The third is appealing — the stub records
   the key and answers `None`, so no reducer changes — but it makes the chase's
   *first* exit the recorded one, which may not be the one that would have
   answered.

4. **Termination and cost.** The bake is 592 µs/file today (~1.2% of gold
   wall). A residualizing chase does strictly more work per key than the one
   that bails at `None`. Budget before building.

5. **Does the `Link` follow need a cycle guard of its own?** The consult path
   would now traverse map-to-map across files. `VisitedKey` guards the live
   chase; the projection needs the equivalent, and `(file, key, receiver,
   arity)` is the shape the spec already names.

## Acceptance test, ready-made

`PERL_LSP_CONCL_EQUIV=1` with `PERL_LSP_ABSENT_ON_NO_ANSWER=1`.

That flag combination is currently **56 breaks**. It is exactly the population
this work must convert: every one of those is a chase that exits cross-file and
should have produced a `Link`. When the residualizing mode is right, those 56
become `Link`s and the run goes green — with the no-answer case now genuinely
meaning no answer.

Then the win is measurable as `consult.baked_open` falling from 91,525.

## What is already true, so the session does not re-derive it

- The bake is deterministic (`the_bake_does_not_depend_on_map_iteration_order`,
  mutation-verified), which the diff-propagation driver depends on.
- Absence is sound, and only because closedness is asked of the INDEX via
  `parents_of` — a per-file bake cannot establish it, because Perl packages are
  open and any file may reopen one without repeating its `@ISA`.
- End-to-end checks cannot score this work. Gold stayed 502/0 and
  `--dump-package` stayed byte-identical across 312 KB under a version with 633
  soundness breaks, because the ladder routes around a missing answer and only
  the cost differs. **Score changes here with `PERL_LSP_CONCL_EQUIV`, which
  compares at the point of the claim.**

---

## Slice 0 measurement: the bridge poison is sound and nearly always vacuous

Instrumented per the design answer's sub-question 1 (`residual.nameable` /
`residual.poisoned` / per-site), over one substrate `--check`.

| exit site | count | nameable? |
|---|---|---|
| `moc_primary` | 46,590 | yes |
| `parent_walk` | 46,590 | yes |
| `bridge` | 46,572 | **no — poisons** |
| `slot_type` | 533 | yes |

Read per EXIT that is 66.8% nameable. Read per CHASE it is far worse, and the
per-chase reading is the one that governs: the three big sites are sequential
fallbacks of the SAME chase (primary → parents → bridges), so a chase that ends
with no answer has hit all three, and one poisoned exit poisons the chase.
46,572 of 46,590 — **99.96% of chases touch the poisoning site.**

That would have ended this line of work. It is also wrong, and the thing that
makes it wrong is not visible from the bake.

**In LIVE mode, the bridge consult yields nothing 131,658 times against 2,251
that yield — it is vacuous 98.3% of the time.** A would-be consult that would
have returned nothing is not a dependence, and counting it as one makes the
poison rate look total when the real one is ~1.7%.

So the bake-time rule "a bridge exit poisons" is SOUND but pessimistic by a
factor of ~59, and the pessimism costs essentially the whole reachable
population. The bake cannot currently do better, because whether any file
bridges to class C is index-side knowledge and the bake has no index — by
design.

**Proposed refinement to the staging.** Make bridge-existence knowable at bake
time, so the exit poisons only when it would really have found something:

- an index-side set of classes that ANY file bridges to, consulted at bake —
  cheap to build (the bridge registry already exists for
  `for_each_entity_bridged_to`), and a set membership test rather than a walk;
- or the same fact recorded per class in the map, decided consult-side where
  the index is present — the shape the closedness check already uses, and it
  has the same "the property is global, not per-file" character that made
  closedness wrong to compute locally.

Either way the measurement to re-run afterwards is the per-chase poison rate,
not the per-exit ratio. **Whoever picks this up should not size the work from
the 66.8%.**

---

## Per-class bridge yield histogram (round-2 point 3)

The 98.3%-vacuous figure is per CALL; the guard is per CLASS. If the real yields
concentrate, those classes stay guarded-off permanently and the decode cost
lands exactly where bridges are real. Measured over one substrate `--check`:

**All 2,251 real yields fall in 13 distinct classes.**

```
415  Mojolicious::_AppSurface
389  Mojo::Server
281  Mojo::UserAgent
269  Mojolicious::Controller
264  Mojo::IOLoop
214  Mojolicious::Plugin::DefaultHelpers
124  Minion::Worker
 94  Mojo::IOLoop::Stream
```

Top eight are 2,050 of 2,251 — 91%. Every one is a Mojo/Minion app surface,
which is what the round-2 answer predicted.

Two consequences, and they point opposite ways:

**The guard is nearly free.** Thirteen classes lose trusted absence. Everything
else keeps it, so the guard costs approximately nothing against the ~127k
absences a check performs — it is not a tax on the layer, it is a fence around
a small pen.

**The follow-on is now sized, and it is small.** Refining "bridged → decode"
into "bridged → consult the bridging files' maps" recovers 2,251 consults, all
in 13 classes. Worth doing for correctness of shape rather than for the number:
it removes the last place where a conclusion form degrades to a decode for a
reason the layer could in principle represent. Anyone sizing it from the raw
`OpenNone` population (91,525) will be disappointed — that population is
dominated by other causes.

---

## The ladder-frame rule's other half, and what it cost the `Link`

Recording says where a chase would have gone. It does not say whether the
chase's answer IS what it finds there. `Link{targets}` claims "the first of
these keys that answers"; that is only the enclosing query's answer if every
frame between them returns the exit's answer unchanged.

Implemented as an **opaque-frame counter** on `QueryState`. `materialize`
enters one around every sub-chase that transforms rather than forwards, and
`note_exit` poisons instead of recording a rung while inside one. Nesting is
why it is a counter and not a flag: the recording site is the innermost frame
and has to know whether ANY ancestor is opaque.

Which frames are opaque, and why:

| frame | why it is not a rung |
|---|---|
| `Edge(Variable)` | the scope walk defers a rep-only answer and lets an outer class identity beat it — the value is chosen ACROSS scopes |
| `Edge(…)` with siblings | the answer is folded with the other witnesses at the attachment |
| `Edge(…)` re-dispatched | `fresh_dispatch_receiver` substitutes a different receiver; `ReturnExpr::Receiver` at the far end then answers about the wrong object |
| `CallReturn` | substitutes the call site's arity AND the dispatch receiver |
| `QualifiedCallReturn` | same, and the lookup class and receiver class deliberately differ |
| `Projected` | returns a value drilled OUT of the sub-chase's answer |
| depth cap | `None` because it ran out of frames, not because there is no answer |

The memo carries a "this subtree recorded an exit" bit alongside each value.
Without it a subtree first reached transparently, then re-reached from inside a
combining frame, returns from the memo and never re-runs `note_exit` — the
second reach silently launders into a rung.

**Result: follow breaks 44 → 0.** The 8 disagreements that remain are all one
shape (`PPI::Token::content`) and all classified `concl.follow_break_guarded`:
the shared cycle guard had that candidate key on the path, so the chase
returned without walking a rung, while the outer frame still standing on it
goes on to walk them. Reproducible across runs.

### And it is worth nothing on this substrate

| | minting off | minting on |
|---|---|---|
| `bagcache.decode` | 4103 | 4104 |
| `consult.baked_open` | 92,393 | 83,983 |
| `consult.baked_follow_incomplete` | 4 | 7,992 |
| `consult.baked_follow` | — | 34 |

Decodes do not move. `follow_one` abandons at the first rung whose map says
`Decode`, and with 84k `OpenNone` still in the maps that is nearly every walk —
the consult then falls through to the decode it would have done anyway. **The
`Link` cannot pay off while `OpenNone` dominates the rungs.** Leverage is in
shrinking that 84k, not in `Link` fidelity.

`PERL_LSP_MINT_LINKS` therefore stays OFF — now for a measured cost reason
rather than a soundness one.

Two findings worth carrying forward:

- **The self-rung.** The cross-file primary records the key being baked as its
  own first rung, which is true of the ladder and useless as a `Link`: the
  consult reached this map by doing exactly that. Left in, it converted
  essentially the whole `OpenNone` population into `Link`s that burn two follow
  hops and abandon — 14,923 incomplete against 15 answered. Filtered in
  `bake_one`.
- **`CallReturn` is the shape a widened `Link` would need**, and it is the one
  the form cannot express: `Link` carries ONE arity and ONE receiver rule, and
  a call frame substitutes both. Widening means residuals that carry their
  binders. Do not build it until the `OpenNone` population is smaller.

### Two measurement traps, both of which produced confident wrong numbers

- **A probe that re-runs the chase changes the run it measures.** Diagnosing
  the last breaks with an extra `attempt` on a fresh `QueryState` took breaks
  from 8 to 2030 and follows from 34 to 4084 — reproducibly, so it read as a
  real regression rather than as the instrument.
- **A classifier that cannot fail is not a classifier.** The first "is this
  break excusable?" probe asked whether a LINK TARGET was on the visited path.
  Before self-rungs were filtered the targets included the key being chased, so
  it matched every time and reported 100% of breaks as cycle-guard artifacts.
  The guard actually cuts on the CANDIDATE key.

### Two defects found on the way, one fixed

- **Fixed: the conclusion fingerprint hashed source but not the env that steers
  the bake.** One `--check` under `PERL_LSP_MINT_LINKS=1` leaves maps that every
  later run reads, and it took a gold row from PASS to FAIL until the cache was
  wiped by hand — looking exactly like a code regression. Bake-steering flags
  now join the fingerprint (`schema.rs::conclusion_fingerprint`); consult-side
  flags deliberately do not.
- **Open: a fingerprint change clears conclusions and nothing re-bakes them.**
  `validate_conclusion_fingerprint` keeps the blobs "because the repair is a
  re-bake", but no one drives that re-bake — the file is not re-persisted, so
  the layer stays dark until a full `--clear-cache`. `conclcache.known_absent`
  reads 156,746 in that state. This is answer-neutral (absent means decode) and
  purely a cost, but it means any measurement taken after a source edit without
  a full clear is measuring an empty layer. It belongs with the flush driver.
- **Fixed on the base, and it was ours: the warm-cache gold failure.**
  `gold-corpus/run.pl` failed `diagnostics/loader-config-conf-shape-closed` on
  a WARM cache and passed cold (502/0 cold, 501/1 warm, on both trees, so it
  predated this arc's commits). Root cause was the bag-column split:
  `record_loader_shapes` resolved each plugin's config value by asking the
  witness bag, and both warm scans decode without the bag column, so the
  projection silently produced nothing on every run after the first.

  Independently diagnosed and fixed on the base (#155) while this arc was in
  flight, by the better of the two available repairs: the warm scans now decode
  through `decode_analysis_parts`, so the copy is MARKED bag-evicted and a type
  query rehydrates instead of reading an empty bag as "no facts". That fixes
  every reader of a warm copy rather than the one that happened to be caught.

  The general form is worth naming either way: **a span plus "go ask the bag"
  is not a fact that survives a strip, and a copy that cannot say which axes it
  is missing makes every such reader silently wrong.** The residency discipline
  covers who may hold a whole copy; it does not yet cover who may DERIVE from
  one, nor require a stripped copy to be honest about it.

  Sweep of the other bag reads reachable from an index path, done after the
  fact: `module_resolver/thread.rs`'s transitive `symbol_return_type_via_bag`
  is safe (`strip_import_copy_one` clones and evicts the CLONE, and the walk
  only runs for names not already cached, i.e. right after a fresh parse), and
  `resolve/definitions.rs` / `resolve/hierarchy.rs` read the cursor's own open
  document, never stripped. The loader shape was the only one.

  One residual #155 does not cover, fixed here: **the config shape was not on
  the Surface.** An edit that only adds a key to a plugin's config hash changes
  no member anywhere, so the verdict read Unchanged, no open consumer
  re-enriched, and every one of them kept diagnosing against the old closed key
  set for the rest of the session. #155 fixes how the shape is READ; this is
  its invalidation, and the two are independent. Projected in
  `Surface::project`, which already runs on the whole analysis before any
  eviction — so it needs no second copy of the answer to keep in sync.

  It also says something about the harness — which the base has since acted on
  (#153): gold ran once against whatever cache was on the box, so the only run
  that still worked was the one being scored. It now runs cold and warm against
  a private throwaway cache dir, with a `warm: xfail` status for known gaps.

---

## The `OpenNone` population, attributed by cause — and the widening premise is dead

`Conclusion::OpenNone` now carries an `OpenReason`, counted at the CONSULT
rather than at the bake. That distinction is the whole point: a bake-side tally
counts KEYS, and the question a widening has to answer is which cause drives
DECODES. The two differ by however often each key is asked, which is exactly
the kind of unweighted total that has mis-sized every step of this arc.

One warm substrate `--check`, 92,256 decodes:

| cause | consults | share |
|---|---|---|
| absent, class known to the map but not provably closed | 64,901 | **70.3%** |
| no answer, chase was opaque (poisoned / recorded nothing) | 9,732 | 10.5% |
| no answer, chase named rungs a `Link` could carry | **8,583** | **9.3%** |
| no answer, only rung was the key being baked | 6,945 | 7.5% |
| absent, class the map has never heard of | 1,567 | 1.7% |
| binder-dependent (answer moved under receiver/arity) | 532 | 0.6% |

Two conclusions, and the first was not visible before this split.

**Widening the `Link` form addresses at most 9.3%, and realistically far less.**
`no_answer_linkable` is the ONLY population a binder-carrying residual could
convert. We already know from the minting measurement that a `Link` mostly
abandons at the first rung whose own map says `Decode` — 7,992 incomplete
follows against 34 answered — so the realizable fraction of that 9.3% is a
fraction of a fraction. **Do not build binder-carrying residuals.**

**The lever is the 70.3%, and it is a different shape entirely.** These are
absences on a class the map DOES conclude about, where the only thing stopping
the absence from being trusted is that the class inherits from somewhere this
file cannot see. The bake cannot enumerate an inherited method — it has no key
to attach anything to, because the name could be anything.

That points at a form this layer does not have: not per-key, but **per-class**.
"Any key absent for class C inherits from these parents" is one fact per class,
and the parents are the one thing the bake genuinely knows. An absent key would
then evaluate to a `Follow` constructed at consult time from the name being
asked, instead of a decode whose entire job is to walk to the same parents.

Stated as a hypothesis, not a finding, because one number would confirm or kill
it and I have not taken it: **of those 64,901, how many are answered by a parent
rung rather than by the candidate file itself?** If most are, the per-class form
converts them; if most resolve locally after the decode, it does not. That is
the measurement the next step should start from — the same "read the composition
before sizing the fix" discipline that produced this table.
