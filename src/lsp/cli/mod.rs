//! The CLI machinery behind the `perl-lsp --…` modes: one-shot startup
//! helpers, usage text, and the utility commands `main()` dispatches to.

use crate::build::{builder, language_driver, plugin};
use crate::index::{document, file_store, module_cache, module_index, module_resolver, resolve};
use crate::lsp::{backend, symbols};
use crate::model::{conventions, file_analysis, witnesses};
use crate::util::timings;

/// Time one CLI query step — `tphase!("completion_items", expr)` prints a
/// `[PHASE]` line when `PERL_LSP_PHASE_TIMING` is set. Sugar over `timings::phase`.
macro_rules! tphase {
    ($label:literal, $body:expr) => {
        $crate::util::timings::phase($label, || $body)
    };
}

mod heatmap;
mod positions;
mod query;

pub(crate) use heatmap::*;
use positions::*;
pub(crate) use query::*;

/// Print the languages this distribution was compiled to serve. A
/// default build prints `perl`; a `cpp-lsp` build (`--features cpp`)
/// prints `perl, cpp`.
pub(crate) fn cli_languages() {
    let reg = crate::build::language_driver::LanguageRegistry::with_enabled();
    println!(
        "perl-lsp {} — languages: {}",
        env!("CARGO_PKG_VERSION"),
        reg.languages().join(", "),
    );
}

