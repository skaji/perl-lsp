# Differential sweep report

- **base** `base` — `perl-lsp 0.6.1`
- **head** `h1` — `perl-lsp 0.7.0`
- corpus: `/home/user/perl-lsp/gold-corpus/local/lib/perl5`
- path: **server** (LSP over stdio), verbs completion, definition, documentSymbol, hover, references
- **excluded as a capability difference:** `typeDefinition` — served by one side only, so every position would report a divergence that is a missing feature rather than a changed answer
- sampled verbs: `references` at 10%
- cross-file readiness: base 96 ms, head 2457 ms
- **base server-wedged**: after_position=165, consecutive_timeouts=3, file=DateTime/TimeZone/America/Boise.pm, line=17, restart=1, verb=definition
- **base restart-rewarm**: confirmed=True, cross_file_ready_ms=90, epoch=1, restart=1
- **base server-wedged**: after_position=479, consecutive_timeouts=3, file=Dist/Zilla/Role/MintingProfile/ShareDir.pm, line=20, restart=2, verb=definition
- **base restart-rewarm**: confirmed=True, cross_file_ready_ms=1087, epoch=2, restart=2
- **base server-wedged**: after_position=528, consecutive_timeouts=3, file=Email/MessageID.pm, line=63, restart=3, verb=definition
- **base restart-rewarm**: confirmed=True, cross_file_ready_ms=95, epoch=3, restart=3
- **base server-wedged**: after_position=679, consecutive_timeouts=3, file=Mojo/Server.pm, line=19, restart=4, verb=definition
- **base restart-rewarm**: confirmed=True, cross_file_ready_ms=1073, epoch=4, restart=4
- **base server-wedged**: after_position=899, consecutive_timeouts=3, file=Plack/HTTPParser.pm, line=0, restart=5, verb=definition
- **base restart-rewarm**: confirmed=True, cross_file_ready_ms=1076, epoch=5, restart=5
- **base server-wedged**: after_position=1008, consecutive_timeouts=3, file=Test/TypeTiny.pm, line=2, restart=6, verb=definition
- **base restart-rewarm**: confirmed=True, cross_file_ready_ms=1092, epoch=6, restart=6
- **base server-wedged**: after_position=1147, consecutive_timeouts=3, file=x86_64-linux-gnu-thread-multi/Class/MOP/Method/Inlined.pm, line=17, restart=7, verb=definition
- **base restart-rewarm**: confirmed=True, cross_file_ready_ms=1088, epoch=7, restart=7
- **base server-wedged**: after_position=1452, consecutive_timeouts=3, file=x86_64-linux-gnu-thread-multi/oose.pm, line=6, restart=8, verb=definition
- **base restart-rewarm**: confirmed=True, cross_file_ready_ms=1093, epoch=8, restart=8
- **base recheck**: empty_first_ask=1483, filled_when_warm=4
- **base complete**: positions_answered=1458
- **head recheck**: empty_first_ask=2437, filled_when_warm=12
- **head complete**: positions_answered=1458

**4302 (position, verb) answers compared — 2747 identical (63.85%), 1555 divergent.**

Of the 4302 compared, 3817 were answered after at least one side restarted (by a generation that did re-warm). A rebuilt index is not the same index.

Of the identical ones, 944 were empty on both sides: positions nobody would ask about, kept in the denominator but called out so the agreement rate is not read as coverage.

## Divergences by shape

`noise` is the WORST self-disagreement across all 6 pairs of same-binary runs, measured over EXACTLY the answers compared here. A shape earns `signal` only by clearing the worst floor observed, not the luckiest.

> The floor is not one number. Across those pairs: `disagree` ranged 14–25. A single pair would have reported any one of those, so a block sitting between the low and high figures cannot be called signal from a two-run floor.

| shape | n | noise | signal | meaning |
|---|---|---|---|---|
| `only-base` | 2 | 0 | 2 | base answers, head empty  (LOST resolution -- regression candidate) |
| `subset` | 52 | 0 | 52 | head found strictly fewer  (regression candidate) |
| `disagree` | 730 | 25 | 705 | both non-empty, neither contains the other |
| `content-differs` | 21 | 0 | 21 | same shape, different content (hover text, etc.) |
| `reranked` | 41 | 168 | **below noise — unreadable** | same candidates, different order (completion ranking moved) |
| `only-head` | 417 | 0 | 417 | head answers, base empty  (new resolution -- intended improvement?) |
| `superset` | 254 | 0 | 254 | head found everything base did, plus more |
| `capped-head` | 16 | 0 | 16 | head's list is a subset because head TRUNCATED it (isIncomplete) — by design, not a loss |
| `timeout-base` | 22 | 0 | 22 | base timed out, head answered |

## Groups

Each row is one claim to adjudicate: *intended improvement*, *regression*, or *wash*.

`distinct` is the number of different (base answer, head answer) PAIRS behind the positions — the count of separate claims. One generated data file can contribute sixty positions that all disagree the same way; that is one thing to adjudicate, not sixty, and reading `n` as the workload is how a sweep gets abandoned as noise.

The `verb noise` column is the floor for that shape ON THAT VERB, which is the only baseline a single-verb block can be read against. It is summed over the verb's kinds, so a block covering one kind sits well under it.

| shape | verb | token kind | n | distinct | verb noise |
|---|---|---|---|---|---|
| `only-base` | completion | method-call | 2 | 2 | 0 |
| `subset` | completion | method-call | 24 | 24 | 0 |
| `subset` | completion | call-site | 12 | 11 | 0 |
| `subset` | completion | module-path | 10 | 6 | 0 |
| `subset` | completion | package | 5 | 3 | 0 |
| `subset` | completion | variable | 1 | 1 | 0 |
| `disagree` | completion | sub-decl | 180 | 171 | 25 |
| `disagree` | definition | use-module | 119 | 29 | 0 |
| `disagree` | completion | variable | 115 | 115 | 25 |
| `disagree` | completion | use-module | 91 | 72 | 25 |
| `disagree` | completion | hash-key | 59 | 51 | 25 |
| `disagree` | definition | module-path | 42 | 25 | 0 |
| `disagree` | completion | module-path | 41 | 30 | 25 |
| `disagree` | completion | call-site | 40 | 39 | 25 |
| `disagree` | completion | package | 17 | 17 | 25 |
| `disagree` | completion | method-call | 17 | 16 | 25 |
| `disagree` | definition | call-site | 9 | 9 | 0 |
| `content-differs` | hover | variable | 13 | 13 | 0 |
| `content-differs` | hover | sub-decl | 6 | 6 | 0 |
| `content-differs` | hover | call-site | 2 | 2 | 0 |
| `reranked` | completion | module-path | 14 | 5 | 168 |
| `reranked` | completion | package | 13 | 9 | 168 |
| `reranked` | completion | use-module | 8 | 7 | 168 |
| `reranked` | completion | call-site | 6 | 6 | 168 |
| `only-head` | definition | use-module | 80 | 26 | 0 |
| `only-head` | completion | module-path | 56 | 24 | 0 |
| `only-head` | completion | use-module | 55 | 19 | 0 |
| `only-head` | definition | module-path | 49 | 31 | 0 |
| `only-head` | hover | module-path | 43 | 28 | 0 |
| `only-head` | definition | call-site | 32 | 24 | 0 |
| `only-head` | hover | call-site | 28 | 23 | 0 |
| `only-head` | completion | call-site | 20 | 15 | 0 |
| `only-head` | definition | method-call | 15 | 15 | 0 |
| `only-head` | hover | method-call | 14 | 14 | 0 |
| `only-head` | completion | method-call | 7 | 6 | 0 |
| `only-head` | hover | use-module | 7 | 7 | 0 |
| `only-head` | completion | hash-key | 3 | 3 | 0 |
| `only-head` | definition | hash-key | 3 | 3 | 0 |
| `only-head` | hover | hash-key | 3 | 3 | 0 |
| `only-head` | references | method-call | 1 | 1 | 0 |
| `only-head` | documentSymbol | file | 1 | 1 | 0 |
| `superset` | completion | package | 109 | 107 | 0 |
| `superset` | completion | hash-key | 35 | 34 | 0 |
| `superset` | completion | module-path | 30 | 28 | 0 |
| `superset` | references | use-module | 21 | 21 | 0 |
| `superset` | completion | use-module | 11 | 8 | 0 |
| `superset` | completion | call-site | 11 | 10 | 0 |
| `superset` | completion | variable | 10 | 10 | 0 |
| `superset` | completion | method-call | 8 | 8 | 0 |
| `superset` | references | call-site | 7 | 7 | 0 |
| `superset` | references | module-path | 6 | 6 | 0 |
| `superset` | references | sub-decl | 4 | 4 | 0 |
| `superset` | references | method-call | 1 | 1 | 0 |
| `superset` | references | package | 1 | 1 | 0 |
| `capped-head` | completion | package | 16 | 1 | 0 |
| `timeout-base` | completion | use-module | 3 | 3 | 0 |
| `timeout-base` | definition | use-module | 3 | 3 | 0 |
| `timeout-base` | references | use-module | 3 | 2 | 0 |
| `timeout-base` | completion | call-site | 3 | 3 | 0 |
| `timeout-base` | definition | call-site | 3 | 3 | 0 |
| `timeout-base` | references | package | 2 | 2 | 0 |
| `timeout-base` | completion | sub-decl | 1 | 1 | 0 |
| `timeout-base` | definition | sub-decl | 1 | 1 | 0 |
| `timeout-base` | references | sub-decl | 1 | 1 | 0 |
| `timeout-base` | references | hash-key | 1 | 1 | 0 |
| `timeout-base` | references | call-site | 1 | 1 | 0 |

