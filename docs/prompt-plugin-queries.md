# Design: query-declared plugin capture

**Status: design, not landed.** Rework of the plugin *capture* layer:
instead of the builder pre-capturing items of interest for every call
site and every plugin, each plugin declares the shapes it cares about as
tree-sitter queries. Core compiles them once, runs them in one pass per
file, and hands each match to the plugin with exactly the decision-ready
data that pattern asked for. The emission side (`EmitAction`), the
manifests, and the cursor-time query hooks are untouched.

Prereq reading: `docs/adr/plugin-system.md` (the current system),
`docs/spike-query-extraction.md` (what queries can and cannot express —
the "three rings"), `src/plugin/mod.rs` (`CallContext`, `ArgInfo`,
`FrameworkPlugin`), CLAUDE.md rules #1, #8, #10.

## 1. The problem: pre-capture

Today's emit-hook dispatch works like this (`builder.rs`
`base_call_context` / `dispatch_method_call_plugins` and the call sites
in `visit_function_call` / `visit_method_call`):

For **every** function call and method call in **every** file, the
builder eagerly builds a `CallContext`: flattens the arg list, builds an
`ArgInfo` per argument (constant-folds strings through
`constant_strings`, classifies `value_shape` one level deep, extracts
anon-sub params, projects `callable_return_edge` through the bag,
resolves `ref_sub_name`), computes the transitive parent list, clones
the package-uses vec, resolves the receiver's type, and serializes the
whole thing across the Rhai boundary for each trigger-applicable plugin.
Each plugin then string-compares `ctx.method_name` against its verb list
and returns `[]` — which is what happens for ~99.9% of call sites.

Four compounding costs:

1. **Compute.** The full `ArgInfo` pre-capture is paid whether or not
   any plugin will match. Bundled plugins mean `plugins.is_empty()` is
   never true in production, so this is every call in every file, on
   every build, including the Rayon workspace index and every dependency
   file the resolver touches.

2. **`CallContext` is a god-struct.** Every new plugin need grows it
   with another optional field populated for everyone but meaningful to
   one consumer: `has_options`, `arg_names`, `receiver_route_defaults`,
   `receiver_call_name`, `receiver_is_package`, `content_span`,
   `sub_params`, `ref_sub_name`, … The `arg_name_verbs()` manifest
   exists *purely* to gate one of these fields' cost — a manifest about
   capture, which is exactly the thing the plugin should be declaring as
   its interest, not negotiating field-by-field.

3. **Interest is opaque to core.** What a plugin matches is buried in
   imperative Rhai `if` chains. Core cannot index it, cannot skip work
   for it, cannot verify it, and cannot report "this plugin never
   matched anything in your workspace" — which feeds the documented
   silent-drop failure mode.

4. **The hook vocabulary is closed.** Dispatch keys on exactly three
   syntactic events (use, function call, method call). A plugin that
   cares about a different shape — a sub attribute, a hash literal in a
   known position, a chained `__PACKAGE__->meta->…` — has no hook, so
   either `CallContext` grows again or the behavior gets hardcoded
   natively (`visit_group_accessors` is one such fossil).

## 2. The shape of the fix

A plugin declares `patterns()`: named tree-sitter queries plus, per
capture, a list of **projections** — which decision-ready views of the
captured node it wants. Core compiles all patterns from all loaded
plugins into one merged `Query` per language, runs one `QueryCursor`
pass per file in a named build phase, evaluates predicates, gates each
match by the plugin's triggers *at the match site's package*, computes
the declared projections for actual matches only, and calls the
plugin's `on_match(pattern_name, m)`, which returns `Vec<EmitAction>`
exactly as the emit hooks do today.

The division of labor is the spike's three rings, applied to plugins:

- **Ring 1 (syntactic selection) → the `.scm` pattern.** Node kinds,
  field structure, literal verb names via `#any-of?`. This replaces both
  the per-call hook invocation and the plugin-side verb string-compares.
- **Ring 2 (pattern-inexpressible syntax) → core projections.** Constant
  folding, separator-agnostic pair walking, sub-param extraction,
  value-shape classification — the `cst.rs` catalog, encoded once in
  core, applied lazily per match. Patterns never attempt these
  (the spike showed the medium fails silently there).
- **Ring 3 (semantics) → the plugin's `on_match` + the engine.** What a
  matched registration *means* — which `EmitAction`s to mint — stays
  Turing-complete Rhai. Types stay in the witness bag.

