# Design brief — the storage engine arc: remaining forward work

The span-free Surface, the freshness engine, the relational shred, and the
R4 enrichment overlay are landed — design and current-state facts live in
`docs/adr/storage-engine.md` and `docs/adr/relational-ref-index.md`. This
brief tracks only what's still open.

## Phase 4 — materialized SQL views

The relational shred (`docs/adr/relational-ref-index.md`) plus the
freshness engine keeping it perpetually true (`docs/adr/storage-engine.md`)
make a class of "interesting data" queries buildable as SQL views over the
existing `refs`/`syms` tables, rather than one-off Rust walks:

- unused exports (exported symbols with zero cross-file `refs` rows)
- implementors-of-a-role (classes bridged/inherited from a role package)
- callers-by-arg-type (call sites filtered by a resolved argument type)

Not yet built. Land as views over the existing schema — no new tables,
no new eviction axis. Revisit whether the freshness engine's hand-rolled
dirty-set walk still suffices once this query graph exists, or whether
that's the point to reconsider Salsa (the trade-off is recorded in
`docs/open-forks.md`).
