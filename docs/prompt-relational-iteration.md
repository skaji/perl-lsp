# Prompt: move iteration to relational

**Status: direction brief for the scaling push. First slice landed (the
member pre-filter, below); everything else is ranked, not started.**

## The thesis

Cross-file enrichment and resolution look superlinear in workspace size
because their unit of work is not the file — it is the file's provider
neighborhood, walked imperatively. The recurring shape: a walker fetches an
object (rehydrating a whole `FileAnalysis` through the blob LRU) in order to
answer a question that is relationally expressible, and every axis of that
walk grows with the workspace:

- **candidates per name** — a package is a SET of files
  (`visible_def_candidates`), 5–12 declaring files for a common name at 122x;
- **cost per touch** — resident-map read below the cache caps, a full
  zstd+bincode decode above them, and the sweep's access pattern is cyclic, so
  past the cap the hit rate collapses to ~0 rather than degrading;
- **recursion over the closure** — enrichment builds enriched overlays of its
  providers, whose enrichment recurses; reuse across consumers is defeated by
  traversal-order-dependent taint plus a 64-entry drop-oldest cache.

Multiply those and a bulk sweep is `N × B(N) × D(N)`. The per-file cost
reductions all closed (`docs/adr/skipping-cross-file-work.md` — eight
proposals, measured, rejected) because they attacked the linear constant, not
the multiplicative factors. The factors fall only by replacing
fetch-and-inspect with an indexed relation.

## The relations already exist

Three seams landed for other reasons and are exactly the right carriers:

- **The row store** (`docs/adr/relational-ref-index.md`): `refs(name, file)`
  candidate pairs and `syms(file, name, key, kind, span, container, flags)`,
  shredded in the same transaction as the blob (cannot drift — the failure
  that retired the RAM-side parallel reverse indexes), version-gated, indexed
  by name/key/file. `container` is the symbol's package attribution.
- **The conclusion layer** (`docs/prompt-conclusion-layer.md`): the registry
  chase partially evaluated per file; cross-file answers residualize as
  `Link` (an ordered MRO ladder as data), never as baked values — so there is
  nothing to taint.
- **The flush worklist**: change-driven propagation with a movement cutoff —
  the declarative fixpoint the per-file recursive enrichment never was.

The migration is therefore mostly re-pointing walkers at relations that are
already maintained, not building new infrastructure.

## The migration ladder

Ranked by (evidence of cost) / (blast radius). Each row names today's walker,
the relation that answers it, and the soundness constraint.

1. **The ancestor walk's per-candidate member probe** — LANDED (this slice).
   `method_resolution_on_class`'s cross-file arm decoded every candidate of
   every ancestor to run an existence scan (`mroc.candidate_wasted`); the
   stamp built on it is ~63% of all blob decodes. Now `candidate_may_declare`
   asks the `syms` rows first and skips the rehydrate on proven absence.
   Constraints honored: fail-open everywhere the store cannot speak (below).

2. **`module_declaring_method_in_package`** — same probe, same guardrails,
   different loop (`queries.rs`): the typeglob-install last resort scans the
   class's provider bucket with a `symbols_present` per candidate, and it runs
   on exactly the misses. One call into the same pre-filter.

3. **Bridge entities by name.** `for_each_entity_bridged_to` decodes every
   module bridging to a class to enumerate entity names, because
   `PluginNamespace.entities` are `SymbolId`s into the evicted symbols vec —
   the bridge survives the strip, the names it points at do not. Either a
   `(class, entity_name, file)` row family or entity names denormalized into
   the plugin lane deletes the per-helper-call scan of every plugin file. The
   visitor also has no early exit; with names resident that stops mattering.

4. **The `MethodCall` stamp as a `Link`.** The frozen `MethodTarget` exists so
   an answer cannot depend on which verb asked; a conclusion `Link` is
   verb-independent by construction and cheap to evaluate. If the stamp's
   product were the ladder rather than the resolved value, the enrichment
   re-stamp collapses into the flush worklist's cutoff.

5. **Return-type consults through conclusions.** `MethodOnClass` is 96.0% of
   measured cross-file consults; serving them from the conclusion map removes
   the chase's bag decodes and the transitive overlay recursion behind them.
   This is the conclusion layer's own roadmap; listed here because it retires
   the `enriched_present` fallback that makes enrichment recursive.

6. **Residency policy for digests.** The export gate won because
   `export`/`export_ok` happen to be non-evictable axes; per-package
   method-name sets and bridge entity names lose because they happen not to
   be. "What survives the strip" should be derived from what the confirm
   loops read, not from field placement. The Surface already computes
   per-file method names and throws them away.

7. **Overlay key honesty.** `enriched_snapshot`'s key fingerprints the file
   plus its *declared* providers, but enrichment also reads bridges and
   loader shapes from undeclared files. Today the staleness channel is masked
   by how rarely an overlay survives to be reused; any retention improvement
   makes it a live correctness hole. Close it before improving retention.

8. **Enrichment as a delta artifact.** Every recursive consumer of an
   enriched analysis reads only the bag (`docs/adr/enrichment-build-cost.md`,
   consumer matrix), and the measured enrichment delta is 4.13% of base heap.
   The truncate-to-baseline + swap dance is imperative state management for
   what is logically a small immutable overlay. This is also the "make a
   build cheap" prerequisite that priced out level-indexed enrichment — the
   algebra was right, the carrier (whole-FA copies) was too heavy.

## The discipline (every slice obeys these)

- **Skips need positive evidence; every unknown fails open.** A relation may
  answer "provably absent" only where it provably covers the file: rows conn
  available, file present in `files` (the single shredded marker), and the
  resident copy actually evicted — a whole copy answers from RAM for free and
  its rows may predate it. Same fail-open shape as `restamp_owed`.
- **Over-approximate toward the decode.** The member probe is kind-blind and
  matches name OR key: a wasted decode is the cheap error, a hidden member is
  the silent-wrong-goto-def one.
- **Know what the rows cannot see.** Deferred plugin emissions materialize
  into resident copies AFTER the shred (`materialize_gated_emissions`), so
  their symbols have no rows; any file carrying `gated_emissions` fails open.
  A new post-shred mutator of cached copies must be added to this list — or
  better, not built.
- **Ship the switch that checks the assumption.** Each skip lands with its
  disable control and an equivalence mode that runs the skipped work anyway
  and screams on divergence (`PERL_LSP_RESTAMP_EQUIV` is the pattern; the
  member pre-filter ships `PERL_LSP_NO_MEMBER_PREFILTER` and
  `PERL_LSP_MEMBER_PREFILTER_EQUIV`).
- **Counters, not vibes.** Every pre-filter counts skip/decode/unknown so the
  hit rate is a ghost-stats read, and the next ranking pass starts from
  numbers (`docs/adr/skipping-cross-file-work.md`'s denominator rules apply).
