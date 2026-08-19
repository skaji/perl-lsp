# Differential sweep report

- **base** `base` — `perl-lsp 0.6.1`
- **head** `head` — `perl-lsp 0.7.0`
- corpus: `/home/user/perl-lsp/gold-corpus/local/lib/perl5`
- path: **server** (LSP over stdio), verbs completion, definition, documentSymbol, hover, references
- **excluded as a capability difference:** `typeDefinition` — served by one side only, so every position would report a divergence that is a missing feature rather than a changed answer
- sampled verbs: `references` at 10%
- cross-file readiness: base 1088 ms, head 1149 ms
- **base server-wedged**: after_position=102, consecutive_timeouts=3, file=Date/Language/Sidama.pm, line=10, restart=1, verb=definition
- **base restart-rewarm**: confirmed=True, cross_file_ready_ms=1068, restart=1
- **base server-wedged**: after_position=888, consecutive_timeouts=3, file=PPI/Util.pm, line=37, restart=2, verb=definition
- **base restart-rewarm**: confirmed=True, cross_file_ready_ms=1089, restart=2
- **base server-wedged**: after_position=1116, consecutive_timeouts=3, file=YAML/PP/Representer.pm, line=46, restart=3, verb=definition
- **base restart-rewarm**: confirmed=True, cross_file_ready_ms=1067, restart=3
- **base server-wedged**: after_position=1173, consecutive_timeouts=3, file=x86_64-linux-gnu-thread-multi/Class/MOP/Mixin/HasAttributes.pm, line=23, restart=4, verb=definition
- **base restart-rewarm**: confirmed=True, cross_file_ready_ms=1096, restart=4
- **base recheck**: empty_first_ask=1310, filled_when_warm=2
- **head recheck**: empty_first_ask=2438, filled_when_warm=13

**4302 (position, verb) answers compared — 2836 identical (65.92%), 1466 divergent.**

Of the identical ones, 945 were empty on both sides: positions nobody would ask about, kept in the denominator but called out so the agreement rate is not read as coverage.

## Divergences by shape

`noise` is the same shape's count when the SAME binary is run twice over these positions. A block at or below its noise floor carries no information — read the `signal` column, not `n`.

| shape | n | noise | signal | meaning |
|---|---|---|---|---|
| `only-base` | 2 | 0 | 2 | base answers, head empty  (LOST resolution -- regression candidate) |
| `subset` | 87 | 0 | 87 | head found strictly fewer  (regression candidate) |
| `disagree` | 788 | 7 | 781 | both non-empty, neither contains the other |
| `content-differs` | 35 | 0 | 35 | same shape, different content (hover text, etc.) |
| `reranked` | 70 | 164 | **below noise — unreadable** | same candidates, different order (completion ranking moved) |
| `only-head` | 263 | 0 | 263 | head answers, base empty  (new resolution -- intended improvement?) |
| `superset` | 182 | 0 | 182 | head found everything base did, plus more |
| `capped-head` | 27 | 0 | 27 | head's list is a subset because head TRUNCATED it (isIncomplete) — by design, not a loss |
| `timeout-base` | 12 | 0 | 12 | base timed out, head answered |

## Groups

Each row is one claim to adjudicate: *intended improvement*, *regression*, or *wash*.

`distinct` is the number of different (base answer, head answer) PAIRS behind the positions — the count of separate claims. One generated data file can contribute sixty positions that all disagree the same way; that is one thing to adjudicate, not sixty, and reading `n` as the workload is how a sweep gets abandoned as noise.

| shape | verb | token kind | n | distinct |
|---|---|---|---|---|
| `only-base` | completion | method-call | 2 | 2 |
| `subset` | completion | method-call | 36 | 34 |
| `subset` | completion | call-site | 24 | 22 |
| `subset` | completion | package | 15 | 13 |
| `subset` | completion | module-path | 11 | 7 |
| `subset` | completion | variable | 1 | 1 |
| `disagree` | completion | sub-decl | 175 | 168 |
| `disagree` | definition | use-module | 145 | 38 |
| `disagree` | completion | variable | 113 | 113 |
| `disagree` | completion | use-module | 95 | 74 |
| `disagree` | definition | module-path | 71 | 45 |
| `disagree` | completion | hash-key | 57 | 43 |
| `disagree` | completion | module-path | 44 | 33 |
| `disagree` | completion | call-site | 36 | 36 |
| `disagree` | completion | package | 28 | 23 |
| `disagree` | completion | method-call | 13 | 12 |
| `disagree` | definition | call-site | 11 | 11 |
| `content-differs` | hover | variable | 15 | 15 |
| `content-differs` | hover | call-site | 6 | 6 |
| `content-differs` | hover | sub-decl | 6 | 6 |
| `content-differs` | hover | method-call | 5 | 5 |
| `content-differs` | hover | module-path | 3 | 3 |
| `reranked` | completion | package | 26 | 10 |
| `reranked` | completion | module-path | 24 | 11 |
| `reranked` | completion | use-module | 15 | 10 |
| `reranked` | completion | call-site | 5 | 5 |
| `only-head` | definition | use-module | 55 | 17 |
| `only-head` | hover | module-path | 43 | 28 |
| `only-head` | completion | module-path | 39 | 13 |
| `only-head` | completion | use-module | 28 | 8 |
| `only-head` | hover | call-site | 17 | 13 |
| `only-head` | definition | module-path | 17 | 10 |
| `only-head` | definition | call-site | 17 | 11 |
| `only-head` | completion | call-site | 13 | 8 |
| `only-head` | definition | method-call | 7 | 7 |
| `only-head` | hover | method-call | 7 | 7 |
| `only-head` | hover | use-module | 7 | 7 |
| `only-head` | completion | method-call | 4 | 3 |
| `only-head` | completion | hash-key | 3 | 3 |
| `only-head` | definition | hash-key | 3 | 3 |
| `only-head` | hover | hash-key | 3 | 3 |
| `superset` | completion | package | 65 | 60 |
| `superset` | completion | hash-key | 35 | 34 |
| `superset` | completion | module-path | 27 | 24 |
| `superset` | references | use-module | 17 | 17 |
| `superset` | completion | variable | 9 | 9 |
| `superset` | completion | use-module | 8 | 7 |
| `superset` | completion | call-site | 8 | 8 |
| `superset` | references | call-site | 5 | 5 |
| `superset` | references | module-path | 5 | 5 |
| `superset` | completion | method-call | 3 | 3 |
| `capped-head` | completion | package | 16 | 1 |
| `capped-head` | completion | sub-decl | 5 | 5 |
| `capped-head` | completion | variable | 3 | 3 |
| `capped-head` | completion | use-module | 1 | 1 |
| `capped-head` | completion | hash-key | 1 | 1 |
| `capped-head` | completion | call-site | 1 | 1 |
| `timeout-base` | completion | use-module | 1 | 1 |
| `timeout-base` | definition | use-module | 1 | 1 |
| `timeout-base` | references | call-site | 1 | 1 |
| `timeout-base` | completion | call-site | 1 | 1 |
| `timeout-base` | definition | call-site | 1 | 1 |
| `timeout-base` | references | method-call | 1 | 1 |
| `timeout-base` | completion | sub-decl | 1 | 1 |
| `timeout-base` | definition | sub-decl | 1 | 1 |
| `timeout-base` | references | package | 1 | 1 |
| `timeout-base` | completion | method-call | 1 | 1 |
| `timeout-base` | definition | method-call | 1 | 1 |
| `timeout-base` | references | use-module | 1 | 1 |

## Examples

### `only-base` · completion · method-call — 2 positions, 2 distinct

- `Dist/Zilla/Role/FileFinderUser.pm:159:50` `find_files`
  - base: `n=1 top=[['(anon)', 3]]`
  - head: `n=0 top=[]`
- `x86_64-linux-gnu-thread-multi/Moose.pm:200:38` `initialize`
  - base: `n=13 top=[['extends', 3], ['with', 3], ['throw_error', 3], ['has', 3]]`
  - head: `n=0 top=[]`

### `subset` · completion · method-call — 36 positions, 34 distinct

- `Mojo/Server.pm:26:17` `req`  (+1 more positions answering identically)
  - base: `n=45 top=[['previous', 2], ['client_read', 3], ['client_write', 3], ['is_empty', 3]]`
  - head: `n=44 top=[['previous', 2], ['client_read', 3], ['client_write', 3], ['is_empty', 3]]`
- `Mojo/Server/Prefork.pm:69:39` `workers`  (+1 more positions answering identically)
  - base: `n=64 top=[['accepts', 2], ['cleanup', 2], ['graceful_timeout', 2], ['heartbeat_timeout', 2]]`
  - head: `n=63 top=[['accepts', 2], ['cleanup', 2], ['graceful_timeout', 2], ['heartbeat_timeout', 2]]`
- `App/Cmd/Simple.pm:187:26` `opt_spec`
  - base: `n=18 top=[['import', 3], ['(anon)', 3], ['usage_desc', 3], ['_cmd_pkg', 3]]`
  - head: `n=17 top=[['import', 3], ['usage_desc', 3], ['_cmd_pkg', 3], ['prepare', 3]]`
- `Catalyst/ActionRole/Scheme.pm:8:42` `env`
  - base: `n=4 top=[['match', 2], ['match_captures', 2], ['list_extra_info', 2], ['(anon)', 3]]`
  - head: `n=3 top=[['match', 2], ['match_captures', 2], ['list_extra_info', 2]]`
- `Class/Data/Inheritable.pm:16:39` `mk_classdata`
  - base: `n=2 top=[['mk_classdata', 3], ['(anon)', 3]]`
  - head: `n=1 top=[['mk_classdata', 3]]`
- …and 29 more distinct claims

### `subset` · completion · call-site — 24 positions, 22 distinct

- `Mojo/JSON/Pointer.pm:5:30` `_pointer`  (+1 more positions answering identically)
  - base: `n=13 top=[['data', 2], ['contains', 3], ['get', 3], ['new', 3]]`
  - head: `n=12 top=[['data', 2], ['contains', 3], ['get', 3], ['new', 3]]`
- `Type/Tie.pm:78:28` `blessed`  (+1 more positions answering identically)
  - base: `n=10 top=[['export_fail', 3], ['set_prototype', 3], ['as_heavy', 3], ['export', 3]]`
  - head: `n=9 top=[['export_fail', 3], ['set_prototype', 3], ['as_heavy', 3], ['export', 3]]`
- `App/Cmd/Simple.pm:127:27` `install_sub`
  - base: `n=8 top=[['_name_of_code', 3], ['_CODELIKE', 3], ['_build_public_installer', 3], ['(anon)', 3]]`
  - head: `n=7 top=[['_name_of_code', 3], ['_CODELIKE', 3], ['_build_public_installer', 3], ['_do_with_warn', 3]]`
- `Catalyst/ActionRole/Scheme.pm:11:58` `orig`
  - base: `n=4 top=[['match', 2], ['match_captures', 2], ['list_extra_info', 2], ['(anon)', 3]]`
  - head: `n=3 top=[['match', 2], ['match_captures', 2], ['list_extra_info', 2]]`
- `Class/Data/Inheritable.pm:10:19` `croak`
  - base: `n=41 top=[['_fetch_sub', 3], ['UTF8_REGEXP_PROBLEM', 3], ['(anon)', 3], ['is_utf8', 3]]`
  - head: `n=40 top=[['_fetch_sub', 3], ['UTF8_REGEXP_PROBLEM', 3], ['is_utf8', 3], ['downgrade', 3]]`
- …and 17 more distinct claims

### `subset` · completion · package — 15 positions, 13 distinct

- `Email/Abstract/EmailSimple.pm:2:36` `Email::Abstract::EmailSimple`  (+1 more positions answering identically)
  - base: `n=17 top=[['object', 3], ['new', 3], ['__class_for', 3], ['_adapter_obj_and_args', 3]]`
  - head: `n=16 top=[['object', 3], ['new', 3], ['__class_for', 3], ['_adapter_obj_and_args', 3]]`
- `x86_64-linux-gnu-thread-multi/Moose/Meta/Method/Accessor/Native/Array.pm:0:52` `Moose::Meta::Method::Accessor::Native::Array`  (+1 more positions answering identically)
  - base: `n=85 top=[['(anon)', 3], ['_new', 3], ['root_types', 3], ['_initialize_body', 3]]`
  - head: `n=84 top=[['_new', 3], ['root_types', 3], ['_initialize_body', 3], ['_inline_curried_arguments', 3]]`
