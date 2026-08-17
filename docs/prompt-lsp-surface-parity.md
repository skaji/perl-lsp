# LSP surface parity — the advertised-capability gap

Forward-design brief. Origin: a capability audit against
EffortlessMetrics/perl-lsp v0.17.0 (2026-08-17), done server-to-server by
installing the rival binary and diffing `initialize` responses.

**Status: the cluster below (1–3) plus a re-scoped documentLink LANDED.**
Each is a CandidateSet/Model projection + CLI verb + LSP handler + gold
rows (`fixtures/type-definition.json`, `type-hierarchy.json`,
`call-hierarchy.json`, `document-link.json`). Two findings that amend this
brief's own argument:

- **typeHierarchy cannot appear in our `initialize` response.** lsp-types
  0.94.1 (pinned by tower-lsp 0.20, its final release) has no
  `type_hierarchy_provider` field — the request/response types exist, the
  capability field doesn't. We serve the verb and advertise it via dynamic
  registration (`initialized`, gated on the client's
  `typeHierarchy.dynamicRegistration`); VS Code sees it, a static
  capability diff still shows a miss. Closing THAT gap means leaving
  tower-lsp 0.20 (the incumbent advertises it statically, so they already
  have) — a maintainer decision, out of this batch's scope.
- **documentLink's original rationale was wrong twice** — goto-def already
  resolves module identifiers in `use`/`use parent`/`use base qw()`/`with`
  (empirically verified). The landed verb covers ONLY the non-symbol
  ranges nothing else reaches: POD `L<...>` links, URLs in comments/POD,
  and existence-checked string-path loads. The empirical coverage table
  lives with the 2026-08-17 audit notes; the durable rule: a verb that
  duplicates goto-def with an underline is not worth surface area, so
  measure coverage before believing a capability-diff line item.

## The finding

**Our advertised capability surface is a strict subset of theirs.** Everything
we advertise, they advertise; they advertise 15 verbs we don't.

Answer *quality* runs the other way — a same-day head-to-head on structural
gold scored **85/98 (ours) vs 24/98 (theirs)**, excluding our deliberately
disabled `linkedEditingRange` (#117). So this is not a capability deficit in
the analysis; it is a wiring deficit at the protocol edge.

That distinction is the whole point of this doc. A marketplace feature-list
comparison reads the `initialize` response, not the answers. We lose a
comparison we would win on merit, and every verb below is wiring over
machinery that already exists.

## The cluster worth doing (value ÷ effort)

These three lean entirely on landed machinery and together close most of the
visible gap. They are one arc, not three.

1. **`typeHierarchyProvider`** — supertypes are `parents_of`; subtypes are the
   `implementations()` CandidateSet projection's fan-out. `GraphView` +
   `for_each_ancestor_class` already walk both directions with the
   `INHERITS | APP_SURFACE` edge mask. No new analysis, only the
   prepare/supertypes/subtypes request triple and the item mapping.
2. **`callHierarchyProvider`** — incoming calls are the `references()`
   projection minted at a declaration (the same one `--heatmap` already uses
   for fan-in, so the counts agree with the heatmap by construction).
   Outgoing calls are `call_bindings` within the sub's body span. Both sides
   are already computed; this is projection plumbing.
3. **`typeDefinitionProvider`** — `$obj` → the definition of its inferred
   class, via `InferredType` + `dispatch_class()`. Smallest surface of the
   three and the best story: it is the one verb that directly exhibits the
   witness-bag differentiator, and the incumbent does not resolve it.

## Also worth having, lower priority

4. **Pull diagnostics (`diagnosticProvider`)** — architecturally aligned with
   the refresh work in `7343ae59`: the client decides when diagnostics
   compute rather than the server managing execution depth. Note the rival
   advertises `interFileDependencies: false`, which is wrong for anything
   doing cross-file analysis; we would set it `true`, and that single field
   is a correctness claim worth making explicitly.
5. **`documentLinkProvider`** — `use Foo::Bar` clicks through to the module.
   Module→path resolution already exists; an afternoon.
6. **`codeLensProvider`** — reference counts per sub, from the same heatmap
   fan-in as (2). Motivation, measured: the rival advertises code lens and
   returns `"0 references"` on every lens in `URI.pm`, one of the most
   referenced modules in the corpus. Real counts are a demonstrable
   differentiator, not parity.

## Explicit non-goals

Do not implement these; each has a reason, and re-deciding them is waste.

- `colorProvider` — Perl has no color literals.
- `notebookDocumentSync` — no notebook story.
- `monikerProvider` — LSIF-era, niche.
- `inlineValueProvider` — needs a debug adapter we don't have.
- `documentOnTypeFormattingProvider` — widely disliked; fights perltidy.
- `declarationProvider` — the declaration/definition split barely exists in
  Perl; it would duplicate goto-def.
- `linkedEditingRangeProvider` — **deliberately off** (#117). Clients with
  linked edits on by default replay keystrokes into every returned range, so
  a mid-typed `$abel` live-renames a declaration whose `$a` prefix matched.
  Keep it off. The co-edit projection stays CLI-queryable via
  `--linked-editing`.

## Invariants for whoever implements this

- **Every verb is a CandidateSet projection or a Model query — never a new
  analysis path.** If a verb seems to need its own walk, that is the signal
  to stop: the answer already exists somewhere and the projection is missing.
  `docs/adr/resolution-candidate-set.md` owns that rule.
- **Counts must agree with `--heatmap` by construction**, i.e. by using the
  same `references()` projection rather than a parallel count. Two reference
  counts that can disagree is the parallel-store bug in a new costume.
- Advertise honestly: a verb we cannot answer well is worse than one we do
  not advertise, because the first is a wrong answer and the second is a
  known gap. The maturity labels in `--languages` carry the same rule.

## Provenance and freshness

Numbers here are measured, not estimated: 15 unadvertised verbs, 85/98 vs
24/98 on structural gold, 626 actual installs against a stale 313 in the
rival's badge, and the `URI.pm` code-lens observation. All from the
2026-08-17 audit against v0.17.0. **Re-verify before acting** — a rival
release moves the capability list, and the quality gap is the part most
likely to change.