Slogan: **the query is the filter, projections are the view, the hook
stays the brain.** Rule #1 is preserved — plugins still never touch
nodes; a pattern is data *about* the tree, executed inside the
sanctioned tree consumer. Rule #8/#10 are strengthened — verb
vocabularies move out of core entirely (no more `arg_name_verbs`-style
cost-gating manifests).

## 3. What a plugin looks like after

`frameworks/mojo-events.rhai`, ported (compare with the current
`on_method_call` version):

```rhai
fn id() { "mojo-events" }

fn triggers() {
    [ #{ ClassIsa: "Mojo::EventEmitter" }, #{ ClassIsa: "Mojolicious::Plugin" } ]
}

fn patterns() {
    [
        #{
            name: "event_call",
            query: `
                (method_call_expression
                  invocant: (_) @recv
                  method: (identifier) @verb
                  (#any-of? @verb "on" "once" "subscribe"
                                  "emit" "unsubscribe" "has_subscribers" "catch")
                  arguments: (arguments . (_) @event . (_)? @callback)
                ) @call
            `,
            projections: #{
                recv:     ["text", "type"],
                event:    ["str", "content_span"],
                callback: ["sub_params", "callable_edge"],
            },
            expect: [
                #{ src: "$e->on(ready => sub ($s) {});", matches: 1,
                   captures: #{ verb: "on", event: "ready" } },
                #{ src: "$e->on($dynamic, sub {});", matches: 1 },
                #{ src: "$e->off('x');", matches: 0 },
            ],
        },
    ]
}

fn on_match(pattern, m) {
    // pattern == "event_call"; m.captures.verb.text, m.captures.event.str,
    // m.captures.recv.type, m.package, m.span — only what was declared.
    ...same body as today's on_method_call, minus the verb if-chain...
}
```

The body logic is unchanged; the `if m == "on" || …` gate and the
`ctx.args[0]` positional digging become structure in the pattern.

## 4. The contract

### 4.1 `patterns()` → `[PatternSpec]`

Read once at plugin load (like every manifest), cached on the plugin
struct. Rust trait additions:

```rust
fn patterns(&self) -> &[PatternSpec] { &[] }
fn on_match(&self, pattern: &str, m: &MatchContext) -> Vec<EmitAction> { Vec::new() }
```

```rust
pub struct PatternSpec {
    pub name: String,                 // unique within the plugin
    pub language: String,             // "perl" (default) | "cpp" | … — which grammar
    pub query: String,                // tree-sitter query source
    pub projections: HashMap<String, Vec<Projection>>, // capture name → views
    pub expects: Vec<PatternExpect>,  // self-verification, §7
}
```

A pattern that fails to compile disables **that pattern** (not the
plugin), logs at `log::error!`, and is a hard error in
`--plugin-check`.

### 4.2 Projection vocabulary (closed, core-owned)

Every projection is an existing extractor, moved behind a name. Each is
computed per matched capture node, on demand — never speculatively.