- `Dist/Zilla/Role/MintingProfile/ShareDir.pm:0:51` `Dist::Zilla::Role::MintingProfile::ShareDir`
  - base: `n=3 top=[['profile_dir', 3], ['(anon)', 3], ['ShareDir', 9]]`
  - head: `n=2 top=[['profile_dir', 3], ['ShareDir', 9]]`
- `Email/Sender/Failure/Permanent.pm:0:41` `Email::Sender::Failure::Permanent`
  - base: `n=20 top=[['code', 3], ['recipients', 3], ['(anon)', 3], ['_set_recipients', 3]]`
  - head: `n=19 top=[['code', 3], ['recipients', 3], ['__recipients', 3], ['BUILD', 3]]`
- `HTTP/Message/PSGI.pm:0:27` `HTTP::Message::PSGI`
  - base: `n=36 top=[['_utf8_downgrade', 3], ['(anon)', 3], ['new', 3], ['parse', 3]]`
  - head: `n=35 top=[['_utf8_downgrade', 3], ['new', 3], ['parse', 3], ['clone', 3]]`
- …and 8 more distinct claims

### `subset` · completion · module-path — 11 positions, 7 distinct

- `x86_64-linux-gnu-thread-multi/Moose/Meta/Method/Accessor/Native/Hash/exists.pm:9:49` `Moose::Meta::Method::Accessor::Native::Hash`  (+3 more positions answering identically)
  - base: `n=85 top=[['(anon)', 3], ['_new', 3], ['root_types', 3], ['_initialize_body', 3]]`
  - head: `n=84 top=[['_new', 3], ['root_types', 3], ['_initialize_body', 3], ['_inline_curried_arguments', 3]]`
- `Log/Log4perl/Appender/String.pm:1:37` `Log::Log4perl::Appender`  (+1 more positions answering identically)
  - base: `n=74 top=[['_INTERNAL_DEBUG', 3], ['import', 3], ['(anon)', 3], ['initialized', 3]]`
  - head: `n=73 top=[['_INTERNAL_DEBUG', 3], ['import', 3], ['initialized', 3], ['new', 3]]`
- `CGI/Carp.pm:328:19` `CGI::Carp::VERSION`
  - base: `n=29 top=[['import', 3], ['realwarn', 3], ['realdie', 3], ['id', 3]]`
  - head: `n=28 top=[['import', 3], ['realwarn', 3], ['realdie', 3], ['id', 3]]`
- `HTTP/Message/PSGI.pm:199:27` `HTTP::Message::PSGI`
  - base: `n=36 top=[['_utf8_downgrade', 3], ['(anon)', 3], ['new', 3], ['parse', 3]]`
  - head: `n=35 top=[['_utf8_downgrade', 3], ['new', 3], ['parse', 3], ['clone', 3]]`
- `Type/Tiny/_DeclaredType.pm:39:19` `SUPER::new`
  - base: `n=143 top=[['new', 3], ['(anon)', 3], ['_croak', 3], ['_swap', 3]]`
  - head: `n=142 top=[['new', 3], ['_croak', 3], ['_swap', 3], ['_USE_XS', 3]]`
- …and 2 more distinct claims

### `subset` · completion · variable — 1 positions, 1 distinct

- `Catalyst/ActionRole/Scheme.pm:17:19` `orig`
  - base: `n=4 top=[['match', 2], ['match_captures', 2], ['list_extra_info', 2], ['(anon)', 3]]`
  - head: `n=3 top=[['match', 2], ['match_captures', 2], ['list_extra_info', 2]]`

### `disagree` · completion · sub-decl — 175 positions, 168 distinct

- `DateTime/TimeZone/America/Argentina/San_Luis.pm:596:19` `has_dst_changes`  (+1 more positions answering identically)
  - base: `n=886 top=[['$VERSION', 6], ['$spans', 6], ['olson_version', 3], ['has_dst_changes', 3]]`
  - head: `n=200 top=[['_max_year', 3], ['_new_instance', 3], ['has_dst_changes', 3], ['olson_version', 3]]`
- `DateTime/TimeZone/America/La_Paz.pm:65:19` `has_dst_changes`  (+1 more positions answering identically)
  - base: `n=887 top=[['$VERSION', 6], ['$spans', 6], ['olson_version', 3], ['has_dst_changes', 3]]`
  - head: `n=200 top=[['_max_year', 3], ['_new_instance', 3], ['has_dst_changes', 3], ['olson_version', 3]]`
- `DateTime/TimeZone/Europe/Astrakhan.pm:616:13` `_max_year`  (+1 more positions answering identically)
  - base: `n=1079 top=[['$VERSION', 6], ['$spans', 6], ['olson_version', 3], ['has_dst_changes', 3]]`
  - head: `n=200 top=[['_max_year', 3], ['_new_instance', 3], ['has_dst_changes', 3], ['olson_version', 3]]`
- `Log/Log4perl/Appender/String.pm:24:7` `log`  (+1 more positions answering identically)
  - base: `n=2561 top=[['$ISA[]', 6], ['$#ISA', 6], ['@ISA', 6], ['@ISA[]', 6]]`
  - head: `n=200 top=[['log', 3], ['new', 3], ['string', 3], ['Log::Log4perl::Appender::String', 7]]`
- `Software/License/EUPL_1_1.pm:11:13` `meta_name`  (+1 more positions answering identically)
  - base: `n=824 top=[['name', 3], ['url', 3], ['meta_name', 3], ['meta2_name', 3]]`
  - head: `n=200 top=[['meta2_name', 3], ['meta_name', 3], ['name', 3], ['spdx_expression', 3]]`
- …and 163 more distinct claims

### `disagree` · definition · use-module — 145 positions, 38 distinct

- `App/Cmd/Simple.pm:175:7` `strict`  (+39 more positions answering identically)
  - base: `[["<ext>/x86_64-linux-gnu/perl-base/strict.pm", [0, 0, 0, 0]]]`
  - head: `[["<ext>/perl/5.38.2/strict.pm", [0, 8, 0, 14]], ["<ext>/x86_64-linux-gnu/perl-base/strict.pm", [0, 8, 0, 14]]]`
- `Catalyst/Plugin/Unicode/Encoding.pm:2:4` `warnings`  (+26 more positions answering identically)
  - base: `[["<ext>/x86_64-linux-gnu/perl-base/warnings.pm", [0, 0, 0, 0]]]`
  - head: `[["<ext>/perl/5.38.2/warnings.pm", [5, 8, 5, 16]], ["<ext>/x86_64-linux-gnu/perl-base/warnings.pm", [5, 8, 5, 16]]]`
- `Catalyst/ActionRole/Scheme.pm:2:4` `Moose::Role`  (+14 more positions answering identically)
  - base: `[["x86_64-linux-gnu-thread-multi/Moose/Role.pm", [0, 0, 0, 0]]]`
  - head: `[["x86_64-linux-gnu-thread-multi/Moose/Role.pm", [2, 8, 2, 19]]]`
- `Dist/Zilla/MVP/Reader/Perl.pm:7:4` `Dist::Zilla::Pragmas`  (+6 more positions answering identically)
  - base: `[["Dist/Zilla/Pragmas.pm", [0, 0, 0, 0]]]`
  - head: `[["Dist/Zilla/Pragmas.pm", [0, 8, 0, 28]]]`
- `Software/License/Custom.pm:5:4` `parent`  (+6 more positions answering identically)
  - base: `[["<ext>/x86_64-linux-gnu/perl-base/parent.pm", [0, 0, 0, 0]]]`
  - head: `[["<ext>/perl/5.38.2/parent.pm", [0, 8, 0, 14]], ["<ext>/x86_64-linux-gnu/perl-base/parent.pm", [0, 8, 0, 14]]]`
- …and 33 more distinct claims

### `disagree` · completion · variable — 113 positions, 113 distinct

- `CGI/Carp.pm:499:23` `no`
  - base: `n=315 top=[['$in', 6], ['$no', 6], ['$appease_cpants_kwalitee', 6], ['die', 3]]`
  - head: `n=200 top=[['_longmess', 3], ['_warn', 3], ['carp', 3], ['carpout', 3]]`
- `Catalyst/ClassData.pm:61:8` `class`
  - base: `n=309 top=[['$class', 6], ['$attribute', 6], ['$warn_on_instance', 6], ['$slot', 6]]`
  - head: `n=200 top=[['mk_classdata', 3], ['refs', 3], ['Catalyst::ClassData', 7], ['$CURLY_SYMBOL', 3]]`
- `Catalyst/Request/PartData.pm:39:10` `ct`
  - base: `n=323 top=[['$ct', 6], ['$charset', 6], ['$class', 6], ['$c', 6]]`
  - head: `n=200 top=[['build_from_part_data', 3], ['content_encoding', 2], ['content_type', 2], ['content_type_charset', 2]]`
- `Catalyst/Test.pm:91:15` `request`
  - base: `n=386 top=[['$self', 6], ['$meth', 6], ['$args', 6], ['$defaults', 6]]`
  - head: `n=200 top=[['_build_ctx_request_export', 3], ['_build_get_export', 3], ['_build_request_export', 3], ['_customize_request', 3]]`
- `Class/Data/Inheritable.pm:25:20` `declaredclass`
  - base: `n=388 top=[['$declaredclass', 6], ['$attribute', 6], ['$data', 6], ['$accessor', 6]]`
  - head: `n=200 top=[['mk_classdata', 3], ['subs', 3], ['vars', 3], ['carp', 3]]`
- …and 108 more distinct claims

### `disagree` · completion · use-module — 95 positions, 74 distinct

- `x86_64-linux-gnu-thread-multi/Moose/Exception/CannotAssignValueToReadOnlyAccessor.pm:3:9` `Moose`  (+15 more positions answering identically)
  - base: `n=359 top=[['Moose::Meta::Attribute::Native::Trait::Array', 9], ['Moose::Exporter', 9], ['Moose::Exception::IncompatibleMetaclassOfSuperclass', 9], ['Moose::Exception::MethodExpectedAMetaclassObject', 9]]`
  - head: `n=200 top=[['Moose', 9], ['Moose::Conflicts', 9], ['Moose::Deprecated', 9], ['Moose::Exception', 9]]`
- `x86_64-linux-gnu-thread-multi/Moose/Meta/Method/Accessor/Native/Hash/values.pm:19:14` `Moose::Role`  (+2 more positions answering identically)
  - base: `n=359 top=[['Meta::Attribute::Native::Trait::Array', 9], ['Exporter', 9], ['Exception::IncompatibleMetaclassOfSuperclass', 9], ['Exception::MethodExpectedAMetaclassObject', 9]]`
  - head: `n=200 top=[['_get_caller', 3], ['after', 3], ['around', 3], ['augment', 3]]`
- `Catalyst/Plugin/Unicode/Encoding.pm:1:10` `strict`  (+1 more positions answering identically)
  - base: `n=308 top=[['Catalyst::Plugin::Unicode::Encoding', 7], ['$CURLY_SYMBOL', 3], ['$GRAMMAR', 3], ['$PERMUTE', 3]]`
  - head: `n=200 top=[['Catalyst::Plugin::Unicode::Encoding', 7], ['$CURLY_SYMBOL', 3], ['$DYNAMIC_FILE_UPLOAD', 3], ['$ENCODING_CONSOLE_IN', 3]]`
- `Test/Deep/RegexpVersion.pm:0:10` `strict`  (+1 more positions answering identically)
  - base: `n=868 top=[['Test::Deep::RegexpVersion', 7], ['$CURLY_SYMBOL', 3], ['$TODO', 3], ['%CanonicalLevelNames', 3]]`
  - head: `n=200 top=[['Test::Deep::RegexpVersion', 7], ['$Bin', 3], ['$Bzip2Error', 3], ['$CURLY_SYMBOL', 3]]`
- `URI/file/QNX.pm:2:10` `strict`  (+1 more positions answering identically)
  - base: `n=1043 top=[['_file_extract_path', 3], ['URI::file::QNX', 7], ['URI::file::Unix', 3], ['$CURLY_SYMBOL', 3]]`
  - head: `n=200 top=[['_file_extract_path', 3], ['URI::file::Unix', 3], ['URI::file::QNX', 7], ['$Bin', 3]]`
- …and 69 more distinct claims

### `disagree` · definition · module-path — 71 positions, 45 distinct

- `Dist/Zilla/Role/ExecFiles.pm:3:4` `Moose::Role`  (+8 more positions answering identically)
  - base: `[["x86_64-linux-gnu-thread-multi/Moose/Role.pm", [0, 0, 0, 0]]]`
  - head: `[["x86_64-linux-gnu-thread-multi/Moose/Role.pm", [2, 8, 2, 19]]]`