/// Analyze one file through its `LanguageDriver` and dump the outline —
/// the multi-language seam at the CLI. Routes by extension; a `.cpp`
/// file goes through the C++ driver (macro reparse → extract) when this
/// binary was built `--features cpp`.
pub(crate) fn cli_lang_analyze(file: &str) {
    let reg = crate::build::language_driver::LanguageRegistry::with_enabled();
    let path = std::path::Path::new(file);
    let Ok(src) = std::fs::read_to_string(path) else {
        eprintln!("cannot read {file}");
        std::process::exit(1);
    };
    let Some(driver) = reg.for_path_sniffed(path, &src) else {
        eprintln!("no driver for {file} (this build serves: {})", reg.languages().join(", "));
        std::process::exit(1);
    };
    // Persist the transitive macro table across invocations — keyed on the
    // file's directory (the CLI has no workspace root). Without this the LSP-only
    // `set_macro_persist_dir` never fires here, so every `--lang-analyze` run
    // re-gathered the whole #include closure cold (op.c: ~1.5s). The on-disk tier
    // now makes a second run warm.
    if let Some(dir) = path.parent() {
        let key = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        crate::build::cpp_reparse::set_macro_persist_dir(
            crate::index::module_cache::cache_dir_for_workspace(Some(&key.to_string_lossy())),
        );
    }
    // `PERL_LSP_BENCH_ITERS=N` re-analyzes N times in-process — the 2nd+ runs
    // are WARM (macro tables cached), which is where the per-analyze macro-
    // expansion cost shows. Combine with `PERL_LSP_PHASE_TIMING=1` for the
    // gather/expand breakdown. Default 1 (single cold analyze).
    let iters: usize = std::env::var("PERL_LSP_BENCH_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n| n >= 1)
        .unwrap_or(1);
    let mut fa = driver.analyze_with_path(&src, Some(std::path::Path::new(path)));
    for _ in 1..iters {
        fa = driver.analyze_with_path(&src, Some(std::path::Path::new(path)));
    }
    println!("# {file} [{}] — {} symbols", driver.id(), fa.symbols().len());
    for s in fa.symbols() {
        let pkg = s.package.as_deref().unwrap_or("");
        let sep = if pkg.is_empty() { "" } else { "::" };
        println!(
            "{:<8} {pkg}{sep}{}\t{}:{}",
            format!("{:?}", s.kind),
            s.name,
            s.span.start.row + 1,
            s.span.start.column + 1,
        );
    }
}

pub(crate) fn print_usage() {
    eprintln!("perl-lsp {} — Perl Language Server", env!("CARGO_PKG_VERSION"));
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  perl-lsp                                              Start LSP server (stdio)");
    eprintln!();
    eprintln!("  Cursor position — two input forms, output MATCHES the form you use:");
    eprintln!("    positional  <file> <line> <col>       0-based line, byte column (engine-native)");
    eprintln!("                                          -> output renders 0-based / byte too");
    eprintln!("    editor      --at <file>:<line>:<col>  1-based line, character column (editor-native,");
    eprintln!("                                          matches grep -n and this tool's own");
    eprintln!("                                          path:line:col output) -> output renders");
    eprintln!("                                          1-based / char, so it round-trips straight");
    eprintln!("                                          back into the next query's --at");
    eprintln!("    e.g.  perl-lsp --definition <root> lib/Foo.pm 12 4");
    eprintln!("          perl-lsp --references <root> --at lib/Foo.pm:13:5");
    eprintln!("    Each cursor query prints a [pos] annotation to stderr showing how the");
    eprintln!("    input was read, the internal 0-based point, and the landed token.");
    eprintln!("    --rename accepts both forms too (its edit spans follow the same rule).");
    eprintln!("    NOTE: --workspace-symbol / --outline JSON always emit engine coordinates");
    eprintln!("          (0-based / byte); the --batch protocol keeps its established output.");
    eprintln!();
    eprintln!("ANALYSIS:");
    eprintln!("  perl-lsp --check [<root>] [--severity error|warning]    Batch diagnostics (CI)");
    eprintln!("                           [--format json|human]");
    eprintln!("                           [--timings]                    Per-module build-timing report (stderr, slowest-first)");
    eprintln!("  perl-lsp --outline <file>                              Document symbol outline");
    eprintln!("  perl-lsp --hover [<root>] <file> <line> <col>         Type info and docs (root = cross-file)");
    eprintln!("  perl-lsp --type-at <file> <line> <col>                 Single type query");
    eprintln!("  perl-lsp --definition <root> <file> <line> <col>       Cross-file goto-def");
    eprintln!("  perl-lsp --implementations <root> <file> <line> <col>  Descendant defs (role composers, overrides)");
    eprintln!("  perl-lsp --references <root> <file> <line> <col>       Cross-file find-refs");
    eprintln!("  perl-lsp --completion <root> <file> <line> <col>       Completion items at point");
    eprintln!("  perl-lsp --signature-help <root> <file> <line> <col>   Signature help at point");
    eprintln!("  perl-lsp --document-highlight <root> <file> <line> <col> In-file occurrences (read/write)");
    eprintln!("  perl-lsp --linked-editing <root> <file> <line> <col>   Linked-editing occurrence set");
    eprintln!("  perl-lsp --semantic-tokens <root> <file>               Semantic token classification");
    eprintln!();
    eprintln!("REFACTORING:");
    eprintln!("  perl-lsp --rename <root> <file> <line> <col> <new>     Cross-file rename");
    eprintln!("  perl-lsp --workspace-symbol <root> <query>             Search symbols");
    eprintln!("  perl-lsp --batch <root>                                Stream JSONL queries (one startup, many)");
    eprintln!();
    eprintln!("INSIGHT:");
    eprintln!("  perl-lsp --heatmap <root> [--csv|--html] [--include-deps] [--all]");
    eprintln!("                                                         Per-symbol usage (fan-in/fan-out)");
    eprintln!("                                                         + unreferenced-symbol candidates");
    eprintln!("                                                         (JSON default; --csv / --html viewer)");
    eprintln!();
    eprintln!("PLUGIN AUTHORING:");
    eprintln!("  perl-lsp --plugin-check <file.rhai>                    Lint a Rhai plugin");
    eprintln!("  perl-lsp --plugin-run <file.rhai> --on <fixture.pl>    Run plugin on one Perl file");
    eprintln!("  perl-lsp --plugin-test <plugin-dir> [--update]         Snapshot-test a plugin dir");
    eprintln!();
    eprintln!("DEBUG:");
    eprintln!("  perl-lsp --dump-package <root> <package>               Dump every sub in <package>");
    eprintln!("                                                         with derived type info");
    eprintln!("  perl-lsp --clear-cache [<root>]                        Wipe the module cache for");
    eprintln!("                                                         <root>, or every project if");
    eprintln!("                                                         <root> is omitted");
    eprintln!("  perl-lsp --parse <file|--> [<lang>]                    Print tree-sitter parse tree");
    eprintln!("                                                         (`-` reads from stdin; <lang>");
    eprintln!("                                                         picks a grammar by id, e.g. cpp)");
    eprintln!();
    eprintln!("MULTI-LANGUAGE (pack drivers, opt-in at build time):");
    eprintln!("  perl-lsp --languages                                   Languages this build serves");
    eprintln!("  perl-lsp --lang-analyze <file>...                      Analyze file(s) via their driver (shared caches)");
    eprintln!("                                                         (route by extension; dump outline)");
    eprintln!("    Build a cpp-lsp:  cargo build --features cpp   (or --features all-langs)");
    eprintln!();
    eprintln!("  perl-lsp --version                                     Print version");
}

// ---- Helpers ----

fn parse_file(path: &str) -> (String, tree_sitter::Tree, file_analysis::FileAnalysis) {
    let source = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Cannot read {}: {}", path, e);
        std::process::exit(1);
    });
    // Route a pack language (cpp, ...) through its driver so the CLI
    // capabilities (--outline, --hover, --batch/gold) match the LSP
    // server. Perl + truly-unrecognized files keep the existing path; an
    // extension no driver claims falls back to a content sniff
    // (`commands.def` is C, not Perl, despite its unowned extension).
    let reg = language_driver::LanguageRegistry::with_enabled();
    if let Some(driver) = reg
        .for_path_sniffed(std::path::Path::new(path), &source)
        .filter(|d| d.id() != "perl")
    {
        let mut parser = driver.make_parser();
        let tree = parser.parse(&source, None).unwrap_or_else(|| {
            eprintln!("Parse failed: {}", path);
            std::process::exit(1);
        });
        let analysis = driver.analyze_with_path(&source, Some(std::path::Path::new(path)));
        return (source, tree, analysis);
    }
    let mut parser = module_resolver::create_parser();
    let tree = parser.parse(&source, None).unwrap_or_else(|| {
        eprintln!("Parse failed: {}", path);
        std::process::exit(1);
    });
    let analysis = builder::build(&tree, source.as_bytes());
    (source, tree, analysis)
}

