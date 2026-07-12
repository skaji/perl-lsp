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

Triaged (veesh, 2026-07-07):

- **Unused exports: build.** One new `syms` flag bit (exported) +
  `REF_ROWS_VERSION` bump; the view is sound in exactly one direction —
  zero cross-file candidate rows ⇒ truly unreferenced (rows are
  name-match candidates, so nonzero means "unknown", never "used") —
  the right polarity for a dead-code queue. Doubles as a sound
  pre-prune for `--heatmap`'s per-declaration references projection.
- **Implementors-of-role: parked awaiting a consumer.** Isa/bridge
  edges aren't shredded; this needs a new edge table — pay that only
  when a code lens or query verb wants it.
- **Callers-by-arg-type: declined as SQL.** Types live in the witness
  bag, and bag + fold stay blob + in-Rust (the ratified hybrid
  boundary). If needed, it's a Rust report walk.

When unused-exports lands, this brief deletes (docs-gc rules). Revisit
Salsa only if the view graph deepens past the dirty-set walk
(`docs/open-forks.md`).