## Examples

### `only-base` · completion · method-call — 2 positions, 2 distinct

- `Dist/Zilla/Role/FileFinderUser.pm:159:50` `find_files`
  - base: `n=1 top=[['(anon)', 3]]`
  - head: `n=0 top=[]`
- `x86_64-linux-gnu-thread-multi/Moose.pm:200:38` `initialize`
  - base: `n=13 top=[['extends', 3], ['with', 3], ['throw_error', 3], ['has', 3]]`
  - head: `n=0 top=[]`

### `subset` · completion · method-call — 24 positions, 24 distinct

- `Catalyst/ActionRole/Scheme.pm:8:42` `env`
  - base: `n=4 top=[['match', 2], ['match_captures', 2], ['list_extra_info', 2], ['(anon)', 3]]`
  - head: `n=3 top=[['match', 2], ['match_captures', 2], ['list_extra_info', 2]]`
- `Class/Data/Inheritable.pm:16:39` `mk_classdata`
  - base: `n=2 top=[['mk_classdata', 3], ['(anon)', 3]]`
  - head: `n=1 top=[['mk_classdata', 3]]`
- `Config/MVP/Assembler.pm:205:47` `current_section`
  - base: `n=13 top=[['sequence_class', 2], ['section_class', 2], ['sequence', 2], ['(anon)', 3]]`
  - head: `n=12 top=[['sequence_class', 2], ['section_class', 2], ['sequence', 2], ['_between_sections', 2]]`
- …and 21 more distinct claims

### `subset` · completion · call-site — 12 positions, 11 distinct

- `Type/Tie.pm:78:28` `blessed`  (+1 more positions answering identically)
  - base: `n=10 top=[['export_fail', 3], ['set_prototype', 3], ['as_heavy', 3], ['export', 3]]`
  - head: `n=9 top=[['export_fail', 3], ['set_prototype', 3], ['as_heavy', 3], ['export', 3]]`
- `Catalyst/ActionRole/Scheme.pm:11:58` `orig`
  - base: `n=4 top=[['match', 2], ['match_captures', 2], ['list_extra_info', 2], ['(anon)', 3]]`
  - head: `n=3 top=[['match', 2], ['match_captures', 2], ['list_extra_info', 2]]`
- `Email/Simple.pm:166:25` `new`
  - base: `n=22 top=[['__crlf_re', 3], ['new', 3], ['_split_head_from_body', 3], ['create', 3]]`
  - head: `n=21 top=[['__crlf_re', 3], ['new', 3], ['_split_head_from_body', 3], ['create', 3]]`
- …and 8 more distinct claims

### `subset` · completion · module-path — 10 positions, 6 distinct

- `x86_64-linux-gnu-thread-multi/Moose/Meta/Method/Accessor/Native/Hash/exists.pm:9:49` `Moose::Meta::Method::Accessor::Native::Hash`  (+3 more positions answering identically)
  - base: `n=85 top=[['(anon)', 3], ['_new', 3], ['root_types', 3], ['_initialize_body', 3]]`
  - head: `n=84 top=[['_new', 3], ['root_types', 3], ['_initialize_body', 3], ['_inline_curried_arguments', 3]]`
- `Log/Log4perl/Appender/String.pm:1:37` `Log::Log4perl::Appender`  (+1 more positions answering identically)
  - base: `n=74 top=[['_INTERNAL_DEBUG', 3], ['import', 3], ['(anon)', 3], ['initialized', 3]]`
  - head: `n=73 top=[['_INTERNAL_DEBUG', 3], ['import', 3], ['initialized', 3], ['new', 3]]`
- `CGI/Carp.pm:328:19` `CGI::Carp::VERSION`
  - base: `n=29 top=[['import', 3], ['realwarn', 3], ['realdie', 3], ['id', 3]]`
  - head: `n=28 top=[['import', 3], ['realwarn', 3], ['realdie', 3], ['id', 3]]`
- …and 3 more distinct claims

### `subset` · completion · package — 5 positions, 3 distinct

- `x86_64-linux-gnu-thread-multi/Moose/Meta/Method/Accessor/Native/Array.pm:0:52` `Moose::Meta::Method::Accessor::Native::Array`  (+1 more positions answering identically)
  - base: `n=85 top=[['(anon)', 3], ['_new', 3], ['root_types', 3], ['_initialize_body', 3]]`
  - head: `n=84 top=[['_new', 3], ['root_types', 3], ['_initialize_body', 3], ['_inline_curried_arguments', 3]]`
- `x86_64-linux-gnu-thread-multi/Moose/Meta/TypeConstraint/Parameterizable.pm:0:52` `Moose::Meta::TypeConstraint::Parameterizable`  (+1 more positions answering identically)
  - base: `n=41 top=[['(anon)', 3], ['parents', 3], ['new', 3], ['coerce', 3]]`
  - head: `n=40 top=[['parents', 3], ['new', 3], ['coerce', 3], ['assert_coerce', 3]]`
- `Log/Log4perl/Appender.pm:1:31` `Log::Log4perl::Appender`
  - base: `n=74 top=[['_INTERNAL_DEBUG', 3], ['import', 3], ['(anon)', 3], ['initialized', 3]]`
  - head: `n=73 top=[['_INTERNAL_DEBUG', 3], ['import', 3], ['initialized', 3], ['new', 3]]`

### `subset` · completion · variable — 1 positions, 1 distinct

- `Catalyst/ActionRole/Scheme.pm:17:19` `orig`
  - base: `n=4 top=[['match', 2], ['match_captures', 2], ['list_extra_info', 2], ['(anon)', 3]]`
  - head: `n=3 top=[['match', 2], ['match_captures', 2], ['list_extra_info', 2]]`

### `disagree` · completion · sub-decl — 180 positions, 171 distinct

- `DateTime/TimeZone/America/Argentina/San_Luis.pm:596:19` `has_dst_changes`  (+1 more positions answering identically)
  - base: `n=252 top=[['$VERSION', 6], ['$spans', 6], ['olson_version', 3], ['has_dst_changes', 3]]`
  - head: `n=200 top=[['_max_year', 3], ['_new_instance', 3], ['has_dst_changes', 3], ['olson_version', 3]]`
- `DateTime/TimeZone/America/La_Paz.pm:65:19` `has_dst_changes`  (+1 more positions answering identically)
  - base: `n=471 top=[['$VERSION', 6], ['$spans', 6], ['olson_version', 3], ['has_dst_changes', 3]]`
  - head: `n=200 top=[['_max_year', 3], ['_new_instance', 3], ['has_dst_changes', 3], ['olson_version', 3]]`
- `DateTime/TimeZone/America/Paramaribo.pm:76:13` `_max_year`  (+1 more positions answering identically)
  - base: `n=471 top=[['$VERSION', 6], ['$spans', 6], ['olson_version', 3], ['has_dst_changes', 3]]`
  - head: `n=200 top=[['_max_year', 3], ['_new_instance', 3], ['has_dst_changes', 3], ['olson_version', 3]]`
- …and 168 more distinct claims

### `disagree` · definition · use-module — 119 positions, 29 distinct

- `App/Cmd/Simple.pm:175:7` `strict`  (+38 more positions answering identically)
  - base: `[["<ext>/x86_64-linux-gnu/perl-base/strict.pm", [0, 0, 0, 0]]]`
  - head: `[["<ext>/perl/5.38.2/strict.pm", [0, 8, 0, 14]], ["<ext>/x86_64-linux-gnu/perl-base/strict.pm", [0, 8, 0, 14]]]`
- `Catalyst/Plugin/Unicode/Encoding.pm:2:4` `warnings`  (+26 more positions answering identically)
  - base: `[["<ext>/x86_64-linux-gnu/perl-base/warnings.pm", [0, 0, 0, 0]]]`
  - head: `[["<ext>/perl/5.38.2/warnings.pm", [5, 8, 5, 16]], ["<ext>/x86_64-linux-gnu/perl-base/warnings.pm", [5, 8, 5, 16]]]`
- `Dist/Zilla/Role/Plugin.pm:3:4` `Moose::Role`  (+10 more positions answering identically)
  - base: `[["x86_64-linux-gnu-thread-multi/Moose/Role.pm", [0, 0, 0, 0]]]`
  - head: `[["x86_64-linux-gnu-thread-multi/Moose/Role.pm", [2, 8, 2, 19]]]`
- …and 26 more distinct claims

### `disagree` · completion · variable — 115 positions, 115 distinct

- `CGI/Carp.pm:499:23` `no`
  - base: `n=76 top=[['$in', 6], ['$no', 6], ['$appease_cpants_kwalitee', 6], ['die', 3]]`
  - head: `n=200 top=[['_longmess', 3], ['_warn', 3], ['carp', 3], ['carpout', 3]]`
- `Catalyst/ClassData.pm:61:8` `class`
  - base: `n=91 top=[['$class', 6], ['$attribute', 6], ['$warn_on_instance', 6], ['$slot', 6]]`
  - head: `n=200 top=[['mk_classdata', 3], ['refs', 3], ['Catalyst::ClassData', 7], ['$CURLY_SYMBOL', 3]]`