fn parse_point(line_str: &str, col_str: &str) -> tree_sitter::Point {
    let line: usize = line_str.parse().unwrap_or_else(|_| {
        eprintln!("line must be a number");
        std::process::exit(1);
    });
    let col: usize = col_str.parse().unwrap_or_else(|_| {
        eprintln!("col must be a number");
        std::process::exit(1);
    });
    tree_sitter::Point::new(line, col)
}

/// Canonicalize `root` and produce the matching `file://...` URI.
/// Returns both because callers usually want the path (for `@INC`
/// project-lib discovery, file walking) AND the URI (the cache hash
/// key, since the LSP server's `cache_dir_for_workspace` is fed the
/// initialize request's `root_uri` string verbatim — `file://...`).
/// Falls back to the raw input if `canonicalize` fails (path doesn't
/// exist, permissions): same string lands in both halves so the
/// caller's downstream "does this dir exist?" check still fires.
fn canonical_root_and_uri(root: &str) -> (std::path::PathBuf, String) {
    let path = std::path::Path::new(root)
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(root));
    let uri = format!("file://{}", path.display());
    (path, uri)
}

/// Full CLI workspace setup: index the workspace, open the SQLite cache,
/// warm cached modules, resolve missing imports + ancestors via @INC,
/// save fresh entries back to disk. Mirrors the LSP server's startup
/// minus the resolver thread (one-shot, synchronous). Used by every
/// CLI command that needs cross-file resolution to behave like the
/// running server.
fn cli_full_startup(root: &str) -> (file_store::FileStore, module_index::ModuleIndex) {
    // `--check --timings` already flipped this on; the env var lets any
    // startup-driven CLI mode (dump-package, etc.) opt in too.
    timings::enable_from_env();
    let (root_path, root_uri) = canonical_root_and_uri(root);
    // Pin repo-local `.perl-lsp/` plugin discovery to the same root the
    // cache keys on, before the first build() (workspace indexing) fires.
    plugin::rhai_host::set_workspace_root(Some(&root_uri));

    // Register workspace files INTO the module index (not just a FileStore)
    // so their plugin bridges + package names participate in cross-file
    // lookups — the whole point of "act like the server just started".
    // Indexing without the index (bridge-less) is what forced callers to
    // hand-roll their own `index_workspace_with_index`, and they drifted.
    let module_index = module_index::ModuleIndex::new_for_cli();
    module_index.mark_long_lived_from_env();
    // Wake the headless resolver: it blocks on this channel for the
    // @INC scan + SQLite warm.
    module_index.set_workspace_root(Some(root_uri.as_str()));
    let ws = file_store::FileStore::new();
    let indexed =
        module_resolver::index_workspace_with_index(&root_path, &ws, Some(&module_index), None);
    // Label the tier: a pack-only workspace printing a bare "Indexed 0 files"
    // reads as "indexing failed" when the pack line below says otherwise.
    if indexed > 0 {
        eprintln!("Indexed {} Perl files", indexed);
    }
    // Pack languages (C++/Python/…) → per-language sub-indexes (separate
    // caches, no cross-language overlap), attached to the hub for routing.
    let pack_indexed = module_resolver::index_pack_languages(
        &root_path,
        Some(&root_uri),
        &module_index,
        None,
        crate::lsp::backend::max_cache_mb_default() as usize * 1024 * 1024,
    );
    if pack_indexed > 0 {
        // Name the languages actually served rather than the
        // generic "pack-language" — a pure-C++ workspace should read "C/C++",
        // not a term that only makes sense from inside this codebase.
        let reg = language_driver::LanguageRegistry::with_enabled();
        let langs: Vec<&'static str> = reg
            .languages()
            .into_iter()
            .filter(|id| *id != "perl")
            .map(language_driver::LanguageRegistry::display_name)
            .collect();
        eprintln!("Indexed {} {} files", pack_indexed, langs.join("/"));
    }

    let mut inc_paths = module_resolver::discover_inc_paths();
    module_resolver::add_project_lib_paths(&mut inc_paths, &root_path);

    let db = module_cache::open_cache_db(Some(&root_uri), "perl");
    let mut stale_set: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Some(ref conn) = db {
        let _ = module_cache::validate_inc_paths(conn, &inc_paths);
        let _ = module_cache::validate_plugin_fingerprint(
            conn,
            &plugin::rhai_host::plugin_fingerprint(),
        );
        let (warmed, stale) = module_cache::warm_cache(
            conn,
            &module_index.cache_raw(),
            module_index.is_long_lived() && module_resolver::eviction_enabled(),
        );
        // Stamp generations for the warm-loaded @INC providers (the warm
        // scan bypasses the registration front doors) so `enrichment_key`
        // reads a real token for every provider.
        module_index.stamp_import_generations();
        if warmed > 0 {
            eprintln!("Cache: {} modules loaded from disk", warmed);
        }
        stale_set = stale.into_iter().collect();
    }

    let mut needed = std::collections::HashSet::new();
    for entry in ws.workspace_raw().iter() {
        for imp in &entry.value().imports {
            needed.insert(imp.module_name.clone());
        }
        for (_pkg, parents) in entry.value().package_parent_edges() {
            for p in parents {
                needed.insert(p.clone());
            }
        }
    }

    let mut parser = module_resolver::create_parser();
    let mut parse_memo: module_resolver::ParseMemo = std::collections::HashMap::new();
    let mut resolved = 0usize;
    let mut already_cached = 0usize;
    // Worklist, not a fixed set: re-exporting modules (Test::Most → Test::More)
    // pull their re-exported producers' surfaces transitively, so those
    // producers must be resolved too even though no workspace file `use`s them
    // directly. Enqueue each resolved module's `reexport_modules`. Bounded by
    // the seen-set (`queued`); the cross-file surface walk handles cycles.
    let mut queue: std::collections::VecDeque<String> = needed.iter().cloned().collect();
    let mut queued: std::collections::HashSet<String> = needed.clone();
    while let Some(name) = queue.pop_front() {
        let cached_arc = if module_index.cache_raw().contains_key(name.as_str())
            && !stale_set.contains(&name)
        {
            already_cached += 1;
            timings::record_cached(&name);
            module_index.get_cached(&name)
        } else {
            if stale_set.contains(&name) {
                parse_memo.remove(&name);
            }
            module_resolver::resolve_and_parse_with_memo(&inc_paths, &name, &mut parser, &mut parse_memo)
                .map(|cached| {
                    if let Some(ref conn) = db {
                        module_cache::save_to_db(
                            conn,
                            &name,
                            &Some(std::sync::Arc::clone(&cached)),
                            module_cache::NAME_KEYED_SOURCE,
                        );
                    }
                    module_index.insert_cache(&name, Some(std::sync::Arc::clone(&cached)));
                    resolved += 1;
                    cached
                })
        };
        if let Some(cached) = cached_arc {
            for re in &cached.analysis.reexport_modules {
                if queued.insert(re.clone()) {
                    queue.push_back(re.clone());
                }
            }
        }
    }
    eprintln!("Modules: {} cached, {} resolved, {} total", already_cached, resolved, queued.len());

    // `warm_cache` populated `cache_raw()` directly, bypassing the reverse
    // index, and `insert_cache` only indexes export/export_ok. Rebuild the
    // full `func → modules` index from the cache so `find_exporters` answers
    // identically on cold and warm runs (B6 export-attribution regression).
    module_index.rebuild_reverse_index_from_cache();

    // Ancestry is now fully populated: materialize deferred cross-file
    // `ClassIsa` plugin emissions (DBIC column/relationship accessors reached
    // through a cross-file base) into the whole resident cached copies, so
    // cross-file goto-def / references see them via `whole_present`. See
    // `ModuleIndex::materialize_gated_emissions` / `GatedEmission`.
    module_index.materialize_gated_emissions();

    (ws, module_index)
}

