# Hitlist — round 7 (leveldb, re2, mojo, DBIx-Class debut)

Corpora: `/root/corpus/{leveldb,re2,mojo,DBIx-Class}` (shallow clones, 2026-07-16).
Raw probe reports: `/home/user/corpus/findings-{leveldb,re2}.md` (mojo/DBIC reports
inline in session; key coordinates reproduced here). All findings grep-verified by
the probes. One row per root cause; probe-report letters in parens.

## Wave-1 slices (fired)

### H7-1 CRITICAL perf/hang — unbounded `expr_type_at_span` ↔ `method_call_return_type_via_bag` mutual recursion (mojo)
Full-workspace build of `/root/corpus/mojo` (112 files) spins 13–15 min wall / 25–29 min
CPU inside a Rayon build worker. gdb stack: `stamp_method_call_targets` →
`method_call_invocant_class` → alternating `expr_type_at_span` /
`method_call_return_type_via_bag` frames, 80+ deep. No cycle guard on the pair
(registry `query_rec` guards its own recursion, but this cycle crosses the
FileAnalysis-method layer so the registry never sees it). Repro:
`perl-lsp --check /root/corpus/mojo` (expect seconds; hangs). Subset bisection: only
the full 112-file tree reproduces — genuine cross-file return-type cycle.
Fix shape: termination on the dispatcher (seen-set / depth cap at the query entry),
per the worklist-invariants rule — not special-casing a worker.

### H7-2 CRITICAL cpp extraction — out-of-line definitions dropped by declarator/qualifier shape (re2 F4/F7, leveldb task-3 partial)
The cpp out-of-line-definition visitor only matches `function_definition.declarator`
= bare `function_declarator` with a one-hop `qualified_identifier`. Dropped, silently:
(a) pointer/reference returns — extra `pointer_declarator` wrapper
    (`Regexp* Regexp::Simplify()` re2/simplify.cc:180; whole Parse→Simplify→Compile
    pipeline invisible; matrix in re2 F4);
(b) multi-level qualifiers (`Prog::Inst::InitAlt` re2/prog.cc:38, all 8 members;
    `Prefilter::Info::ToString` re2/prefilter.cc:276; 3-level
    `Prefilter::Info::Walker::ShortVisit` re2/prefilter.cc:520);
(c) out-of-line constructors (`RE2::RE2(...)` re2/re2.cc:145,154,158,162 — goto-def
    from decl lands on *deleted* sibling ctors instead, re2 F1).
Downstream: refs undercounts (`Inst` 50/62, `Simplify` 7/10), rename drops the
definition it renames (re2 F6 → non-compiling edit). Same-fix neighbor: registered
out-of-line methods get `package: <namespace>` instead of owning class (re2 F8 —
`RE2::Init` reported as package `re2`).

### H7-3 CRITICAL cpp resolution — header decl ↔ out-of-line def not linked cross-file (leveldb tasks 1/3)
Even for REGISTERED defs: goto-def from a call site stops at the header declaration
when the definition lives in another `.cc` (`WriteBatchInternal::InsertInto`
db/db_impl.cc:1245 → stops at write_batch_internal.h:38, real def
write_batch.cc:132; `MemTable::Add` write_batch.cc:122 → stops at memtable.h:56,
def memtable.cc:76). Same-file resolution reaches the def fine. Rename anchored at
a member method's header decl edits ONLY the decl (db/db_impl.h:133
`MakeRoomForWrite` → 1-edit non-compiling set); anchored at def/call-site it edits
`.cc` sites but misses the header. Free functions link fine from either anchor
(`NewEmptyIterator` 4/4). Also: explicit qualification ignored (`DB::Put(o,k,v)`
db/db_impl.cc:1199 goto-def → header pure-virtual, not the same-file
`DB::Put` body at db_impl.cc:1489).

### H7-4 Perl small-fix bundle
- **hover chain-span lie** (DBIC F2): hover on `$schema` inside a multi-line chain
  returns `->first`'s POD. `file_analysis.rs` `hover_info` RefKind::Variable arm's
  dynamic-dispatch heuristic matches any MethodCall ref whose span CONTAINS the
  point — chain-wide spans always do. Repro: DBIx-Class t/100populate.t:36 col 16
  (hover) vs goto-def (correct).
- **POD `L</section>` / `L</"section">` render** (DBIC F3, mojo F3): `pod.rs:492`
  splitn on '/' with empty module part → ` (search)`. 145 hits in ResultSet.pm,
  293 across mojo lib/. Render as `section` (or quoted text), no leading gap.
- **`raw_return_type: "("`** (DBIC F8): `search`/`search_rs`/`page`/
  `as_subselect_rs` in DBIx::Class::ResultSet.pm itself report type `"("`;
  `$rs` locals too. Self-hosting case: invocant is the un-parametrized base class.