- `Catalyst/Request/PartData.pm:39:10` `ct`
  - base: `n=93 top=[['$ct', 6], ['$charset', 6], ['$class', 6], ['$c', 6]]`
  - head: `n=200 top=[['build_from_part_data', 3], ['content_encoding', 2], ['content_type', 2], ['content_type_charset', 2]]`
- …and 112 more distinct claims

### `disagree` · completion · use-module — 91 positions, 72 distinct

- `x86_64-linux-gnu-thread-multi/Moose/Exception/IllegalInheritedOptions.pm:3:9` `Moose`  (+10 more positions answering identically)
  - base: `n=407 top=[['Moose::Exception::InvalidArgPassedToMooseUtilMetaRole', 9], ['Moose::Exception::InvalidNameForType', 9], ['Moose::Exception::NoAttributeFoundInSuperClass', 9], ['Moose::Meta::Instance', 9]]`
  - head: `n=200 top=[['Moose', 9], ['Moose::Conflicts', 9], ['Moose::Deprecated', 9], ['Moose::Exception', 9]]`
- `x86_64-linux-gnu-thread-multi/Moose/Exception/CannotAssignValueToReadOnlyAccessor.pm:3:9` `Moose`  (+3 more positions answering identically)
  - base: `n=400 top=[['Moose::Exception::InvalidArgPassedToMooseUtilMetaRole', 9], ['Moose::Exception::InvalidNameForType', 9], ['Moose::Exception::NoAttributeFoundInSuperClass', 9], ['Moose::Meta::Instance', 9]]`
  - head: `n=200 top=[['Moose', 9], ['Moose::Conflicts', 9], ['Moose::Deprecated', 9], ['Moose::Exception', 9]]`
- `x86_64-linux-gnu-thread-multi/Moose/Meta/Method/Accessor/Native/Hash/values.pm:19:14` `Moose::Role`  (+2 more positions answering identically)
  - base: `n=359 top=[['Exception::InvalidArgPassedToMooseUtilMetaRole', 9], ['Exception::InvalidNameForType', 9], ['Exception::NoAttributeFoundInSuperClass', 9], ['Meta::Instance', 9]]`
  - head: `n=200 top=[['_get_caller', 3], ['after', 3], ['around', 3], ['augment', 3]]`
- …and 69 more distinct claims

### `disagree` · completion · hash-key — 59 positions, 51 distinct

- `Software/License/EUPL_1_2.pm:11:29` `open_source`  (+1 more positions answering identically)
  - base: `n=861 top=[['name', 3], ['url', 3], ['meta_name', 3], ['meta2_name', 3]]`
  - head: `n=200 top=[['meta2_name', 3], ['meta_name', 3], ['name', 3], ['spdx_expression', 3]]`
- `Software/License/FreeBSD.pm:10:29` `open_source`  (+1 more positions answering identically)
  - base: `n=861 top=[['name', 3], ['url', 3], ['meta_name', 3], ['meta2_name', 3]]`
  - head: `n=200 top=[['meta2_name', 3], ['meta_name', 3], ['name', 3], ['spdx_expression', 3]]`
- `Software/License/GFDL_1_3.pm:9:29` `open_source`  (+1 more positions answering identically)
  - base: `n=861 top=[['name', 3], ['url', 3], ['meta_name', 3], ['meta2_name', 3]]`
  - head: `n=200 top=[['meta2_name', 3], ['meta_name', 3], ['name', 3], ['spdx_expression', 3]]`
- …and 48 more distinct claims

### `disagree` · definition · module-path — 42 positions, 25 distinct

- `x86_64-linux-gnu-thread-multi/Moose/Exception/CannotAssignValueToReadOnlyAccessor.pm:4:9` `Moose::Exception`  (+7 more positions answering identically)
  - base: `[["x86_64-linux-gnu-thread-multi/Moose/Exception.pm", [0, 0, 0, 0]]]`
  - head: `[["x86_64-linux-gnu-thread-multi/Moose/Exception.pm", [0, 8, 0, 24]]]`
- `x86_64-linux-gnu-thread-multi/Moose/Meta/Method/Accessor/Native/Array.pm:25:3` `Moose::Role`  (+6 more positions answering identically)
  - base: `[["x86_64-linux-gnu-thread-multi/Moose/Role.pm", [0, 0, 0, 0]]]`
  - head: `[["x86_64-linux-gnu-thread-multi/Moose/Role.pm", [2, 8, 2, 19]]]`
- `Test/TypeTiny.pm:6:4` `Scalar::Util`  (+2 more positions answering identically)
  - base: `[["<ext>/perl-base/Scalar/Util.pm", [0, 0, 0, 0]]]`
  - head: `[["<ext>/5.38.2/Scalar/Util.pm", [6, 8, 6, 20]], ["<ext>/perl-base/Scalar/Util.pm", [6, 8, 6, 20]]]`
- …and 22 more distinct claims

### `disagree` · completion · module-path — 41 positions, 30 distinct

- `x86_64-linux-gnu-thread-multi/Moose/Exception/CannotAssignValueToReadOnlyAccessor.pm:4:25` `Moose::Exception`  (+10 more positions answering identically)
  - base: `n=359 top=[['Exception::InvalidArgPassedToMooseUtilMetaRole', 9], ['Exception::InvalidNameForType', 9], ['Exception::NoAttributeFoundInSuperClass', 9], ['Meta::Instance', 9]]`
  - head: `n=200 top=[['_get_caller', 3], ['after', 3], ['around', 3], ['augment', 3]]`
- `DateTime/TimeZone/Asia/Karachi.pm:126:28` `DateTime::TimeZone::INFINITY`  (+1 more positions answering identically)
  - base: `n=1 top=[['Asia::Karachi', 9]]`
  - head: `n=200 top=[['INFINITY', 3], ['IS_DST', 3], ['LOCAL_END', 3], ['LOCAL_START', 3]]`
- `Date/Language/Brazilian.pm:15:24` `Date::Language`
  - base: `n=154 top=[['$VERSION', 6], ['format_a', 3], ['format_A', 3], ['format_b', 3]]`
  - head: `n=200 top=[['format_A', 3], ['format_B', 3], ['format_a', 3], ['format_b', 3]]`
- …and 27 more distinct claims

### `disagree` · completion · call-site — 40 positions, 39 distinct

- `Class/Data/Inheritable.pm:10:19` `croak`  (+1 more positions answering identically)
  - base: `n=40 top=[['_fetch_sub', 3], ['UTF8_REGEXP_PROBLEM', 3], ['(anon)', 3], ['is_utf8', 3]]`
  - head: `n=40 top=[['_fetch_sub', 3], ['UTF8_REGEXP_PROBLEM', 3], ['is_utf8', 3], ['downgrade', 3]]`
- `Catalyst/ClassData.pm:43:9` `confess`
  - base: `n=84 top=[['$class', 6], ['$attribute', 6], ['$warn_on_instance', 6], ['$slot', 6]]`
  - head: `n=200 top=[['mk_classdata', 3], ['refs', 3], ['Catalyst::ClassData', 7], ['$CURLY_SYMBOL', 3]]`
- `Config/MVP/Reader/Hash.pm:26:14` `name`
  - base: `n=153 top=[['$name', 6], ['$self', 6], ['$location', 6], ['$assembler', 6]]`
  - head: `n=200 top=[['read_into_assembler', 3], ['Config::MVP::Reader::Hash', 7], ['$CURLY_SYMBOL', 3], ['$DYNAMIC_FILE_UPLOAD', 3]]`
- …and 36 more distinct claims

### `disagree` · completion · package — 17 positions, 17 distinct

- `App/Cmd/Simple.pm:108:21` `eq`
  - base: `n=39 top=[['$class', 6], ['$i', 6], ['import', 3], ['(anon)', 3]]`
  - head: `n=200 top=[['_cmd_pkg', 3], ['import', 3], ['usage_desc', 3], ['refs', 3]]`
- `CGI/Carp.pm:0:17` `CGI::Carp`
  - base: `n=1 top=[['Carp', 9]]`
  - head: `n=200 top=[['Accept', 3], ['Area', 3], ['CLEAR', 3], ['DELETE', 3]]`
- `CGI/Struct.pm:0:19` `CGI::Struct`
  - base: `n=1 top=[['Struct', 9]]`
  - head: `n=200 top=[['Accept', 3], ['Area', 3], ['CLEAR', 3], ['DELETE', 3]]`
- …and 14 more distinct claims

### `disagree` · completion · method-call — 17 positions, 16 distinct

- `x86_64-linux-gnu-thread-multi/Moose/Exception/CannotOverrideALocalMethod.pm:15:59` `method_name`  (+1 more positions answering identically)
  - base: `n=8 top=[['method_name', 2], ['_build_message', 3], ['trace', 2], ['_build_trace', 2]]`
  - head: `n=8 top=[['method_name', 2], ['_build_message', 3], ['trace', 2], ['message', 2]]`
- `App/Cmd/Simple.pm:187:26` `opt_spec`
  - base: `n=4 top=[['import', 3], ['(anon)', 3], ['usage_desc', 3], ['_cmd_pkg', 3]]`
  - head: `n=17 top=[['import', 3], ['usage_desc', 3], ['_cmd_pkg', 3], ['prepare', 3]]`
- `Dist/Zilla/Role/Plugin.pm:57:46` `plugin_name`
  - base: `n=11 top=[['plugin_name', 2], ['zilla', 2], ['logger', 2], ['(anon)', 3]]`
  - head: `n=11 top=[['plugin_name', 2], ['zilla', 2], ['logger', 2], ['mvp_multivalue_args', 3]]`