fn is_json_format(args: &[String]) -> bool {
    args.windows(2).any(|w| w[0] == "--format" && w[1] == "json")
}

fn get_arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].as_str())
}

fn severity_rank(s: &str) -> u8 {
    match s {
        "error" => 0,
        "warning" => 1,
        "info" => 2,
        "hint" => 3,
        _ => 1,
    }
}

/// --clear-cache [<root>] — Remove the SQLite module cache.
///
/// Without `<root>`: nuke the entire `~/.cache/perl-lsp` (or
/// `$XDG_CACHE_HOME/perl-lsp`) tree — every project the user has
/// touched. With `<root>`: only the per-project cache dir
/// (`<base>/<workspace_hash>`) is removed; other projects keep theirs.
///
/// `<root>` is canonicalized and joined with `file://` to match the
/// hash key the LSP server and `cli_full_startup` write under (the
/// initialize request hands a `file://` URI; both CLI and server feed
/// that string into `cache_dir_for_workspace`). Without canonicalizing
/// here a relative path would hash to a different bucket and silently
/// "clear" a non-existent dir.
///
/// On the next LSP start the cache is recreated from scratch — the
/// resolver re-resolves modules lazily and the workspace indexer
/// re-walks the tree. Pure side-effect command; prints what was
/// removed so it's obvious whether anything happened.
pub(crate) fn cli_clear_cache(root: Option<&str>) {
    let target = match root {
        Some(r) => {
            let (_, root_uri) = canonical_root_and_uri(r);
            module_cache::cache_dir_for_workspace(Some(&root_uri))
        }
        None => module_cache::cache_base_dir(),
    };
    let Some(path) = target else {
        eprintln!("Cannot determine cache dir: $HOME and $XDG_CACHE_HOME are both unset");
        std::process::exit(1);
    };
    if !path.exists() {
        eprintln!("Cache already absent: {}", path.display());
        return;
    }
    match std::fs::remove_dir_all(&path) {
        Ok(()) => eprintln!("Cleared cache: {}", path.display()),
        Err(e) => {
            eprintln!("Failed to remove {}: {}", path.display(), e);
            std::process::exit(1);
        }
    }
}

