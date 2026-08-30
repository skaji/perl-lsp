# Vendored and generated code: role remap, two levers, span-aware markers

Status: DESIGNED, not started. Decisions below were ratified in discussion
(2026-08-30) against a primary-sourced survey of rust-analyzer, clangd,
gopls, Pyright, TypeScript, golangci-lint, Biome, Sorbet, JetBrains,
linguist, and the Perl LSP field. The performance pressure that opened this
(Znuny's 9.3 GB) was since fixed at the type (`SharedKeys`,
`docs/adr/structural-shapes.md`) — this brief is now a PRODUCT feature:
don't lint code the user doesn't own.

## The design

**Vendored = the DEPENDENCY role, in-tree.** A vendored subtree is remapped
to the existing `RoleMask` DEPENDENCY tier — not a new flag consumers
check. Everything follows from the role: no sweep/`--check` diagnostics, no
dead-export queue entries, demoted workspace-symbol ranking — while
goto-def INTO the vendor keeps working, because dependencies index. This is
rust-analyzer's shape exactly (`CrateOrigin::Library` / `SourceRoot::
is_library` gating both diagnostics paths), the strongest confirmation the
survey found. gopls is the counterexample: dependency-diagnostic
suppression is a still-open issue there (golang/go#74130).

**Two levers, not one** (Pyright's `exclude` vs `ignore`; Biome's
force-ignore-for-output vs ignore-for-generated):
- `vendor` — the role remap above. Analyzed, navigable, silent.
- `exclude` — not indexed at all. For build OUTPUT: `blib/` (a copy of
  `lib/` — indexing it mints duplicate definitions), `.build/`, `_build/`.

**Rename across the role boundary REFUSES, loudly** — rust-analyzer's
`"Cannot rename a non-local definition"`, never a silently narrowed edit
set. A vendored fork CAN call workspace code (the dependency edge is not
one-way in Perl the way a crates.io dep is), so a partial edit set breaks
builds invisibly.

**Dead-export asymmetry:** vendor stops PRODUCING dead-code entries but
keeps COUNTING as a referencer — otherwise a workspace sub consumed only by
vendored code is falsely queued. (staticcheck removed whole-program unused
detection over exactly this class of unsoundness.)

**Detection, layered; precedence user > inner project > outer (clangd's
documented rule):**
1. Convention defaults, ON by default, enumerable via a dump verb and
   overridable (golangci-lint v2 deleted its invisible built-in list as a
   support burden — printable is the lesson): `cpan-lib`, `local/lib/perl5`,
   `local/`, `extlib/`, `inc/`, plus linguist's generic regexes
   (`(3rd|[Tt]hird)[-_]?[Pp]arty/`, `(^|/)vendors?/`,
   `(^|/)[Ee]xtern(als?)?/`). Confirmed gap: linguist's own vendor.yml has
   ZERO Perl entries — nobody has codified these.
2. `.gitattributes` `linguist-vendored` — behind an OPT-IN flag, default
   off. It is a GitHub-UI convention, not an ecosystem signal (GitLab
   incomplete; cloc/scc/ESLint requests open-unimplemented); most Perl
   repos set it to fix their language bar, not to configure an LSP.
3. Explicit config in `.perl-lsp/` (the repo-local home that already
   holds plugins; `scan_entrypoint_scripts`' `extra:` param is the
   anticipated config seam).

**Generated files are SPANS in Perl, not files.** The flagship generated
files — DBIC Schema::Loader — carry a MIDPOINT marker
(`# DO NOT MODIFY THIS OR ANYTHING ABOVE! md5sum:<22>` — Base.pm
`_sig_comment`/`_write_classfile`): generated above, hand-written below.
File-wide suppression would silence the user's own code, a failure no
other ecosystem's marker can produce. If honored: as a span, tri-state
config named after golangci-lint's (`strict`/`lax`/`disable`, default
strict = checksum-anchored match), and start with the EDIT half (suppress
fixes/auto-import/rename into the generated span — Go's landed precedent,
golang/go#75948) before the diagnostic half.

**No inference cap rides along.** The cap idea died with the `SharedKeys`
fix — the cost was clone transport, not key count. If a degradation lever
is ever added anyway: hardcoded not configurable (Pyright's refusal: "a
knob that results in hangs"), and REPORTED at the site naming the config
key (Biome's UX), never silent.

## Survey artifacts worth keeping at hand

rust-analyzer `!source_root.is_library` diagnostics gate + rename bail
(base-db input.rs, main_loop.rs, ide-db rename.rs) · Pyright
`exclude`/`ignore` semantics (configuration.md; discussion #7984) ·
golangci-lint `linters.exclusions.generated: strict|lax|disable`, strict =
`go/ast.IsGenerated` · Biome files.maxSize warning that names its own
config key · TS `skipLibCheck` gates on `isDeclarationFile`, not path ·
LSP has no library-root concept (LSP#472 closed wontfix, 2018).

## Non-goals

Read-only enforcement in the editor (client concern; even rust-analyzer's
is an open request). Auto-detecting generated files by content heuristics
(linguist's generated.rb is ~30 tool-specific literal headers — a
maintenance treadmill; the DBIC marker plus explicit config covers Perl).
