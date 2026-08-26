# ADR: Reparse as a stratified pre-extraction phase

## Context

A declaration in a (possibly imported) dependency can change how a *different*
file parses:

- **Perl prototype.** `sub sner($) {...}` makes `sner` unary, so `sner 1, 2`
  parses as `sner(1), 2`; `sub sner() {...}` makes it nullary, so `sner + 1`
  is `sner() + 1`. tree-sitter-perl cannot know the grouping until it has
  seen the prototype, which may live behind a `use`.
- **C++ macro.** `class API_EXPORT Widget {...}` only parses as a class once
  `API_EXPORT` is known to expand to (near) nothing. Until then
  tree-sitter-cpp reparses the whole class as a `function_definition` — the
  class evaporates; its methods are recovered as free functions, so names
  survive but the class structure does not. This is the single most
  ubiquitous real-world idiom (every exported / `__declspec` / Windows
  header) and the highest-value reparse target.

Both are the same fact wearing two hats: **the parse of a file depends on
declarations in its dependencies** — not on inferred types, on declarations.
The engine already crosses files for types; a source transform run before
extraction lets the parse cross files too, along the same dependency edge.

## Decision

Run reparse as its own fixpoint, strictly upstream of and never interleaved
with the type worklist:

```
parse → reparse*(facts) → extract → worklist*(types)
        └── fact fixpoint ──┘        └── type fixpoint ──┘
```

**Why this is sound.** The worklist's monotonicity is about witnesses:
append-only, finite lattice, fixpoint when the snapshot stops moving. A
reparse changes the tree, hence spans, hence the identity of every
`Expr(span)` / `Variable{scope}` attachment — interleaved with the fold it
would orphan witnesses and force deletion, breaking monotonicity. The escape
is that **parse-changing facts are type-independent**: a prototype shape or
a macro definition is a declaration, known before any inference runs. So the
dependency is one-directional and the two fixpoints never need to interleave.
The bag is seeded only from the settled tree.

The reparse fixpoint has its own, simpler soundness argument: state is the
set of known proto/macro facts, monotone-increasing and bounded by the
finite set of names in scope; each round either learns ≥1 new fact or halts;
a fact's content is fixed, so learn-order does not change the result.

**The type→parse "leak" is actually declaration→parse.** C++'s `a < b > (c)`
(template instantiation vs. two comparisons) looks type-dependent, but the
disambiguator is "is `a` a template name?" — answered by `a`'s declaration,
not an inferred type. Same for `typename T::x` and most-vexing-parse cases:
the deciding fact is declaration-level, so it lives in the same monotone
fact-set as prototypes and macros and reparses in the stratified phase. No
case of an *inferred* type changing the parse is known to exist for these
languages; if one surfaces, isolate it rather than promoting the whole
pipeline to interleaved.

**Overload does not threaten stratification — it's dispatch, not grouping.**
"Type-sensitive" splits into two claims: type-sensitive *parsing* (does tree
shape depend on a type? — always reduces to a declaration fact for these
languages, never an inferred type) and type-sensitive *dispatch* (given a
fixed parse, which function does a call/operator invoke? — inherently about
inferred types, resolved downstream in the worklist, never touching the
parse). `$a + $b` / `a + b` / `$obj->[0]` parse identically whether or not
the operator is overloaded; overload only changes which sub runs. It is
operator-spelled method dispatch and rides the same monotonic resolution as
any other invocant-typed call — it never asks the parse to change. Overload
*resolution* itself (ranking, ADL, conversions, templates, ODR) is a genuine
depth concern that stays on the Clang/EDG-frontend side, orthogonal to
stratification.

**Anchors carry spans back to user text.** A transform shifts byte offsets,
so every span the skeleton extracts from the transformed buffer must map
back to original source for goto/rename/refs to land correctly. The model
(Zed's `Anchor`: a position bound to a logical point, not a raw offset) is
carried alongside the transformed source as a map (`AnchorMap` / `SpliceMap`
depending on language); extracted symbol and ref spans resolve through it.

**Expansion is amortized to definition time, never per-edit.** The per-edit
path never shells to a real preprocessor. A macro's parameterized template
is learned once (sentinel-probe the definition, observe where the probe
lands in the expansion) and baked into a plugin fingerprinted on the
`#define` text, so editing the macro invalidates it through the existing
plugin-fingerprint hard-clear. Unlearned macros degrade honestly to opaque
tokens (skeleton-only navigation, no type claim) rather than a guess.

## Status

**C++ macro expansion is production, wired into the `cpp` feature's
`PackDriver` (`src/build/cpp_reparse/`, `preprocess_validated_with` as the
driver's `transform`).** The validate-by-reparse gate keeps a splice only
when it does not increase the parser's own ERROR+MISSING count, which makes
expansion monotone: high-value idioms (attribute/declspec macros hiding a
class or struct) land, and the nested/token-paste tail is discarded rather
than corrupting a file. The classification of *what a macro's body means*
(type alias, constant, function-like sub, member-block role, …) is a
separate, more refined layer than plain expand-if-harmless — see
`docs/adr/macro-handling.md`, which owns that design.

**The Perl prototype reparenthesizer (`src/build/reparse.rs`) proved the
seam but is not wired into the build pipeline** — it is kept as a measured,
standalone module (`reparse_tests.rs`) rather than a live production path,
because the C++ side absorbed the generalization effort first.

**Multimethod dispatch is an unexplored generalization, not started.**
`ReturnExpr::UnionOnArgs { branches: Vec<(ArgGuard, ReturnExpr)> }` is
already a dispatch table keyed on `ArgGuard` (arity-based today). Dispatch
on the runtime types of multiple arguments — C++ overload resolution's
selection-among-declared-signatures shape, or a Perl plugin declaring an
arg-type→impl table — would generalize `ArgGuard` to a type-tuple pattern
matched against `q.arg_types`, reusing `UnionOnArgs` and the `Arg(n)`
positional witness with no new dispatch machinery. No code implements this;
it is future direction, not a tracked open fork.
