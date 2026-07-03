# hitlist-2 — dogfood round 2 (abseil / fmt / json / redis)

Task-driven CLI dogfooding on spike `3f5a227e`, three sonnet agents, every
gr count grep-sanity-checked. Full tables in the agents' reports; this is
the synthesized, deduplicated list. Wave-2 (minimal repros + the gr-count
mechanics) findings appended at the bottom.

## What held (the earned wins — regression-guard these)

- gi family walk: `formatter` primary → 10 specs across 7 headers ✅
- Wrapper-macro delegation: ranked gd (`s_malloc` → `(delegates to
  zmalloc)`), gr folds wrapper call sites (330, grep-consistent) ✅
- Config-variant macros end-to-end (`REDIS_STATIC` gr exact) ✅
- Enums at scale exact (gr 5/5, 20/20 vs grep) ✅
- Plain free functions: gr 63 vs grep 64 ✅
- Outline never crashes (json.hpp 26k lines, 1834 entries, 2.5s);
  disambiguates specs by full argument spelling ✅
- Struct outlines (robj bitfields owned) ✅; 5,255-ref query no hang ✅
- Template-instance gd (`flat_hash_map<string,string>` → the template) ✅
- Type-aware member completion (`status.` annotated with return types) ✅

## THE THEME: one-symbol verbs key on the bare name

gd / gr / hover / completion resolve by bare name — no template-args, no
arity, no namespace, no enclosing class. Invisible on unique names,
catastrophic on collisions:

1. **gr on a spec member sweeps the world** — `formatter<weekday>::format`
   → 1621 hits (~270:1 noise), incl. gtest.h. `basic_json::dump` → 45 vs
   8 real. [fmt, json]
2. **Type-REFERENCE gd ignores the spec ladder** — `: formatter<std::tm,
   Char>` base-clause use → the PRIMARY (3×). The dispatch ladder exists
   for member resolution (slice b/c) but the type-name gd lane never
   consults canonical-spelling → per-spec class. [fmt]
3. **Overload arity-blind** — `vformat` 2-arg gd → the 3-arg overload;
   gr mixes both. [fmt]
4. **Namespace-blind** — `detail::vformat_to` gr returns the PUBLIC
   `fmt::vformat_to`'s sites, missing its own def+caller. [fmt]
5. **`ns::EnumClass::kValue` middle-segment gd dark** (bare 2-part works).
   [abseil]

## gr counts: BOTH over- and under-collection, by lane

- OVER: the bare-name sweeps above.
- UNDER, whole files dropped: `ABSL_GUARDED_BY` 21/66 (10 of 27 files),
  `ABSL_ATTRIBUTE_LIFETIME_BOUND` 160/294, `format_to` 10/93. Redis
  (same-dir `"x.h"` includes) matches grep EXACTLY — abseil/fmt
  (`<absl/…>`/`<fmt/…>` project-root includes) drop files. **Hypothesis
  (wave-2 tested): `resolve_include` fails project-root-relative
  includes → partial include closures → the visibility gate eats
  legitimately-including files.**
- Enum value as TEMPLATE ARGUMENT not a ref (`MakeError<StatusCode::
  kNotFound>`). [abseil]
- Ref inside another macro's BODY missed (`OBJ_ENCODING_EMBSTR` in
  `sdsEncodedObject`'s #define). [redis]

## Structural parse/extraction bugs (wave-2 reduced to minimal repros)

6. **Outline scope desync** — multi-line `noexcept(...)` ctors and
   `&&`-ref-qualified members desync scope tracking; everything after
   reparents wrong (~800 lines in raw_hash_set.h), `private:` leaks as a
   Variable. 3 files independently. [abseil]
7. **Reopened namespaces lose attribution** — only the FIRST `namespace
   detail {` becomes a Package; later reopenings orphan their symbols
   (json.hpp: 1 of 36 → parser/lexer/serializer all orphaned; also fmt
   base.h/format.h). Cascades into scope-blind completion. [fmt, json]
