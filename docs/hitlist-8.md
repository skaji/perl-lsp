# Hitlist — round 8 — ROUND CLOSED, all rows LANDED

Final tip gate: cpp 1428/0, default 1374/0, gold 438 PASS / 0 FAIL / 0 XPASS
/ 0 CRASH both modes armed. Landing SHAs: H7-9 `df663b5`, H7-8/15 `6a02ad0`,
H7-13 `557a84b`, H8-1 `56b6a77`, H8-2 `bb4551a`, H8-3 `5d69fad`, parks
(macro position + include guards) `88a91e8`+`73994d5`. Close spot-checks:
implementations Iterator::Seek 11 sites, references Regexp::Simplify 9
(incl. the formerly macro-renamed pair), proxy.pl inner-closure `$c` →
Mojolicious::Controller, `->cds` references 80.

Wave-3 fix slices (H7-8/15, H7-9, H7-13 — briefs in hitlist-7) ran alongside a
verification re-probe of all round-7 fixes: **12 HOLDS, 1 PARTIAL, zero
regressions** (the partial is the parked macro-position bug surfacing in
outline — `simplify.cc:180` labels as `DontCallSimplify`; same root cause).
Fresh-hunt findings below; coordinates grep-verified by the probe.

Wave-3 outcomes (agent reports; re-landing after a sandbox rollback ate the
local branches — see the skill's operational-discipline section, added then):
- **H7-8 + H7-15** fixed on `h8-resultset-typing`: fold-time receiver-relative
  fallbacks for fluent/RowOf chains; query-time `resolve_dbic_source_moniker`
  (basename/`source_name` → result class, isa-gated); list-context row
  extraction. EXTRACT_VERSION → 171. Acceptance: inline `$artist` hover →
  Artist; `->cds` goto-def exact; cds references 80/97 (principled misses);
  `$art->update` → C3-effective `CascadeActions::update`.
- **H7-9** diagnosed as ALREADY FIXED by the CANTOPEN-race fix (files dropped
  from the row-narrowed sweep when rehydration hit the WAL window — also the
  nondeterminism). Deliverable = regression net: `__PACKAGE__->verb` cross-file
  unit test + `belongsto-fixture` gold row. 64 hits / 43 files, stable ×6.
- **H7-13** fixed on `h13-field-completion`: field type-witnesses get
  class-body-extent spans (the temporal filter rejected below-method decls);
  bare no-local receivers resolve as implicit `this->` members
  (capability-gated); `param_region` exclusion kills the nested-struct leak.

## H8-1 — mojo route-param typing lost inside nested closures
`examples/proxy.pl`: `any '/*w' => sub ($c) { ... ->catch(sub ($err) {
$c->render(...) }) }` — hover resolves `$c` lexically to the outer route sub,
but the Lite route-param synthesis that types `$c` as
`Mojolicious::Controller` does not survive into the inner closure body:
`--type-at examples/proxy.pl 15 5` → none; goto-def on `->render`
(`--at examples/proxy.pl:16:9`) → dark. The `->then`/`->catch` callback
referencing the outer `$c` is the standard Mojo async idiom.

## H8-2 — anon-sub completion leak, resolved-receiver path — LANDED (round-8 inline)
H7-10(a) filtered anon subs via `conventions::is_callable_sub_name` on the
UNRESOLVED-receiver member slot. The RESOLVED-class enumeration path lacked the
same filter: `--completion /root/corpus/DBIx-Class --at t/18insert_default.t:12:20`
(`$rs->`, correctly typed `DBIx::Class::ResultSet<Artist>`) offered
`(anon) DBIx::Class::ResultSet → Numeric` (source: ResultSet.pm:16
`*__HM_DEDUP = sub () { 0 }`). Fix: callability gate on the shared `visible`
closure in `collect_ancestor_methods` — one spelling covers the local,
plugin-namespace, and cross-file enumeration loops.

## H8-3 — cpp implementations dark for virtual overrides; namespace-blind at class level
`--implementations /root/corpus/leveldb --at include/leveldb/iterator.h:48:16`
(pure-virtual `Iterator::Seek`) → `[]`, despite 8 grep-confirmed
`: public Iterator` subclasses overriding it. `--references` on the same
symbol works (30 refs) and hover shows the base-class relation — the cpp
`: public X` edge is never wired into the INHERITS_INV graph that
`implementations_of` walks. Worse, `--implementations` on the class token
(iterator.h:24:29) returns 2 false positives from the unrelated nested
`SkipList<...>::Iterator` (db/skiplist.h) — the namespace-blind name-identity
family (see PARKED: cpp rename identity), manifesting in implementations.
Pre-existing (not a round-7 regression); first time this verb was probed on cpp.
