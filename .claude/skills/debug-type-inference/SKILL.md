---
name: debug-type-inference
description: Debug why a Perl sub resolves to the wrong type (or to no type) using perl-lsp --dump-package — reads return_type_provenance to name the reducer or plugin that answered, and vars_in_scope to find which hop of a method chain broke. Use when hover/completion shows a wrong or missing type, or when a witness-bag change needs its effect traced.
---

# Debugging type inference

`perl-lsp --dump-package <root> <package>` runs full server startup (workspace
index, SQLite warm, on-demand @INC resolve, enrichment) then dumps every sub in
`<package>` as JSON. Per sub: bag-resolved params, `return_type`,
arity-projected returns at 0/1/2/None, witness count, framework, parents, plus:

- **`return_type_provenance`** — traces every non-default return type.
  `PluginOverride{plugin_id, reason}`, `ReducerFold{reducer, evidence}` (e.g.
  `reducer="return_arms"`), `Delegation{delegation_kind, via}`. Wire new
  derivation paths via `Builder.type_provenance` keyed by SymbolId; flushes into
  `FileAnalysis.type_provenance`. Variants in `file_analysis.rs::TypeProvenance`.
- **`vars_in_scope`** — every TC scoped to the sub's body. Surfaces chain
  assignment results: `$route` typed as `Mojolicious::Routes::Route` → chain
  typer worked. Combine with provenance on each method in the chain to find
  which hop broke.

The bag rules themselves — reducer order, the two collect/reduce phases, the
worklist invariants — live in `CLAUDE.md` under "Type inference (witness bag)".