8. **`using Base::insert;` member re-exports** — abseil containers' whole
   public API: invisible in outline, gd resolves to an UNRELATED
   container's `insert` (confidently wrong), gr/hover empty. [abseil]
9. **Pointer-returning prototypes dropped** — `robj *createObject(...)`
   decls: 8 of 16 sampled dropped from outline, params leak as orphan
   Variables (pointer-decl ambiguity). [redis]
10. **Function-pointer typedefs invisible** — `typedef void *(*Name)(…)`:
    gr empty even on self. The other idiom (`typedef int Name(Args)`)
    has gr working but gd/hover dark — verbs disagree on one symbol.
    [redis]
11. **`MACRO\nclass X : …` kills the class** — NLOHMANN_BASIC_JSON_TPL_
    DECLARATION (macro standing in for `template<…>`): basic_json has NO
    Class entry, 292/316 members orphaned, `j.` completion returns zero
    real members. (strip_declarator_macros covers `class MACRO X`, not
    `MACRO\nclass X`.) [json]
12. **Operator overloads: total blind spot** — 0/43 in format.h outline;
    gd/gr/hover all dead on `operator+`. [fmt]
13. **`.def` parsed as PERL** — commands.def (12.7k lines, the command
    dispatch table) entirely dark; unknown extensions fall back to the
    Perl grammar. Needs content-sniffing or a config seam. [redis]

## Cross-verb inconsistencies

14. **hover dark where gd works** (template instances, guarded fields,
    macro-body refs) and vice versa (gd dark where hover resolves —
    `.gossip` nested union field). Hover is NOT a CandidateSet projection
    yet (the ADR's "future" row) — this is that gap, measured, in three
    projects.
15. **decl→def preference missing for plain C/C++** — static forward
    decl → itself; `extern struct redisServer server;` → itself (not
    server.c:84); FMT_API decl → stuck, never reaches the FMT_FUNC body.
    (The macro lane HAS prefer-definition; the plain lane doesn't.)
    [redis, fmt]
16. **Union members through anonymous-struct nesting** — `hdr->data.ping`
    gd/hover dark cross-file (the union DX covered the defining file's
    outline/hover; nested-anon + cross-file access doesn't resolve).
    [redis]

## Noise / cosmetics

17. Include-guard reachability label is zero-signal (`(if
    !defined(__REDIS_H))` printed on nearly every gd in the header —
    suppress the file's own guard). [redis]
18. Completion offers private members/statics (no access-specifier
    filter); `fmt::` qualified completion = unfiltered file-local soup.
19. Kind mislabels: enumerator/field/macro-annotated member → *variable*
    / *method* / *function*. `bool` → `Numeric`.
20. CLI coordinate conventions inconsistent (outline JSON 0-idx;
    definition/references output 1-idx). "pack-language files" banner on
    a pure-C++ workspace.
21. Duplicate class entries when a body contains certain macros. [json]

## Suggested fix-slice grouping (for the next queue)

- **A. Include-closure resolution for project-root includes** (the
  under-collection root, if wave-2 confirms) — highest gr-correctness
  value.
- **B. Identity precision for one-symbol verbs** — member refs key on
  owner class (kills #1); type-ref gd consults the spec ladder (#2);
  namespace participation (#4, #5). Arity (#3) = additive depth,
  evaluate.
- **C. Extraction structural fixes** — #6 scope desync, #7 reopened
  namespaces, #9 pointer prototypes, #10 fn-ptr typedefs, #12 operators,
  #11 MACRO\nclass (macro lane), #8 using-re-exports (model as the
  import-edge/role shape it is).
- **D. Hover joins the CandidateSet** (#14) + decl→def preference (#15).
- **E. Small: #13 .def language routing, #16 nested-union access, #17
  guard-label suppression, #18-21 polish.**
