# hitlist-5 — fresh C++ daily-driver dogfood (abseil + split nlohmann)

> **STATUS (fix/inline-ns-transparency).** **Family A — LANDED** (slices 1 + 2).
> `pack_inline_owner_set` is now threaded into the owner-comparison seam so
> goto-def (`member_def_location` → `pack_member_of`), completion, and the
> `refs_to` package gate all honor inline-namespace transparency; the pack
> `Scope::member` miss no longer manufactures a file-top `1:1` (returns no-def,
> fail-safe). Abseil acceptance: `absl::AsciiStrToLower(&result)` gd was
> `check_op.h:1:1` → now `ascii.h:188:6` (real decl); `absl::ascii_isspace`
> references 1 → 8 (cross-file qualified + unqualified uses). Gold rows:
> `cpp-inline-ns-transparency-*` (definition + references),
> `cpp-inline-ns-absent-member-goto-def-fail-safe`.
>
> **PARKED residuals** (same family, different seam — not this resolution-gate
> slice):
> - **A1 function-designator emission.** `absl::ascii_isspace` passed by name to
>   `std::find_if_not` (no `()`, ascii.h L245/251/259/265) is never emitted as a
>   `FunctionCall`/use ref, so references still misses those 4 in-file sites.
>   This is a builder ref-EMISSION gap, not the resolution gate.
> - **A3 completion detail cosmetics.** Now that the owner set unifies, the
>   `head`/`ascii_internal` detail-noise cleanup can ride the same set — cosmetic.
> - **Family B (nlohmann `Container::emplace_back`).** The pack `Scope::member`
>   fail-safe is now in place for the `FunctionCall` shape; the nlohmann alias-
>   member variant wasn't re-probed here.
> - **Family C (member-name over-report), D, E** — untouched (different seams).

Probe base: `182dc236` (tip of `spike/cpp-support`), binary `cargo build
--release --features all-langs` (verified `--languages`: perl, cpp, python, r,
cmake). Corpora: `/home/veesh/personal/cpp-bench/abseil-cpp` (flagship modern
C++; `ABSL_NAMESPACE_BEGIN` macro-wrapped headers) and
`/home/veesh/personal/cpp-bench/json/include/nlohmann/` (the SPLIT headers —
`basic_json` member attribution deliberately NOT re-probed; assigned elsewhere).

All coordinates 0-indexed `line:col`. Every probe ran warm/synchronous through
`perl-lsp --batch <root>` (full startup — workspace index + macro gather
complete before the first answer), so NONE of these are the cold-open window.
Warmup discipline: the first CLI query on each fresh root was discarded. Every
reference count is grep-calibrated. Reduced fixture:
`docs/hitlist-5-fixtures/inline_ns_transparency.h`.

---

## TL;DR — the one mechanism behind most of it

C++ **inline-namespace transparency** is implemented (`resolve.rs::
pack_inline_owner_set`) but **wired into completion only**
(`complete_pack_qualified`). The goto-def owner lookup
(`member_def_location`/`pack_member_of`) and the references package gate
(`pkg_agrees` inside `refs_to`) both compare the *raw innermost package* and
never expand the inline-owner set. Abseil's `ABSL_NAMESPACE_BEGIN` expands to
`inline namespace head {` (`absl/base/options.h:154` literally defines
`ABSL_OPTION_INLINE_NAMESPACE_NAME head`), so **every `absl::` symbol is filed
under package `head`.** A qualified `absl::X` use carries `resolved_package =
"absl"`; the def symbol's package is `"head"`; the gate says they disagree and
drops / mis-routes the query. nlohmann's `NLOHMANN_JSON_NAMESPACE_BEGIN` is the
same shape. This single seam gap produces the references undercount AND the
`check_op.h:1:1` garbage goto-def.

---

## Family A — inline-namespace transparency honored by completion, not by gd/gr

**Owning seam.** `src/resolve.rs`:
- `pack_inline_owner_set` (≈3501) computes `owner ∪ {inline namespaces under
  it}` — the correct transparent set. Its ONLY caller is
  `complete_pack_qualified` (≈2324).
- `pack_member_of` (≈3455): `Sub|Method => s.package.as_deref() ==
  Some(owner)` — exact match, no inline expansion. Used by
  `member_def_location` (≈1102), the owner-anchored goto-def step (≈1491).
- `refs_to` package gate `pkg_agrees(ns_relative, pkg, scope)` (≈4325):
  compares a call's `resolved_package` to the target's callable scope — again
  the raw innermost package.

### A1 — references undercount on qualified / function-designator uses (HIGH pain)

`references` on `absl::ascii_isspace` (ascii.h def L104:12) → **1 result**: the
declaration itself. grep: 4 in-file uses (L245/251/259/265, all
`absl::ascii_isspace` passed to `std::find_if_not`) + 16 repo-wide. From a use
site (L244:59) → `[]` (empty).

