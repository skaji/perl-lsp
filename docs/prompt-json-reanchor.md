# The re-anchor invariant: bounding misparse blast radius on class membership

Closes the open half of `docs/adr/config-superposition-declarations.md`
Case A (the json.hpp `basic_json` attribution blast radius) and the
matching residual-bug entry in `docs/PARKED.md`.

## The bug, measured

`nlohmann/json.hpp` (amalgamated, 26k lines). `basic_json` spans lines
**20641–25771** (~5130 lines). Baseline attribution (`--lang-analyze`,
symbols in the class's line range grouped by their `package`):

| package               | count |
|-----------------------|------:|
| `nlohmann` (namespace)|   673 |
| `basic_json`          |    92 |
| nested (`json_value`…)|    64 |

Only 92 members carry `basic_json` membership; **673 fall through to the
enclosing `nlohmann` namespace**. Attribution is contiguous: every member
up to line **21449** attributes to `basic_json`, and everything from
**21454 → 25771** attributes to `nlohmann`. A single break point, ~84%
of the class dark: empty member completion, cross-file hover corruption,
goto-def failure over the back of the class.

## The deep trigger (named, with evidence)

**`#if JSON_DIAGNOSTIC_POSITIONS` in constructor-initializer / declaration
position.** json.hpp:21396:

```cpp
    basic_json(const BasicJsonType& val)
#if JSON_DIAGNOSTIC_POSITIONS
        : start_position(val.start_pos()),
          end_position(val.end_pos())
#endif
    {
```

The directive sits between the declarator and the body `{`. tree-sitter-cpp
mis-recovers the guarded init-list region and, as a knock-on, matches the
class body's opening `{` (line 20643) against the **wrong** closing `}` —
the constructor body's `}` at line 21450 — so the `class_specifier` node
**truncates at row 21450** instead of the true 25771. Evidence
(faithful pipeline: gather → `preprocess_validated_with` → parse):

- The tree has a `basic_json class_specifier` spanning rows **20641..21450**
  (truncated) — not one reaching 25771.
- Members inside the truncated node (≤21449) attribute to `basic_json`;
  members after it are siblings in the namespace `declaration_list`, so the
  sticky context is `nlohmann`.
- Blanking just the five `#if…#endif` lines at 21397–21400 shifts the break
  down and recovers 92→164 `basic_json` members — the `#if` is causal.
- There are **6** `JSON_DIAGNOSTIC_POSITIONS` `#if`s in member position
  inside `basic_json` (21397, 21708, 21782, 21794, 21820, 24828); each is a
  fresh truncation point. Blanking one only moves the break to the next.

### Why slice-1's `strip_declaration_position_directives` misses it

The slice-1 repair (`cpp_reparse::strip_declaration_position_directives`)
blanks a misparsing conditional directive **only when the `preproc_if` node
itself carries `parse_damage > 0` and has a `field_declaration_list`
ancestor**. Here the misparse ERROR lands as a **sibling** of the
`preproc_if` (in the init-list gap between declarator and body), so
`parse_damage(preproc_if) == 0` and the region is skipped. An isolated
80-line class of the same shape DOES get repaired (the ERROR lands inside a
recoverable field list) and attributes fine — which is exactly why the ADR
recorded "the ctor `#if` in isolation causes only local damage." The real
file's damage escapes the repair's gate. Chasing that gate is a
point-repair for one construct; the next mis-recovering construct re-opens
the blast radius.

## The fix: re-anchor on the ORIGINAL source's brace structure

**The invariant:** a member attributes to the innermost container whose
brace range **textually** encloses it, even when that container's
`class_specifier`/`namespace` node is corrupted — so no local misparse has
unbounded blast radius. This bounds EVERY future misparse cause, not just
`JSON_DIAGNOSTIC_POSITIONS`.

### Why the recovery must read the ORIGINAL source, not the transformed tree

The obvious mechanism — brace-match the class body `{` over the tree's
`{`/`}` tokens — **fails**, because the C++ macro-expansion transform
introduces a brace imbalance. Measured over the `basic_json` byte range:

| source                 | `{` | `}` |
|------------------------|----:|----:|
| original               | 646 | 646 (balanced) |
| transformed (raw text) | 682 | 710 |
| transformed (tree `{`/`}` tokens) | 631 | 643 |

So a brace-match on the transformed source closes early (raw at row 24280,
tokens at row 21870) — never at 25771. The **original** source is
balanced, and a comment/string/char-literal-aware brace scan from the class
body `{` (row 20643) lands exactly on **row 25771**. Recovery therefore
runs **after `remap_spans`**, when skeleton symbols are back in ORIGINAL
coordinates, and brace-matches the ORIGINAL source.

### Mechanism

A post-remap pass (`SkeletonAnalysis::reanchor_truncated_containers`, gated
by a declarative pack capability — brace-delimited container scopes, never
`lang == cpp`):

1. **Container extents.** Every container symbol (kinds that own a
   member scope: class/union/struct/namespace) is a real, declared symbol
   with a name + start position. Brace-match the ORIGINAL source from its
   body `{` (comment/string/char-literal aware) to get its true
   `[open, close]` byte range. Forward declarations (`class X;` — a `;`
   before any `{`) are skipped: no body, not a container.

2. **Nesting.** Containers nest by range containment — each container's
   parent is the innermost other container that encloses it.

3. **Re-anchor, upgrade-only.** For each symbol, find the innermost
   container whose true range encloses it (`textual`). Re-attribute the
   symbol's `package` to `textual` **only when `textual` is a proper
   descendant of the symbol's current container** (or the symbol had no
   container). This is the anti-fabrication guard:
   - Truncation fall-through — member fell OUT to the enclosing namespace
     (`current = nlohmann`, `textual = basic_json`, and `basic_json` nests
     inside `nlohmann`): **upgrade** to `basic_json`. ✓
   - Out-of-line definition (`void nlohmann::Foo::bar(){}` at namespace
     level, `package` set to `Foo` by the `::`-qualifier): `textual =
     nlohmann` is an ANCESTOR of `Foo`, not a descendant — **left alone**. ✓
   - Already-correct member (`current == textual`): no change. ✓

   Upgrade-only means the pass never moves a symbol sideways or to a
   shallower scope — it only pulls members back INTO the deeper class they
   textually live in. It never invents membership: every target is a real
   declared container that textually encloses the member (the
   `structure_count` anti-gaming spirit — recovery of real members, never
   fabrication).

### Why general over a point-fix

A repair for the `JSON_DIAGNOSTIC_POSITIONS` construct (fixing the
`strip_*` gate so the ctor-`#if` blanks) leaves construct Y's blast radius
unbounded — the next tree-sitter-cpp mis-recovery that truncates a
`class_specifier` re-opens the same 4000-line hole. The re-anchor invariant
is indifferent to the CAUSE of truncation: as long as the original source's
braces balance (they do — it is real, compiling code) and the container is
a declared symbol, membership recovers. Point-repairs remain welcome as
complements (they reduce the count that needs recovering), but the
invariant is the deliverable.

## Acceptance

- RED fixture: a class with a `#if`-in-ctor-initializer mid-body followed
  by many members; a LATE member must attribute to the class (member
  completion / goto-def / hover).
- Real json.hpp: `basic_json` member attribution before/after
  (headline: the ~673-member `nlohmann`→`basic_json` recovery).