- `x86_64-linux-gnu-thread-multi/Moose/Exception/CannotAssignValueToReadOnlyAccessor.pm:4:9` `Moose::Exception`  (+7 more positions answering identically)
  - base: `[["x86_64-linux-gnu-thread-multi/Moose/Exception.pm", [0, 0, 0, 0]]]`
  - head: `[["x86_64-linux-gnu-thread-multi/Moose/Exception.pm", [0, 8, 0, 24]]]`
- `Software/License/EUPL_1_1.pm:5:12` `Software::License`  (+4 more positions answering identically)
  - base: `[["Software/License.pm", [0, 0, 0, 0]]]`
  - head: `[["Software/License.pm", [2, 8, 2, 25]]]`
- `Test/TypeTiny.pm:6:4` `Scalar::Util`  (+2 more positions answering identically)
  - base: `[["<ext>/perl-base/Scalar/Util.pm", [0, 0, 0, 0]]]`
  - head: `[["<ext>/5.38.2/Scalar/Util.pm", [6, 8, 6, 20]], ["<ext>/perl-base/Scalar/Util.pm", [6, 8, 6, 20]]]`
- `Dist/Zilla/Role/AfterRelease.pm:4:6` `Dist::Zilla::Role::Plugin`  (+1 more positions answering identically)
  - base: `[["Dist/Zilla/Role/Plugin.pm", [0, 0, 0, 0]]]`
  - head: `[["Dist/Zilla/Role/Plugin.pm", [0, 8, 0, 33]]]`
- …and 40 more distinct claims

### `disagree` · completion · hash-key — 57 positions, 43 distinct

- `x86_64-linux-gnu-thread-multi/Moose/Exception/Role/Attribute.pm:6:6` `is`  (+2 more positions answering identically)
  - base: `n=939 top=[['$VERSION', 6], ['attribute', 2], ['is_attribute_set', 2], ['Moose::Exception::Role::Attribute', 7]]`
  - head: `n=200 top=[['attribute', 2], ['is_attribute_set', 2], ['Moose::Exception::Role::Attribute', 7], ['$Bin', 3]]`
- `Software/License/EUPL_1_2.pm:11:29` `open_source`  (+1 more positions answering identically)
  - base: `n=824 top=[['name', 3], ['url', 3], ['meta_name', 3], ['meta2_name', 3]]`
  - head: `n=200 top=[['meta2_name', 3], ['meta_name', 3], ['name', 3], ['spdx_expression', 3]]`
- `Software/License/FreeBSD.pm:10:29` `open_source`  (+1 more positions answering identically)
  - base: `n=825 top=[['name', 3], ['url', 3], ['meta_name', 3], ['meta2_name', 3]]`
  - head: `n=200 top=[['meta2_name', 3], ['meta_name', 3], ['name', 3], ['spdx_expression', 3]]`
- `Software/License/GFDL_1_3.pm:9:29` `open_source`  (+1 more positions answering identically)
  - base: `n=829 top=[['name', 3], ['url', 3], ['meta_name', 3], ['meta2_name', 3]]`
  - head: `n=200 top=[['meta2_name', 3], ['meta_name', 3], ['name', 3], ['spdx_expression', 3]]`
- `x86_64-linux-gnu-thread-multi/Moose/Exception/CannotAssignValueToReadOnlyAccessor.pm:9:7` `isa`  (+1 more positions answering identically)
  - base: `n=870 top=[['$VERSION', 6], ['value', 2], ['_build_message', 3], ['Moose::Exception::CannotAssignValueToReadOnlyAccessor', 7]]`
  - head: `n=200 top=[['_build_message', 3], ['value', 2], ['Moose::Exception::CannotAssignValueToReadOnlyAccessor', 7], ['$Bin', 3]]`
- …and 38 more distinct claims

### `disagree` · completion · module-path — 44 positions, 33 distinct

- `x86_64-linux-gnu-thread-multi/Moose/Exception/CannotAssignValueToReadOnlyAccessor.pm:4:25` `Moose::Exception`  (+10 more positions answering identically)
  - base: `n=359 top=[['Meta::Attribute::Native::Trait::Array', 9], ['Exporter', 9], ['Exception::IncompatibleMetaclassOfSuperclass', 9], ['Exception::MethodExpectedAMetaclassObject', 9]]`
  - head: `n=200 top=[['_get_caller', 3], ['after', 3], ['around', 3], ['augment', 3]]`
- `DateTime/TimeZone/Asia/Karachi.pm:126:28` `DateTime::TimeZone::INFINITY`  (+1 more positions answering identically)
  - base: `n=1 top=[['Asia::Karachi', 9]]`
  - head: `n=200 top=[['INFINITY', 3], ['IS_DST', 3], ['LOCAL_END', 3], ['LOCAL_START', 3]]`
- `Date/Language/Brazilian.pm:15:24` `Date::Language`
  - base: `n=415 top=[['$VERSION', 6], ['format_a', 3], ['format_A', 3], ['format_b', 3]]`
  - head: `n=200 top=[['format_A', 3], ['format_B', 3], ['format_a', 3], ['format_b', 3]]`
- `Date/Language/Sidama.pm:10:24` `Date::Language`
  - base: `n=804 top=[['format_a', 3], ['format_A', 3], ['format_b', 3], ['format_B', 3]]`
  - head: `n=200 top=[['format_A', 3], ['format_B', 3], ['format_a', 3], ['format_b', 3]]`
- `DateTime/TimeZone/America/Anchorage.pm:21:88` `DateTime::TimeZone`
  - base: `n=1 top=[['TimeZone::America::Anchorage', 9]]`
  - head: `n=200 top=[['DefaultLocale', 3], ['INFINITY', 3], ['MAX_NANOSECONDS', 3], ['NAN', 3]]`
- …and 28 more distinct claims

### `disagree` · completion · call-site — 36 positions, 36 distinct

- `Catalyst/ClassData.pm:43:9` `confess`
  - base: `n=302 top=[['$class', 6], ['$attribute', 6], ['$warn_on_instance', 6], ['$slot', 6]]`
  - head: `n=200 top=[['mk_classdata', 3], ['refs', 3], ['Catalyst::ClassData', 7], ['$CURLY_SYMBOL', 3]]`
- `Config/MVP/Reader/Hash.pm:26:14` `name`
  - base: `n=385 top=[['$name', 6], ['$self', 6], ['$location', 6], ['$assembler', 6]]`
  - head: `n=200 top=[['read_into_assembler', 3], ['Config::MVP::Reader::Hash', 7], ['$CURLY_SYMBOL', 3], ['$DYNAMIC_FILE_UPLOAD', 3]]`
- `Dist/Zilla/Role/FileFinderUser.pm:140:19` `alias`
  - base: `n=1496 top=[['$alias', 6], ['$orig', 6], ['$self', 6], ['$start', 6]]`
  - head: `n=200 top=[['Dist::Zilla::Role::FileFinderUser', 7], ['$CURLY_SYMBOL', 3], ['$DYNAMIC_FILE_UPLOAD', 3], ['$ENCODING_CONSOLE_IN', 3]]`
- `Dist/Zilla/Role/MintingProfile/ShareDir.pm:23:24` `path`
  - base: `n=1488 top=[['$self', 6], ['$profile_name', 6], ['$profile_dir', 6], ['profile_dir', 3]]`
  - head: `n=200 top=[['profile_dir', 3], ['Dist::Zilla::Role::MintingProfile::ShareDir', 7], ['$CURLY_SYMBOL', 3], ['$DYNAMIC_FILE_UPLOAD', 3]]`
- `File/Copy/Recursive.pm:281:40` `stat`
  - base: `n=2587 top=[['$rc', 6], ['$file', 6], ['$file_ut', 6], ['$org', 6]]`
  - head: `n=200 top=[['_bail_if_changed', 3], ['dircopy', 3], ['dirmove', 3], ['fcopy', 3]]`
- …and 31 more distinct claims

### `disagree` · completion · package — 28 positions, 23 distinct

- `YAML/PP/Emitter.pm:2:25` `YAML::PP::Emitter`  (+2 more positions answering identically)
  - base: `n=56 top=[['new', 3], ['clone', 3], ['_arg_yaml_version', 3], ['loader', 3]]`
  - head: `n=59 top=[['new', 3], ['clone', 3], ['_arg_yaml_version', 3], ['loader', 3]]`
- `Mojo/Server/CGI.pm:0:25` `Mojo::Server::CGI`  (+1 more positions answering identically)
  - base: `n=33 top=[['app', 3], ['(anon)', 3], ['reverse_proxy', 3], ['trusted_proxies', 3]]`
  - head: `n=33 top=[['app', 3], ['reverse_proxy', 3], ['trusted_proxies', 3], ['build_app', 3]]`
- `Type/Tiny/ConstrainedObject.pm:0:37` `Type::Tiny::ConstrainedObject`  (+1 more positions answering identically)
  - base: `n=153 top=[['_croak', 3], ['_swap', 3], ['_USE_XS', 3], ['(anon)', 3]]`
  - head: `n=153 top=[['_croak', 3], ['_swap', 3], ['_USE_XS', 3], ['_USE_MOUSE', 3]]`
- `x86_64-linux-gnu-thread-multi/Moose/Meta/TypeConstraint/Parameterizable.pm:0:52` `Moose::Meta::TypeConstraint::Parameterizable`  (+1 more positions answering identically)
  - base: `n=29 top=[['(anon)', 3], ['parents', 3], ['new', 3], ['coerce', 3]]`
  - head: `n=40 top=[['parents', 3], ['new', 3], ['coerce', 3], ['assert_coerce', 3]]`
- `App/Cmd/Simple.pm:108:21` `eq`
  - base: `n=281 top=[['$class', 6], ['$i', 6], ['import', 3], ['(anon)', 3]]`
  - head: `n=200 top=[['_cmd_pkg', 3], ['import', 3], ['usage_desc', 3], ['refs', 3]]`
- …and 18 more distinct claims

### `disagree` · completion · method-call — 13 positions, 12 distinct

- `x86_64-linux-gnu-thread-multi/Moose/Exception/CannotOverrideALocalMethod.pm:15:59` `method_name`  (+1 more positions answering identically)
  - base: `n=8 top=[['method_name', 2], ['_build_message', 3], ['trace', 2], ['_build_trace', 2]]`
  - head: `n=8 top=[['method_name', 2], ['_build_message', 3], ['trace', 2], ['message', 2]]`
- `Mojolicious/Commands.pm:81:65` `start`
  - base: `n=99 top=[['commands', 2], ['(anon)', 3], ['controller_class', 2], ['exception_format', 2]]`
  - head: `n=77 top=[['commands', 2], ['controller_class', 2], ['exception_format', 2], ['home', 2]]`
- `PPI/Token/DashedWord.pm:67:38` `set_class`
  - base: `n=1 top=[['{key}', 15]]`
  - head: `n=55 top=[['__TOKENIZER__on_char', 3], ['new', 3], ['set_class', 3], ['set_content', 3]]`
- `Path/Class/File.pm:34:50` `_spec_class`
  - base: `n=23 top=[['new', 3], ['dir_class', 3], ['as_foreign', 3], ['stringify', 3]]`
  - head: `n=36 top=[['new', 3], ['dir_class', 3], ['as_foreign', 3], ['stringify', 3]]`
- `Plack/Middleware/HTTPExceptions.pm:12:18` `rethrow`
  - base: `n=4 top=[['prepare_app', 3], ['call', 3], ['(anon)', 3], ['transform_error', 3]]`
  - head: `n=9 top=[['prepare_app', 3], ['call', 3], ['transform_error', 3], ['wrap', 3]]`
- …and 7 more distinct claims

### `disagree` · definition · call-site — 11 positions, 11 distinct

- `PPI.pm:18:9` `Structure`
  - base: `[["PPI/Structure.pm", [0, 0, 0, 0]]]`
  - head: `[["PPI/Structure.pm", [0, 8, 0, 22]]]`
- `PPI.pm:24:9` `Tokenizer`
  - base: `[["PPI/Tokenizer.pm", [0, 0, 0, 0]]]`
  - head: `[["PPI/Tokenizer.pm", [0, 8, 0, 22]]]`
- `PPI/Exception/ParserRejection.pm:3:9` `Exception`
  - base: `[["PPI/Exception.pm", [0, 0, 0, 0]]]`
  - head: `[["PPI/Exception.pm", [0, 8, 0, 22]], ["PPI/XSAccessor.pm", [71, 1, 71, 15]]]`
