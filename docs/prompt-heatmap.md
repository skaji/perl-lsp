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
perl-lsp --heatmap <root> [--csv] [--include-deps] [--all]
```

- `<root>` — workspace root; runs `cli_full_startup(root)` ("act like the LSP
  just started": workspace index + @INC resolve + SQLite warm).
- `--csv` — emit CSV instead of JSON (both are always ingestible; SARIF is the
  gold interchange format but is deferred — see "What's next").
- `--include-deps` — also count references found in cached `@INC` dependency
  modules (default: open + workspace files only).
- `--all` — keep every counted symbol in the `symbols` array; by default the
  array is trimmed to callables and dead candidates (packages with nonzero
  fan-in are summarized but not individually listed unless `--all`).

Output goes to stdout; the startup chatter (`Indexed N files`, cache lines)
goes to stderr, so `--heatmap … 2>/dev/null | jq` is clean.

## Metrics

Per symbol (subs, methods, packages/classes/modules — anonymous and
non-identifier-named symbols are skipped, they have no nameable graph):

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

## Implementation notes

- CLI entry: `cli_heatmap` in `src/main.rs`, dispatched from the `--heatmap`
  arm, mirroring `--workspace-symbol`.
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

- **SARIF 2.1.0** output (`--format sarif`) — the gold interchange format for
  the automotive/static-analysis tool ecosystem; deferred from v1.
- **Fan-out depth / transitive reach** — current fan-out is one hop, intra-file.
  A transitive callee count needs the cross-file call graph walked (the
  `GraphView` seam already exists for inheritance/bridges).
- **Visualization** — the JSON is shaped for a butterfly/treemap front-end;
  a small HTML viewer over `symbols[]` would land the DX story.
- **Precision split for fan-in** — optionally separate call-site fan-in from
  declaration-adjacent mentions (import/export lists) once `RefLocation`
  carries its `RefKind`.
