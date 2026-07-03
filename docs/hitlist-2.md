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
- UNDER, whole files dropped: `ABSL_GUARDED_BY` 21/66, `format_to`
  10/93. **Wave-2 ROOT-CAUSED (include-closure hypothesis REFUTED —
  resolve_include is fine; gd+completion prove closures resolve):**
  - **Split macro identity** (the macro undercounts): a function-like
    macro's occurrences split between left-unexpanded → Sub-classified
    call refs and expanded-and-erased → re-minted Variable refs →
    `FileScopeValue`; the two TargetKinds NEVER unify, so gr from any
    origin sweeps only its own lane (adjacent lines in one file give
    disjoint sets). Rule-#10 two-representations bug at the
    expansion-policy seam (`resolve.rs:172-189` Function target also
    mints empty def_paths → gate inactive on that lane;
    `language_driver.rs:700-759` re-mint; matchers `resolve.rs:3298` vs
    `:3406`). FIX: one canonical macro identity, both spellings match it.
  - **`format_to` = finding #4** (namespace-blind): 9/10 "refs" are
    sibling overload DEFS; every dropped site is a QUALIFIED
    `fmt::format_to(...)` call failing the `Sub{package}` match. Same
    fix bucket as #4/#5 (namespace participation), not a closure fix.
  - Redis is grep-exact because C has no namespaces and its macros never
    hit the lane split — structural immunity. One abseil "drop" was
    grep over-counting a `//` comment; gr was CORRECT there.
- Enum value as TEMPLATE ARGUMENT not a ref (`MakeError<StatusCode::
  kNotFound>`). [abseil]
- Ref inside another macro's BODY missed (`OBJ_ENCODING_EMBSTR` in
  `sdsEncodedObject`'s #define). [redis]

## Structural parse/extraction bugs (wave-2 reduced to minimal repros)

6. **Outline scope desync** — multi-line `noexcept(...)` ctors and
   `&&`-ref-qualified members desync scope tracking; everything after
   reparents wrong (~800 lines in raw_hash_set.h), `private:` leaks as a
   Variable. 3 files independently. [abseil]
7. **Reopened namespaces lose attribution** — WAVE-2 CORRECTION: plain
   reopenings work; the killer is MACRO-GUARDED namespace opens
   (`FMT_BEGIN_NAMESPACE`/`NLOHMANN_JSON_NAMESPACE_BEGIN`) — stripping
   those macros from json.hpp makes all 36 reopenings attribute. The
   macro-before-declaration gap, generalized to `namespace`. xfail:
   `reopen_ns.cpp`. [fmt, json]
8. **`using Base::insert;` member re-exports** — WAVE-2 CORRECTION: gd
   from a use is CORRECT (reaches Base::insert); broken = outline (the
   re-export invisible under Derived) and HOVER (wrong bare-name match,
   disagreeing with gd at the same position). xfail rows: outline +
   hover on `using_reexport.cpp`. [abseil]
9. **Pointer-returning prototypes dropped** — `robj *createObject(...)`
   decls: 8 of 16 sampled dropped from outline, params leak as orphan
   Variables (pointer-decl ambiguity). [redis]
10. **Function-pointer typedefs** — WAVE-2 CORRECTION: gr on the typedef
    works; gd FROM a use is dark. xfail: `fnptr_typedef.cpp`. [redis]
11. **basic_json invisible — root-caused DEEPER (wave-2, no minimal
    repro possible)**: the literal `MACRO\nclass X` shape WORKS via the
    expansion reparse. The real mechanism: expansion validation is
    PER-FILE — one bad macro elsewhere in json.hpp
    (`NLOHMANN_JSON_NAMESPACE_BEGIN`, object-like body embedding an
    unexpanded `##` call) expands to invalid syntax, and the whole
    file's GOOD expansions are discarded with it. Architectural:
    per-splice (not per-file) expansion validation. [json]
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

- **A. Macro identity unification** (the under-collection root, wave-2
  confirmed) — one canonical target for a function-like macro's
  Sub-shaped AND re-minted-Variable occurrences; also give the Function
  lane real def_paths. Highest gr-correctness value.
  **LANDED (wave-3)**: one `FileScopeValue` identity from every spelling
  (def, unexpanded decl-position Sub/Method/Variable artifacts,
  unexpanded calls, erased re-mints — which now survive the same-start
  claim via name-keyed claims and cover the declarator-strip blanks via
  the between-splice diff). `ABSL_GUARDED_BY` gr: 56/56/56 from
  def/use/adjacent-use (grep 66 − 10 comment mentions — grep-exact, 0
  false positives, 26 files). `ABSL_ATTRIBUTE_LIFETIME_BOUND`:
  231 from any origin (was 21/14/6-style splits); the residual ~58 sites
  are files whose PER-FILE expansion validation rejected the whole file
  (#11) so the token is swallowed by the grammar — no ref to unify; they
  join automatically when per-splice validation lands. The Function-lane
  `def_paths` is deliberately still EMPTY: the gate keys on the decl
  header being a def candidate, and #9 (pointer-returning prototypes
  dropped) starves it (`zmalloc` 330 → 3 measured); activate together
  with #9.