| projection | applies to | yields | today's equivalent |
|---|---|---|---|
| `text` | any node | `String` | `ArgInfo.text` (always included, free) |
| `span` | any node | `Span` | always included, free |
| `str` | literal / bareword / var | `Option<String>` | `ArgInfo.string_value` (folds through `constant_strings`) |
| `strs` | same | `Vec<String>` | `ArgInfo.string_values` (loop/`qw` fan-out) |
| `content_span` | string literal | `Option<Span>` | `ArgInfo.content_span` (inside the quotes) |
| `shape` | any expression | `ValueShape` | `ArgInfo.value_shape` |
| `pairs` | paren/brace list | `Vec<(String, String)>` | `cst::pair_nodes` — separator-agnostic positional pairing (the fat-comma discipline, encoded once; today's Rhai-side `classified_pairs` host fn re-pairs projected args and retires onto this) |
| `list` | list/args node | `Vec<(String, Span)>` | `CallContext.arg_names` via `cst::string_list` |
| `sub_params` | anon sub / block | `Vec<EmittedParam>` | `ArgInfo.sub_params` |
| `callable_edge` | sub / coderef expr | `Option<WitnessAttachment>` | `ArgInfo.callable_return_edge` |
| `ref_sub_name` | refgen | `Option<String>` | `ArgInfo.ref_sub_name` |
| `type` | any expression | `Option<InferredType>` | `receiver_type` / `ArgInfo.inferred_type` (bag query at the node's span) |
| `route_defaults` | any expression | `Vec<(String, String)>` | `receiver_route_defaults` (flattened `BrandedRoute`) |
| `is_package_receiver` | expression | `bool` | `receiver_is_package` |
| `isa` | type-constraint value | `Option<InferredType>` | `HasOptions.isa_type` — resolves through the `type_constraint_*` plugin seam; landing this dissolves `HasOptions` entirely, as roadmapped |
| `args` | an `arguments` node | `Vec<ArgInfo>` | `CallContext.args` — the transition projection; ported plugins keep their bodies nearly verbatim, then slim down |

Growing this table is the new "grow the enum" rule: when a plugin needs
a view that doesn't exist, add a projection (one core extractor, one
Rhai-visible name) — never hand out node access, never grow a
context struct that everyone pays for.

### 4.3 `MatchContext`

```rust
pub struct MatchContext {
    pub pattern: String,
    pub span: Span,                       // the pattern's root/@call capture
    pub package: Option<String>,          // enclosing package at the match
    pub package_parents: Vec<String>,     // transitive, for the plugin's own checks
    pub package_uses: Vec<String>,
    pub captures: HashMap<String, CaptureValue>,
}
```

Capture cardinality follows the query's statically-known quantifiers
(`Query::capture_quantifiers`): a `One`/`ZeroOrOne` capture binds a
single value map (`m.captures.event.str`); a `ZeroOrMore`/`OneOrMore`
capture binds an array of value maps (`for a in m.captures.attr { … }`).
An unmatched optional capture is `()` on the Rhai side.

### 4.4 Predicates

Two tiers:

**Match-time text predicates** — `#eq?`, `#not-eq?`, `#match?`,
`#not-match?`, `#any-of?`, `#not-any-of?`. These operate on capture
*text* — ring-1 facts only. The tree-sitter Rust binding evaluates
them itself when `QueryCursor::matches` is given the source text
(verified by the spike, and already load-bearing in
`query_cache::cpanfile_requires`'s `#eq?`), so core implements
nothing here. Unknown predicate names are NOT match-time filters —
the binding surfaces them via `Query::general_predicates`, which is
exactly the reservation the deferred tier needs.

**Deferred host predicates** — conditions that *cannot* be answered at
match time and must not pretend to be. The motivating case is receiver
gating: "this `->enqueue(...)` counts only when the receiver isa
`Minion`" is resolved cross-file, at query time, against the module
index — that is the whole point of the `ReceiverGated` seam
(`docs/adr/receiver-gated-dispatch.md`). So:

```scheme
(method_call_expression
  invocant: (_) @recv
  method: (identifier) @verb (#any-of? @verb "enqueue" "enqueue_p")
  (#receiver-isa? @recv "Minion")
  arguments: (arguments . (_) @task_name))
```

`#receiver-isa?` does **not** filter the match. It tags the match, and
every gate-sensitive emission produced from it is recorded wrapped in
`ReceiverGated<…>` with the declared target class — landing on exactly
the existing query-time resolution path
(`FileAnalysis::applicable_dispatches`, resolved in `refs_to` /
goto-def). The match-time `type` projection remains available as a
*hint* (same role `receiver_class` plays in
`record_provisional_dispatch` today), but the verdict is query-time by
construction — the pattern medium is not allowed to lie about when
receiver types are knowable.

This gives `dispatch_verbs()` a migration path onto patterns (the
manifest's `verb`/`target_class`/`name_arg_index` triple becomes a
pattern + `#receiver-isa?` + a `str` projection), but the manifest is
NOT retired by this design — it stays valid data; unifying it is a
follow-on (§10 phase 4, optional).

New deferred predicates beyond `#receiver-isa?` need the same sign-off
bar as new builder side-effect `EmitAction`s: each one is a promise
about query-time machinery, not just capture.

### 4.5 Gating and the fixed point

A plugin's `triggers()` remain the default applicability gate, but
evaluated **per match site**: the match's enclosing package's `uses` ∪
transitive parents (both complete, because dispatch runs post-walk —
§5). Verdicts are memoized per `(plugin, package)`.

Emissions can change gating: `PackageParent` can make a `ClassIsa`
trigger true; `SyntheticUse` can make a `UsesModule` trigger true. The
dispatch phase therefore runs to a fixed point: evaluate gates → run
newly-applicable `(plugin, pattern)` matches (in span order) → apply
emissions → re-evaluate. Monotone (uses/parents only grow), deduped by
`(plugin, pattern, match span)` so nothing dispatches twice, bounded by
the same discipline as the worklist fold.

## 5. Execution model

Pattern dispatch is a **named build phase**, run after the live walk
and before `resolve_variable_refs()` — i.e. it becomes pipeline phase
1.5 in the "Build pipeline phases" list. Rationale:

- `package_uses`, `constant_strings`, scopes, and walk-time TCs are
  complete, so projections see the whole file. This *fixes* two
  order-sensitivity bugs in today's interleaved dispatch: a call
  textually before its `use` now fires (package-level gating), and a
  constant declared after its use site now folds.
- One `QueryCursor` pass over the tree replaces per-node dispatch
  bookkeeping. Matches are collected and dispatched sorted by
  `(start_byte, pattern_index)` — deterministic document order.
- The walk has already emitted `Expr(span)` witnesses at every
  meaningful expression (phase-1 contract), so the `type` /
  `callable_edge` projections are pure bag queries — no emit-then-query
  dance, no walk-state coupling.
- Downstream phases (2–9) run after, so emissions (Symbols, TCs via
  `VarType`, `NamedSubParamType`, provisional dispatches) feed
  enrichment, the bag seed, and the fold exactly as they do today.

**Stateful DSLs replay in span order.** The topic-route stack
(`topic_route_dsl`: verbs whose implicit controller base a
`SetRouteBase` emission sets, bracketed by `group_fn`) is currently
maintained during the walk. Post-walk, core replays it over the
span-ordered match stream: group-scope matches push/pop, base-setter
emissions set the innermost frame, verb-call matches read it. Same
manifest, same semantics, different driver. This is the hardest
migration item and is sequenced last (§10 phase 3).

**`on_use` stays walk-interleaved — deliberately.** Kit expansion
(`SyntheticUse`) must precede the walk's own framework-gated native
behavior: `visit_has_call` synthesizes Moo accessors *during* the walk,
gated by `framework_modes`, which the kit's inner `use Moo` populates.
Until native `has` synthesis itself moves onto the plugin seam (already
the roadmapped direction — CLAUDE.md "Inheritance & frameworks"),
`on_use` cannot move without a second walk. It keeps its current hook
and its trigger-exception semantics; `use`-shaped *patterns* are still
allowed for plugins that only need post-walk facts from a use site.

## 6. Core implementation sketch

New module `src/plugin/patterns.rs` (Build layer in
`layering_tests.rs`'s `layer_map`; it is a builder plugin in the
sanctioned sense — tree access stays inside `build()`'s call graph):

- **Compile & merge.** At registry construction, per language:
  concatenate every loaded plugin's pattern sources into one
  `tree_sitter::Query`, keeping a `pattern_index → (plugin_id,
  pattern_name, projections, deferred_predicates)` table. Compiled once
  per process per plugin-fingerprint (the existing `cached_query`
  pointer-identity cache generalizes to a hash key; `Query::new` cost —
  ~400ms for the Perl skeleton — is why per-file compilation is
  forbidden). A source-offset map recovers per-plugin error positions
  from merged-query compile errors.
- **Run.** In the dispatch phase: one `QueryCursor` over the tree
  (text predicates evaluated by the binding); group by pattern; drop
  matches whose `(plugin, package)` gate is false this round; sort;
  project; convert
  `MatchContext` → Rhai `Dynamic` (mirroring today's `CallContext`
  serde path in `rhai_host.rs`); call `on_match`; apply emissions via
  the existing `apply_emit_action` (namespace tagging, provenance —
  unchanged). Loop per §4.5.
- **Projection engine.** Thin dispatcher from `Projection` enum to the
  existing extractors (`arg_info_for`'s pieces, `extract_arg_name_list`,
  `cst::pair_nodes`, `cst::string_list`, `invocant_type_at_node`, the
  constraint fold). No new extraction logic — this is a re-plumbing of
  what `base_call_context` already owns, made lazy.
- **Rhai host.** `patterns()` read at load like every other manifest
  (same `call_fn` + serde pattern as `overrides()`); `on_match`
  dispatched like `on_method_call` (same silent-drop policy, same
  `max_operations` kill switch, same `ctx["call"]`-style reserved-word
  caveats documented).

Cache: pattern dispatch changes *when* emissions happen, and the fixed
point can change *what* is emitted for order-sensitive files — bump
`EXTRACT_VERSION` when the phase lands. The plugin fingerprint already
hashes plugin sources, so editing a pattern invalidates cached blobs
with zero new machinery.

## 7. Verification: answering the ring-2 silent failure

The spike's sharpest warning transfers directly: a wrong pattern
doesn't error, it silently matches nothing — and the field-table trap
is real (`variables:` on `variable_declaration` prints in the CST and
works via `child_by_field_name` but matches **zero** in the query
engine; cost in the spike: 45% of variable recall, silently). A plugin
system built on queries without a verification story would be strictly
worse than today. Three mandatory mitigations:

1. **Self-verifying patterns.** The `expect` list on each
   `PatternSpec`: source snippets with expected match counts and
   (optionally) expected capture texts. `--plugin-check` parses each
   snippet with the target grammar and asserts. A pattern with no
   expects gets a `--plugin-check` warning. This catches the trap class
   at author time, not in production.
2. **Match telemetry.** Per-pattern match counts, routed through
   `timings.rs` (`PERL_LSP_PLUGIN_STATS`, same gate-read-once
   discipline). One workspace index run answers "which of my patterns
   never fired."
3. **The trap library.** `PLUGIN_AUTHORING.md` grows a "query traps"
   section: field queryability must be probed per node kind
   (`perl-lsp --parse` is the probe), anchor (`.`) semantics, hidden
   nodes, quantifier capture cardinality. The existing snapshot harness
   (`--plugin-test` fixtures + `expected.json`) continues to cover
   end-to-end emission parity.

## 8. Performance expectations

Removed, per call site in every build: arg-list flattening + per-arg
`ArgInfo` construction (string extraction, constant-fold lookups,
sub-param walks, bag round-trips), `transitive_parents` + uses-vec
clones for trigger checks, and per-plugin Rhai serialization of
contexts that return `[]`. Added, per file: one merged query pass
(tree-sitter query execution over an already-parsed tree; the spike ran
skeleton-scale queries over the substrate without it registering as a
cost center), plus projection + Rhai dispatch proportional to **actual
matches** — for typical files, zero.

Measure, don't assume: the phase gets a `bphase!` timer, and phase 4's
acceptance gate (§10) includes a before/after `--timings` comparison
over the gold-corpus substrate.

## 9. What does not change

- **`EmitAction`** — the entire emission vocabulary, `apply_emit_action`,
  namespace tagging, provenance, `Silent`/`exclusive` query-hook
  semantics.
- **Manifests** — `overrides`, `dispatch_verbs`, `param_types`,
  `type_constraint_names`/`_inner`, `app_surface_consumers`,
  `role_makers`, `column_keyed_verbs`, `fluent_verbs`, `load_verbs`,
  `attribute_macros`, `topic_route_dsl`. They are not capture; they are
  vocabulary applied at other seams. Exception: `arg_name_verbs` is
  subsumed by the `list` projection and retires in phase 4.
- **Query hooks** (`on_signature_help`, `on_completion`) — cursor-time,
  orthogonal axis.
- **`on_use`** — walk-interleaved, §5.
- **The pure-function boundary** — no `&mut Builder` crosses to Rhai;
  projections are computed core-side and handed over as data.
- **The engine** — witnesses, reducers, resolve, module index: zero
  edits. (The spike's "engine-touch list: ZERO" result is the precedent
  this design leans on.)

## 10. Migration

Every phase gates on: `cargo test` + `./e2e/run.sh` + `gold-corpus/run.pl`
+ plugin snapshot parity (`--plugin-test` fixtures byte-identical, except
diffs traceable to the documented ordering fixes, which update snapshots
explicitly).

- **Phase 0 — infrastructure.** `PatternSpec`/`MatchContext`/projection
  engine, merged compile + cache, text-predicate evaluation, the
  dispatch phase + fixed point, Rhai `patterns()`/`on_match` wiring,
  `--plugin-check` expects, match telemetry. Legacy hooks keep working;
  nothing ported yet. New layering entries; unit tests pin quantifier
  cardinality, predicate evaluation, gate memoization, dedup.
- **Phase 1 — first port + recipe.** Port `mojo-events` (simplest
  call-shaped plugin). Differential snapshot. Write the porting recipe
  in `PLUGIN_AUTHORING.md`. This phase also proves the `args`
  transition projection keeps bodies nearly verbatim.
- **Phase 2 — the verb plugins.** `dbic`, `dbic-resultddl`,
  `data-printer`, `catalyst`, `dancer`, `minion`, `type-tiny`, `moo`.
  `has` becomes a pattern whose captures carry `list` (attr names,
  covering `has [qw/a b/]`), `pairs` (options), and `isa` — dissolving
  `HasOptions`. `#receiver-isa?` lands here (minion's enqueue pattern),
  wired to the existing `ReceiverGated` recording.
- **Phase 3 — the stateful Mojo trio.** `mojo-helpers`, `mojo-routes`,
  `mojo-lite`: chained-receiver patterns (the invocant-is-a-call shape
  is *more* direct as a pattern than `receiver_call_name`), the
  span-ordered topic-route replay, `route_defaults` projection.
- **Phase 4 — retire pre-capture.** Delete `on_function_call` /
  `on_method_call` hooks, `base_call_context`/`arg_info_for`'s
  eager call-site path, `arg_name_verbs`, and the `CallContext` fields
  nothing native reads. `EXTRACT_VERSION` bump. Timings comparison is
  the acceptance evidence. Optional follow-on: generate core-owned
  patterns from the `dispatch_verbs`/`load_verbs` manifests so
  `record_provisional_dispatch`/`record_plugin_loads` ride the same
  query pass and the per-call manifest probes disappear.

## 11. Risks and honest limits

- **ERROR-node degradation.** Queries don't match inside ERROR nodes;
  the walk's ERROR-recovery heuristics don't transfer. Mid-edit
  (incomplete source), pattern-driven emissions can drop out where
  today's dispatch sometimes survives. Assess in phase 1's differential
  against broken-source fixtures; the cursor-time hooks (completion /
  sig-help) don't depend on this, and open-document analyses refresh on
  every keystroke, so the window is one edit cycle. If parity matters
  somewhere specific, that pattern's site can keep a native fallback —
  measured, not assumed.
- **Ordering-semantics change.** Post-walk gating fires plugins for
  calls textually before the enabling `use`, and constants fold
  file-wide. Strictly more correct, but it changes emissions for some
  files: snapshots update, `EXTRACT_VERSION` bumps, gold rows get
  re-verified (XPASS promotions are plausible).
- **Grammar drift.** A `ts-parser-perl` bump can silently change node
  kinds/fields under user patterns. The plugin fingerprint doesn't see
  grammar versions — include the grammar/ABI version in the fingerprint
  hash when this lands, so a grammar bump re-resolves cached modules
  and `--plugin-check` expects catch pattern breakage.
- **Query subtleties as a new author-facing surface.** Anchors,
  quantifiers, hidden nodes, field queryability. This is real; §7 is
  the answer, and the bar is that the *verified* authoring experience
  (pattern + expects) is safer than today's unverifiable if-chains, not
  that queries are foolproof.
- **The fixed point must stay boring.** Same discipline as the worklist
  fold: dispatch dedup by `(plugin, pattern, span)`, monotone gate
  inputs, and a debug-only iteration cap. No plugin-observable
  iteration count.

## 12. Relation to the pack/multi-language seam

`PatternSpec.language` is why this design is bigger than Perl: a pack
language's plugins (today `cpp-attributes.rhai`, manifest-only because
`CallContext` is Perl-shaped) get the same capture mechanism by
declaring `language: "cpp"` patterns — dispatched from
`PackDriver::analyze_with_path`'s pipeline (after extraction, before
`into_file_analysis`, so emissions ride the same assembly). The
skeleton-spike vocabulary and this design's projection vocabulary are
siblings: one extracts the language's own structure, the other extracts
framework shapes on top of it, both feeding the same engine through
data. No pack work is in scope here beyond keeping the `language` field
honest.

## 13. Spike findings (phase 0 + phase 1, landed on this branch)

The infrastructure and the first port were spiked to test the design's
load-bearing claims. What landed:

- `src/plugin/mod.rs` — `PatternSpec` / `PatternExpect` / `CaptureData`
  / `CaptureValue` / `MatchContext`, trait methods `patterns()` +
  `on_match()`, and `trigger_fires` (the single trigger-matching
  implementation, now shared by `applicable()` and per-match gating).
- `src/plugin/rhai_host.rs` — `patterns()` manifest loading (same
  fail-safe contract as `overrides()`), `on_match` dispatch.
- `src/builder/pattern_dispatch.rs` — the driver: per-source compiled
  query cache, post-walk dispatch with fixed-point gating, projection
  engine (`str`/`strs`/`content_span`/`shape`/`sub_params`/
  `callable_edge`/`ty` — all routed through `arg_info_for` /
  `invocant_type_at_node`), and `verify_pattern_expects` (the expects
  runner, test-driven pending `--plugin-check` wiring).
- `frameworks/mojo-events.rhai` — ported end-to-end. The verb if-chain
  and positional arg digging became the pattern; `on_match` keeps the
  old hook's body semantics. All 6 existing mojo-events builder tests
  pass unchanged, as does the full suite (1312 unit + integration).
- `src/builder/pattern_dispatch_tests.rs` — bundled-expects
  verification (every declared pattern must ship expects and they must
  hold), the fixed-point gating round-trip (a `PackageParent` emission
  from an Always plugin enabling a `ClassIsa`-gated plugin's match in
  round 2), and per-package gating (same verb in a non-firing package
  stays silent, and the emission lands in the firing package).

Claims confirmed:

- **Text predicates come free.** The Rust binding evaluates
  `#eq?`/`#any-of?` when `matches()` gets the source text. §4.4's
  original claim that core must implement them was wrong; corrected.
- **The port is a real simplification.** The pattern absorbs the verb
  set and the arg positions; the projections declared are exactly the
  three the plugin reads (`ty` on the receiver, `str`+`content_span`
  on the event, `sub_params` on the callback). No `CallContext` field
  is touched.
- **Fixed-point gating works** and terminates via dedup + monotone
  inputs, as designed.

Traps found (now encoded in the driver, and belonging in the trap
library):

- **Emissions need the match site's walk context restored.** Two real
  bugs during the spike: `apply_emit_action` panics on an empty scope
  stack (dispatch must run inside the file scope, before the final
  `pop_scope`), and symbols get stamped with the walk-stale
  `current_package` unless it's swapped to the match site's package
  for the whole emission application, not just for projections. The
  driver now pushes `scope_at_point(match)` and swaps
  `current_package` around `on_match` + emission application.
- **Placement is load-bearing**: dispatch must precede the deferred
  `VarType` / named-sub-param flushes or those emission kinds are
  silently dropped. The build-pipeline phase list must name this
  ordering when the phase lands for real.
- **Grammar shape, not design flaw**: single-argument method calls
  carry the literal directly under `arguments:`; multi-arg calls wrap
  a `list_expression`. Patterns over call args need the alternation
  (see mojo-events' `event_call`). The expects mechanism caught the
  gap immediately — which is the §7 story working as intended.
- **`package` is a Rhai reserved word** — `MatchContext.package` must
  be read as `m["package"]`, same footgun family as `ctx["call"]`.

Verification: full `cargo test` green (1312 unit + integration,
including the 6 pre-existing mojo-events behavior tests, unchanged);
gold harness against the pinned substrate — FAIL 0, XPASS 0, CRASH 0
(DateTime rows dropped for a sandbox substrate artifact, unrelated);
`--plugin-check frameworks/mojo-events.rhai` ok (its hook detection
now counts `on_match`). e2e needs nvim, absent in the sandbox — CI
covers it.

Deliberately not spiked (unchanged design claims): deferred host
predicates (`#receiver-isa?` → `ReceiverGated`), the `pairs` / `list`
/ `isa` / `args` / `route_defaults` projections, per-language merged
queries (the spike compiles one query per pattern spec), match
telemetry, `--plugin-check` expects wiring, the topic-route replay,
and the phase-4 pre-capture retirement.

## 14. Open questions (deliberately deferred)

- Per-pattern trigger overrides (`when:` on a `PatternSpec`) — wanted
  eventually (mojo-events' two-trigger split is really per-pattern),
  but v1 keeps gating plugin-level to stay small.
- Migrating native `has` synthesis (`visit_has_call`) fully onto the
  moo plugin's patterns — the prerequisite for ever moving `on_use`
  post-walk. Out of scope; this design removes no native paths beyond
  the pre-capture plumbing.
- Unifying `dispatch_verbs`/`load_verbs` manifests into core-generated
  patterns (§10 phase 4's optional tail).
- A `#in-package-using?` deferred predicate family, if per-pattern
  gating (first bullet) turns out to want pattern-local expression.