/// Pretty-print the tree-sitter parse tree for a Perl source file.
/// `<file>` may be `-` to read from stdin. Mirrors `tree-sitter parse`
/// output shape — `(node_kind [row, col] - [row, col]` per line,
/// 2-space indent per depth, field names prefixed (`field: kind`).
pub(crate) fn cli_parse(path: &str, lang: Option<&str>) {
    use std::io::Read;
    let source = if path == "-" {
        let mut s = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut s) {
            eprintln!("read stdin: {}", e);
            std::process::exit(1);
        }
        s
    } else {
        match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("read {}: {}", path, e);
                std::process::exit(1);
            }
        }
    };
    // Route to a grammar: an explicit `lang` id (stdin can't route by
    // extension) wins, else the file's extension (cpp/python/r/cmake), else
    // a content sniff for an extension no driver claims, so
    // --parse shows the SAME tree the pack extractor sees. Perl + stdin +
    // truly-unrecognized files keep the Perl grammar.
    let mut parser = if let Some(id) = lang {
        let reg = crate::build::language_driver::LanguageRegistry::with_enabled();
        match reg.for_id(id) {
            Some(d) => d.make_parser(),
            None => {
                eprintln!(
                    "unknown language `{}`; served: {}",
                    id,
                    reg.languages().join(", ")
                );
                std::process::exit(1);
            }
        }
    } else if path != "-" {
        let reg = crate::build::language_driver::LanguageRegistry::with_enabled();
        reg.for_path_sniffed(std::path::Path::new(path), &source)
            .filter(|d| d.id() != "perl")
            .map(|d| d.make_parser())
            .unwrap_or_else(builder::create_parser)
    } else {
        builder::create_parser()
    };
    let Some(tree) = parser.parse(&source, None) else {
        eprintln!("parse failed");
        std::process::exit(1);
    };
    use std::io::IsTerminal;
    let color = std::io::stdout().is_terminal();
    // 6 rainbow ANSI 256-color picks. Both the paren AND the node
    // kind name share the depth's color — that's the visual cue
    // for "this paren matches this kind." Field names + line
    // ranges get distinct colors so they don't blur into the
    // rainbow.
    const RAINBOW: [&str; 6] = [
        "\x1b[38;5;196m", // red
        "\x1b[38;5;208m", // orange
        "\x1b[38;5;226m", // yellow
        "\x1b[38;5;46m",  // green
        "\x1b[38;5;39m",  // blue
        "\x1b[38;5;165m", // magenta
    ];
    const FIELD: &str = "\x1b[38;5;245m"; // gray
    const RANGE: &str = "\x1b[38;5;242m"; // darker gray
    const RESET: &str = "\x1b[0m";
    fn walk(node: tree_sitter::Node, field: Option<&str>, depth: usize, color: bool) {
        let pad = "  ".repeat(depth);
        let (hue, fc, rc, rs) = if color {
            (RAINBOW[depth % 6], FIELD, RANGE, RESET)
        } else {
            ("", "", "", "")
        };
        let prefix = field
            .map(|f| format!("{}{}: {}", fc, f, rs))
            .unwrap_or_default();
        let s = node.start_position();
        let e = node.end_position();
        print!(
            "{}{}{}({}{} {}[{}, {}] - [{}, {}]{}",
            pad,
            prefix,
            hue,
            node.kind(),
            rs,
            rc,
            s.row,
            s.column,
            e.row,
            e.column,
            rs,
        );
        let mut cursor = node.walk();
        let mut field_idx: u32 = 0;
        for child in node.children(&mut cursor) {
            let fname = node.field_name_for_child(field_idx);
            field_idx += 1;
            if !child.is_named() {
                continue;
            }
            println!();
            walk(child, fname, depth + 1, color);
        }
        print!("{}){}", hue, rs);
    }
    walk(tree.root_node(), None, 0, color);
    println!();
}