- …and 13 more distinct claims

### `disagree` · definition · call-site — 9 positions, 9 distinct

- `PPI.pm:18:9` `Structure`
  - base: `[["PPI/Structure.pm", [0, 0, 0, 0]]]`
  - head: `[["PPI/Structure.pm", [0, 8, 0, 22]]]`
- `PPI.pm:24:9` `Tokenizer`
  - base: `[["PPI/Tokenizer.pm", [0, 0, 0, 0]]]`
  - head: `[["PPI/Tokenizer.pm", [0, 8, 0, 22]]]`
- `PPI/Exception/ParserRejection.pm:3:9` `Exception`
  - base: `[["PPI/Exception.pm", [0, 0, 0, 0]]]`
  - head: `[["PPI/Exception.pm", [0, 8, 0, 22]], ["PPI/XSAccessor.pm", [71, 1, 71, 15]]]`
- …and 6 more distinct claims

### `content-differs` · hover · variable — 13 positions, 13 distinct

- `Config/MVP/Assembler.pm:205:26` `self`
  - base: `len=293 '```perl sub current_section { ``` *class Config::MVP::Assembler — resolved from `$self`* pod =method current_section pod'`
  - head: `len=74 '```perl my ($self, $name, $value) = @_; ``` *type: Config::MVP::Assembler*'`
- `Email/MessageID.pm:64:36` `_SYS_HOSTNAME_LONG`
  - base: `len=35 '```perl my $_SYS_HOSTNAME_LONG; ```'`
  - head: `len=51 '```perl my $_SYS_HOSTNAME_LONG; ``` *type: Numeric*'`
- `Mojo/IOLoop/Subprocess.pm:63:40` `stream`
  - base: `len=72 '```perl my $stream = Mojo::IOLoop::Stream->new($reader)->timeout(0); ```'`
  - head: `len=101 '```perl my $stream = Mojo::IOLoop::Stream->new($reader)->timeout(0); ``` *type: Mojo::IOLoop::Stream*'`
- …and 10 more distinct claims

### `content-differs` · hover · sub-decl — 6 positions, 6 distinct

- `Mojo/IOLoop/Subprocess.pm:24:4` `run_p`
  - base: `len=452 '```perl sub run_p { ``` *package Mojo::IOLoop::Subprocess* *returns: Mojo::Promise* ```perl my $promise = $subprocess->r'`
  - head: `len=450 '```perl sub run_p { ``` *package Mojo::IOLoop::Subprocess* *returns: Mojo::Promise* ```perl my $promise = $subprocess->r'`
- `Mojo/JSON/Pointer.pm:6:4` `get`
  - base: `len=641 "```perl sub get { shift->_pointer(1, @_) } ``` *package Mojo::JSON::Pointer* ```perl my $value = $pointer->get('/foo/bar"`
  - head: `len=639 "```perl sub get { shift->_pointer(1, @_) } ``` *package Mojo::JSON::Pointer* ```perl my $value = $pointer->get('/foo/bar"`
- `Mojo/Server/Prefork.pm:21:4` `check_pid`
  - base: `len=284 '```perl sub check_pid { ``` *package Mojo::Server::Prefork* *returns: Maybe<String>* ```perl my $pid = $prefork->check_p'`
  - head: `len=282 '```perl sub check_pid { ``` *package Mojo::Server::Prefork* *returns: Maybe<String>* ```perl my $pid = $prefork->check_p'`
- …and 3 more distinct claims

### `content-differs` · hover · call-site — 2 positions, 2 distinct

- `Log/Log4perl/Appender.pm:76:35` `new`
  - base: `len=124 '```perl sub new { ``` *package Log::Log4perl::Appender* *returns: HashRef* #############################################'`
  - head: `len=140 '```perl sub new { ``` *package Log::Log4perl::Appender* *returns: Log::Log4perl::Appender* #############################'`
- `x86_64-linux-gnu-thread-multi/Moose/Meta/TypeConstraint/Parameterizable.pm:66:41` `find_or_create_isa_type_constraint`
  - base: `len=121 '```perl sub find_or_create_isa_type_constraint($type_constraint_name, $options) ``` *from `Moose::Util::TypeConstraints`'`
  - head: `len=158 '```perl sub find_or_create_isa_type_constraint($type_constraint_name, $options) → Moose::Meta::TypeConstraint::Class ```'`

### `reranked` · completion · module-path — 14 positions, 5 distinct

- `x86_64-linux-gnu-thread-multi/Moose/Exception/CannotCreateMethodAliasLocalMethodIsPresent.pm:5:34` `Moose::Exception::Role::Role`  (+5 more positions answering identically)
  - base: `n=13 top=[['ParamsHash', 9], ['Class', 9], ['AttributeName', 9], ['EitherAttributeOrAttributeName', 9]]`
  - head: `n=13 top=[['Instance', 9], ['Method', 9], ['Role', 9], ['Class', 9]]`
- `PPI/Exception/ParserRejection.pm:7:26` `PPI::Exception`  (+4 more positions answering identically)
  - base: `n=94 top=[['Token::Pod', 9], ['Statement::Include::Perl6', 9], ['Document::File', 9], ['Statement::Variable', 9]]`
  - head: `n=94 top=[['Structure::Constructor', 9], ['Transform', 9], ['Token::_QuoteEngine::Full', 9], ['Token::QuoteLike::Words', 9]]`
- `App/Cmd/Simple.pm:5:21` `App::Cmd::Command`
  - base: `n=4 top=[['App::Cmd::Command::help', 9], ['App::Cmd::Command::version', 9], ['App::Cmd::Command', 9], ['App::Cmd::Command::commands', 9]]`
  - head: `n=4 top=[['App::Cmd::Command::version', 9], ['App::Cmd::Command::help', 9], ['App::Cmd::Command', 9], ['App::Cmd::Command::commands', 9]]`
- …and 2 more distinct claims

### `reranked` · completion · package — 13 positions, 9 distinct

- `PPI/Structure.pm:0:22` `PPI::Structure`  (+2 more positions answering identically)
  - base: `n=94 top=[['Token::Pod', 9], ['Statement::Include::Perl6', 9], ['Document::File', 9], ['Statement::Variable', 9]]`
  - head: `n=94 top=[['Structure::Constructor', 9], ['Transform', 9], ['Token::_QuoteEngine::Full', 9], ['Token::QuoteLike::Words', 9]]`
- `x86_64-linux-gnu-thread-multi/Moose/Meta/Method/Accessor/Native/Array/accessor.pm:0:62` `Moose::Meta::Method::Accessor::Native::Array::accessor`  (+1 more positions answering identically)
  - base: `n=28 top=[['_inline_check_var_is_valid_index', 3], ['pop', 9], ['sort_in_place', 9], ['natatime', 9]]`
  - head: `n=28 top=[['_inline_check_var_is_valid_index', 3], ['uniq', 9], ['Writer', 9], ['pop', 9]]`
- `x86_64-linux-gnu-thread-multi/Moose/Meta/Method/Accessor/Native/Hash/exists.pm:0:59` `Moose::Meta::Method::Accessor::Native::Hash::exists`  (+1 more positions answering identically)
  - base: `n=16 top=[['_inline_check_var_is_valid_key', 3], ['exists', 9], ['set', 9], ['is_empty', 9]]`
  - head: `n=16 top=[['_inline_check_var_is_valid_key', 3], ['clear', 9], ['Writer', 9], ['shallow_clone', 9]]`
- …and 6 more distinct claims

### `reranked` · completion · use-module — 8 positions, 7 distinct

- `PPI/Token/Comment.pm:61:14` `PPI::Token`  (+1 more positions answering identically)
  - base: `n=47 top=[['PPI::Token::Pod', 9], ['PPI::Token::Regexp::Substitute', 9], ['PPI::Token::Quote::Interpolate', 9], ['PPI::Token::QuoteLike::Words', 9]]`
  - head: `n=47 top=[['PPI::Token::_QuoteEngine::Full', 9], ['PPI::Token::QuoteLike::Words', 9], ['PPI::Tokenizer', 9], ['PPI::Token::Attribute', 9]]`
- `Catalyst/ClassData.pm:5:15` `Moose::Util`
  - base: `n=4 top=[['Moose::Util::TypeConstraints::Builtins', 9], ['Moose::Util::MetaRole', 9], ['Moose::Util::TypeConstraints', 9], ['Moose::Util', 9]]`
  - head: `n=4 top=[['Moose::Util', 9], ['Moose::Util::TypeConstraints', 9], ['Moose::Util::TypeConstraints::Builtins', 9], ['Moose::Util::MetaRole', 9]]`
- `Log/Log4perl/Appender.pm:99:37` `Log::Log4perl::Config`
  - base: `n=5 top=[['Log::Log4perl::Config::Watch', 9], ['Log::Log4perl::Config::BaseConfigurator', 9], ['Log::Log4perl::Config', 9], ['Log::Log4perl::Config::PropertyConfigurator', 9]]`
  - head: `n=5 top=[['Log::Log4perl::Config', 9], ['Log::Log4perl::Config::BaseConfigurator', 9], ['Log::Log4perl::Config::PropertyConfigurator', 9], ['Log::Log4perl::Config::DOMConfigurator', 9]]`
- …and 4 more distinct claims

### `reranked` · completion · call-site — 6 positions, 6 distinct

- `Catalyst/Request/PartData.pm:68:20` `new`
  - base: `n=9 top=[['raw_data', 2], ['name', 2], ['size', 2], ['headers', 2]]`
  - head: `n=9 top=[['raw_data', 2], ['name', 2], ['size', 2], ['headers', 2]]`
- `PPI.pm:18:18` `Structure`
  - base: `n=11 top=[['PPI::Structure::For', 9], ['PPI::Structure::Block', 9], ['PPI::Structure::Subscript', 9], ['PPI::Structure::Condition', 9]]`
  - head: `n=11 top=[['PPI::Structure::Constructor', 9], ['PPI::Structure::Block', 9], ['PPI::Structure::For', 9], ['PPI::Structure::List', 9]]`
- `PPI/Exception/ParserRejection.pm:3:18` `Exception`
  - base: `n=2 top=[['PPI::Exception', 9], ['PPI::Exception::ParserRejection', 9]]`
  - head: `n=2 top=[['PPI::Exception::ParserRejection', 9], ['PPI::Exception', 9]]`
- …and 3 more distinct claims

### `only-head` · definition · use-module — 80 positions, 26 distinct

- `Dist/Zilla/Plugin/PkgDist.pm:3:4` `Moose`  (+17 more positions answering identically)
  - base: `[]`
  - head: `[["x86_64-linux-gnu-thread-multi/Moose.pm", [2, 8, 2, 13]]]`
- `DateTime/TimeZone/America/Argentina/San_Luis.pm:13:4` `namespace::autoclean`  (+11 more positions answering identically)
  - base: `[]`
  - head: `[["namespace/autoclean.pm", [3, 8, 3, 28]]]`
- `Dist/Zilla/MVP/Reader/Perl.pm:7:4` `Dist::Zilla::Pragmas`  (+6 more positions answering identically)
  - base: `[]`
  - head: `[["Dist/Zilla/Pragmas.pm", [0, 8, 0, 28]]]`
- …and 23 more distinct claims

### `only-head` · completion · module-path — 56 positions, 24 distinct

- `Text/Unidecode/x19.pm:1:22` `Text::Unidecode::Char`  (+18 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=14 top=[['DEBUG', 3], ['unidecode', 3], ['make_placeholder_map', 3], ['make_placeholder_map_nulls', 3]]`
- `DateTime/TimeZone/Asia/Kolkata.pm:13:24` `namespace::autoclean`  (+3 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=1 top=[['namespace::autoclean', 9]]`
- `DateTime/TimeZone/America/Argentina/San_Luis.pm:17:20` `Class::Singleton`  (+2 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=1 top=[['Class::Singleton', 9]]`
- …and 21 more distinct claims

### `only-head` · completion · use-module — 55 positions, 19 distinct

- `DateTime/TimeZone/America/Argentina/San_Luis.pm:13:24` `namespace::autoclean`  (+11 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=1 top=[['namespace::autoclean', 9]]`
- `Dist/Zilla/MVP/Reader/Perl.pm:7:24` `Dist::Zilla::Pragmas`  (+6 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=1 top=[['Dist::Zilla::Pragmas', 9]]`
- `DateTime/TimeZone/America/Argentina/Cordoba.pm:19:31` `DateTime::TimeZone::OlsonDB`  (+5 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=5 top=[['DateTime::TimeZone::OlsonDB::Zone', 9], ['DateTime::TimeZone::OlsonDB::Rule', 9], ['DateTime::TimeZone::OlsonDB', 9], ['DateTime::TimeZone::OlsonDB::Observance', 9]]`
- …and 16 more distinct claims

### `only-head` · definition · module-path — 49 positions, 31 distinct

- `Software/License/EUPL_1_1.pm:5:12` `Software::License`  (+4 more positions answering identically)
  - base: `[]`
  - head: `[["Software/License.pm", [2, 8, 2, 25]]]`
- `DateTime/TimeZone/Asia/Kolkata.pm:13:4` `namespace::autoclean`  (+3 more positions answering identically)
  - base: `[]`
  - head: `[["namespace/autoclean.pm", [3, 8, 3, 28]]]`
- `Date/Language/Brazilian.pm:10:4` `Date::Language`  (+2 more positions answering identically)
  - base: `[]`
  - head: `[["Date/Language.pm", [1, 8, 1, 22]]]`
- …and 28 more distinct claims

### `only-head` · hover · module-path — 43 positions, 28 distinct

- `x86_64-linux-gnu-thread-multi/Moose/Exception/CannotAssignValueToReadOnlyAccessor.pm:4:9` `Moose::Exception`  (+7 more positions answering identically)
  - base: `∅`
  - head: `len=65 '```perl package Moose::Exception ``` *namespace* — `Exception.pm`'`
- `Software/License/EUPL_1_1.pm:5:12` `Software::License`  (+4 more positions answering identically)
  - base: `∅`
  - head: `len=73 '```perl package Software::License 0.104007 ``` *namespace* — `License.pm`'`
- `Date/Language/Brazilian.pm:15:10` `Date::Language`  (+1 more positions answering identically)
  - base: `∅`
  - head: `len=62 '```perl package Date::Language ``` *namespace* — `Language.pm`'`
- …and 25 more distinct claims

### `only-head` · definition · call-site — 32 positions, 24 distinct

- `Text/Unidecode/x35.pm:1:50` `make_placeholder_map`  (+3 more positions answering identically)
  - base: `[]`
  - head: `[["Text/Unidecode.pm", [117, 0, 117, 0]]]`
- `Date/Language/Brazilian.pm:10:10` `Language`  (+2 more positions answering identically)
  - base: `[]`
  - head: `[["Date/Language.pm", [1, 8, 1, 22]]]`
- `Date/Language/Brazilian.pm:27:16` `_build_lookups`  (+2 more positions answering identically)
  - base: `[]`
  - head: `[["Date/Language.pm", [14, 0, 14, 0]]]`
- …and 21 more distinct claims

### `only-head` · hover · call-site — 28 positions, 23 distinct

- `Text/Unidecode/x35.pm:1:50` `make_placeholder_map`  (+3 more positions answering identically)
  - base: `∅`
  - head: `len=153 '```perl sub make_placeholder_map() → Sequence<String> ``` =============================================================='`
- `Date/Language/Brazilian.pm:27:16` `_build_lookups`  (+2 more positions answering identically)
  - base: `∅`
  - head: `len=56 '```perl sub _build_lookups() ``` *from `Date::Language`*'`
- `App/Cmd/Simple.pm:127:16` `install_sub`
  - base: `∅`
  - head: `len=38 '```perl use v5.8.0; ``` — `Install.pm`'`
- …and 20 more distinct claims

### `only-head` · completion · call-site — 20 positions, 15 distinct

- `Text/Unidecode/x35.pm:1:70` `make_placeholder_map`  (+3 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=14 top=[['DEBUG', 3], ['unidecode', 3], ['make_placeholder_map', 3], ['make_placeholder_map_nulls', 3]]`
- `Date/Language/Brazilian.pm:10:18` `Language`  (+2 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=37 top=[['Date::Language::French', 9], ['Date::Language', 9], ['Date::Language::Czech', 9], ['Date::Language::Bulgarian', 9]]`
- `App/Cmd/Simple.pm:127:27` `install_sub`
  - base: `n=0 top=[]`
  - head: `n=7 top=[['_name_of_code', 3], ['_CODELIKE', 3], ['_build_public_installer', 3], ['_do_with_warn', 3]]`
- …and 12 more distinct claims

### `only-head` · definition · method-call — 15 positions, 15 distinct

- `Catalyst/ClassData.pm:53:9` `make_mutable`
  - base: `[]`
  - head: `[["x86_64-linux-gnu-thread-multi/Class/MOP/Class.pm", [1306, 0, 1306, 0]]]`
- `Catalyst/Test.pm:359:22` `new`
  - base: `[]`
  - head: `[["URI.pm", [53, 0, 53, 0]]]`
- `Dist/Zilla/Role/ExecFiles.pm:18:16` `zilla`
  - base: `[]`
  - head: `[["Dist/Zilla/Role/Plugin.pm", [37, 0, 37, 0]]]`
- …and 12 more distinct claims

### `only-head` · hover · method-call — 14 positions, 14 distinct

- `Catalyst/ClassData.pm:53:9` `make_mutable`
  - base: `∅`
  - head: `len=97 '```perl sub make_mutable($self) ``` *class Class::MOP::Class* *returns: Maybe<Class::MOP::Class>*'`
- `Catalyst/Test.pm:359:22` `new`
  - base: `∅`
  - head: `len=1242 '```perl sub new($class, $uri, $scheme) ``` *class URI* *returns: URI::_foreign* Constructs a new URI object. The string '`
- `Dist/Zilla/Role/ExecFiles.pm:18:16` `zilla`
  - base: `∅`
  - head: `len=175 '```perl sub zilla() ``` *class Dist::Zilla::Role::ExecFiles (from Dist::Zilla::Role::Plugin)* This attribute contains th'`
- …and 11 more distinct claims

### `only-head` · completion · method-call — 7 positions, 6 distinct

- `Catalyst/ClassData.pm:53:21` `make_mutable`  (+1 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=161 top=[['initialize', 3], ['reinitialize', 3], ['_construct_class_instance', 3], ['_real_ref_name', 3]]`
- `Catalyst/Test.pm:359:25` `new`
  - base: `n=0 top=[]`
  - head: `n=27 top=[['HAS_RESERVED_SQUARE_BRACKETS', 3], ['_obj_eq', 3], ['new', 3], ['new_abs', 3]]`
- `LWP/Protocol/mailto.pm:161:31` `new`
  - base: `n=0 top=[]`
  - head: `n=52 top=[['new', 3], ['parse', 3], ['clone', 3], ['code', 3]]`
- …and 3 more distinct claims

### `only-head` · hover · use-module — 7 positions, 7 distinct

- `Config/MVP/Assembler.pm:204:53` `section`
  - base: `∅`
  - head: `len=380 '```perl sub throw() ``` *class Config::MVP::Error (from Throwable)* pod =method throw pod pod Something::Throwable->thro'`
- `HTTP/Message.pm:318:14` `Compress::Raw::Zlib`
  - base: `∅`
  - head: `len=63 '```perl package Compress::Raw::Zlib ``` *namespace* — `Zlib.pm`'`
- `LWP.pm:4:8` `LWP::UserAgent`
  - base: `∅`
  - head: `len=63 '```perl package LWP::UserAgent ``` *namespace* — `UserAgent.pm`'`
- …and 4 more distinct claims

### `only-head` · completion · hash-key — 3 positions, 3 distinct

- `Catalyst/Test.pm:286:35` `app`
  - base: `n=0 top=[]`
  - head: `n=13 top=[['request', 6], ['get', 6], ['ctx_request', 6], ['content_like', 6]]`
- `PPI/Tokenizer.pm:541:89` `line_cursor`
  - base: `n=0 top=[]`
  - head: `n=8 top=[['source', 6], ['source_bytes', 6], ['document', 6], ['token', 6]]`
- `x86_64-linux-gnu-thread-multi/Class/MOP/Mixin/HasOverloads.pm:72:27` `coderef_package`
  - base: `n=0 top=[]`
  - head: `n=200 top=[['_SET_FALLBACK_EACH_TIME', 3], ['_overload_for', 3], ['_overload_info', 3], ['_overload_info_for', 3]]`

### `only-head` · definition · hash-key — 3 positions, 3 distinct

- `Catalyst/Test.pm:286:32` `app`
  - base: `[]`
  - head: `[["Catalyst/Test.pm", [301, 8, 301, 11]]]`
- `Mojo/Server/Prefork.pm:167:16` `finish`
  - base: `[]`
  - head: `[["Mojo/Server/Prefork.pm", [137, 12, 137, 18]]]`
- `x86_64-linux-gnu-thread-multi/Class/MOP/Mixin/HasAttributes.pm:48:65` `class_name`
  - base: `[]`
  - head: `[["x86_64-linux-gnu-thread-multi/Class/MOP/Mixin.pm", [14, 0, 14, 0]]]`

### `only-head` · hover · hash-key — 3 positions, 3 distinct

- `Catalyst/Test.pm:286:32` `app`
  - base: `∅`
  - head: `len=91 '**Hash key `app`** - `app => ref($class) eq "CODE" ? $class : $class->_finalized_psgi_app,`'`
- `Mojo/Server/Prefork.pm:167:16` `finish`
  - base: `∅`
  - head: `len=127 '**handler `finish`** on `Mojo::Server::Prefork` *1 registration stacks:* - **line 138:** `()` *Dispatch via:* `**->emit('`
- `x86_64-linux-gnu-thread-multi/Class/MOP/Mixin/HasAttributes.pm:48:65` `class_name`
  - base: `∅`
  - head: `len=143 '```perl sub _throw_exception($class, $exception_type, @args_to_exception) ``` *class Class::MOP::Mixin::HasAttributes (f'`

### `only-head` · references · method-call — 1 positions, 1 distinct

- `Mojo/Server.pm:26:14` `req`
  - base: `[]`
  - head: `[["Mojo/Server.pm", [26, 14, 26, 17]], ["Mojo/Server.pm", [27, 7, 27, 10]], ["Mojo/Server/CGI.pm", [10, 17, 10, 20]], ["Mojo/Server/PSGI.pm", [7, 17, 7, 20]], ["Mojo/Transaction.pm", [10, 4, 10, 7]], …`

### `only-head` · documentSymbol · file — 1 positions, 1 distinct

- `Plack/HTTPParser.pm:None:None` `?`
  - base: `[]`
  - head: `[["Plack::HTTPParser", 3], ["Plack::HTTPParser::@EXPORT", 13]]`

### `superset` · completion · package — 109 positions, 107 distinct

- `URI/Escape.pm:0:19` `URI::Escape`  (+2 more positions answering identically)
  - base: `n=67 top=[['_punycode', 9], ['tn3270', 9], ['file::Win32', 9], ['sips', 9]]`
  - head: `n=94 top=[['HAS_RESERVED_SQUARE_BRACKETS', 3], ['_obj_eq', 3], ['new', 3], ['new_abs', 3]]`
- `Catalyst/ActionRole/Scheme.pm:0:36` `Catalyst::ActionRole::Scheme`
  - base: `n=1 top=[['Scheme', 9]]`
  - head: `n=4 top=[['QueryMatching', 9], ['ConsumesContent', 9], ['HTTPMethods', 9], ['Scheme', 9]]`
- `Catalyst/Request/PartData.pm:0:35` `Catalyst::Request::PartData`
  - base: `n=1 top=[['PartData', 9]]`
  - head: `n=87 top=[['env', 3], ['action', 3], ['user', 3], ['snippets', 3]]`
- …and 104 more distinct claims

### `superset` · completion · hash-key — 35 positions, 34 distinct

- `x86_64-linux-gnu-thread-multi/Moose/Meta/Method/Accessor/Native/Array.pm:15:53` `argument`  (+1 more positions answering identically)
  - base: `n=1 top=[['_inline_check_var_is_valid_index', 3]]`
  - head: `n=200 top=[['_inline_check_var_is_valid_index', 3], ['Moose::Meta::Method::Accessor::Native::Array', 7], ['$Bin', 3], ['$Bzip2Error', 3]]`
- `Config/MVP/Reader/Hash.pm:35:32` `key`
  - base: `n=1 top=[['read_into_assembler', 3]]`
  - head: `n=200 top=[['read_into_assembler', 3], ['Config::MVP::Reader::Hash', 7], ['$CURLY_SYMBOL', 3], ['$DYNAMIC_FILE_UPLOAD', 3]]`
- `DateTime/TimeZone/America/Argentina/Cordoba.pm:592:34` `spans`
  - base: `n=4 top=[['olson_version', 3], ['has_dst_changes', 3], ['_max_year', 3], ['_new_instance', 3]]`
  - head: `n=200 top=[['_max_year', 3], ['_new_instance', 3], ['has_dst_changes', 3], ['olson_version', 3]]`
- …and 31 more distinct claims

### `superset` · completion · module-path — 30 positions, 28 distinct

- `DateTime/TimeZone/America/Boa_Vista.pm:21:66` `Class::Singleton`  (+2 more positions answering identically)
  - base: `n=2 top=[['Tiny', 9], ['Struct', 9]]`
  - head: `n=35 top=[['Inspector::Functions', 9], ['Method::Modifiers', 9], ['Inspector', 9], ['MOP::Mixin::AttributeCore', 9]]`
- `Config/MVP/Assembler.pm:65:35` `Config::MVP::Sequence`
  - base: `n=1 top=[['Assembler', 9]]`
  - head: `n=12 top=[['Reader::Findable::ByExtension', 9], ['Error', 9], ['Assembler::WithBundles', 9], ['Reader::Hash', 9]]`
- `Config/MVP/Reader/Hash.pm:4:28` `Config::MVP::Reader`
  - base: `n=1 top=[['Reader::Hash', 9]]`
  - head: `n=12 top=[['Reader::Findable::ByExtension', 9], ['Error', 9], ['Assembler::WithBundles', 9], ['Reader::Hash', 9]]`
- …and 25 more distinct claims

### `superset` · references · use-module — 21 positions, 21 distinct

- `DateTime/TimeZone/America/Argentina/San_Luis.pm:13:4` `namespace::autoclean`
  - base: `[["DateTime/Locale/Base.pm", [4, 4, 4, 24]], ["DateTime/Locale/Data.pm", [18, 4, 18, 24]], ["DateTime/Locale/FromData.pm", [4, 4, 4, 24]], ["DateTime/Locale/Util.pm", [4, 4, 4, 24]], ["DateTime/TimeZo…`
  - head: `[["DateTime/Locale.pm", [6, 4, 6, 24]], ["DateTime/Locale/Base.pm", [4, 4, 4, 24]], ["DateTime/Locale/Data.pm", [18, 4, 18, 24]], ["DateTime/Locale/FromData.pm", [4, 4, 4, 24]], ["DateTime/Locale/Util…`
- `DateTime/TimeZone/America/Manaus.pm:19:4` `DateTime::TimeZone::OlsonDB`
  - base: `[["DateTime/TimeZone/America/Boise.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/America/Halifax.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/America/Indiana/Knox.pm", [19, 4, 19, 31]], ["DateTime/TimeZone…`
  - head: `[["DateTime/TimeZone/Africa/Abidjan.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/Africa/Algiers.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/Africa/Bissau.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/Africa…`
- `DateTime/TimeZone/America/Rio_Branco.pm:13:4` `namespace::autoclean`
  - base: `[["DateTime/TimeZone/America/Boise.pm", [13, 4, 13, 24]], ["DateTime/TimeZone/America/Halifax.pm", [13, 4, 13, 24]], ["DateTime/TimeZone/America/Indiana/Knox.pm", [13, 4, 13, 24]], ["DateTime/TimeZone…`
  - head: `[["DateTime/Locale.pm", [6, 4, 6, 24]], ["DateTime/Locale/Base.pm", [4, 4, 4, 24]], ["DateTime/Locale/Data.pm", [18, 4, 18, 24]], ["DateTime/Locale/FromData.pm", [4, 4, 4, 24]], ["DateTime/Locale/Util…`
- …and 18 more distinct claims

### `superset` · completion · use-module — 11 positions, 8 distinct

- `Email/Simple.pm:5:8` `Carp`  (+2 more positions answering identically)
  - base: `n=2 top=[['Carp', 9], ['Carp::Heavy', 9]]`
  - head: `n=3 top=[['Carp::Heavy', 9], ['Carp', 9], ['Carp::Clan', 9]]`
- `Plack/Middleware/HTTPExceptions.pm:6:13` `Try::Tiny`  (+1 more positions answering identically)
  - base: `n=1 top=[['Try::Tiny', 9]]`
  - head: `n=2 top=[['Try::Tiny', 9], ['Try::Tiny::ScopeGuard', 9]]`
- `Catalyst/Request/PartData.pm:4:10` `Encode`
  - base: `n=24 top=[['Encode::CN::HZ', 9], ['Encode::MIME::Name', 9], ['Encode::JP::H2Z', 9], ['Encode::Unicode::UTF7', 9]]`
  - head: `n=25 top=[['Encode', 9], ['Encode::Locale', 9], ['Encode::MIME::Header::ISO_2022_JP', 9], ['Encode::Encoder', 9]]`
- …and 5 more distinct claims

### `superset` · completion · call-site — 11 positions, 10 distinct

- `Mojo/JSON/Pointer.pm:5:30` `_pointer`  (+1 more positions answering identically)
  - base: `n=5 top=[['data', 2], ['contains', 3], ['get', 3], ['new', 3]]`
  - head: `n=12 top=[['data', 2], ['contains', 3], ['get', 3], ['new', 3]]`
- `Date/Language/Brazilian.pm:27:30` `_build_lookups`
  - base: `n=1 top=[['Brazilian', 9]]`
  - head: `n=98 top=[['_build_lookups', 3], ['new', 3], ['DESTROY', 3], ['AUTOLOAD', 3]]`
- `Date/Language/Finnish.pm:34:30` `_build_lookups`
  - base: `n=1 top=[['Finnish', 9]]`
  - head: `n=98 top=[['_build_lookups', 3], ['new', 3], ['DESTROY', 3], ['AUTOLOAD', 3]]`
- …and 7 more distinct claims

### `superset` · completion · variable — 10 positions, 10 distinct

- `DateTime/TimeZone/America/Argentina/Cordoba.pm:592:44` `spans`
  - base: `n=4 top=[['olson_version', 3], ['has_dst_changes', 3], ['_max_year', 3], ['_new_instance', 3]]`
  - head: `n=200 top=[['_max_year', 3], ['_new_instance', 3], ['has_dst_changes', 3], ['olson_version', 3]]`
- `DateTime/TimeZone/America/Boa_Vista.pm:340:44` `spans`
  - base: `n=4 top=[['olson_version', 3], ['has_dst_changes', 3], ['_max_year', 3], ['_new_instance', 3]]`
  - head: `n=200 top=[['_max_year', 3], ['_new_instance', 3], ['has_dst_changes', 3], ['olson_version', 3]]`
- `DateTime/TimeZone/America/Rio_Branco.pm:322:44` `spans`
  - base: `n=4 top=[['olson_version', 3], ['has_dst_changes', 3], ['_max_year', 3], ['_new_instance', 3]]`
  - head: `n=200 top=[['_max_year', 3], ['_new_instance', 3], ['has_dst_changes', 3], ['olson_version', 3]]`
- …and 7 more distinct claims

### `superset` · completion · method-call — 8 positions, 8 distinct

- `Dist/Zilla/Role/ExecFiles.pm:18:21` `zilla`
  - base: `n=2 top=[['dir', 2], ['find_files', 3]]`
  - head: `n=13 top=[['dir', 2], ['find_files', 3], ['plugin_name', 2], ['zilla', 2]]`
- `Mojo/DOM/CSS.pm:23:24` `tree`
  - base: `n=26 top=[['DEBUG', 3], ['tree', 2], ['matches', 3], ['select', 3]]`
  - head: `n=34 top=[['DEBUG', 3], ['tree', 2], ['matches', 3], ['select', 3]]`
- `Mojo/JSON/Pointer.pm:13:24` `data`
  - base: `n=5 top=[['data', 2], ['contains', 3], ['get', 3], ['new', 3]]`
  - head: `n=12 top=[['data', 2], ['contains', 3], ['get', 3], ['new', 3]]`
- …and 5 more distinct claims

### `superset` · references · call-site — 7 positions, 7 distinct

- `Config/MVP/Assembler.pm:134:22` `throw`
  - base: `[["Config/MVP/Assembler.pm", [134, 22, 134, 27]], ["Config/MVP/Assembler.pm", [164, 22, 164, 27]], ["Config/MVP/Assembler.pm", [204, 22, 204, 27]]]`
  - head: `[["Config/MVP/Assembler.pm", [134, 22, 134, 27]], ["Config/MVP/Assembler.pm", [164, 22, 164, 27]], ["Config/MVP/Assembler.pm", [204, 22, 204, 27]], ["Config/MVP/Reader/Finder.pm", [75, 22, 75, 27]], […`
- `Date/Language/Sidama.pm:9:10` `Language`
  - base: `[["Date/Language/Brazilian.pm", [10, 4, 10, 18]], ["Date/Language/Brazilian.pm", [15, 10, 15, 24]], ["Date/Language/Finnish.pm", [12, 10, 12, 24]], ["Date/Language/Finnish.pm", [16, 4, 16, 18]], ["Dat…`
  - head: `[["Date/Format.pm", [21, 19, 21, 33]], ["Date/Format.pm", [21, 35, 21, 49]], ["Date/Language.pm", [1, 8, 1, 22]], ["Date/Language/Afar.pm", [9, 4, 9, 18]], ["Date/Language/Afar.pm", [10, 10, 10, 24]],…`
- `Dist/Zilla/Role/MintingProfile/ShareDir.pm:23:20` `path`
  - base: `[["Dist/Zilla/Role/MintingProfile/ShareDir.pm", [23, 20, 23, 24]]]`
  - head: `[["Dist/Zilla.pm", [739, 18, 739, 22]], ["Dist/Zilla.pm", [741, 15, 741, 19]], ["Dist/Zilla.pm", [748, 2, 748, 6]], ["Dist/Zilla/App/Command/add.pm", [74, 13, 74, 17]], ["Dist/Zilla/App/Command/author…`
- …and 4 more distinct claims

### `superset` · references · module-path — 6 positions, 6 distinct

- `DateTime/TimeZone/America/Argentina/Rio_Gallegos.pm:18:4` `DateTime::TimeZone`
  - base: `[["DateTime/TimeZone/America/Anchorage.pm", [18, 4, 18, 22]], ["DateTime/TimeZone/America/Argentina/Cordoba.pm", [18, 4, 18, 22]], ["DateTime/TimeZone/America/Argentina/Jujuy.pm", [18, 4, 18, 22]], ["…`
  - head: `[["DateTime/TimeZone.pm", [0, 8, 0, 26]], ["DateTime/TimeZone/Africa/Abidjan.pm", [18, 4, 18, 22]], ["DateTime/TimeZone/Africa/Algiers.pm", [18, 4, 18, 22]], ["DateTime/TimeZone/Africa/Bissau.pm", [18…`
- `DateTime/TimeZone/America/Rio_Branco.pm:19:4` `DateTime::TimeZone::OlsonDB`
  - base: `[["DateTime/TimeZone/America/Boise.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/America/Halifax.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/America/Indiana/Knox.pm", [19, 4, 19, 31]], ["DateTime/TimeZone…`
  - head: `[["DateTime/TimeZone/Africa/Abidjan.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/Africa/Algiers.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/Africa/Bissau.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/Africa…`
- `DateTime/TimeZone/Pacific/Kosrae.pm:13:4` `namespace::autoclean`
  - base: `[["DateTime/TimeZone/America/Boise.pm", [13, 4, 13, 24]], ["DateTime/TimeZone/America/Halifax.pm", [13, 4, 13, 24]], ["DateTime/TimeZone/America/Indiana/Knox.pm", [13, 4, 13, 24]], ["DateTime/TimeZone…`
  - head: `[["DateTime/Locale.pm", [6, 4, 6, 24]], ["DateTime/Locale/Base.pm", [4, 4, 4, 24]], ["DateTime/Locale/Data.pm", [18, 4, 18, 24]], ["DateTime/Locale/FromData.pm", [4, 4, 4, 24]], ["DateTime/Locale/Util…`
- …and 3 more distinct claims

### `superset` · references · sub-decl — 4 positions, 4 distinct

- `Dist/Zilla/MVP/Reader/Perl.pm:19:4` `read_into_assembler`
  - base: `[["Dist/Zilla/MVP/Reader/Perl.pm", [19, 4, 19, 23]]]`
  - head: `[["Config/MVP/Reader.pm", [75, 11, 75, 30]], ["Config/MVP/Reader.pm", [97, 4, 97, 23]], ["Config/MVP/Reader/Finder.pm", [121, 4, 121, 23]], ["Config/MVP/Reader/Hash.pm", [21, 4, 21, 23]], ["Config/MVP…`
- `Mojo/BaseUtil.pm:14:4` `monkey_patch`
  - base: `[["Mojo/BaseUtil.pm", [10, 35, 10, 47]], ["Mojo/BaseUtil.pm", [14, 4, 14, 16]]]`
  - head: `[["Mojo/Base.pm", [41, 22, 41, 34]], ["Mojo/Base.pm", [90, 20, 90, 32]], ["Mojo/Base.pm", [110, 22, 110, 34]], ["Mojo/Base.pm", [133, 22, 133, 34]], ["Mojo/BaseUtil.pm", [10, 35, 10, 47]], ["Mojo/Base…`
- `Mojo/WebSocket.pm:144:4` `server_handshake`
  - base: `[["Mojo/WebSocket.pm", [21, 5, 21, 21]], ["Mojo/WebSocket.pm", [144, 4, 144, 20]]]`
  - head: `[["Mojo/Server/Daemon.pm", [8, 23, 8, 39]], ["Mojo/Server/Daemon.pm", [98, 31, 98, 47]], ["Mojo/WebSocket.pm", [21, 5, 21, 21]], ["Mojo/WebSocket.pm", [144, 4, 144, 20]]]`
- …and 1 more distinct claims

### `superset` · references · method-call — 1 positions, 1 distinct

- `PPI/Util.pm:36:23` `new`
  - base: `[["PPI/Document.pm", [190, 4, 190, 7]], ["PPI/Document/File.pm", [49, 4, 49, 7]], ["PPI/Document/File.pm", [59, 27, 59, 30]], ["PPI/Lexer.pm", [196, 31, 196, 34]], ["PPI/Transform.pm", [180, 31, 180, …`
  - head: `[["Dist/Zilla/Plugin/PkgDist.pm", [86, 34, 86, 37]], ["Dist/Zilla/Role/PPI.pm", [42, 32, 42, 35]], ["PPI/Document.pm", [190, 4, 190, 7]], ["PPI/Document/File.pm", [49, 4, 49, 7]], ["PPI/Document/File.…`

### `superset` · references · package — 1 positions, 1 distinct

- `YAML/PP/Representer.pm:2:8` `YAML::PP::Representer`
  - base: `[["YAML/PP/Representer.pm", [2, 8, 2, 29]]]`
  - head: `[["YAML/PP/Dumper.pm", [9, 4, 9, 25]], ["YAML/PP/Dumper.pm", [46, 23, 46, 44]], ["YAML/PP/Representer.pm", [2, 8, 2, 29]]]`

### `capped-head` · completion · package — 16 positions, 1 distinct

- `x86_64-linux-gnu-thread-multi/Moose/Exception/CannotAssignValueToReadOnlyAccessor.pm:0:61` `Moose::Exception::CannotAssignValueToReadOnlyAccessor`  (+15 more positions answering identically)
  - base: `n=234 top=[['trace', 3], ['_build_trace', 3], ['message', 3], ['_build_message', 3]]`
  - head: `n=200 top=[['BUILD', 3], ['_build_message', 3], ['_build_trace', 3], ['as_string', 3]]`

### `timeout-base` · completion · use-module — 3 positions, 3 distinct

- `DateTime/TimeZone/America/Boise.pm:17:20` `Class::Singleton`
  - base: `∅`
  - head: `n=1 top=[['Class::Singleton', 9]]`
- `Email/MessageID.pm:63:63` `Sys::Hostname::Long`
  - base: `∅`
  - head: `n=1 top=[['hostname', 3]]`
- `Test/TypeTiny.pm:2:10` `strict`
  - base: `∅`
  - head: `n=200 top=[['EXTENDED_TESTING', 3], ['_mk_message', 3], ['match', 3], ['matchfor', 3]]`

### `timeout-base` · definition · use-module — 3 positions, 3 distinct

- `DateTime/TimeZone/America/Boise.pm:17:4` `Class::Singleton`
  - base: `∅`
  - head: `[["Class/Singleton.pm", [21, 8, 21, 24]]]`
- `Email/MessageID.pm:63:44` `Sys::Hostname::Long`
  - base: `∅`
  - head: `[]`
- `Test/TypeTiny.pm:2:4` `strict`
  - base: `∅`
  - head: `[["<ext>/perl/5.38.2/strict.pm", [0, 8, 0, 14]], ["<ext>/x86_64-linux-gnu/perl-base/strict.pm", [0, 8, 0, 14]]]`

### `timeout-base` · references · use-module — 3 positions, 2 distinct

- `x86_64-linux-gnu-thread-multi/Class/MOP/Method/Inlined.pm:3:4` `strict`  (+1 more positions answering identically)
  - base: `∅`
  - head: `[["Apache/LogFormat/Compiler.pm", [2, 4, 2, 10]], ["App/Cmd.pm", [37, 7, 37, 13]], ["App/Cmd/ArgProcessor.pm", [0, 4, 0, 10]], ["App/Cmd/Command.pm", [0, 4, 0, 10]], ["App/Cmd/Command/commands.pm", [0…`
- `Dist/Zilla/Role/MintingProfile/ShareDir.pm:8:4` `namespace::autoclean`
  - base: `∅`
  - head: `[["DateTime/Locale.pm", [6, 4, 6, 24]], ["DateTime/Locale/Base.pm", [4, 4, 4, 24]], ["DateTime/Locale/Data.pm", [18, 4, 18, 24]], ["DateTime/Locale/FromData.pm", [4, 4, 4, 24]], ["DateTime/Locale/Util…`

### `timeout-base` · completion · call-site — 3 positions, 3 distinct

- `Mojo/Server.pm:19:19` `app`
  - base: `∅`
  - head: `n=24 top=[['app', 2], ['reverse_proxy', 2], ['trusted_proxies', 2], ['build_app', 3]]`
- `x86_64-linux-gnu-thread-multi/Class/MOP/Method/Inlined.pm:17:27` `isa`
  - base: `∅`
  - head: `n=2 top=[['_uninlined_body', 3], ['can_be_inlined', 3]]`
- `x86_64-linux-gnu-thread-multi/oose.pm:6:15` `Util`
  - base: `∅`
  - head: `n=4 top=[['Moose::Util', 9], ['Moose::Util::TypeConstraints', 9], ['Moose::Util::TypeConstraints::Builtins', 9], ['Moose::Util::MetaRole', 9]]`

### `timeout-base` · definition · call-site — 3 positions, 3 distinct

- `Mojo/Server.pm:19:16` `app`
  - base: `∅`
  - head: `[["Mojo/Server.pm", [10, 4, 10, 7]]]`
- `x86_64-linux-gnu-thread-multi/Class/MOP/Method/Inlined.pm:17:24` `isa`
  - base: `∅`
  - head: `[]`
- `x86_64-linux-gnu-thread-multi/oose.pm:6:11` `Util`
  - base: `∅`
  - head: `[["x86_64-linux-gnu-thread-multi/Moose/Util.pm", [0, 8, 0, 19]]]`

### `timeout-base` · references · package — 2 positions, 2 distinct

- `DateTime/TimeZone/America/Boise.pm:9:8` `DateTime::TimeZone::America::Boise`
  - base: `∅`
  - head: `[["DateTime/TimeZone/America/Boise.pm", [9, 8, 9, 42]]]`
- `Test/TypeTiny.pm:0:8` `Test::TypeTiny`
  - base: `∅`
  - head: `[["Test/TypeTiny.pm", [0, 8, 0, 22]]]`

### `timeout-base` · completion · sub-decl — 1 positions, 1 distinct

- `Dist/Zilla/Role/MintingProfile/ShareDir.pm:20:15` `profile_dir`
  - base: `∅`
  - head: `n=200 top=[['profile_dir', 3], ['Dist::Zilla::Role::MintingProfile::ShareDir', 7], ['$CURLY_SYMBOL', 3], ['$DYNAMIC_FILE_UPLOAD', 3]]`

### `timeout-base` · definition · sub-decl — 1 positions, 1 distinct

- `Dist/Zilla/Role/MintingProfile/ShareDir.pm:20:4` `profile_dir`
  - base: `∅`
  - head: `[["Dist/Zilla/Role/MintingProfile/ShareDir.pm", [20, 4, 20, 15]]]`

### `timeout-base` · references · sub-decl — 1 positions, 1 distinct

- `Email/MessageID.pm:61:4` `create_host`
  - base: `∅`
  - head: `[["Email/MessageID.pm", [45, 28, 45, 39]], ["Email/MessageID.pm", [61, 4, 61, 15]]]`

### `timeout-base` · references · hash-key — 1 positions, 1 distinct

- `Mojo/Server.pm:12:4` `trusted_proxies`
  - base: `∅`
  - head: `[["Mojo/Server.pm", [11, 68, 11, 83]], ["Mojo/Server.pm", [12, 4, 12, 19]], ["Mojo/Server.pm", [26, 46, 26, 61]], ["Mojo/Server/Hypnotoad.pm", [24, 12, 24, 27]], ["Mojolicious/Command/daemon.pm", [26,…`

### `timeout-base` · references · call-site — 1 positions, 1 distinct

- `Path/Class/File.pm:187:16` `new`
  - base: `∅`
  - head: `[["Catalyst.pm", [1296, 33, 1296, 36]], ["Catalyst.pm", [1298, 37, 1298, 40]], ["Catalyst.pm", [3417, 36, 3417, 39]], ["Catalyst.pm", [3583, 53, 3583, 56]], ["Path/Class.pm", [20, 30, 20, 33]], ["Path…`
