# ADR: Generate kit plugins from Import::Base data tables

`Import::Base` subclasses are how real Perl shops centralize their
import dance — `use Co::Base -Class;` collapses a dozen real `use`
lines via `@IMPORT_MODULES` / `%IMPORT_BUNDLES`. `EmitAction::SyntheticUse`
is the runtime primitive that expresses the expansion. This ADR is about
authorship: the plugins get
generated, not hand-written. The generator lives in `perl-gen/`; the
LSP side adds nothing.

## Decisions worth keeping

### `require`, don't `use` — read variables, don't execute import

`Import::Base` subclasses ARE their `our @IMPORT_MODULES` and
`%IMPORT_BUNDLES` declarations. `require <Kit>` populates them at
compile time without triggering the kit's `import` method. The
generator reads them directly — no `Import::Into` monkey-patching,
no probe stand-in for the *consumer's* `import` dispatch.

The alternative — monkey-patch `Import::Into` and run `Kit->import`
in a probe — works and is the only path for hand-rolled
`sub import` kits. Skip it when reading the tables suffices.

A `%IMPORT_BUNDLES` *value* can itself be a coderef, and that one the
generator does run — against `App::PerlLSP::PluginGen::Probe`, a
recording stand-in (`can`/`AUTOLOAD`) — because a coderef's side
effects (`extends`/`with`/`load_components`) and return value are
the only way to see what it wires up. See "Coderefs" below.

### Generator is Perl, output is a committed `.rhai`

Lives in `perl-gen/` as a CPAN-shaped Perl module. Specifically not a
Rust subcommand of `perl-lsp`. Kit authors write Perl; their tools
should too. Output is a normal `.rhai` plugin file the user commits
to their `$PERL_LSP_PLUGIN_DIR` — same loader, same `--plugin-check`,
same fingerprint-invalidates-cache pipeline as any hand-written
plugin. The generator is just authorship automation.

Specifically not in-memory runtime auto-detection. That would
execute Perl during LSP indexing and hide what's actually loaded.
Committed `.rhai` is inspectable and diffable.

### Sigil interpretation mirrors what real source produces

Import::Base's prefix sigils (`>` Import::Into, `<` emit-first /
base-class, `-` / `>-` unimport) map 1:1 to `SyntheticUse` field
shape (`module`, `args`, `imports`). The synthetic and real paths
end up with identical data going into `process_use`, so downstream
framework detection / parent classes / plugin re-dispatch fire
identically. The `<` prefix's emit-first ordering is preserved in
the rendered `.rhai` so `<Mojo::Base 'X'` registers before
bundle-mates that depend on it.

Bundle dispatch matches Import::Base's `_parse_args`: the first
non-dash arg is the bundle name (`'Class'`); the dashed form
(`-Class`) is accepted as a synonym because some kit READMEs
document it that way.

### Coderefs are probed, best-effort

`%IMPORT_BUNDLES` values can contain coderefs (`load_components`,
`extends('X')`, ...). The generator runs each one, once, against the
recording `Probe` stand-in: verbs it recognizes (`extends`/`with`/
`parent`/`base`/`load_components`) become `PackageParent` emissions,
and the coderef's return value walks the same entry parser as a
static list, becoming `SyntheticUse` (args/imports dropped — a
coderef computes those at runtime, and fabricating a list we can't
verify is worse than a bare `use`). A `// best-effort: ... was
probed by running it ONCE` comment marks the emission so the author
knows conditional branches on the consumer or bundle name weren't
seen. When the probe run itself dies, or a verb it calls has no
`PackageParent`/`SyntheticUse` mapping, that one entry falls back to
`// TODO: coderef at <kit>.pm:<line>` and the user hand-authors it.
B::Deparse + pattern-matching for the cases the probe can't reach is
deferred — adds machinery only worth it if the same shapes recur
across many kits.

## What's intentionally not here

- **Import::Into / hand-rolled `sub import` kits.** Imperative
  bodies have no data tables to read; needs the probe-package
  recorder. Hand-author a per-kit plugin meanwhile.
- **Runtime auto-discovery.** No silent in-memory generation from
  the workspace; what the LSP loads must be on disk.
- **`SyntheticUnuse`.** No primitive yet for `no MOD` directives.
- **Merge mode for coderef hand-edits.** v1 regenerates whole
  files; keep manual additions in a separate companion plugin
  until sentinel-marker preservation earns its weight.
- **Coderef recognition / DDL DSL plugins.** Separate plugin
  concerns, not generator concerns. Belong in `frameworks/` if
  they earn their keep.