- `PPI/Statement/End.pm:47:9` `Statement`
  - base: `[["PPI/Statement.pm", [0, 0, 0, 0]]]`
  - head: `[["PPI/Statement.pm", [0, 8, 0, 22]], ["PPI/XSAccessor.pm", [98, 1, 98, 15]]]`
- `PPI/Token/Number.pm:32:9` `Token`
  - base: `[["PPI/Token.pm", [0, 0, 0, 0]]]`
  - head: `[["PPI/Token.pm", [0, 8, 0, 18]], ["PPI/XSAccessor.pm", [149, 1, 149, 11]]]`
- …and 6 more distinct claims

### `content-differs` · hover · variable — 15 positions, 15 distinct

- `Config/MVP/Assembler.pm:205:26` `self`
  - base: `len=293 '```perl sub current_section { ``` *class Config::MVP::Assembler — resolved from `$self`* pod =method current_section pod'`
  - head: `len=74 '```perl my ($self, $name, $value) = @_; ``` *type: Config::MVP::Assembler*'`
- `Dist/Zilla/Plugin/PkgDist.pm:62:5` `self`
  - base: `len=211 "```perl sub log() ``` *class Dist::Zilla::Role::Plugin — resolved from `$self`* The plugin's `log` method delegates to t"`
  - head: `len=72 '```perl my ($self, $file) = @_; ``` *type: Dist::Zilla::Plugin::PkgDist*'`
- `Email/MessageID.pm:64:36` `_SYS_HOSTNAME_LONG`
  - base: `len=35 '```perl my $_SYS_HOSTNAME_LONG; ```'`
  - head: `len=51 '```perl my $_SYS_HOSTNAME_LONG; ``` *type: Numeric*'`
- `Mojolicious/Command/prefork.pm:26:39` `prefork`
  - base: `len=261 '```perl sub max_requests() ``` *class Mojo::Server::Daemon — resolved from `$prefork`* *returns: Numeric* ```perl my $ma'`
  - head: `len=102 '```perl my $prefork = Mojo::Server::Prefork->new(app => $self->app); ``` *type: Mojo::Server::Prefork*'`
- `Mojolicious/Plugin/EPRenderer.pm:25:41` `name`
  - base: `len=320 '```perl sub add_handler() ``` *class Mojolicious::Renderer — resolved from `$name`* *returns: Mojolicious::Renderer* ```'`
  - head: `len=80 '```perl my $name = $options->{inline} // $renderer->template_name($options); ```'`
- …and 10 more distinct claims

### `content-differs` · hover · call-site — 6 positions, 6 distinct

- `LWP/Debug/TraceHTTP.pm:25:11` `mcall`
  - base: `len=217 '```perl sub mcall($o, $method, $proto) → Maybe<Data::Dump::Trace::Call> ``` Calls the given method with the given argume'`
  - head: `len=215 '```perl sub mcall($o, $method, $proto) → Maybe<Data::Dump::Trace::Call> ``` Calls the given method with the given argume'`
- `Log/Log4perl/Appender.pm:76:35` `new`
  - base: `len=124 '```perl sub new { ``` *package Log::Log4perl::Appender* *returns: HashRef* #############################################'`
  - head: `len=140 '```perl sub new { ``` *package Log::Log4perl::Appender* *returns: Log::Log4perl::Appender* #############################'`
- `Mojo/IOLoop/Subprocess.pm:84:14` `parent`
  - base: `len=190 '```perl sub on() ``` *class Mojo::EventEmitter — resolved from `$parent`* ```perl my $cb = $e->on(foo => sub {...}); ```'`
  - head: `len=45 '```perl my ($self, $child, $parent) = @_; ```'`
- `Mojo/JSON/Pointer.pm:8:33` `new`
  - base: `len=342 '```perl sub new($class) ``` *class Mojo::Base* *returns: Mojo::Base* ```perl my $object = SubClass->new; my $object = Su'`
  - head: `len=293 '```perl sub new { @_ > 1 ? shift->SUPER::new(data => shift) : shift->SUPER::new } ``` *package Mojo::JSON::Pointer* ```p'`
- `Path/Class/File.pm:187:16` `new`
  - base: `len=66 '```perl sub new { ``` *class Path::Class::File* *returns: HashRef*'`
  - head: `len=76 '```perl sub new { ``` *class Path::Class::File* *returns: Path::Class::File*'`
- …and 1 more distinct claims

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
- `Path/Class/File.pm:113:4` `spew`
  - base: `len=576 '```perl sub spew { ``` *package Path::Class::File* The opposite of (slurp), this takes a list of strings and prints them'`
  - head: `len=572 '```perl sub spew { ``` *package Path::Class::File* The opposite of slurp, this takes a list of strings and prints them t'`
- `Type/Tie.pm:231:5` `EXISTS`
  - base: `len=82 '```perl sub EXISTS { exists $_[0]->_REF->{ $_[1] } } ``` *package Type::Tie::HASH*'`
  - head: `len=98 '```perl sub EXISTS { exists $_[0]->_REF->{ $_[1] } } ``` *package Type::Tie::HASH* *returns: Bool*'`
- …and 1 more distinct claims

### `content-differs` · hover · method-call — 5 positions, 5 distinct

- `Mojo/Date.pm:18:38` `parse`
  - base: `len=64 '```perl sub parse { ``` *class Mojo::Date* *returns: Mojo::Date*'`
  - head: `len=802 "```perl sub parse { ``` *package Mojo::Date* *returns: Mojo::Date* ```perl $date = $date->parse('Sun Nov 6 08:49:37 1994"`
- `Mojo/Server/CGI.pm:28:17` `res`
  - base: `len=680 '```perl sub res() ``` *class Mojo::Transaction::HTTP (from Mojo::Transaction)* *returns: Mojo::Message::Response* ```per'`
  - head: `len=79 '```perl has res => sub { Mojo::Message::Response->new }; ``` — `Transaction.pm`'`
- `Mojolicious/Command/prefork.pm:22:48` `keep_alive_timeout`
  - base: `len=465 '```perl sub keep_alive_timeout() ``` *class Mojo::Server::Prefork (from Mojo::Server::Daemon)* ```perl my $timeout = $da'`
  - head: `len=484 '```perl sub keep_alive_timeout() ``` *class Mojo::Server::Prefork (from Mojo::Server::Daemon)* *returns: Numeric* ```per'`
- `Mojolicious/Commands.pm:81:60` `start`
  - base: `len=491 '```perl sub start($self) ``` *class Mojolicious* *returns: Numeric* ```perl $app->start; $app->start(@ARGV); ``` Start t'`
  - head: `len=42 '```perl sub start { ``` — `Mojolicious.pm`'`
- `Mojolicious/Plugin/EPRenderer.pm:19:41` `add_handler`
  - base: `len=296 '```perl sub add_handler() ``` *class Mojolicious::Renderer* *returns: Mojolicious::Renderer* ```perl $renderer = $render'`
  - head: `len=97 '```perl sub add_handler { $_[0]->handlers->{$_[1]} = $_[2] and return $_[0] } ``` — `Renderer.pm`'`

### `content-differs` · hover · module-path — 3 positions, 3 distinct

- `Mojo/JSON/Pointer.pm:8:26` `SUPER::new`
  - base: `len=342 '```perl sub new($class) ``` *class Mojo::Base* *returns: Mojo::Base* ```perl my $object = SubClass->new; my $object = Su'`
  - head: `len=293 '```perl sub new { @_ > 1 ? shift->SUPER::new(data => shift) : shift->SUPER::new } ``` *package Mojo::JSON::Pointer* ```p'`
- `Software/License/Custom.pm:85:22` `SUPER::new`
  - base: `len=954 '```perl sub new($class, $arg) ``` *class Software::License* *returns: Software::License* pod =head1 SYNOPSIS pod pod my '`
  - head: `len=2387 '```perl sub new { ``` *package Software::License::Custom* pod =head1 DESCRIPTION pod pod This module extends L<Software:'`
- `Type/Tiny/_DeclaredType.pm:39:9` `SUPER::new`
  - base: `len=68 '```perl sub new($class) ``` *class Type::Tiny* *returns: Type::Tiny*'`
  - head: `len=57 '```perl sub new { ``` *package Type::Tiny::_DeclaredType*'`

### `reranked` · completion · package — 26 positions, 10 distinct

- `Dist/Zilla/Role/AfterRelease.pm:0:39` `Dist::Zilla::Role::AfterRelease`  (+7 more positions answering identically)
  - base: `n=45 top=[['Releaser', 9], ['BeforeBuild', 9], ['InstallTool', 9], ['BuildRunner', 9]]`
  - head: `n=45 top=[['PrereqScanner', 9], ['FileInjector', 9], ['ReleaseStatusProvider', 9], ['AfterBuild', 9]]`
- `DateTime/TimeZone/Pacific/Efate.pm:9:42` `DateTime::TimeZone::Pacific::Efate`  (+3 more positions answering identically)
  - base: `n=30 top=[['Guam', 9], ['Apia', 9], ['Efate', 9], ['Pago_Pago', 9]]`
  - head: `n=30 top=[['Nauru', 9], ['Fiji', 9], ['Guadalcanal', 9], ['Galapagos', 9]]`
- `PPI/Exception.pm:0:22` `PPI::Exception`  (+3 more positions answering identically)
  - base: `n=94 top=[['Token::Prototype', 9], ['Token::QuoteLike::Backtick', 9], ['Statement::Package', 9], ['Token::Structure', 9]]`
  - head: `n=94 top=[['Document::Fragment', 9], ['Structure::When', 9], ['Token::Number::Hex', 9], ['Transform::UpdateCopyright', 9]]`
- `x86_64-linux-gnu-thread-multi/Moose/Meta/Method/Accessor/Native/Array/accessor.pm:0:62` `Moose::Meta::Method::Accessor::Native::Array::accessor`  (+1 more positions answering identically)
  - base: `n=28 top=[['_inline_check_var_is_valid_index', 3], ['sort_in_place', 9], ['accessor', 9], ['shallow_clone', 9]]`
  - head: `n=28 top=[['_inline_check_var_is_valid_index', 3], ['unshift', 9], ['grep', 9], ['count', 9]]`
- `x86_64-linux-gnu-thread-multi/Moose/Meta/Method/Accessor/Native/Hash/exists.pm:0:59` `Moose::Meta::Method::Accessor::Native::Hash::exists`  (+1 more positions answering identically)
  - base: `n=16 top=[['_inline_check_var_is_valid_key', 3], ['Writer', 9], ['exists', 9], ['values', 9]]`
  - head: `n=16 top=[['_inline_check_var_is_valid_key', 3], ['Writer', 9], ['accessor', 9], ['shallow_clone', 9]]`
- …and 5 more distinct claims

### `reranked` · completion · module-path — 24 positions, 11 distinct

- `x86_64-linux-gnu-thread-multi/Moose/Exception/CannotCreateMethodAliasLocalMethodIsPresent.pm:5:34` `Moose::Exception::Role::Role`  (+5 more positions answering identically)
  - base: `n=13 top=[['Method', 9], ['RoleForCreateMOPClass', 9], ['RoleForCreate', 9], ['Attribute', 9]]`
  - head: `n=13 top=[['Method', 9], ['InvalidAttributeOptions', 9], ['Attribute', 9], ['InstanceClass', 9]]`
- `PPI/Exception/ParserRejection.pm:7:26` `PPI::Exception`  (+4 more positions answering identically)
  - base: `n=94 top=[['Token::Prototype', 9], ['Token::QuoteLike::Backtick', 9], ['Statement::Package', 9], ['Token::Structure', 9]]`
  - head: `n=94 top=[['Document::Fragment', 9], ['Structure::When', 9], ['Token::Number::Hex', 9], ['Transform::UpdateCopyright', 9]]`
- `Dist/Zilla/Plugin/PkgDist.pm:9:25` `Dist::Zilla::Role::PPI`  (+3 more positions answering identically)
  - base: `n=45 top=[['Releaser', 9], ['BeforeBuild', 9], ['InstallTool', 9], ['BuildRunner', 9]]`
  - head: `n=45 top=[['PrereqScanner', 9], ['FileInjector', 9], ['ReleaseStatusProvider', 9], ['AfterBuild', 9]]`
- `Mojo/DOM/CSS.pm:1:14` `Mojo::Base`  (+1 more positions answering identically)
  - base: `n=2 top=[['Mojo::BaseUtil', 9], ['Mojo::Base', 9]]`
  - head: `n=2 top=[['Mojo::Base', 9], ['Mojo::BaseUtil', 9]]`
- `App/Cmd/Simple.pm:5:21` `App::Cmd::Command`
  - base: `n=4 top=[['App::Cmd::Command', 9], ['App::Cmd::Command::version', 9], ['App::Cmd::Command::help', 9], ['App::Cmd::Command::commands', 9]]`
  - head: `n=4 top=[['App::Cmd::Command::help', 9], ['App::Cmd::Command::version', 9], ['App::Cmd::Command::commands', 9], ['App::Cmd::Command', 9]]`
- …and 6 more distinct claims

### `reranked` · completion · use-module — 15 positions, 10 distinct

- `Mojo/JSON/Pointer.pm:1:14` `Mojo::Base`  (+3 more positions answering identically)
  - base: `n=2 top=[['Mojo::BaseUtil', 9], ['Mojo::Base', 9]]`
  - head: `n=2 top=[['Mojo::Base', 9], ['Mojo::BaseUtil', 9]]`
- `Email/Simple.pm:5:8` `Carp`  (+1 more positions answering identically)
  - base: `n=3 top=[['Carp', 9], ['Carp::Clan', 9], ['Carp::Heavy', 9]]`
  - head: `n=3 top=[['Carp::Clan', 9], ['Carp', 9], ['Carp::Heavy', 9]]`
- `PPI/Token/Comment.pm:61:14` `PPI::Token`  (+1 more positions answering identically)
  - base: `n=47 top=[['PPI::Token::Prototype', 9], ['PPI::Token::QuoteLike::Backtick', 9], ['PPI::Token::Structure', 9], ['PPI::Token::Unknown', 9]]`
  - head: `n=47 top=[['PPI::Token::Number::Hex', 9], ['PPI::Token::Quote::Interpolate', 9], ['PPI::Token::Pod', 9], ['PPI::Token::QuoteLike', 9]]`
- `Catalyst/ClassData.pm:5:15` `Moose::Util`
  - base: `n=4 top=[['Moose::Util::TypeConstraints::Builtins', 9], ['Moose::Util::MetaRole', 9], ['Moose::Util', 9], ['Moose::Util::TypeConstraints', 9]]`
  - head: `n=4 top=[['Moose::Util::MetaRole', 9], ['Moose::Util::TypeConstraints', 9], ['Moose::Util::TypeConstraints::Builtins', 9], ['Moose::Util', 9]]`
- `Log/Log4perl/Appender.pm:99:37` `Log::Log4perl::Config`
  - base: `n=5 top=[['Log::Log4perl::Config::BaseConfigurator', 9], ['Log::Log4perl::Config::DOMConfigurator', 9], ['Log::Log4perl::Config::Watch', 9], ['Log::Log4perl::Config::PropertyConfigurator', 9]]`
  - head: `n=5 top=[['Log::Log4perl::Config::Watch', 9], ['Log::Log4perl::Config::DOMConfigurator', 9], ['Log::Log4perl::Config', 9], ['Log::Log4perl::Config::PropertyConfigurator', 9]]`
- …and 5 more distinct claims

### `reranked` · completion · call-site — 5 positions, 5 distinct

- `Catalyst/Request/PartData.pm:68:20` `new`
  - base: `n=9 top=[['raw_data', 2], ['name', 2], ['size', 2], ['headers', 2]]`
  - head: `n=9 top=[['raw_data', 2], ['name', 2], ['size', 2], ['headers', 2]]`
- `PPI.pm:18:18` `Structure`
  - base: `n=11 top=[['PPI::Structure::Given', 9], ['PPI::Structure', 9], ['PPI::Structure::When', 9], ['PPI::Structure::For', 9]]`
  - head: `n=11 top=[['PPI::Structure::When', 9], ['PPI::Structure::Block', 9], ['PPI::Structure::For', 9], ['PPI::Structure::Given', 9]]`
- `PPI/Statement/End.pm:47:18` `Statement`
  - base: `n=17 top=[['PPI::Statement::Package', 9], ['PPI::Statement::Scheduled', 9], ['PPI::Statement::UnmatchedBrace', 9], ['PPI::Statement::Data', 9]]`
  - head: `n=17 top=[['PPI::Statement::Break', 9], ['PPI::Statement::Package', 9], ['PPI::Statement::Include::Perl6', 9], ['PPI::Statement::End', 9]]`
- `PPI/Token/Number.pm:32:14` `Token`
  - base: `n=47 top=[['PPI::Token::Prototype', 9], ['PPI::Token::QuoteLike::Backtick', 9], ['PPI::Token::Structure', 9], ['PPI::Token::Unknown', 9]]`
  - head: `n=47 top=[['PPI::Token::Number::Hex', 9], ['PPI::Token::Quote::Interpolate', 9], ['PPI::Token::Pod', 9], ['PPI::Token::QuoteLike', 9]]`
- `PPI/Token/Quote/Single.pm:35:21` `Quote`
  - base: `n=11 top=[['PPI::Token::QuoteLike::Backtick', 9], ['PPI::Token::QuoteLike::Regexp', 9], ['PPI::Token::Quote::Double', 9], ['PPI::Token::Quote::Literal', 9]]`
  - head: `n=11 top=[['PPI::Token::Quote::Interpolate', 9], ['PPI::Token::QuoteLike', 9], ['PPI::Token::QuoteLike::Readline', 9], ['PPI::Token::QuoteLike::Regexp', 9]]`

### `only-head` · definition · use-module — 55 positions, 17 distinct

- `Dist/Zilla/Plugin/PkgDist.pm:3:4` `Moose`  (+17 more positions answering identically)
  - base: `[]`
  - head: `[["x86_64-linux-gnu-thread-multi/Moose.pm", [2, 8, 2, 13]]]`
- `DateTime/TimeZone/America/Argentina/San_Luis.pm:13:4` `namespace::autoclean`  (+8 more positions answering identically)
  - base: `[]`
  - head: `[["namespace/autoclean.pm", [3, 8, 3, 28]]]`
- `DateTime/TimeZone/America/Anchorage.pm:17:4` `Class::Singleton`  (+5 more positions answering identically)
  - base: `[]`
  - head: `[["Class/Singleton.pm", [21, 8, 21, 24]]]`
- `DateTime/TimeZone/America/Argentina/Cordoba.pm:19:4` `DateTime::TimeZone::OlsonDB`  (+5 more positions answering identically)
  - base: `[]`
  - head: `[["DateTime/TimeZone/OlsonDB.pm", [0, 8, 0, 35]]]`
- `DateTime/TimeZone/America/La_Paz.pm:18:4` `DateTime::TimeZone`  (+2 more positions answering identically)
  - base: `[]`
  - head: `[["DateTime/TimeZone.pm", [0, 8, 0, 26]]]`
- …and 12 more distinct claims

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
- `Dist/Zilla/Role/AfterRelease.pm:4:6` `Dist::Zilla::Role::Plugin`  (+1 more positions answering identically)
  - base: `∅`
  - head: `len=77 '```perl package Dist::Zilla::Role::Plugin 6.037 ``` *namespace* — `Plugin.pm`'`
- `x86_64-linux-gnu-thread-multi/Moose/Exception/CannotCreateMethodAliasLocalMethodIsPresent.pm:5:6` `Moose::Exception::Role::Role`  (+1 more positions answering identically)
  - base: `∅`
  - head: `len=72 '```perl package Moose::Exception::Role::Role ``` *namespace* — `Role.pm`'`
- …and 23 more distinct claims

### `only-head` · completion · module-path — 39 positions, 13 distinct

- `Text/Unidecode/x19.pm:1:22` `Text::Unidecode::Char`  (+18 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=14 top=[['DEBUG', 3], ['unidecode', 3], ['make_placeholder_map', 3], ['make_placeholder_map_nulls', 3]]`
- `DateTime/TimeZone/America/Argentina/San_Luis.pm:17:20` `Class::Singleton`  (+2 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=1 top=[['Class::Singleton', 9]]`
- `DateTime/TimeZone/America/Boise.pm:19:31` `DateTime::TimeZone::OlsonDB`  (+2 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=5 top=[['DateTime::TimeZone::OlsonDB::Rule', 9], ['DateTime::TimeZone::OlsonDB::Change', 9], ['DateTime::TimeZone::OlsonDB::Zone', 9], ['DateTime::TimeZone::OlsonDB', 9]]`
- `DateTime/TimeZone/Asia/Gaza.pm:2889:39` `DateTime::TimeZone::OlsonDB::Rule`  (+2 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=17 top=[['new', 3], ['parse_file', 3], ['_parse_line', 3], ['_parse_rule', 3]]`
- `DateTime/TimeZone/Asia/Kolkata.pm:13:24` `namespace::autoclean`  (+2 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=1 top=[['namespace::autoclean', 9]]`
- …and 8 more distinct claims

### `only-head` · completion · use-module — 28 positions, 8 distinct

- `DateTime/TimeZone/America/Argentina/San_Luis.pm:13:24` `namespace::autoclean`  (+8 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=1 top=[['namespace::autoclean', 9]]`
- `DateTime/TimeZone/America/Anchorage.pm:17:20` `Class::Singleton`  (+5 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=1 top=[['Class::Singleton', 9]]`
- `DateTime/TimeZone/America/Argentina/Cordoba.pm:19:31` `DateTime::TimeZone::OlsonDB`  (+5 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=5 top=[['DateTime::TimeZone::OlsonDB::Rule', 9], ['DateTime::TimeZone::OlsonDB::Change', 9], ['DateTime::TimeZone::OlsonDB::Zone', 9], ['DateTime::TimeZone::OlsonDB', 9]]`
- `DateTime/TimeZone/America/La_Paz.pm:18:22` `DateTime::TimeZone`  (+2 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=200 top=[['DateTime::TimeZone', 9], ['DateTime::TimeZone::Africa::Abidjan', 9], ['DateTime::TimeZone::Africa::Algiers', 9], ['DateTime::TimeZone::Africa::Bissau', 9]]`
- `Config/MVP/Assembler.pm:204:60` `section`
  - base: `n=0 top=[]`
  - head: `n=200 top=[['_between_sections', 2], ['_between_sections', 2], ['add_value', 3], ['begin_section', 3]]`
- …and 3 more distinct claims

### `only-head` · hover · call-site — 17 positions, 13 distinct

- `Text/Unidecode/x35.pm:1:50` `make_placeholder_map`  (+3 more positions answering identically)
  - base: `∅`
  - head: `len=153 '```perl sub make_placeholder_map() → Sequence<String> ``` =============================================================='`
- `Date/Language/Brazilian.pm:27:16` `_build_lookups`  (+1 more positions answering identically)
  - base: `∅`
  - head: `len=56 '```perl sub _build_lookups() ``` *from `Date::Language`*'`
- `App/Cmd/Simple.pm:127:16` `install_sub`
  - base: `∅`
  - head: `len=38 '```perl use v5.8.0; ``` — `Install.pm`'`
- `Catalyst/Test.pm:48:29` `throw`
  - base: `∅`
  - head: `len=117 '```perl sub throw($class) ``` *class Catalyst::Exception (from Catalyst::Exception::Basic)* Throws a fatal exception.'`
- `Config/MVP/Assembler.pm:134:22` `throw`
  - base: `∅`
  - head: `len=380 '```perl sub throw() ``` *class Config::MVP::Error (from Throwable)* pod =method throw pod pod Something::Throwable->thro'`
- …and 8 more distinct claims

### `only-head` · definition · module-path — 17 positions, 10 distinct

- `DateTime/TimeZone/America/Argentina/San_Luis.pm:17:4` `Class::Singleton`  (+2 more positions answering identically)
  - base: `[]`
  - head: `[["Class/Singleton.pm", [21, 8, 21, 24]]]`
- `DateTime/TimeZone/America/Boise.pm:19:4` `DateTime::TimeZone::OlsonDB`  (+2 more positions answering identically)
  - base: `[]`
  - head: `[["DateTime/TimeZone/OlsonDB.pm", [0, 8, 0, 35]]]`
- `DateTime/TimeZone/Asia/Kolkata.pm:13:4` `namespace::autoclean`  (+2 more positions answering identically)
  - base: `[]`
  - head: `[["namespace/autoclean.pm", [3, 8, 3, 28]]]`
- `Date/Language/Brazilian.pm:10:4` `Date::Language`  (+1 more positions answering identically)
  - base: `[]`
  - head: `[["Date/Language.pm", [1, 8, 1, 22]]]`
- `Catalyst/ClassData.pm:4:4` `Class::MOP`
  - base: `[]`
  - head: `[["x86_64-linux-gnu-thread-multi/Class/MOP.pm", [0, 8, 0, 18]]]`
- …and 5 more distinct claims

### `only-head` · definition · call-site — 17 positions, 11 distinct

- `Text/Unidecode/x35.pm:1:50` `make_placeholder_map`  (+3 more positions answering identically)
  - base: `[]`
  - head: `[["Text/Unidecode.pm", [117, 0, 117, 0]]]`
- `Date/Language/Brazilian.pm:10:10` `Language`  (+2 more positions answering identically)
  - base: `[]`
  - head: `[["Date/Language.pm", [1, 8, 1, 22]]]`
- `Date/Language/Brazilian.pm:27:16` `_build_lookups`  (+1 more positions answering identically)
  - base: `[]`
  - head: `[["Date/Language.pm", [14, 0, 14, 0]]]`
- `Catalyst/Test.pm:48:29` `throw`
  - base: `[]`
  - head: `[["Catalyst/Exception/Basic.pm", [32, 0, 32, 0]]]`
- `Config/MVP/Assembler.pm:134:22` `throw`
  - base: `[]`
  - head: `[["Throwable.pm", [65, 0, 65, 0]]]`
- …and 6 more distinct claims

### `only-head` · completion · call-site — 13 positions, 8 distinct

- `Text/Unidecode/x35.pm:1:70` `make_placeholder_map`  (+3 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=14 top=[['DEBUG', 3], ['unidecode', 3], ['make_placeholder_map', 3], ['make_placeholder_map_nulls', 3]]`
- `Date/Language/Brazilian.pm:10:18` `Language`  (+2 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=37 top=[['Date::Language::Bulgarian', 9], ['Date::Language::Russian_koi8r', 9], ['Date::Language::Italian', 9], ['Date::Language::Amharic', 9]]`
- `Catalyst/Test.pm:48:34` `throw`
  - base: `n=0 top=[]`
  - head: `n=4 top=[['message', 2], ['as_string', 3], ['throw', 3], ['rethrow', 3]]`
- `Config/MVP/Assembler.pm:134:27` `throw`
  - base: `n=0 top=[]`
  - head: `n=12 top=[['message', 2], ['as_string', 3], ['previous_exception', 2], ['throw', 3]]`
- `PPI/Statement/Package.pm:118:19` `isa`
  - base: `n=0 top=[]`
  - head: `n=43 top=[['significant', 3], ['class', 3], ['tokens', 3], ['content', 3]]`
- …and 3 more distinct claims

### `only-head` · definition · method-call — 7 positions, 7 distinct

- `Catalyst/ClassData.pm:53:9` `make_mutable`
  - base: `[]`
  - head: `[["x86_64-linux-gnu-thread-multi/Class/MOP/Class.pm", [1306, 0, 1306, 0]]]`
- `Catalyst/Test.pm:359:22` `new`
  - base: `[]`
  - head: `[["URI.pm", [53, 0, 53, 0]]]`
- `Path/Class/File.pm:34:39` `_spec_class`
  - base: `[]`
  - head: `[["Path/Class/Entity.pm", [29, 0, 29, 0]]]`
- `x86_64-linux-gnu-thread-multi/Class/MOP/Method/Wrapped.pm:127:11` `original_method`
  - base: `[]`
  - head: `[["x86_64-linux-gnu-thread-multi/Class/MOP/Method.pm", [92, 0, 92, 0]]]`
- `x86_64-linux-gnu-thread-multi/Class/MOP/Mixin.pm:11:23` `initialize`
  - base: `[]`
  - head: `[["x86_64-linux-gnu-thread-multi/Class/MOP/Class.pm", [26, 0, 26, 0]]]`
- …and 2 more distinct claims

### `only-head` · hover · method-call — 7 positions, 7 distinct

- `Catalyst/ClassData.pm:53:9` `make_mutable`
  - base: `∅`
  - head: `len=97 '```perl sub make_mutable($self) ``` *class Class::MOP::Class* *returns: Maybe<Class::MOP::Class>*'`
- `Catalyst/Test.pm:359:22` `new`
  - base: `∅`
  - head: `len=1242 '```perl sub new($class, $uri, $scheme) ``` *class URI* *returns: URI::_foreign* Constructs a new URI object. The string '`
- `Path/Class/File.pm:34:39` `_spec_class`
  - base: `∅`
  - head: `len=113 '```perl sub _spec_class($class, $type) ``` *class Path::Class::File (from Path::Class::Entity)* *returns: String*'`
- `x86_64-linux-gnu-thread-multi/Class/MOP/Method/Wrapped.pm:127:11` `original_method`
  - base: `∅`
  - head: `len=95 '```perl sub original_method() ``` *class Class::MOP::Method::Wrapped (from Class::MOP::Method)*'`
- `x86_64-linux-gnu-thread-multi/Class/MOP/Mixin.pm:11:23` `initialize`
  - base: `∅`
  - head: `len=69 '```perl sub initialize($class) ``` *class Class::MOP::Class* Creation'`
- …and 2 more distinct claims

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
- `LWP/Protocol/mailto.pm:7:8` `HTTP::Response`
  - base: `∅`
  - head: `len=62 '```perl package HTTP::Response ``` *namespace* — `Response.pm`'`
- `Log/Log4perl/Appender.pm:99:16` `Log::Log4perl::Config`
  - base: `∅`
  - head: `len=67 '```perl package Log::Log4perl::Config ``` *namespace* — `Config.pm`'`
- …and 2 more distinct claims

### `only-head` · completion · method-call — 4 positions, 3 distinct

- `Catalyst/ClassData.pm:53:21` `make_mutable`  (+1 more positions answering identically)
  - base: `n=0 top=[]`
  - head: `n=161 top=[['initialize', 3], ['reinitialize', 3], ['_construct_class_instance', 3], ['_real_ref_name', 3]]`
- `Catalyst/Test.pm:359:25` `new`
  - base: `n=0 top=[]`
  - head: `n=27 top=[['HAS_RESERVED_SQUARE_BRACKETS', 3], ['_obj_eq', 3], ['new', 3], ['new_abs', 3]]`
- `Log/Log4perl/Config/BaseConfigurator.pm:22:15` `file`
  - base: `n=0 top=[]`
  - head: `n=10 top=[['_INTERNAL_DEBUG', 3], ['eval_if_perl', 3], ['compile_if_perl', 3], ['leaf_path_to_hash', 3]]`

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

### `superset` · completion · package — 65 positions, 60 distinct

- `Mojo/BaseUtil.pm:0:22` `Mojo::BaseUtil`  (+2 more positions answering identically)
  - base: `n=67 top=[['Message::Request', 9], ['WebSocket', 9], ['UserAgent::CookieJar', 9], ['Cookie', 9]]`
  - head: `n=70 top=[['Path', 9], ['Asset::Memory', 9], ['UserAgent::Transactor', 9], ['Template', 9]]`
- `Dist/Zilla/Plugin/PkgDist.pm:0:36` `Dist::Zilla::Plugin::PkgDist`  (+1 more positions answering identically)
  - base: `n=45 top=[['PodCoverageTests', 9], ['MetaNoIndex', 9], ['TestRelease', 9], ['RemovePrereqs', 9]]`
  - head: `n=46 top=[['AutoPrereqs', 9], ['MetaYAML', 9], ['TestRelease', 9], ['GatherDir', 9]]`
- `Email/MessageID.pm:2:24` `Email::MessageID`  (+1 more positions answering identically)
  - base: `n=46 top=[['Abstract::MailMessage', 9], ['Sender::Simple', 9], ['Sender::Transport::SMTP', 9], ['MIME::Header::AddressList', 9]]`
  - head: `n=47 top=[['Sender::Transport::DevNull', 9], ['Sender::Role::HasMessage', 9], ['MIME::ContentType', 9], ['MIME::Encodings', 9]]`
- `URI/Escape.pm:0:19` `URI::Escape`  (+1 more positions answering identically)
  - base: `n=67 top=[['ldap', 9], ['ssh', 9], ['file::Mac', 9], ['ftps', 9]]`
  - head: `n=94 top=[['HAS_RESERVED_SQUARE_BRACKETS', 3], ['_obj_eq', 3], ['new', 3], ['new_abs', 3]]`
- `Catalyst/ActionRole/Scheme.pm:0:36` `Catalyst::ActionRole::Scheme`
  - base: `n=1 top=[['Scheme', 9]]`
  - head: `n=4 top=[['HTTPMethods', 9], ['ConsumesContent', 9], ['QueryMatching', 9], ['Scheme', 9]]`
- …and 55 more distinct claims

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
- `DateTime/TimeZone/America/Argentina/Jujuy.pm:574:34` `spans`
  - base: `n=4 top=[['olson_version', 3], ['has_dst_changes', 3], ['_max_year', 3], ['_new_instance', 3]]`
  - head: `n=200 top=[['_max_year', 3], ['_new_instance', 3], ['has_dst_changes', 3], ['olson_version', 3]]`
- `DateTime/TimeZone/America/Argentina/Rio_Gallegos.pm:592:34` `spans`
  - base: `n=4 top=[['olson_version', 3], ['has_dst_changes', 3], ['_max_year', 3], ['_new_instance', 3]]`
  - head: `n=200 top=[['_max_year', 3], ['_new_instance', 3], ['has_dst_changes', 3], ['olson_version', 3]]`
- …and 29 more distinct claims

### `superset` · completion · module-path — 27 positions, 24 distinct

- `DateTime/TimeZone/America/Boa_Vista.pm:21:66` `Class::Singleton`  (+2 more positions answering identically)
  - base: `n=2 top=[['Struct', 9], ['Tiny', 9]]`
  - head: `n=35 top=[['MOP::MiniTrait', 9], ['Load', 9], ['MOP::Method::Generated', 9], ['MOP::Mixin::HasMethods', 9]]`
- `Mojo/Server.pm:6:14` `Mojo::Util`  (+1 more positions answering identically)
  - base: `n=1 top=[['Mojo::Util', 9]]`
  - head: `n=2 top=[['Mojo::Util', 9], ['Mojo::Util::_Guard', 9]]`
- `Config/MVP/Assembler.pm:65:35` `Config::MVP::Sequence`
  - base: `n=1 top=[['Assembler', 9]]`
  - head: `n=12 top=[['Reader::Hash', 9], ['Sequence', 9], ['Reader::Finder', 9], ['Reader::Findable', 9]]`
- `Config/MVP/Reader/Hash.pm:4:28` `Config::MVP::Reader`
  - base: `n=1 top=[['Reader::Hash', 9]]`
  - head: `n=12 top=[['Reader::Hash', 9], ['Sequence', 9], ['Reader::Finder', 9], ['Reader::Findable', 9]]`
- `DateTime/TimeZone/America/Argentina/Cordoba.pm:576:28` `DateTime::TimeZone::INFINITY`
  - base: `n=1 top=[['America::Argentina::Cordoba', 9]]`
  - head: `n=200 top=[['INFINITY', 3], ['IS_DST', 3], ['LOCAL_END', 3], ['LOCAL_START', 3]]`
- …and 19 more distinct claims

### `superset` · references · use-module — 17 positions, 17 distinct

- `DateTime/TimeZone/America/Argentina/San_Luis.pm:13:4` `namespace::autoclean`
  - base: `[["DateTime/TimeZone/America/Anchorage.pm", [13, 4, 13, 24]], ["DateTime/TimeZone/America/Argentina/Cordoba.pm", [13, 4, 13, 24]], ["DateTime/TimeZone/America/Argentina/Jujuy.pm", [13, 4, 13, 24]], ["…`
  - head: `[["DateTime/Locale.pm", [6, 4, 6, 24]], ["DateTime/Locale/Base.pm", [4, 4, 4, 24]], ["DateTime/Locale/Data.pm", [18, 4, 18, 24]], ["DateTime/Locale/FromData.pm", [4, 4, 4, 24]], ["DateTime/Locale/Util…`
- `DateTime/TimeZone/America/Manaus.pm:19:4` `DateTime::TimeZone::OlsonDB`
  - base: `[["DateTime/TimeZone/America/Anchorage.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/America/Argentina/Cordoba.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/America/Argentina/Jujuy.pm", [19, 4, 19, 31]], ["…`
  - head: `[["DateTime/TimeZone/Africa/Abidjan.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/Africa/Algiers.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/Africa/Bissau.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/Africa…`
- `DateTime/TimeZone/America/Rio_Branco.pm:13:4` `namespace::autoclean`
  - base: `[["DateTime/TimeZone/America/Anchorage.pm", [13, 4, 13, 24]], ["DateTime/TimeZone/America/Argentina/Cordoba.pm", [13, 4, 13, 24]], ["DateTime/TimeZone/America/Argentina/Jujuy.pm", [13, 4, 13, 24]], ["…`
  - head: `[["DateTime/Locale.pm", [6, 4, 6, 24]], ["DateTime/Locale/Base.pm", [4, 4, 4, 24]], ["DateTime/Locale/Data.pm", [18, 4, 18, 24]], ["DateTime/Locale/FromData.pm", [4, 4, 4, 24]], ["DateTime/Locale/Util…`
- `DateTime/TimeZone/America/Sao_Paulo.pm:12:4` `warnings`
  - base: `[["App/Cmd.pm", [4, 4, 4, 12]], ["App/Cmd/ArgProcessor.pm", [1, 4, 1, 12]], ["App/Cmd/Command.pm", [1, 4, 1, 12]], ["App/Cmd/Command/commands.pm", [1, 4, 1, 12]], ["App/Cmd/Command/help.pm", [1, 4, 1,…`
  - head: `[["Apache/LogFormat/Compiler.pm", [3, 4, 3, 12]], ["App/Cmd.pm", [4, 4, 4, 12]], ["App/Cmd/ArgProcessor.pm", [1, 4, 1, 12]], ["App/Cmd/Command.pm", [1, 4, 1, 12]], ["App/Cmd/Command/commands.pm", [1, …`
- `DateTime/TimeZone/Europe/Astrakhan.pm:19:4` `DateTime::TimeZone::OlsonDB`
  - base: `[["DateTime/TimeZone/America/Anchorage.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/America/Argentina/Cordoba.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/America/Argentina/Jujuy.pm", [19, 4, 19, 31]], ["…`
  - head: `[["DateTime/TimeZone/Africa/Abidjan.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/Africa/Algiers.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/Africa/Bissau.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/Africa…`
- …and 12 more distinct claims

### `superset` · completion · variable — 9 positions, 9 distinct

- `DateTime/TimeZone/America/Argentina/Cordoba.pm:592:44` `spans`
  - base: `n=4 top=[['olson_version', 3], ['has_dst_changes', 3], ['_max_year', 3], ['_new_instance', 3]]`
  - head: `n=200 top=[['_max_year', 3], ['_new_instance', 3], ['has_dst_changes', 3], ['olson_version', 3]]`
- `DateTime/TimeZone/America/Boa_Vista.pm:340:44` `spans`
  - base: `n=4 top=[['olson_version', 3], ['has_dst_changes', 3], ['_max_year', 3], ['_new_instance', 3]]`
  - head: `n=200 top=[['_max_year', 3], ['_new_instance', 3], ['has_dst_changes', 3], ['olson_version', 3]]`
- `DateTime/TimeZone/America/Rio_Branco.pm:322:44` `spans`
  - base: `n=4 top=[['olson_version', 3], ['has_dst_changes', 3], ['_max_year', 3], ['_new_instance', 3]]`
  - head: `n=200 top=[['_max_year', 3], ['_new_instance', 3], ['has_dst_changes', 3], ['olson_version', 3]]`
- `DateTime/TimeZone/America/Sao_Paulo.pm:862:44` `spans`
  - base: `n=4 top=[['olson_version', 3], ['has_dst_changes', 3], ['_max_year', 3], ['_new_instance', 3]]`
  - head: `n=200 top=[['_max_year', 3], ['_new_instance', 3], ['has_dst_changes', 3], ['olson_version', 3]]`
- `DateTime/TimeZone/Europe/Budapest.pm:1393:44` `spans`
  - base: `n=7 top=[['olson_version', 3], ['has_dst_changes', 3], ['_max_year', 3], ['_new_instance', 3]]`
  - head: `n=200 top=[['_last_observance', 3], ['_last_offset', 3], ['_max_year', 3], ['_new_instance', 3]]`
- …and 4 more distinct claims

### `superset` · completion · use-module — 8 positions, 7 distinct

- `Plack/Middleware/HTTPExceptions.pm:6:13` `Try::Tiny`  (+1 more positions answering identically)
  - base: `n=1 top=[['Try::Tiny', 9]]`
  - head: `n=2 top=[['Try::Tiny', 9], ['Try::Tiny::ScopeGuard', 9]]`
- `Catalyst/Request/PartData.pm:4:10` `Encode`
  - base: `n=24 top=[['Encode::JP', 9], ['Encode::Guess', 9], ['Encode::MIME::Name', 9], ['Encode::Alias', 9]]`
  - head: `n=25 top=[['Encode', 9], ['Encode::Locale', 9], ['Encode::MIME::Header::ISO_2022_JP', 9], ['Encode::CN', 9]]`
- `LWP/Debug/TraceHTTP.pm:18:14` `Data::Dump`
  - base: `n=5 top=[['Data::Dump', 9], ['Data::Dump::Filtered', 9], ['Data::Dump::FilterContext', 9], ['Data::Dump::Trace', 9]]`
  - head: `n=7 top=[['Data::Dump::Filtered', 9], ['Data::Dump::Trace::Call', 9], ['Data::Dump::FilterContext', 9], ['Data::Dump::Trace::Wrapper', 9]]`
- `Mojo/IOLoop/Subprocess.pm:3:10` `Config`
  - base: `n=18 top=[['Config::MVP::Assembler::WithBundles', 9], ['Config::MVP::Assembler', 9], ['Config::MVP', 9], ['Config::INI', 9]]`
  - head: `n=19 top=[['Config::MVP::Reader::Hash', 9], ['Config::MVP', 9], ['Config::MVP::Sequence', 9], ['Config::MVP::Reader::Finder', 9]]`
- `Type/Params/Parameter.pm:206:35` `Storable`
  - base: `n=25 top=[['_croak', 3], ['new', 3], ['name', 3], ['has_name', 3]]`
  - head: `n=200 top=[['_all_aliases', 3], ['_code_for_default', 3], ['_croak', 3], ['_dont_validate_slurpy', 3]]`
- …and 2 more distinct claims

### `superset` · completion · call-site — 8 positions, 8 distinct

- `Date/Language/Brazilian.pm:27:30` `_build_lookups`
  - base: `n=1 top=[['Brazilian', 9]]`
  - head: `n=98 top=[['_build_lookups', 3], ['new', 3], ['DESTROY', 3], ['AUTOLOAD', 3]]`
- `Date/Language/Finnish.pm:34:30` `_build_lookups`
  - base: `n=1 top=[['Finnish', 9]]`
  - head: `n=98 top=[['_build_lookups', 3], ['new', 3], ['DESTROY', 3], ['AUTOLOAD', 3]]`
- `PPI/Exception.pm:75:12` `caller`
  - base: `n=4 top=[['new', 3], ['throw', 3], ['message', 3], ['callers', 3]]`
  - head: `n=200 top=[['callers', 3], ['message', 3], ['new', 3], ['throw', 3]]`
- `Plack/Handler/Apache2/Registry.pm:13:34` `load_app`
  - base: `n=2 top=[['handler', 3], ['fixup_path', 3]]`
  - head: `n=7 top=[['handler', 3], ['fixup_path', 3], ['new', 3], ['preload', 3]]`
- `Type/Tiny/ConstrainedObject.pm:44:24` `Object`
  - base: `n=6 top=[['HashRef', 9], ['Tied', 9], ['ArrayRef', 9], ['Tuple', 9]]`
  - head: `n=45 top=[['_HAS_REFUTILXS', 3], ['_croak', 3], ['Stringable', 3], ['LazyLoad', 3]]`
- …and 3 more distinct claims

### `superset` · references · call-site — 5 positions, 5 distinct

- `Config/MVP/Assembler.pm:134:22` `throw`
  - base: `[["Config/MVP/Assembler.pm", [134, 22, 134, 27]], ["Config/MVP/Assembler.pm", [164, 22, 164, 27]], ["Config/MVP/Assembler.pm", [204, 22, 204, 27]]]`
  - head: `[["Config/MVP/Assembler.pm", [134, 22, 134, 27]], ["Config/MVP/Assembler.pm", [164, 22, 164, 27]], ["Config/MVP/Assembler.pm", [204, 22, 204, 27]], ["Config/MVP/Reader/Finder.pm", [75, 22, 75, 27]], […`
- `Path/Class/File.pm:187:16` `new`
  - base: `[["Path/Class/File.pm", [13, 4, 13, 7]], ["Path/Class/File.pm", [187, 16, 187, 19]], ["Path/Class/File.pm", [195, 23, 195, 26]]]`
  - head: `[["Catalyst.pm", [1296, 33, 1296, 36]], ["Catalyst.pm", [1298, 37, 1298, 40]], ["Catalyst.pm", [3417, 36, 3417, 39]], ["Catalyst.pm", [3583, 53, 3583, 56]], ["Path/Class.pm", [20, 30, 20, 33]], ["Path…`
- `Plack/Handler/Apache2/Registry.pm:13:26` `load_app`
  - base: `[["Plack/Handler/Apache2/Registry.pm", [13, 26, 13, 34]]]`
  - head: `[["Plack/Handler/Apache2.pm", [23, 16, 23, 24]], ["Plack/Handler/Apache2.pm", [27, 4, 27, 12]], ["Plack/Handler/Apache2.pm", [125, 33, 125, 41]], ["Plack/Handler/Apache2/Registry.pm", [13, 26, 13, 34]…`
- `URI/_ldap.pm:83:8` `uri_unescape`
  - base: `[["URI/Escape.pm", [148, 28, 148, 40]], ["URI/Escape.pm", [215, 4, 215, 16]], ["URI/URL.pm", [18, 19, 18, 31]], ["URI/URL.pm", [121, 11, 121, 23]], ["URI/URL.pm", [146, 8, 146, 20]], ["URI/_emailauth.…`
  - head: `[["Cookie/Baker.pm", [108, 28, 108, 40]], ["Cookie/Baker.pm", [113, 30, 113, 42]], ["HTTP/Message/PSGI.pm", [46, 42, 46, 54]], ["LWP/Protocol/http.pm", [97, 46, 97, 58]], ["LWP/Protocol/http.pm", [110…`
- `x86_64-linux-gnu-thread-multi/Moose/Meta/Method/Accessor/Native/Array/sort_in_place.pm:6:12` `Util`
  - base: `[["Data/OptList.pm", [6, 4, 6, 16]], ["Email/Stuffer.pm", [167, 4, 167, 16]], ["File/ShareDir.pm", [482, 4, 482, 16]], ["Log/Dispatchouli.pm", [12, 4, 12, 16]], ["Log/Dispatchouli/Proxy.pm", [9, 4, 9,…`
  - head: `[["Config/MVP/Assembler/WithBundles.pm", [5, 4, 5, 16]], ["Data/OptList.pm", [6, 4, 6, 16]], ["Dist/Zilla/Role/Plugin.pm", [8, 4, 8, 16]], ["Email/Stuffer.pm", [167, 4, 167, 16]], ["File/ShareDir.pm",…`

### `superset` · references · module-path — 5 positions, 5 distinct

- `DateTime/TimeZone/America/Argentina/Rio_Gallegos.pm:18:4` `DateTime::TimeZone`
  - base: `[["DateTime/TimeZone/America/Anchorage.pm", [18, 4, 18, 22]], ["DateTime/TimeZone/America/Argentina/Cordoba.pm", [18, 4, 18, 22]], ["DateTime/TimeZone/America/Argentina/Jujuy.pm", [18, 4, 18, 22]], ["…`
  - head: `[["DateTime/TimeZone.pm", [0, 8, 0, 26]], ["DateTime/TimeZone/Africa/Abidjan.pm", [18, 4, 18, 22]], ["DateTime/TimeZone/Africa/Algiers.pm", [18, 4, 18, 22]], ["DateTime/TimeZone/Africa/Bissau.pm", [18…`
- `DateTime/TimeZone/America/Rio_Branco.pm:19:4` `DateTime::TimeZone::OlsonDB`
  - base: `[["DateTime/TimeZone/America/Anchorage.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/America/Argentina/Cordoba.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/America/Argentina/Jujuy.pm", [19, 4, 19, 31]], ["…`
  - head: `[["DateTime/TimeZone/Africa/Abidjan.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/Africa/Algiers.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/Africa/Bissau.pm", [19, 4, 19, 31]], ["DateTime/TimeZone/Africa…`
- `DateTime/TimeZone/Pacific/Kosrae.pm:13:4` `namespace::autoclean`
  - base: `[["DateTime/Locale.pm", [6, 4, 6, 24]], ["DateTime/Locale/Base.pm", [4, 4, 4, 24]], ["DateTime/Locale/Data.pm", [18, 4, 18, 24]], ["DateTime/Locale/FromData.pm", [4, 4, 4, 24]], ["DateTime/Locale/Util…`
  - head: `[["DateTime/Locale.pm", [6, 4, 6, 24]], ["DateTime/Locale/Base.pm", [4, 4, 4, 24]], ["DateTime/Locale/Data.pm", [18, 4, 18, 24]], ["DateTime/Locale/FromData.pm", [4, 4, 4, 24]], ["DateTime/Locale/Util…`
- `Software/License/GFDL_1_3.pm:4:12` `Software::License`
  - base: `[["Software/License.pm", [2, 8, 2, 25]], ["Software/License/AGPL_3.pm", [4, 12, 4, 29]], ["Software/License/Apache_1_1.pm", [4, 12, 4, 29]], ["Software/License/Apache_2_0.pm", [4, 12, 4, 29]], ["Softw…`
  - head: `[["Dist/Zilla.pm", [19, 4, 19, 21]], ["Software/License.pm", [2, 8, 2, 25]], ["Software/License/AGPL_3.pm", [4, 12, 4, 29]], ["Software/License/Apache_1_1.pm", [4, 12, 4, 29]], ["Software/License/Apac…`
- `Text/Unidecode/xa4.pm:1:1` `Text::Unidecode::Char`
  - base: `[["Text/Unidecode/x19.pm", [1, 18, 1, 22]], ["Text/Unidecode/x1e.pm", [1, 18, 1, 22]], ["Text/Unidecode/x24.pm", [1, 18, 1, 22]], ["Text/Unidecode/x28.pm", [1, 18, 1, 22]], ["Text/Unidecode/x30.pm", […`
  - head: `[["Text/Unidecode/x00.pm", [1, 18, 1, 22]], ["Text/Unidecode/x01.pm", [1, 18, 1, 22]], ["Text/Unidecode/x02.pm", [1, 18, 1, 22]], ["Text/Unidecode/x03.pm", [1, 18, 1, 22]], ["Text/Unidecode/x04.pm", […`

### `superset` · completion · method-call — 3 positions, 3 distinct

- `x86_64-linux-gnu-thread-multi/Class/MOP/Method/Inlined.pm:46:54` `_expected_method_class`
  - base: `n=2 top=[['_uninlined_body', 3], ['can_be_inlined', 3]]`
  - head: `n=31 top=[['_uninlined_body', 3], ['can_be_inlined', 3], ['new', 3], ['_initialize_body', 3]]`
- `x86_64-linux-gnu-thread-multi/Class/MOP/Overload.pm:25:32` `_throw_exception`
  - base: `n=19 top=[['new', 3], ['operator', 3], ['method_name', 3], ['has_method_name', 3]]`
  - head: `n=31 top=[['new', 3], ['operator', 3], ['method_name', 3], ['has_method_name', 3]]`
- `x86_64-linux-gnu-thread-multi/Moose/Meta/Class/Immutable/Trait.pm:37:42` `name`
  - base: `n=4 top=[['add_role', 3], ['calculate_all_roles', 3], ['calculate_all_roles_with_inheritance', 3], ['does_role', 3]]`
  - head: `n=18 top=[['add_role', 3], ['calculate_all_roles', 3], ['calculate_all_roles_with_inheritance', 3], ['does_role', 3]]`

### `capped-head` · completion · package — 16 positions, 1 distinct

- `x86_64-linux-gnu-thread-multi/Moose/Exception/CannotAssignValueToReadOnlyAccessor.pm:0:61` `Moose::Exception::CannotAssignValueToReadOnlyAccessor`  (+15 more positions answering identically)
  - base: `n=234 top=[['trace', 3], ['_build_trace', 3], ['message', 3], ['_build_message', 3]]`
  - head: `n=200 top=[['BUILD', 3], ['_build_message', 3], ['_build_trace', 3], ['as_string', 3]]`

### `capped-head` · completion · sub-decl — 5 positions, 5 distinct

- `Email/Abstract/EmailSimple.pm:36:13` `as_string`
  - base: `n=2546 top=[['target', 3], ['construct', 3], ['get_header', 3], ['get_body', 3]]`
  - head: `n=200 top=[['as_string', 3], ['construct', 3], ['get_body', 3], ['get_header', 3]]`
- `Email/Abstract/MIMEEntity.pm:8:16` `is_available`
  - base: `n=2546 top=[['$is_avail', 6], ['is_available', 3], ['target', 3], ['construct', 3]]`
  - head: `n=200 top=[['construct', 3], ['get_body', 3], ['is_available', 3], ['set_body', 3]]`
- `Email/MessageID.pm:61:15` `create_host`
  - base: `n=2547 top=[['$_SYS_HOSTNAME_LONG', 6], ['new', 3], ['create_host', 3], ['create_user', 3]]`
  - head: `n=200 top=[['address', 3], ['as_string', 3], ['create_host', 3], ['create_user', 3]]`
- `Email/Simple.pm:329:8` `crlf`
  - base: `n=2555 top=[['$GROUCHY', 6], ['$CREATOR', 6], ['__crlf_re', 3], ['new', 3]]`
  - head: `n=200 top=[['__crlf_re', 3], ['__head', 3], ['_split_head_from_body', 3], ['as_string', 3]]`
- `HTTP/Message.pm:158:15` `add_content`
  - base: `n=2580 top=[['$VERSION', 6], ['$MAXIMUM_BODY_SIZE', 6], ['$CRLF', 6], ['_utf8_downgrade', 3]]`
  - head: `n=200 top=[['AUTOLOAD', 3], ['DESTROY', 3], ['_boundary', 3], ['_content', 3]]`

### `capped-head` · completion · variable — 3 positions, 3 distinct

- `Email/Abstract/MIMEEntity.pm:33:17` `obj`
  - base: `n=2555 top=[['$class', 6], ['$obj', 6], ['$body', 6], ['$lines[]', 6]]`
  - head: `n=200 top=[['construct', 3], ['get_body', 3], ['is_available', 3], ['set_body', 3]]`
- `Email/Simple.pm:81:19` `mycrlf`
  - base: `n=2560 top=[['$class', 6], ['$text', 6], ['$arg', 6], ['$text_ref', 6]]`
  - head: `n=200 top=[['__crlf_re', 3], ['__head', 3], ['_split_head_from_body', 3], ['as_string', 3]]`
- `HTTP/Message.pm:72:9` `hdr`
  - base: `n=2587 top=[['$class', 6], ['$str', 6], ['$hdr[]', 6], ['$#hdr', 6]]`
  - head: `n=200 top=[['AUTOLOAD', 3], ['DESTROY', 3], ['_boundary', 3], ['_content', 3]]`

### `capped-head` · completion · use-module — 1 positions, 1 distinct

- `Email/Sender/Failure/Permanent.pm:6:6` `Moo`
  - base: `n=2540 top=[['Email::Sender::Failure::Permanent', 7], ['$CURLY_SYMBOL', 3], ['$DYNAMIC_FILE_UPLOAD', 3], ['$ENCODING_CONSOLE_IN', 3]]`
  - head: `n=200 top=[['Email::Sender::Failure::Permanent', 7], ['$CURLY_SYMBOL', 3], ['$DYNAMIC_FILE_UPLOAD', 3], ['$ENCODING_CONSOLE_IN', 3]]`

### `capped-head` · completion · hash-key — 1 positions, 1 distinct

- `Email/Simple.pm:153:39` `Date`
  - base: `n=2561 top=[['$class', 6], ['$args{}', 6], ['$headers', 6], ['$GROUCHY', 6]]`
  - head: `n=200 top=[['__crlf_re', 3], ['__head', 3], ['_split_head_from_body', 3], ['as_string', 3]]`

### `capped-head` · completion · call-site — 1 positions, 1 distinct

- `HTTP/Message.pm:868:54` `rand`
  - base: `n=2583 top=[['$size', 6], ['$b', 6], ['$VERSION', 6], ['$MAXIMUM_BODY_SIZE', 6]]`
  - head: `n=200 top=[['AUTOLOAD', 3], ['DESTROY', 3], ['_boundary', 3], ['_content', 3]]`

### `timeout-base` · completion · use-module — 1 positions, 1 distinct

- `Date/Language/Sidama.pm:10:8` `base`
  - base: `∅`
  - head: `n=1 top=[['base', 9]]`

### `timeout-base` · definition · use-module — 1 positions, 1 distinct

- `Date/Language/Sidama.pm:10:4` `base`
  - base: `∅`
  - head: `[["<ext>/perl/5.38.2/base.pm", [1, 8, 1, 12]], ["<ext>/x86_64-linux-gnu/perl-base/base.pm", [1, 8, 1, 12]]]`

### `timeout-base` · references · call-site — 1 positions, 1 distinct

- `Date/Language/Sidama.pm:9:10` `Language`
  - base: `∅`
  - head: `[["Date/Format.pm", [21, 19, 21, 33]], ["Date/Format.pm", [21, 35, 21, 49]], ["Date/Language.pm", [1, 8, 1, 22]], ["Date/Language/Afar.pm", [9, 4, 9, 18]], ["Date/Language/Afar.pm", [10, 10, 10, 24]],…`

### `timeout-base` · completion · call-site — 1 positions, 1 distinct

- `PPI/Util.pm:37:45` `_SCALAR0`
  - base: `∅`
  - head: `n=200 top=[['FALSE', 3], ['HAVE_UNICODE', 3], ['TRUE', 3], ['_Document', 3]]`

### `timeout-base` · definition · call-site — 1 positions, 1 distinct

- `PPI/Util.pm:37:37` `_SCALAR0`
  - base: `∅`
  - head: `[["x86_64-linux-gnu-thread-multi/Params/Util.pm", [85, 0, 85, 0]]]`

### `timeout-base` · references · method-call — 1 positions, 1 distinct

- `PPI/Util.pm:36:23` `new`
  - base: `∅`
  - head: `[["Dist/Zilla/Plugin/PkgDist.pm", [86, 34, 86, 37]], ["Dist/Zilla/Role/PPI.pm", [42, 32, 42, 35]], ["PPI/Document.pm", [190, 4, 190, 7]], ["PPI/Document/File.pm", [49, 4, 49, 7]], ["PPI/Document/File.…`

### `timeout-base` · completion · sub-decl — 1 positions, 1 distinct

- `YAML/PP/Representer.pm:46:25` `preserve_scalar_style`
  - base: `∅`
  - head: `n=200 top=[['_represent_node_nonref', 3], ['_represent_noderef', 3], ['clone', 3], ['new', 3]]`

### `timeout-base` · definition · sub-decl — 1 positions, 1 distinct

- `YAML/PP/Representer.pm:46:4` `preserve_scalar_style`
  - base: `∅`
  - head: `[["YAML/PP/Representer.pm", [46, 4, 46, 25]]]`

### `timeout-base` · references · package — 1 positions, 1 distinct

- `YAML/PP/Representer.pm:2:8` `YAML::PP::Representer`
  - base: `∅`
  - head: `[["YAML/PP/Dumper.pm", [9, 4, 9, 25]], ["YAML/PP/Dumper.pm", [46, 23, 46, 44]], ["YAML/PP/Representer.pm", [2, 8, 2, 29]]]`

### `timeout-base` · completion · method-call — 1 positions, 1 distinct

- `x86_64-linux-gnu-thread-multi/Class/MOP/Mixin/HasAttributes.pm:23:36` `name`
  - base: `∅`
  - head: `n=6 top=[['add_attribute', 3], ['has_attribute', 3], ['get_attribute', 3], ['remove_attribute', 3]]`

### `timeout-base` · definition · method-call — 1 positions, 1 distinct

- `x86_64-linux-gnu-thread-multi/Class/MOP/Mixin/HasAttributes.pm:23:32` `name`
  - base: `∅`
  - head: `[]`

### `timeout-base` · references · use-module — 1 positions, 1 distinct

- `x86_64-linux-gnu-thread-multi/Class/MOP/Mixin/HasAttributes.pm:6:4` `Scalar::Util`
  - base: `∅`
  - head: `[["B/Hooks/EndOfScope/PP/HintHash.pm", [12, 4, 12, 16]], ["Capture/Tiny.pm", [11, 4, 11, 16]], ["Catalyst.pm", [53, 4, 53, 16]], ["Catalyst/Action.pm", [22, 4, 22, 16]], ["Catalyst/Component.pm", [10,…`
