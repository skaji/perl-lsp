# Prompt: move iteration to relational

**Status: direction brief for the scaling push. Landed so far: the member
pre-filter (ladder 1), the bridged-walk early exit (ladder 3's cheap half),
the seam-retry gate (`serves_enriched`), and `RetainedReader` under both the
conclusion loader and the bag-rehydrate loaders. The rest is ranked, not
started.**

**Honest sizing note for the retained-connection family**: connection-per-miss
was the dominant term ONLY on a synthetic no-locality corpus (551 unrelated
dists: 69,933 conclusion-cache misses; real apps measured 511 on crm and 668
on Koha — misses are a LOCALITY property, not a file-count one). On real code
the fix saves ~0.4 s per cold check. It stays because per-call open is the
wrong shape regardless and the cliff exists for whoever indexes a dep-mirror
tree — but nothing further in this family is worth cutting on cost grounds.

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

2. **`module_declaring_method_in_package`** — LANDED. Same probe, same
   guardrails, different loop; it runs on exactly the misses. Cold
   substrate: 24,506 of 25,523 candidate scans skipped (96%), found-rate
   unchanged; slice 1's equivalence evidence covers it a fortiori
   (`has_sub_in_package` is a strict subset of the walk's member test).

3. **Bridge entities by name — LANDED, without the row family.** Bridged
   entities are standard symbols of their file, already shredded, so a
   container-BLIND rows probe (`sym_name_row_exists` — an entity's container
   is the plugin's home package, not the bridged class) gates each
   candidate's decode. `for_each_entity_bridged_to_named` carries the name
   as a pre-filter license; the first-match-by-name consumers ride it, the
   early exit bounds hits, the probe deletes the per-miss scan. The
   `(class, entity_name, file)` row family stays unbuilt unless a corpus
   shows the name-only probe leaking (a plugin file declaring the queried
   name OUTSIDE the bridging namespace still decodes — over-approx, sound).

4a. **Cross-file consults are point-free, memoized per walk and per sweep —
   LANDED** (`24f66e0a`), and it is the arc's largest single win. The chain
   that found it: FHEM (a package-main corpus, ~n providers of one name ×
   n files × keys) drove 12.3M SlotType chase ATTEMPTS through an arm with
   no memo tier; the session memo was inert on the enrichment cascade (no
   session open — `session.absent` was the tell) and keyed on the consumer's
   POINT (meaningless in provider coordinates; every call site a fresh
   miss); and first-encounter pairs PER BUILD are the n², which no
   thread-local memo can span. The fix: point-free cross-file sub-queries
   (gold's 503 exact assertions license the semantics), a session around
   the overlay build, and `SweepAnswerGuard` — a sweep-wide, stamp-guarded,
   worker-shared (candidate, point-free query) → verdict store. Measured:
   attempts 918x down, wall 2.06x at n=250, n=500 from killed to
   completing, overlay builds 8.9x cheaper at unchanged count (the overlay
   was a CARRIER of chase cost, not a cost). The surviving ~keys×n attempts
   are the linear first-encounter floor — do NOT gold-plate this arm
   further (key-set gating exists as an idea; 918x is past the knee and
   the residual wall lives elsewhere). A failed first cut (reverted) is
   part of this row's record: a memo added where no session existed and
   keyed with the point displaced 20 of 12.3M — inert by construction,
   found by measurement.

   **The RSS half closed as two mechanisms, seven dead suspects later**
   (walk residency, overlay clones, sweep path memo, arena retention,
   diagnostics channel — each killed by a control arm or a byte counter,
   never by arithmetic): (a) **brk retention of decode-and-drop churn**
   owns the SUSTAINED figure — `MALLOC_MMAP_THRESHOLD_=65536` returns 53%
   (the server's number; ship posture is a product trade — jemalloc /
   threshold / accept); (b) **per-worker in-flight working sets** own the
   CREST — one file's sweep memo measured 633 MB, peak linear in worker
   count at an invariant per-file footprint — fixed by byte-capping the
   per-file sweep memo (256 MB drop-oldest, `PERL_LSP_SWEEP_MEMO_MB`),
   the corpus-neutral shape: a worker cap was measured nearly free on
   FHEM (memory-bound) but would tax a CPU-bound corpus. The path memo
   itself is LOAD-BEARING (1.55x wall) and stays.

4. **The registry chase's no-answer fetches, and `main`'s program scope.**
   Measured on a real-CPAN corpus (12k files, 54% package-less scripts)
   against the substrate: top-level `PackageSymbol` query DENSITY is nearly
   equal (~180–200/file both), so query volume is not the differentiator —
   the OUTCOME MIX is. Substrate: ~3% of queries reach a candidate bag fetch
   and 52% of fetches answer nothing; real-CPAN: ~44% fetch and **94% of
   1.06M fetches answer nothing** — a million decodes to learn nothing,
   at ~1.5 candidates per escalation (so candidate-set narrowing is NOT the
   lever; the fan-out was never large). Three levers, separable:
   - **Don't decode to learn nothing** — RETIRED as a new probe by the
     consult decomposition (12k real-CPAN corpus: `baked_open` 59.9%,
     `not_local` 12.1%, `absent_but_inherits` 0.3%). The conclusion map's
     `NotLocal` verdict IS the symbol-absence skip, already live and
     carrying an eighth of consults; adding a rows probe beside it would be
     a third absence oracle where the leak is elsewhere. The leak:
     the OpenReason distribution, which is CORPUS-SHAPED — substrate-era
     prior: `AbsentNotClosed` 70.3% of decodes; 12k real-CPAN reading:
     `no_answer_linkable` 49.7%, `no_answer_self_only` 25.5%,
     `absent_not_closed` 22.8%. Two levers, both conclusion-layer roadmap,
     ranked by that reading: (a) **`Link` minting** — built, flag-gated
     (`PERL_LSP_MINT_LINKS`), parked on a substrate measurement (decodes
     4,103→4,104; "a follow abandons at the first rung whose own map says
     Decode") that predates knowing the linkable population is half of all
     open reasons on real CPAN — unparked-by-corpus is one A/B away, and a
     follow's win is bounded by TARGET-side closedness, so score
     `baked_follow` vs `baked_follow_incomplete` in both arms, not just
     decode count; (b) **world-level closedness** (the flush evaluates
     against a world; its product can stamp classes whose full ancestry
     enumeration is known) — converts the `absent_not_closed` fifth and
     raises every follow's completion odds, so it compounds (a).
   - **`main` is program-scoped**: an empty package resolves to `"main"`
     (`resolve/collect.rs`), and two scripts never share a stash, so a
     workspace-relation answer for a script's `main` is cross-program
     pollution — wrong, not just wasteful. Fix is the ScopedLookup/T3
     shape: main's candidates from asker F = F plus F's load closure's
     explicit-`package main` files; a scope rule keyed on program-locality,
     never a string match. Cost share is measured by the `mocpkg.*` /
     `mocfetch.*` attribution counters (negligible on the substrate, whose
     indexed root holds few scripts; unmeasured on script-heavy corpora).
   - **The R4 escalation lane**: `consult.moc_primary` counts raw-bag-None
     escalations to `enriched_present` — each is an overlay consult, its
     cost read directly from `consult.enriched` accumulated ns.

5. **The `MethodCall` stamp as a `Link`.** The frozen `MethodTarget` exists so
   an answer cannot depend on which verb asked; a conclusion `Link` is
   verb-independent by construction and cheap to evaluate. If the stamp's
   product were the ladder rather than the resolved value, the enrichment
   re-stamp collapses into the flush worklist's cutoff.

6. **Return-type consults through conclusions.** `MethodOnClass` is 96.0% of
   measured cross-file consults; serving them from the conclusion map removes
   the chase's bag decodes and the transitive overlay recursion behind them.
   This is the conclusion layer's own roadmap; listed here because it retires
   the `enriched_present` fallback that makes enrichment recursive.

7. **Residency policy for digests.** The export gate won because
   `export`/`export_ok` happen to be non-evictable axes; per-package
   method-name sets and bridge entity names lose because they happen not to
   be. "What survives the strip" should be derived from what the confirm
   loops read, not from field placement. The Surface already computes
   per-file method names and throws them away.

8. **Overlay key honesty — the enumerable half LANDED.** The key was already
   more honest than this rung claimed (depth-20 dep-closure walk, all loader
   shapes hashed); the real gaps were the BRIDGE and PROVIDER relations,
   whose members are not candidates of any walked name. Now
   `hash_relations_for` rides the key: bridge membership + member
   registration generations (the stamp freezes a bridging module's identity
   into overlay `MethodTarget`s) and provider membership, for the walked
   closure plus the file's own registration names. Pinned by
   `a_new_bridge_to_the_consumers_class_moves_the_enrichment_key`. The
   RESIDUAL is principled, not accidental: the stamp reads the whole world
   (any invocant class), which no closure-scoped key can cover — that
   residual collapses when rung 5 lands (a `Link`-shaped stamp freezes no
   world-dependent values), which makes 5 the true closer of 8.

9. **Enrichment as a delta artifact.** Every recursive consumer of an
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
- **A plausible product landing near a measured total is not an
  attribution.** Four fits failed in one night of the FHEM investigation
  (12 GB / 232 entries; 929 × 12 MB walk residency; clone-boundedness;
  "the sweep owns it so the clones own it") — every one was arithmetic
  that terminated at "the numbers work" instead of at "I changed one thing
  and the number moved." An attribution is a control arm or a byte counter
  at the holder; everything else is a hypothesis and must be labeled one.
- **An attempt counter is not a completion counter.** `moc.provider_fetched`
  counts attempts; the sweep memo made most of them Arc bumps, and a slice
  was cut against the wrong number before the reconciliation
  (`sweep.memo_hit + memo_miss ≈ attempts`; only `rehydrate.loader`'s n
  decodes) was done. State which kind any quoted counter is, and show the
  reconciliation when the two could diverge.
- **Counters, not vibes.** Every pre-filter counts skip/decode/unknown so the
  hit rate is a ghost-stats read, and the next ranking pass starts from
  numbers (`docs/adr/skipping-cross-file-work.md`'s denominator rules apply).
