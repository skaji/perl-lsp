# Code-usage heatmap (`perl-lsp --heatmap`)

A reporting view over the **existing** cross-file reference graph — not a new
analysis tier. Per-symbol fan-in is a projection of the resolution
CandidateSet (`docs/adr/resolution-candidate-set.md`): the set is minted at
each symbol's declaration and `references()` is counted, so the heatmap
answers exactly what `textDocument/references` answers there — and inherits
every construction axis (visibility masks, group/attr field splats, override
families, future closure/delegation gating) without heatmap changes.

The incumbent it mirrors is SciTools Understand's "Butterfly" view
(callers + callees = fan-in / fan-out over a cross-file reference graph). It is
positioned as an **insight / DX** deliverable, not a defect catalog — so it
sidesteps the compliance / tool-qualification apparatus a MISRA checker carries.

## Invocation

```
perl-lsp --heatmap <root> [--csv|--html] [--include-deps] [--all]
```

- `<root>` — workspace root; runs `cli_full_startup(root)` ("act like the LSP
  just started": workspace index + @INC resolve + SQLite warm).
- `--csv` — emit CSV instead of JSON (both are always ingestible; SARIF is the
  gold interchange format but is deferred — see "What's next").
- `--html` — emit a self-contained, offline HTML viewer over the same report
  (see "Visualization" below). Mutually exclusive with `--csv`; if both are
  passed, `--csv` wins (it returns first).
- `--include-deps` — also count references found in cached `@INC` dependency
  modules (default: open + workspace files only).
- `--all` — keep every counted symbol in the `symbols` array; by default the
  array is trimmed to callables and dead candidates (packages with nonzero
  fan-in are summarized but not individually listed unless `--all`).

Output goes to stdout; the startup chatter (`Indexed N files`, cache lines)
goes to stderr, so `--heatmap … 2>/dev/null | jq` is clean.

## Metrics

Per symbol (subs, methods, packages/classes/modules — anonymous and
non-identifier-named symbols are skipped, they have no nameable graph;
`SymKind::Handler` — routes / Minion tasks / events — is *also* elided today,
but that is a listing gap, not a graph gap: their fan-in is well-defined and
already built. See "What's next" #1):

- **`fan_in`** — number of reference *sites* across the searched roles, with the
  symbol's own declaration(s) excluded. A "reference site" is **any** mention
  the builder records: call sites, qualified accesses, import-spec mentions
  (`use M qw(foo)`), and export-list mentions (`our @EXPORT_OK = qw(foo)`). This
  is deliberately the broad "how many references" definition; it never
  *under*-counts a live symbol's reachability (the safe direction for dead-code).
- **`fan_out`** — number of **distinct** callees a sub/method references inside
  its own body (intra-file span containment over `FunctionCall` / `MethodCall` /
  `DispatchCall` refs; self-recursion excluded). `null` for packages.
- **`exported`** — whether the symbol is in the file's export surface.
- **`dead_code_candidate`** — `fan_in == 0` **and** no reachability guard fired.
- **`reachable_guard`** — when `fan_in == 0` but the symbol is *not* flagged,
  the reason it is treated as reachable (see below). `null` otherwise.

## Output schema (JSON, `schema: "perl-lsp.heatmap.v1"`)

```jsonc
{
  "schema": "perl-lsp.heatmap.v1",
  "kind": "usage-heatmap",
  "label": "dead_code_candidate = UNREFERENCED SYMBOL (reachability heuristic); NOT MISRA C:2012 Rule 2.2 dead code, which is undecidable",
  "soundness": "over-approximate reachability: ...",
  "root": "<root>",
  "files_indexed": 29,
  "dynamic_dispatch_sites": 2,     // workspace-wide count of $obj->$method sites
  "include_deps": false,
  "summary": { "symbols_reported": 273, "dead_code_candidates": 43 },
  "symbols": [
    { "name": "add", "kind": "Sub", "package": "Calc::Util",
      "file": "/abs/path/Util.pm", "line": 7, "col": 5,
      "fan_in": 4, "fan_out": 0, "exported": true,
      "dead_code_candidate": false, "reachable_guard": null }
  ],
  "dead_code_candidates": [ /* the subset with dead_code_candidate == true */ ]
}
```

`symbols` is sorted heaviest-`fan_in` first (the hotspots). `line`/`col` are
1-based, character-counted (same rendering as `--references`). CSV mode emits
the same columns with RFC-4180 escaping.

## The over-approximation (honest labelling)

This is **not** MISRA C:2012 Rule 2.2 dead code — that rule is undecidable, and
asserting it would invite a tool-qualification burden. A
`dead_code_candidate` here is an **unreferenced symbol**: a reachability
*heuristic*. The output `label` and `soundness` fields say so in-band, and the
`--heatmap` help text repeats it.

Reachability is **over-approximated**: the analysis errs toward *reachable*
(may under-report dead code) so it never falsely flags a live symbol. A
zero-fan-in symbol is shielded from the dead list — with `reachable_guard` set —
when any of these hold (checked most-specific first):

| guard | rule |
|---|---|
| `exported` | name is in the file's export surface — an external consumer may import it |
| `constructor` | conventional constructor (`new`) — frameworks instantiate it |
| `framework-synthesized` | symbol is plugin-minted (Moo accessors, routes, DBIC rels), not user-written; the framework calls it through machinery the static graph doesn't model |
| `package-implicit-use` | packages/classes/modules — reachable via `require`, app entrypoints, dynamic class strings; too many invisible vectors to flag |
| `dynamic-dispatch` | a **method-shaped** sub (declared in a non-`main` package) when the workspace contains **any** `$obj->$method` dispatch — see below |

### Dynamic dispatch is the load-bearing soundness gate

Perl method dispatch is fundamentally dynamic. The builder records a
`dynamic_dispatch_sites` count per file: every `$obj->$method(...)` whose method
name is a scalar rather than a bareword. Such a call produces **no nameable
`MethodCall` ref** (the dispatched method is unknown at build time unless
constant-folding happens to resolve it), so it is invisible to the static
reference graph.

When that count is `> 0` anywhere in the workspace, a sub that *could* be a
method (declared in a class — i.e. a non-`main` package) cannot be proven
unreferenced: an unresolved dynamic dispatch could target it. So it is shielded.
`main`-script free functions are excluded from this shield — they aren't class
methods, so their `FunctionCall` graph is authoritative.

## Honest failure modes

Even for a flagged candidate, "unreferenced" ≠ "safe to delete". The static
graph cannot see:

- **Symbolic code refs** — `\&name`, `&{$name}`, `*{"${pkg}::name"}` — invoke a
  sub by a string the analysis doesn't track. A flagged *function* candidate
  assumes none of these reach it.
- **`->$method` with an unresolved name** — counted as a `dynamic_dispatch_site`
  (which shields methods workspace-wide) but the *specific* target is unknown.
- **`AUTOLOAD`** — methods materialized at call time have no declaration to count.
- **String `eval`** — code (and calls) built at runtime are opaque.
- **External callers** — anything outside the indexed workspace (and, without
  `--include-deps`, outside open+workspace files). Exported symbols are guarded
  for exactly this reason.
- **Entrypoint-script free-subs** — a top-level `sub` in package `main` of an
  executable script (`#!/usr/bin/perl`, no `package`) is flagged when nothing
  calls it *within the static graph*, but a script is itself an entrypoint:
  its subs may be exercised by the runtime flow, a test/spec harness, or
  `\&main::foo` introspection. Proving these reachable is the job of a
  **deferred entrypoint-analysis tier** (the same tier `scan_entrypoint_scripts`
  /`file_analysis.rs`'s entrypoint-scan lint anticipate). Until it lands these
  are **deliberately listed** rather than blanket-shielded — under-shielding a
  script's own dead helpers is the honest direction, and `main`-script funcs
  are already excluded from the dynamic-dispatch shield (above) by the same
  reasoning. So a `main` package heavy with zero-fan-in subs (common in
  spec/fixture scripts) is expected output, not a bug.

Treat the dead list as a **review queue**, not a delete list.

### C/C++ (pack languages)

Pack-language files (C/C++/…) light up the heatmap on the SAME machinery:
symbols are gathered from the per-language sub-indexes (not the Perl
`FileStore`) via `ModuleIndex::for_each_pack_index` →
`for_each_registered_file`, and fan-in is the identical `references()`
projection — routed through the pack sub-index (`pack_routed()`, VISIBLE-wide
because pack workspace files ride the DEPENDENCY role). Free functions group
by file (like Perl's `main`); class / namespace members group by their class
(`sym.package`). No language branch: a cpp `FileAnalysis` exposes symbols and
refs the same way Perl's does.

**C/C++ dead-code is more over-approximate than Perl's**, because a
zero-fan-in symbol has more invisible reachability vectors. Two are cheaply
shielded and never flagged:

- **`main`** — the runtime enters through it over the ABI, never a source call
  site (guard `entry-point`).
- **Address-taken / used-as-value functions** — `&fn` or a bare
  function-pointer decay is a *reference* (not a call), so it lands in
  `fan_in` and the symbol is never a candidate. No special guard: the
  reference graph already carries it.

The remaining vectors are **not** cheaply decidable, so a zero-fan-in symbol
that hits one is still listed — honestly, as a review-queue entry:

- **Exported / `extern "C"` ABI surface** — a library's public functions are
  called by consumers outside the indexed tree. (We do *not* blanket-shield
  external-linkage functions: that would silently drop every genuinely-unused
  internal helper — the actual C dead-code use case.)
- **Function-pointer callbacks** — a callback registered into a table/struct
  the graph doesn't follow reads as unreferenced unless its name (or `&name`)
  appears at the registration site.
- **Templates instantiated in an unscanned translation unit** — a template
  used only from a TU outside the workspace reads as dead.
- **Prototype vs definition** — a function declared in a header and defined in
  a `.c`/`.cpp` lists as two rows (one per file), exactly as a Perl package
  reopened across files does; fan-in is identical on both.

## Implementation notes

- CLI entry: `cli_heatmap` in `src/main.rs`, dispatched from the `--heatmap`
  arm, mirroring `--workspace-symbol`.
- Symbol gathering: Perl files from `ws.workspace_raw()`, pack-language files
  from `idx.for_each_pack_index(|_lang, pack| pack.for_each_registered_file(…))`
  — the same seam `workspace/symbol` and Mode-B diagnostics use. Both loops
  call the one `heatmap_symbol_row` helper (fan-in/fan-out/guard/row), so their
  counts are the same `references()` projection by construction; the only
  branch is the `is_pack` routing fact (which sub-index + `pack_routed()`
  VISIBLE walk + the `entry-point` guard). `files_indexed` sums both.
- Identity + counting: `resolve::resolve(...)` at the symbol's declared name
  token, then `references()` — the heatmap never maps a `Symbol` to a target
  itself, so its counts cannot diverge from the references verb (the N-path
  asymmetry the CandidateSet ADR exists to prevent). `heatmap_symbol_eligible`
  in `main.rs` is only a listing policy (which kinds a usage report shows),
  not an identity decision. Should whole-workspace scale ever demand a bulk
  path, it must be built as a CandidateSet-based enumeration (one construction
  shared with the projections), not a parallel walk over raw refs.
- Fan-in counts the `references()` image minus declaration sites (by
  `AccessKind::Declaration`, plus the symbol's own name-token span — group
  answers mark their local decl spelling as a plain Read).
- Visibility: `--include-deps` rides `CandidateSet::with_visibility` (VISIBLE
  vs the EDITABLE default), so the scope knob is construction-time and every
  projection inherits it. Override fan-out honors the same `OverrideScope`
  env knob as CLI references/rename.
- A `sub` declared in a class carries the method override family (a Perl sub
  IS a method when called as one), so `$obj->name` call sites count — the
  old symbol-side target minting missed these.
- Known references-side asymmetry the migration surfaced (documented per the
  ADR's landing notes, not silently fixed): a Moo `rwp`/`writer` synthesized
  method shares the attr's declaration token, and the decl-side group answer
  does not include the writer's call sites (references at the call site DOES
  link back). Its heatmap row therefore reports the attr-group image, not the
  writer's name-keyed count; the dynamic-dispatch guard keeps it off the dead
  list.
- Dynamic-dispatch signal: `FileAnalysis.dynamic_dispatch_sites` (`u32`),
  populated in `Builder::visit_method_call` when the method name is a scalar.
  Rides the bincode cache blob (`#[serde(default)]`); `EXTRACT_VERSION` bumped.

## What's next

The two highest-value residuals lead. Both are the **same** generalization:
the framework plugin already knows an edge the static call graph can't see, and
the reference machinery already computes the count — the heatmap just isn't
consuming it. Both must stay generic and plugin-owned (rule #10); neither is a
per-verb or per-name allowlist in core.

### 1. Unblock Handlers — plugin-owned "definition site"

`heatmap_symbol_eligible` admits only `Sub|Method|Package|Class|Module`, eliding
`SymKind::Handler`. The elision was rationalized as "handlers have no meaningful
cross-file usage count" — **that is false**, and verified so on a live fixture.
`references()` on a Handler already returns every wire-up *and* every dispatch
site:

- `$r->get('/users')->to('Users#list')` → the controller `sub list` already
  reports `fan_in = 1` from the route today (this half works, because the callee
  resolves to a real `Sub`; the `->to('Class#method')` linkage is what supplies
  the ref).
- `$app->minion->add_task(cleanup => …)` + `->enqueue('cleanup')` → `references`
  on the `cleanup` Handler returns **two** sites (the `add_task` definition and
  the `enqueue` call). A never-enqueued `vacuum` task returns **only** its
  definition. So an orphaned Minion task / route / event is already visible in
  the graph — it just never reaches the heatmap because Handlers are elided.

The work:

- **Make Handlers heatmap-eligible** — they then get fan-in / dead-candidate
  treatment like any callable.
- **Give a Handler a generic "definition site."** The blocker for a *correct*
  fan-in is that a Handler's registration (`add_task(cleanup => …)`,
  `->to('X#y')`, `->on(evt => …)`) is itself one of its refs — so the current
  "subtract `AccessKind::Declaration` + the decl name-token span" logic won't
  exclude it, and every wired Handler would read `fan_in ≥ 1` off its own
  wire-up. *Which* arg/span is the definition is **plugin knowledge** (the
  string key in `add_task`, the `Controller#action` string in `->to`, the event
  name in `->on`). So the plugin that mints the Handler must also stamp its
  definition span — a generic tag on the emitted Handler (the Handler-shaped
  equivalent of `AccessKind::Declaration`) — and the heatmap subtracts *that*.
  Never a per-verb definition rule in core.

Outcome: orphan-route / never-enqueued-task / never-emitted-event detection
falls out of the existing dead-code lens — no new analysis, just correct
definition-site accounting.

### 2. Plugin-declared "framework-consumed" reachability

The `dynamic-dispatch` shield is workspace-global and coarse — it shields *every*
non-`main` method when *any* `$obj->$method` exists, yet misses the sharp common
case: a framework **lifecycle hook** with zero static callers and no dynamic
dispatch anywhere. Verified false-positive: a Mojolicious `sub startup ($self)`
(the app entry point, invoked by Mojo core out-of-workspace) is flagged
**dead** — a direct violation of the "never falsely flags a live symbol"
promise.

The framework plugin already knows which method names/roles that framework
invokes through its own machinery (`startup`, `run`, `BUILD`/`DEMOLISH`, Moose
triggers, DBIC `sqlt_deploy_hook`, …). Let a plugin **mark a symbol as
framework-consumed** — a witness/attribute on the symbol asserting "an invisible
framework edge reaches this." Consumers then:

- treat it as reachable → a new, precise `reachable_guard = "framework-consumed"`
  (narrower and more honest than the blanket dynamic-dispatch shield);
- **and likely skip it for fan-out** — its callees are framework-driven, not
  authored call intent, so counting them dilutes the hotspot signal.

This is the dead-code-reachability projection of the same edge the graph-walking
`APP_SURFACE_CLASS` seam already models for the Mojo app surface — core stays a
generic dispatcher, the plugin owns the rule.

### Also deferred (unchanged priority)

- **SARIF 2.1.0** output (`--format sarif`) — the gold interchange format for
  the automotive/static-analysis tool ecosystem; deferred from v1.
- **Fan-out depth / transitive reach** — current fan-out is one hop, intra-file.
  A transitive callee count needs the cross-file call graph walked (the
  `GraphView` seam already exists for inheritance/bridges).
- **Precision split for fan-in** — optionally separate call-site fan-in from
  declaration-adjacent mentions (import/export lists) once `RefLocation`
  carries its `RefKind`.

## Visualization (`--html`)

`--heatmap <root> --html` renders the *same* report (no new computation —
`heatmap_html()` just wraps the JSON value the JSON/CSV paths share) as one
self-contained HTML document on stdout:

```
perl-lsp --heatmap <root> --html 2>/dev/null > heatmap.html && open heatmap.html
```

Why this shape:

- **Self-contained, offline.** The template `src/heatmap.html` is compiled in
  via `include_str!` and the report JSON is inlined into a
  `<script type="application/json">` blob, so the file opens off a `file://`
  URL with no server, no CDN, no build step. The embed replaces every `<`
  with its JSON unicode escape so a hostile path can't close the script
  element early; it round-trips back to `<` through `JSON.parse`. Drawing
  is dependency-free SVG.
- **Three views over `symbols[]`, one dataset:**
  - **Treemap** — squarified (Bruls/Huizing/van Wijk), grouped by package;
    tile area is `fan_in + 1` (so zero-fan-in symbols still occupy a cell),
    color is a `sqrt`-lifted fan-in heat ramp, dead-code candidates get a
    dashed-amber outline. Hover for detail; click copies `file:line:col`.
  - **Butterfly** — back-to-back fan-in (callers) / fan-out (callees) bars
    for the hottest symbols, the classic "who calls / who do I call" read.
  - **Dead code** — the candidate queue as a sortable table, framed as a
    *review queue, not a delete list* (carrying the same honest labelling).
- **Carries the soundness story.** The `label` / `soundness` strings and the
  `dynamic_dispatch_sites` count render in the header, so the viewer can't be
  mistaken for a sound dead-code prover.
