# Hitlist — round 9 — CLOSED, all rows LANDED

H9-1/H9-2 landed `41a77cb` (generation guard + deferred reconcile);
H9-3 landed `d0ec2ac` (GatherCache: single-flight + byte-capped LRU ×4).
Final gate: cpp 1446/0, default 1391/0, gold 438/0/0/0 armed.

Dogfood-tier rows (behavior fixes — NOT tighten scope). Evidence from the
first-change slice's part-3 assessment (branch `first-change-notify`,
merged; measurements in its report + docs/forks-resolved.md follow-ups).

## H9-1 CORRECTNESS — stale-winner race on the consumer re-register swap
`module_resolver.rs:2044-2057`: `unregister_file` → `register_symbols*` is
last-writer-wins with NO generation guard. A save-during-bulk-index lets a
stale (pre-save) bulk-index result land AFTER the invalidation's fresh
result, reverting the consumer to pre-save analysis until its next edit.
Fix shape: a per-path generation stamp checked at the swap (register only
if the analysis generation ≥ the registered one), or route both writers
through one guarded seam. Also covers hazard 3 (under-invalidation of
not-yet-registered consumers: the invalidation scan misses them; the bulk
pass serves whatever bytes it read — fresh iff read after the save).

## H9-2 PERF/DESIGN — save-during-bulk-index cone runs twice, uncoordinated
`index_workspace_with_index` (module_resolver.rs:739, par_iter :1644) vs
`pack_file_changed` (:1892, par_iter :1981): no shared dedup queue;
`pack_change_lock` serializes only invalidations against each other. A
widely-included header saved mid-index re-analyzes its whole cone twice,
interleaved. Chromium-scale: the cone is ~the tree. Design direction (from
the storm analysis): during the initial index, saves feed a deferred
invalidation set reconciled ONCE at index completion — needs the H9-1
generation stamp as its foundation.

## H9-3 RESIDENCY/PERF — cpp gather caches: unbounded growth + no single-flight
`cpp_reparse.rs`: `pre_expanded_cache` (:2198), `macro_table_cache`
(:1904), `header_cache` (:2380), `include_closure_cache` (:2501) — plain
`OnceLock<Mutex<HashMap>>`, NO byte cap, NO LRU; eviction only via explicit
invalidation. Population is check-release-compute-insert (:2214-2229) — two
Rayon workers expanding sibling TUs' shared cone (op.c/sv.c ≈ 90% overlap)
duplicate the full expansion; last insert wins. At Chromium scale the
expanded variants stack unboundedly. COUPLED fixes: adding a cap makes
thrash real, which makes single-flight mandatory (evicted-cone re-expansion
storms) — land single-flight first or together, byte-account per the
residency discipline, update docs/prompt-storage-residuals.md.