- **workspace-symbol duplicate `has` accessors** (mojo F8): byte-identical Method
  entries ×2 per accessor (getter/fluent-writer twin symbols not deduped at the
  search-result layer).
- **`[pos]` "past end of file" lie + `--at` path vs root** (all four corpora):
  annotation claims a line count for files never opened; `--at` file paths resolve
  against CWD only. Resolve against CWD then `<root>` fallback; honest message on
  unreadable file.

## Wave-2 candidates (encode xfail, fix after wave-1 merges)

- **H7-5 ClassIsa cross-file trigger** (DBIC F1, CRITICAL, architectural): plugin
  `Trigger::ClassIsa("DBIx::Class")` evaluated on local-file-only parents → DBIC
  synthesis dark for 49/54 of DBIC's own test schema (2-hop
  `Result::* → DBICTest::BaseResult → DBIx::Class::Core`). 1-hop works (proof:
  DBICTest::DynamicForeignCols::Computer). Known latent hazard —
  `#[ignore]`d `probe_class_isa_trigger_through_cross_file_parent`
  (builder_tests.rs:11515), `docs/prompt-enrichment-inheritance-residual.md`.
  97 `->cds` call sites / 0 found; rename/completion/goto-def all dark.
- **H7-6 rename over-reach, both engines** (DBIC F7, leveldb task 5b): Perl —
  renaming a synthesized `id` column proposes 33 files incl. DBIx::Class::PK's
  generic `id()` (bare-name HashKey/accessor matching, no owner gate). Cpp —
  renaming `Iterator` proposes edits inside vendored gtest (namespace-blind class
  name identity). Destructive-if-applied class of bug.
- **H7-7 implementations blind to mixin overrides** (DBIC F9): `load_components`
  puts overrides on sibling parents, not descendants; `implementations_of`
  (resolve.rs) walks INHERITS_INV descendants only → 0/5 real `update` overrides.
  Also goto-def on correctly-typed `$art->update({...})` (t/53lean_startup.t:180)
  never reaches Row.pm.
- **H7-8 inline `->search(...)->first` loses parametric row type** (DBIC F4):
  RowOf verb composed on a fluent-verb result inside one expression → no type;
  identical composition through an intermediate variable works (matrix in report).
- **H7-9 `belongs_to` references stop at query's own file** (DBIC F10): 78 call
  sites across 43 files, ~7-8 returned (count nondeterministic between runs —
  separate smell). `__PACKAGE__->verb(...)` cross-file ref walk.
- **H7-10 bogus `(anon) sub` completion item** (DBIC F5): unresolved-receiver
  member slots (and even string-literal interiors) return one anonymous-sub item.
- **H7-11 `has x => sub { $ENV{X} || 10 }` kills getter type** (mojo F5): arity-0
  disappears from the map; bare-literal and `->new` defaults infer fine.
- **H7-12 `Mojo::DOM::attr` arity-1 misprojection** (mojo F6): compound guard
  `unless @_ > 1 || ref $_[0]` mapped to wrong branch → arity 1 reports the
  fluent self-return.
- **H7-13 cpp member-field receiver completion doesn't narrow** (leveldb task 4,
  re2 F3): `field_->` / `field.` dumps the in-scope grab-bag (omits the real
  members); parameter/local receivers narrow correctly. Also: narrowed lists leak
  private + nested-struct members and truncate `cleanup_head_` → `cleanup_head`
  (leveldb task 4-secondary).

## Parked this round (PARKED.md gets the durable entries on round close)

- monkey_patch-synthesized methods invisible (mojo F7 — `$ua->get`): needs
  loop-unrolled `monkey_patch __PACKAGE__, lc $name, sub{...}` emission (plugin
  emit-hook shaped; real design work).
- raw `$_[N]`/`@_` subs get no param/return inference (mojo F4; `on` vs `once`).
- `emit('x')` ↔ `on(x =>)` event linking in references (mojo F9; `dispatchers`
  field exists on outline, unreachable from references).
- cpp hover renders methods field-shaped — `Valid: Bool`, no signature/const
  (leveldb task 4b).
- leveldb db_iter.cc `k` else-branch dark spot (hover/def/refs) — unreduced;
  coordinates in findings-leveldb.md task 4c; synthetic repro attempts failed.
- include-guard `#define`s listed as kind Variable in outline/workspace-symbol.
- `Modules: N resolved` counter static across a 40-min session (DBIC note) —
  possible contributor to H7-9.
- cpp macro transform is position-blind: `#define Simplify DontCallSimplify`
  (re2/simplify.cc:201) renames occurrences BEFORE the `#define` line too, so
  the extracted `Regexp::Simplify` def at simplify.cc:180 and the call at :31
  carry the expanded name — the residual 2-ref shortfall on H7-2's references
  acceptance. Surfaced by H7-2; extraction itself is correct. Fix belongs in
  cpp_reparse's expansion ordering (only expand at/after the directive).