- **B. Identity precision for one-symbol verbs** — member refs key on
  owner class (kills #1); type-ref gd consults the spec ladder (#2);
  namespace participation (#4, #5). Arity (#3) = additive depth,
  evaluate.
  **LANDED (wave-3)** except #3: bare unresolved reads match a member
  target only when the member is an enum-constant shape (name hoists to
  the enclosing scope) — `formatter<weekday,Char>::format` gr 1621 → 17
  (family decls + receiver-resolved sites; unresolvable-receiver member
  accesses stay EXCLUDED — with owner-keyed identity the bare-name
  bucket is unbounded noise, and gd/hover still serve those sites),
  `basic_json::dump` 45 → 16. Type-ref gd walks the dispatch ladder
  (chrono.h:1904 → 2101 first, primary base.h:633 kept). Qualified calls
  mint `FunctionCall{resolved_package}` refs (span = bare tail);
  matching is namespace-aware (`pkg_agrees`: innermost-segment tails,
  None-tolerant only under partial attribution, pack files only) —
  `format_to` gr 10 → 90/26 files (grep 93 incl. comments).
  `detail::vformat_to` vs `fmt::vformat_to` separate BY QUALIFIER at
  call sites everywhere; def-side separation needs namespace attribution
  inside base.h (broken past line ~922 — #6/#7's desync), so both defs
  currently mint `Sub{None}` and their gr sets merge on the real corpus;
  the machinery separates wherever attribution exists (gold `nsqual.cpp`
  locks it). `ns::Enum::kValue` gd resolves through the qualifier
  (status.cc:223 → status.h enumerator), middle-segment gd via the
  bare-word fallback.
  **#3 arity — evaluated, NOT taken**: no cheap ranking falls out. The
  UnionOnArgs machinery discriminates RETURN TYPES by a Perl
  `cursor_context` arity hint against builder-recorded `ReturnInfo`
  arms; pack call refs carry no argument count and pack Sub symbols no
  parameter count, so an arity-aware gd ranking needs extraction to mint
  both (plus default/variadic/template-param counting rules) before a
  ranking tier in `definitions()` can consume them. That is extraction
  work (the sibling's lane), not additive resolution depth — deferred.
- **C. Extraction structural fixes** — #6 scope desync, #7 reopened
  namespaces, #9 pointer prototypes, #10 fn-ptr typedefs, #12 operators,
  #11 MACRO\nclass (macro lane), #8 using-re-exports (model as the
  import-edge/role shape it is).
- **D. Hover joins the CandidateSet** (#14) + decl→def preference (#15).
  **LANDED (wave-3)**: pack hover = `CandidateSet::hover_candidate()` (the
  top-ranked `definitions()` candidate, presented — member drill-downs stay
  adapter-side over the same invocant resolution). Measured: the
  `flat_hash_map<…>` instantiation (reflection.cc), the fmt spec
  base-clause (chrono.h:1904 → the spec, primary kept), and the
  `ABSL_GUARDED_BY` token all hover where gd answers — the last one had
  been a bare-name HIJACK (a mis-extracted decl artifact); the projection
  kills the class, not the instance. decl→def: bodied defs rank above
  bodiless decls of the same identity, decl kept — t_string.c forward decl
  → :244 (from decl AND call), `extern … server;` → server.c:85 (reverse
  closure), FMT_API vformat → FMT_FUNC format-inl.h:1457 (overload
  siblings rank too; arity is still #3-deferred). `extern` rides
  `Symbol.attributes` (EXTRACT_VERSION 139). The A-slice's deferred
  Function-lane def_paths gate is ACTIVE (minted set-level under pack
  routing); its starving case was the textual-inclusion fragments —
  `ae.c → #include "ae_epoll.c"` — fixed by extending the backward gate
  with the direct seers' closures. gr: `zmalloc` 330 (exact),
  `ABSL_GUARDED_BY` 56 (exact), `format_to` 90, `Perl_croak_nocontext`
  199 (was 194 pre-slice-C; the extras are newly-extracted prototype
  decls, grep-consistent).
- **E. Small: #13 .def language routing, #16 nested-union access, #17
  guard-label suppression, #18-21 polish.**