`references` on the `AsciiStrToLower(std::string*)` decl (ascii.h L188) → 4
hits (the .h decl + 2 inline overload names + ascii.cc def) but **misses the
call site at L206** (`absl::AsciiStrToLower(&result)`) and every test/bench
caller.

Contrast (works): `references` on `StripLeadingAsciiWhitespace` (ascii.h L243)
→ 6 hits including cross-file `absl/time/format.cc:110/115` — because those
calls are *unqualified and same-inline-namespace* as the def, so the raw
package matches.

**Reduced repro** (`inline_ns_transparency.h`): a free function defined inside
`inline namespace v1 { }` under `namespace mylib`. `references` on its def →
**only the def**; BOTH the unqualified `is_thing(1)` and the qualified
`mylib::is_thing(2)` calls are dropped (def package `v1`; call
resolved_package `mylib`). Remove the inline namespace and the qualified call
reappears. That isolates the inline layer as the discriminator.

Two dropped shapes: (a) qualified `outer::f(...)` calls where the def sits in
an inline child of `outer`; (b) function-designator uses (`absl::ascii_isspace`
passed by name, no `()`) — likely not emitted/matched as a `FunctionCall` ref
at all (secondary gap in the same family).

### A2 — goto-def on a qualified free-function call → garbage `check_op.h:1:1` (HIGHEST pain)

`definition` on `absl::AsciiStrToLower(&result)` (ascii.h L205:8) →
`absl/log/internal/check_op.h:1:1`. `check_op.h` contains **no**
`AsciiStrToLower` (grep-confirmed). Same for `absl::AsciiStrToUpper` (L236:8) →
`check_op.h:1:1`. `1:1` is the file-top signature of the module→file fallback:
owner-anchored `member_def_location("absl", "AsciiStrToLower")` misses (package
`head` ≠ `absl`), the by-name fallback can't disambiguate the 4 overloads, so
`absl::AsciiStrToLower` is treated as a module path and jumps to some file
declaring namespace `absl`. A **confidently wrong answer** — worse than "not
found."

Single-definition names accidentally survive: `absl::ascii_isspace` (L244:59)
→ correctly `ascii.h:105:13`, because the unique-name by-name fallback resolves
it even though the owner-anchored path missed. So the bug hides on simple names
and bites on the overloaded / common ones.

### A3 — completion leaks the phantom `head` and nested namespaces (LOW / cosmetic)

`completion` after `absl::` (ascii.h L244:59) DOES list the free functions
(transparency works here) but every item's detail reads `<name>  head`, and the
list includes the sub-namespaces `head` and `ascii_internal` themselves. Members
are present and usable; the noise is cosmetic. Filed for completeness — the
Slice-1 unification should make the detail read `absl`.

---

## Family B — unresolved `Scope::member` returns file-top `1:1` instead of nothing (HIGH pain)

`definition` on `Container::emplace_back` (ordered_map.hpp L82:19) →
`ordered_map.hpp:1:1`. `Container` is a `using`-alias for `std::vector<...>`;
`emplace_back` has no local def. The correct answer is "no definition" (it's a
std member). Instead the goto-def tail treats `Container::emplace_back` as a
module path and returns the file top — the same module→file fallback as A2.

**Owning seam.** The PackageRef / type-fallback tail of the pack goto-def in
`resolve.rs` (after `member_def_location` misses). When `Scope` is a known
namespace/alias and the member did not resolve, it must return empty, not a
`1:1` file location. This overlaps A2's fallback: fixing it hardens both.

---

## Family C — member-name references over-report across unrelated classes (MEDIUM pain)

`references` on `ordered_map::key_type` (the `using key_type = Key;` typedef,
ordered_map.hpp L31:10) → hits in
`detail/iterators/iter_impl.hpp:731`, and other files — i.e. **every class's
`key_type`** in the workspace, not `ordered_map`'s. The member/typedef
references arm in `refs_to` matches by bare name with weak class scoping. This
is the SAME "identity not scoped to its owner" root as Family A but in the
over-count direction. `find-references` on any common member name (`key_type`,
`size_type`, `value_type`, `at`, `find`) is polluted.

---

## Family D — bare sibling-method call resolves to a foreign same-named method (HIGH pain)

`definition` on the unqualified sibling call `emplace(key, T{})` inside
`ordered_map::operator[]` (ordered_map.hpp L103:15) → `json.hpp:3303:31`, and
`hover` there → `std::pair<iterator,bool> emplace(Args&&...)` labeled
*function* — that is **`basic_json::emplace`, the wrong class**. The correct
target is `ordered_map`'s own `emplace(const key_type&, T&&)` at
ordered_map.hpp:73.

**Owning seam.** `language_driver.rs::emit_return_fuel`'s bare
sibling-method-call pinning (`resolved_package` → enclosing class) plus the
method-call resolution in `resolve.rs`: the pin to enclosing `ordered_map`
either wasn't applied or lost to a workspace-wide same-name method winner
(`basic_json::emplace`). Because the wrong target is `basic_json`, this is
**adjacent to the assigned "json.hpp basic_json re-anchor" work** — see overlap
list; the sibling-pinning failure may share that root.

