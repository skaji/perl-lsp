# The warm lane records a different Surface than the cold lane

`Surface::project` reads the witness bag. The warm lane projects from
bag-EVICTED copies. So the same unchanged file fingerprints differently
depending on whether the process that recorded it built the analysis or
decoded it, and nothing about the degraded projection says it is partial.

Reproduced as a unit property in
`surface_tests::a_bag_evicted_analysis_projects_a_different_surface`: build an
analysis, project + fingerprint; `evict_witness_bag()`, project + fingerprint;
the two differ. Refs eviction adds nothing further — the bag is the whole
difference.

Observed end to end on the substrate (3,515 files), same bytes both runs:

```
cold:  record .../PPI/Statement.pm fp=2ee5dacc900de432   <- also the stamp written to the row
warm:  record .../PPI/Statement.pm fp=8736e9cbba9a1885
```

## Why nobody noticed

`FreshnessIndex` is in-memory and per-process. Within one process every record
comes from the same lane, so the index is self-consistent and every file is
`FirstSeen` on a fresh start. Nothing compared a recorded fingerprint against
one written by an earlier process until the self-validating conclusions row
(`docs/prompt-enrichment-alternatives.md` §6b) did.

The class was already known in the narrow: `index_perl.rs` rehydrates the bag
before recording for files with `plugin.loads`, with a comment explaining that
a bag-evicted projection "records NO shape" and every diagnostic downstream
goes quiet. That mitigation covers loader shapes. The `despan`ed
`InferredType`s on methods and values have the same dependency and are not
covered.

## What it costs

Measured on the substrate, same binary, the consult-time stamp compare
bypassed vs active (per-arm caches, three warm runs each, byte-identical
counters within an arm):

| | stamp active | stamp bypassed |
|---|---:|---:|
| `moc.provider_fetched` | 57,481 | **17,419** |
| `consult.baked_open` | 2,084 | 11,934 |
| `conclrow.valid` | 14,578 | — |
| `conclrow.stale` | 47,967 | — |

76.7% of rows are rejected as stale against rows that are in fact CORRECT —
the files did not change, the two projections disagree. The chase that
replaces them costs 3.3x the provider fetches. Wall does not separate at this
scale (4.45-5.17 s vs 4.10-4.84 s), which is the substrate being the wrong
corpus for this layer rather than the work being free.

The soundness the stamp buys is real and independent of this; the cost is not
inherent to it and disappears entirely once the two producers agree.

## Wider consequence, beyond the conclusions row

A warm-start freshness verdict is computed over a degraded Surface. An edit
that changes only bag-derived content therefore compares equal to the
degraded baseline and reads `Unchanged`, so no consumer re-enriches and every
one of them keeps answering against the pre-edit state. That is the failure
`loader_shapes` was added to the Surface to prevent, arriving through the
residency door instead of the projection door.

## Resolution: option (1), persisted surfaces

Ruled by the coordinator and landed. `Surface::project`'s output is stored
beside the blob it was projected from, in a `surfaces` table created with
`CREATE TABLE IF NOT EXISTS` in the always-run batch — no `SCHEMA_VERSION`
bump, no cache-wide rebuild. The warm lane adopts the persisted projection
instead of re-deriving a degraded twin, so cold and warm record the same
surface for the same bytes by construction.

Option (2) — stamping on something both producers compute identically — was
disqualified on scope, not cost: it removes the 3.3x that made the wrong-answer
bug visible while leaving the bug. **Agreement between the lanes is the
property; the stamp is not.**

Option (3) remains the right END STATE and deliberately did not gate this: it
is the only shape that makes the degraded projection impossible rather than
avoided, which is rule #10's "encode the property on the value so consumers
cannot ask the wrong question". A live wrong-answer bug should not wait behind
a refactor of the analysis shape.

### Version gate

Per ROW, not per database, and independent of `SCHEMA_VERSION`:
`PERL_LSP_SURFACE_FINGERPRINT` is derived at compile time over `src/model/` +
`Cargo.lock`. Narrower than the conclusion fingerprint on purpose — the
whole-tree hash would invalidate every persisted surface on an edit to an LSP
handler, paying a corpus-wide re-projection for a change that cannot alter one.

A persisted projection outliving its projector is the same class as a
derivation outliving its source: the bytes deserialize cleanly and describe a
shape this build no longer produces. A mismatch reads ABSENT, so the caller
re-projects — exactly what it did before the store existed — and the file goes
back on the repair frontier, which shares the conclusion repair's with-bag
decode and rewrites both products of the one projection.

### Measured, substrate, warm

| | before | after |
|---|---:|---:|
| `moc.provider_fetched` | 57,481 | **18,924** |
| `consult.baked_open` | 2,084 | 11,804 |
| `conclrow.valid` / `.stale` | 14,578 / 47,967 | 36,143 / **0** |
| `surface.adopted_persisted` | — | 3,336 |

Stale rejections go to zero. The stamp now costs 8.6% more fetches than
bypassing it entirely (18,924 vs 17,419) rather than 230%, and that residue is
the 1,635 `conclrow.unrecorded` — paths the freshness index has no record for
at all, which is the fail-open direction working as designed.

## The fork, as it stood

Three shapes, none of them obviously dominant at the time:

1. **Persist the Surface** and have the warm lane `record_surface_value` the
   decoded projection instead of re-projecting from a stripped copy. Cold and
   warm then record identical Surfaces by construction, and this fixes the
   freshness verdict as well as the stamp. Precedent exists: the pack warm
   lane already reads a persisted Surface out of the `stubs` table. Cost: a
   `SCHEMA_VERSION` bump (a cache-wide rebuild) or a second stub-like store,
   plus the bytes.

2. **Stamp on something both producers compute identically** — the encoded
   analysis bytes, or the `modules` row's own `(mtime, size, extract_version)`
   — instead of the Surface fingerprint. Cheaper, and it answers the actual
   question ("did this row outlive the blob it was baked from"), but it drops
   the property that makes the Surface fingerprint attractive: it is
   content-keyed, so it catches a file that changed and changed back to a
   DIFFERENT surface-equal state, and it is the value the freshness index
   already holds, so the consult needs no extra read.

3. **Make the projection bag-independent** by baking the bag-derived parts of
   the Surface into the analysis at build time. Largest change, and the only
   one that makes the degraded projection impossible rather than avoided.

Whichever is taken, the tripwire test asserts the drift as it stands today, so
a fix flips it rather than passing unnoticed.