---

## Finding E — namespace-scope `extern` variable decls are not symbols (MEDIUM-LOW pain)

`definition` on `ascii_internal::kPropertyBits` (ascii.h L90:26) → "No
definition found"; same for `ascii_internal::kToLower` (L183:25). The decls
(`ABSL_DLL extern const unsigned char kPropertyBits[256];`, ascii.h L71/74/77)
are never emitted as symbols — absent from `--outline`. Goto-def on a
namespaced global variable is dead.

**Owning seam.** `queries/cpp/skeleton.scm` has no capture for `extern`
variable declarations at namespace scope (only class fields / locals). Add a
namespaced-variable-decl capture + `query_extract.rs` emission.

---

## Calibration — what WORKS (so the families above are precise, not blanket)

- Hover on defs and simple refs: correct signatures + `*function*`/`*method*`.
- Enum-value goto-def + hover **cross-file**: `value_t::object`
  (from_json.hpp L97:22) → `value_t.hpp:56:5`, hover `object: value_t`
  *enumerator*. Solid.
- Typedef goto-def within a struct: `key_type` use (ordered_map.hpp L72:44) →
  the `using key_type` at L31. Solid.
- Unqualified in-namespace call goto-def: `StripLeadingAsciiWhitespace(str)`
  (L279:2) → def L243. Solid.
- `this->` member completion inside `ordered_map` (L74:29): lists the struct's
  methods + typedefs (emplace/erase/find/insert/at/count). Noise: also lists
  in-scope locals (`key`,`t`,`it`) and misses inherited `std::vector` members —
  minor, members present.
- references on unqualified-called free functions: found cross-file. So
  references is NOT globally broken — specifically the qualified / designator /
  member-name shapes (Families A1, C).

---

## Family synthesis (mechanism → seam → findings)

| Mechanism | Owning seam | Findings |
|---|---|---|
| Inline-namespace owner set expanded for completion only | `resolve.rs`: `pack_member_of` / `member_def_location` / `pkg_agrees` don't call `pack_inline_owner_set` | A1, A2 (resolvable half), A3, root of B |
| `Scope::member` miss → module→file `1:1` fallback | goto-def PackageRef/type tail in `resolve.rs` | A2 (garbage half), B |
| Reference identity matched by bare name, not owner-scoped | `refs_to` member/typedef arm | C |
| Bare sibling-method not pinned to enclosing type | `emit_return_fuel` pinning + method-call resolution | D |
| No symbol for namespace-scope `extern` var | `queries/cpp/skeleton.scm` + `query_extract.rs` | E |

---

## Proposed fix slices (ordered by daily-driver pain)

1. **Inline-transparency unification (opus, resolve core).** Thread
   `pack_inline_owner_set` into `pack_member_of` + `member_def_location` + the
   `refs_to` `pkg_agrees` gate, so gd/gr honor inline transparency exactly as
   completion already does (DRY — one owner-expansion helper, three consumers).
   Kills A1 (qualified-call undercount), the resolvable half of A2, fixes A3
   detail, and removes the root that pushes B into the fallback. Highest ROI.
   Add the function-designator (address-of / passed-by-name) ref-emission check
   as a sub-task of A1.

2. **Kill the `1:1` module-file fallback for failed `Scope::member` (opus/sonnet,
   resolve goto-def tail).** When `Scope` is a known namespace/alias and the
   member didn't resolve, return empty, never a file-top location. Kills B,
   makes A2 fail safe (no-def) instead of garbage even before Slice 1 lands.

3. **Sibling / implicit-this method pinning (opus, coordinate with basic_json
   assignee).** Ensure a bare sibling-method call inside a struct method pins to
   the enclosing type before any workspace-wide same-name method can win. Kills
   D. Verify against the assigned basic_json re-anchor work first — likely
   shared root.

4. **Member-name reference scoping (opus/sonnet, `refs_to` member arm).** Gate
   member/typedef references on the owning class so `key_type` refs don't leak
   across every class. Kills C.

5. **Namespace-scope `extern` variable emission (sonnet, query + extract).**
   Capture + emit namespaced `extern` var decls as symbols. Kills E.

---

## Overlap with in-flight assigned work

- **Family D (ordered_map `emplace` → `basic_json::emplace`)**: the WRONG target
  lands in `json.hpp`'s `basic_json`. Adjacent to — possibly the same root as —
  the assigned "json.hpp basic_json re-anchor blast radius." Do NOT fix D in
  isolation; reconcile with that assignee (sibling-pinning may be their fix's
  downstream). Flagged, not claimed.
- **No overlap** with: the cold-open degraded window (every probe here was warm
  `--batch`), op.c salvage-budget/pTHX_ (Perl-C, not touched), or cross-file C
  free-function return typing (these findings are C++ *resolution identity* —
  gd/gr/completion — not C return-type inference).
- Families A, B, C, E are **new** — not on the assigned list.
