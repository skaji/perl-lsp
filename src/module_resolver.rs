//! Module resolver: background thread that resolves Perl modules from `@INC`.
//!
//! Discovers `@INC` paths, locates `.pm` files, parses them in-process with
//! tree-sitter-perl, and extracts export metadata for the module index.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tower_lsp::lsp_types::*;
use tower_lsp::lsp_types::{notification, request};
use tower_lsp::Client;
use tree_sitter::Parser;

use crate::cpanfile;
use crate::module_cache;
use crate::module_index::{CachedModule, ModuleEdgeIndexes, ResolveNotify, ResolveQueue, WorkspaceRootChannel};

/// Callback invoked after each module is resolved. Used to trigger diagnostic refresh.
pub type OnResolved = Box<dyn Fn() + Send + Sync>;

/// Spawn the resolver thread. Returns immediately; the thread runs in the background.
///
/// The `on_resolved` callback fires after each module is inserted into the cache,
/// allowing the backend to re-publish diagnostics.
pub fn spawn_resolver(
    cache: Arc<DashMap<String, Option<Arc<CachedModule>>>>,
    edges: Arc<ModuleEdgeIndexes>,
    stale_modules: Arc<DashMap<String, ()>>,
    available_modules: Arc<DashMap<String, PathBuf>>,
    builtins: Arc<DashMap<String, String>>,
    queue: Arc<ResolveQueue>,
    resolved: Arc<ResolveNotify>,
    workspace_root: Arc<WorkspaceRootChannel>,
    client: Client,
    on_resolved: OnResolved,
) {
    let handle = tokio::runtime::Handle::current();

    std::thread::Builder::new()
        .name("module-resolver".into())
        .spawn(move || {
            let mut inc_paths = discover_inc_paths();

            // Wait for workspace root from initialize() for per-project cache path.
            let ws_root = wait_for_workspace_root(&workspace_root);

            // Auto-discover project-local lib paths (lib/, local/lib/perl5/).
            if let Some(ref root_uri) = ws_root {
                if let Some(root_path) = uri_to_path(root_uri) {
                    add_project_lib_paths(&mut inc_paths, &root_path);
                }
            }

            // Scan @INC for available module names (fast, no parsing — just readdir)
            scan_inc_module_names(&inc_paths, &available_modules);
            log::info!("@INC scan: {} modules available", available_modules.len());

            // Warm the in-memory cache from SQLite.
            let db = module_cache::open_cache_db(ws_root.as_deref(), "perl");
            if let Some(ref conn) = db {
                let _ = module_cache::validate_inc_paths(conn, &inc_paths);
                let _ = module_cache::validate_plugin_fingerprint(
                    conn,
                    &crate::plugin::rhai_host::plugin_fingerprint(),
                );
                // Hydrate Perl builtin hover docs (cached in SQLite,
                // re-parsed from perlfunc.pod only when the perl
                // version tag changes).
                match module_cache::hydrate_builtins(conn) {
                    Ok(map) => {
                        for entry in map.iter() {
                            builtins.insert(entry.key().clone(), entry.value().clone());
                        }
                    }
                    Err(e) => log::warn!("Builtins hydrate failed: {}", e),
                }
                let (n, stale_names) = module_cache::warm_cache(conn, &cache);
                log::info!("Warmed module cache: {} entries loaded from disk, {} stale", n, stale_names.len());
                // Queue stale modules for priority re-resolution.
                for name in &stale_names {
                    stale_modules.insert(name.clone(), ());
                }
                if !stale_names.is_empty() {
                    let mut pq = queue.priority.lock().unwrap();
                    pq.extend(stale_names);
                    queue.condvar.notify_one();
                }
                // Build reverse index from warmed cache.
                rebuild_reverse_index(&cache, &edges);
            }

            // Track which extract version each module was resolved at.
            let mut seen: HashMap<String, i64> = HashMap::new();

            // One parser + one parent-fallback memo for the whole sweep.
            // Without the memo, every child whose own exports are empty re-parses
            // its parent (e.g. ~50× Exporter, ~30× URI on a cold cpanfile run).
            let mut parser = create_parser();
            let mut parse_memo: ParseMemo = HashMap::new();

            // Queue cpanfile dependencies (non-blocking — lets priority items go first).
            // Track total for progress reporting in the main loop.
            let mut cpanfile_total = 0usize;
            let mut cpanfile_done = 0usize;
            if let Some(ref root_uri) = ws_root {
                if let Some(root_path) = uri_to_path(root_uri) {
                    let cpanfile_modules = cpanfile::parse_cpanfile(&root_path);
                    let to_resolve: Vec<String> = cpanfile_modules
                        .into_iter()
                        .filter(|m| !cache.contains_key(m.as_str()))
                        .collect();

                    if !to_resolve.is_empty() {
                        cpanfile_total = to_resolve.len();
                        log::info!("cpanfile: {} modules queued for indexing", cpanfile_total);

                        // Start progress bar.
                        let token = NumberOrString::String("perl-lsp/indexing".to_string());
                        let _ = handle.block_on(client.send_request::<request::WorkDoneProgressCreate>(
                            WorkDoneProgressCreateParams { token: token.clone() },
                        ));
                        handle.block_on(client.send_notification::<notification::Progress>(
                            ProgressParams {
                                token,
                                value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(
                                    WorkDoneProgressBegin {
                                        title: "Indexing Perl modules".into(),
                                        cancellable: Some(false),
                                        message: None,
                                        percentage: Some(0),
                                    },
                                )),
                            },
                        ));

                        let mut pending = queue.pending.lock().unwrap();
                        pending.extend(to_resolve);
                        queue.condvar.notify_one();
                    }
                }
            }

            // Main resolve loop — drain priority first, then pending.
            loop {
                let batch = drain_next_batch(&queue);

                for module_name in batch {
                    // Allow re-resolution when extract version is outdated.
                    if let Some(&ver) = seen.get(&module_name) {
                        if ver >= module_cache::EXTRACT_VERSION {
                            continue;
                        }
                    }
                    seen.insert(module_name.clone(), module_cache::EXTRACT_VERSION);

                    let is_re_resolve = stale_modules.contains_key(&module_name);
                    if is_re_resolve {
                        log::info!("Re-resolving stale module '{}'", module_name);
                        // Stale entry must not be served from the run-local memo.
                        parse_memo.remove(&module_name);
                    } else {
                        log::info!("Resolving module '{}'", module_name);
                    }

                    let result = parse_module(&inc_paths, &module_name, &mut parser, &mut parse_memo);
                    match &result {
                        Some(m) => log::info!(
                            "Resolved '{}': {} export, {} export_ok",
                            module_name,
                            m.analysis.export.len(),
                            m.analysis.export_ok.len()
                        ),
                        None => log::info!("No exports found for '{}'", module_name),
                    }
                    insert_into_cache(&cache, &edges, &module_name, result.clone());

                    if let Some(ref conn) = db {
                        save_module_generation(conn, &module_name, &result);
                    }

                    // Descend into the module's own dependencies so the
                    // chain keeps resolving beyond the open doc's direct
                    // imports. Without this the cache stops at depth 1 —
                    // e.g. opening a Mojolicious::Lite script resolves
                    // Mojolicious.pm, but Mojolicious.pm's
                    // `has routes => sub { Mojolicious::Routes->new }`
                    // never triggers a resolve on Mojolicious::Routes,
                    // and `$r->get` on line 71 of the demo chain-dies
                    // because the intermediate class is a cache miss.
                    //
                    // The `seen` guard above makes this cycle-safe: a
                    // transitively-enqueued name that was already
                    // resolved at the current EXTRACT_VERSION gets
                    // skipped on its next turn.
                    if let Some(ref m) = result {
                        let mut pending = queue.pending.lock().unwrap();
                        let enqueue = |pending: &mut Vec<String>, name: String| {
                            if name.is_empty() { return; }
                            if cache.contains_key(&name) { return; }
                            if seen.contains_key(&name) { return; }
                            if !pending.iter().any(|p| p == &name) {
                                pending.push(name);
                            }
                        };
                        // Explicit imports — the module's own `use` statements.
                        for imp in &m.analysis.imports {
                            enqueue(&mut pending, imp.module_name.clone());
                        }
                        // Re-export edges — a re-exporting module (Test::Most →
                        // Test::More) pulls its producers' surfaces transitively,
                        // so those producers must be resolved even when no file
                        // `use`s them directly.
                        for re in &m.analysis.reexport_modules {
                            enqueue(&mut pending, re.clone());
                        }
                        // Parent classes — inheritance chain.
                        for parents in m.analysis.package_parents.values() {
                            for parent in parents {
                                enqueue(&mut pending, parent.clone());
                            }
                        }
                        // ClassName return types — `has foo => sub { Bar->new }`,
                        // plugin-emitted typed Subs, method return annotations.
                        // These are the chain-invisible-but-reachable classes
                        // the user's chain walks through at query time.
                        for sym in &m.analysis.symbols {
                            use crate::file_analysis::{InferredType, SymKind, SymbolDetail};
                            if !matches!(sym.kind, SymKind::Sub | SymKind::Method) { continue; }
                            if !matches!(sym.detail, SymbolDetail::Sub { .. }) { continue; }
                            if let Some(InferredType::ClassName(c)) =
                                m.analysis.symbol_return_type_via_bag(sym.id, None)
                            {
                                enqueue(&mut pending, c);
                            }
                        }
                        if !pending.is_empty() {
                            queue.condvar.notify_one();
                        }
                    }

                    // Remove from stale set after successful re-resolution.
                    if is_re_resolve {
                        stale_modules.remove(&module_name);
                    }

                    // Report cpanfile progress.
                    if cpanfile_total > 0 && cpanfile_done < cpanfile_total {
                        cpanfile_done += 1;
                        let pct = (cpanfile_done * 100 / cpanfile_total) as u32;
                        let token = NumberOrString::String("perl-lsp/indexing".to_string());
                        if cpanfile_done < cpanfile_total {
                            handle.block_on(client.send_notification::<notification::Progress>(
                                ProgressParams {
                                    token,
                                    value: ProgressParamsValue::WorkDone(WorkDoneProgress::Report(
                                        WorkDoneProgressReport {
                                            cancellable: Some(false),
                                            message: Some(format!("{} ({}/{})", module_name, cpanfile_done, cpanfile_total)),
                                            percentage: Some(pct),
                                        },
                                    )),
                                },
                            ));
                        } else {
                            handle.block_on(client.send_notification::<notification::Progress>(
                                ProgressParams {
                                    token,
                                    value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(
                                        WorkDoneProgressEnd {
                                            message: Some(format!("Indexed {} modules", cpanfile_total)),
                                        },
                                    )),
                                },
                            ));
                        }
                    }

                    // Signal waiters and trigger diagnostic refresh.
                    {
                        let _g = resolved.mu.lock().unwrap();
                        resolved.cv.notify_all();
                    }
                    on_resolved();
                }
            }
        })
        .expect("failed to spawn module-resolver thread");
}

/// Drain the next batch from the queue, checking priority first.
fn drain_next_batch(queue: &ResolveQueue) -> Vec<String> {
    // Check priority first
    {
        let mut priority = queue.priority.lock().unwrap();
        if !priority.is_empty() {
            return std::mem::take(&mut *priority);
        }
    }
    // Wait for pending
    let mut pending = queue.pending.lock().unwrap();
    loop {
        if !pending.is_empty() {
            // Before draining pending, re-check priority
            let mut priority = queue.priority.lock().unwrap();
            if !priority.is_empty() {
                return std::mem::take(&mut *priority);
            }
            return std::mem::take(&mut *pending);
        }
        pending = queue.condvar.wait(pending).unwrap();
    }
}

/// Headless resolver — no Client, no LSP progress. Same @INC scan,
/// project-local lib discovery, SQLite warm/persist, and index feeds
/// as the full resolver. Serves tests AND one-shot CLI sessions
/// (`ModuleIndex::new_for_cli`), which previously had NO resolver at
/// all and could only read what editor sessions had cached.
#[doc(hidden)]
pub fn spawn_test_resolver(
    cache: Arc<DashMap<String, Option<Arc<CachedModule>>>>,
    edges: Arc<ModuleEdgeIndexes>,
    stale_modules: Arc<DashMap<String, ()>>,
    available_modules: Arc<DashMap<String, PathBuf>>,
    queue: Arc<ResolveQueue>,
    resolved: Arc<ResolveNotify>,
    workspace_root: Arc<WorkspaceRootChannel>,
) {
    std::thread::Builder::new()
        .name("module-resolver-test".into())
        .spawn(move || {
            let mut inc_paths = discover_inc_paths();
            let ws_root = wait_for_workspace_root(&workspace_root);

            if let Some(ref root_uri) = ws_root {
                if let Some(root_path) = uri_to_path(root_uri) {
                    add_project_lib_paths(&mut inc_paths, &root_path);
                }
            }

            scan_inc_module_names(&inc_paths, &available_modules);

            let db = module_cache::open_cache_db(ws_root.as_deref(), "perl");
            if let Some(ref conn) = db {
                let _ = module_cache::validate_inc_paths(conn, &inc_paths);
                let _ = module_cache::validate_plugin_fingerprint(
                    conn,
                    &crate::plugin::rhai_host::plugin_fingerprint(),
                );
                let (_, stale_names) = module_cache::warm_cache(conn, &cache);
                for name in stale_names {
                    stale_modules.insert(name, ());
                }
                rebuild_reverse_index(&cache, &edges);
            }

            let mut seen: HashMap<String, i64> = HashMap::new();
            let mut parser = create_parser();
            let mut parse_memo: ParseMemo = HashMap::new();
            loop {
                let batch = drain_next_batch(&queue);
                for module_name in batch {
                    if let Some(&ver) = seen.get(&module_name) {
                        if ver >= module_cache::EXTRACT_VERSION {
                            continue;
                        }
                    }
                    seen.insert(module_name.clone(), module_cache::EXTRACT_VERSION);
                    if stale_modules.contains_key(&module_name) {
                        parse_memo.remove(&module_name);
                    }

                    let result = parse_module(&inc_paths, &module_name, &mut parser, &mut parse_memo);
                    insert_into_cache(&cache, &edges, &module_name, result.clone());
                    if let Some(ref conn) = db {
                        save_module_generation(conn, &module_name, &result);
                    }
                    stale_modules.remove(&module_name);
                    let _g = resolved.mu.lock().unwrap();
                    resolved.cv.notify_all();
                }
            }
        })
        .expect("failed to spawn test module-resolver thread");
}

// ---- Internal helpers ----

fn wait_for_workspace_root(ws_root_channel: &WorkspaceRootChannel) -> Option<String> {
    let mut guard = ws_root_channel.root.lock().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while guard.is_none() {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            log::warn!("Timed out waiting for workspace root; using global cache");
            break;
        }
        let (g, _) = ws_root_channel
            .condvar
            .wait_timeout(guard, remaining)
            .unwrap();
        guard = g;
    }
    guard.clone().flatten()
}

/// Insert a resolved module into the cache and update the edge indexes.
fn insert_into_cache(
    cache: &DashMap<String, Option<Arc<CachedModule>>>,
    edges: &ModuleEdgeIndexes,
    module_name: &str,
    result: Option<Arc<CachedModule>>,
) {
    if let Some(ref cached) = result {
        edges.feed(module_name, &cached.analysis);
    } else if matches!(cache.get(module_name).as_deref(), Some(Some(_))) {
        // On-demand @INC resolution missed this module (`None`), but the
        // workspace indexer already built it (e.g. a project module under a
        // relative `use lib` the resolver's @INC doesn't cover). Don't let
        // the miss clobber the indexed copy — and don't leave the reverse
        // index pointing at a module the cache no longer holds (the orphan
        // that broke cross-file Handler / dispatch lookup). Keep the Some.
        return;
    }
    cache.insert(module_name.to_string(), result);
}

/// Rebuild edge indexes from existing cache (e.g. after warming from
/// SQLite). The warm path writes blobs straight into the cache without
/// touching the indexes, so skipping this leaves every reverse lookup
/// blind on warm starts (cold/warm attribution, the B6 class).
fn rebuild_reverse_index(
    cache: &DashMap<String, Option<Arc<CachedModule>>>,
    edges: &ModuleEdgeIndexes,
) {
    edges.clear();
    for entry in cache.iter() {
        if let Some(ref cached) = *entry.value() {
            edges.feed(entry.key(), &cached.analysis);
        }
    }
}

// ---- Module parsing ----

/// Run-local memo for `resolve_and_parse_with_memo`. Persists across many
/// top-level calls within a single resolver sweep so that parent-fallback
/// recursion (e.g. 50 children all inheriting from `Exporter`) parses each
/// parent exactly once.
pub type ParseMemo = HashMap<String, Option<Arc<CachedModule>>>;

/// Parse a module file directly in-process.
/// tree-sitter-perl is stable — no subprocess isolation needed.
fn parse_module(
    inc_paths: &[PathBuf],
    module_name: &str,
    parser: &mut Parser,
    memo: &mut ParseMemo,
) -> Option<Arc<CachedModule>> {
    resolve_and_parse_with_memo(inc_paths, module_name, parser, memo)
}

pub use crate::builder::create_parser;

// ---- Resolution ----

pub fn resolve_module_path(inc_paths: &[PathBuf], module_name: &str) -> Option<PathBuf> {
    let rel_path = module_name.replace("::", "/") + ".pm";
    for inc in inc_paths {
        let full = inc.join(&rel_path);
        if full.is_file() {
            return Some(full);
        }
    }
    None
}

#[allow(dead_code)]
pub fn resolve_and_parse(
    inc_paths: &[PathBuf],
    module_name: &str,
    parser: &mut Parser,
) -> Option<Arc<CachedModule>> {
    let mut memo: ParseMemo = HashMap::new();
    resolve_and_parse_with_memo(inc_paths, module_name, parser, &mut memo)
}

/// Parse a module while sharing a memo across calls. Callers that resolve
/// many modules in a loop (the resolver thread, CLI startup) should hoist
/// one `ParseMemo` and reuse it so parent-fallback recursion doesn't
/// re-parse the same ancestor for each child.
pub fn resolve_and_parse_with_memo(
    inc_paths: &[PathBuf],
    module_name: &str,
    parser: &mut Parser,
    memo: &mut ParseMemo,
) -> Option<Arc<CachedModule>> {
    let mut visiting: std::collections::HashSet<String> = std::collections::HashSet::new();
    resolve_and_parse_inner(inc_paths, module_name, parser, &mut visiting, memo)
}

fn resolve_and_parse_inner(
    inc_paths: &[PathBuf],
    module_name: &str,
    parser: &mut Parser,
    visiting: &mut std::collections::HashSet<String>,
    memo: &mut ParseMemo,
) -> Option<Arc<CachedModule>> {
    if let Some(cached) = memo.get(module_name) {
        return cached.clone();
    }
    if !visiting.insert(module_name.to_string()) {
        // Cycle in `@ISA` parent fallback — bail rather than blow the stack.
        return None;
    }

    let bench = std::env::var_os("PERL_LSP_BENCH").is_some();
    let bench_start = if bench { Some(std::time::Instant::now()) } else { None };

    let path = resolve_module_path(inc_paths, module_name)?;
    let metadata = std::fs::metadata(&path).ok()?;
    if metadata.len() > 1_000_000 {
        if let Some(start) = bench_start {
            eprintln!("bench\t{}\t{}\toversize\t{}", module_name, start.elapsed().as_micros(), metadata.len());
        }
        return None;
    }
    let bytes = metadata.len();
    let source = std::fs::read_to_string(&path).ok()?;

    let timing = crate::timings::is_enabled();
    let t_parse = if timing { Some(std::time::Instant::now()) } else { None };
    let tree = parser.parse(&source, None)?;
    let parse_dur = t_parse.map(|s| s.elapsed()).unwrap_or_default();

    let t_build = if timing { Some(std::time::Instant::now()) } else { None };
    let mut analysis = crate::builder::build(&tree, source.as_bytes());
    let build_dur = t_build.map(|s| s.elapsed()).unwrap_or_default();
    crate::timings::record_built(module_name, parse_dur, build_dur);

    // If this module has no exports but inherits via @ISA (e.g. DDP → Data::Printer),
    // fall back to the first parent's exports. This only patches `export`/`export_ok`;
    // the parent's own cached analysis is still the source of truth for its symbols.
    if analysis.export.is_empty() && analysis.export_ok.is_empty() {
        let parents = crate::module_index::primary_package_parents(&analysis, module_name);
        for parent in &parents {
            if let Some(parent_cached) =
                resolve_and_parse_inner(inc_paths, parent, parser, visiting, memo)
            {
                if !parent_cached.analysis.export.is_empty()
                    || !parent_cached.analysis.export_ok.is_empty()
                {
                    analysis.export = parent_cached.analysis.export.clone();
                    analysis.export_ok = parent_cached.analysis.export_ok.clone();
                    break;
                }
            }
        }
    }

    let symbols = analysis.symbols.len();
    let result = Arc::new(CachedModule::new(path, Arc::new(analysis)));
    if let Some(start) = bench_start {
        eprintln!("bench\t{}\t{}\t{}\t{}", module_name, start.elapsed().as_micros(), symbols, bytes);
    }
    memo.insert(module_name.to_string(), Some(result.clone()));
    Some(result)
}

// ---- @INC discovery ----

pub fn discover_inc_paths() -> Vec<PathBuf> {
    let output = std::process::Command::new("perl")
        .args(["-e", r#"print join "\n", @INC"#])
        .stdin(std::process::Stdio::null())
        .output();

    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .collect(),
        _ => vec![],
    }
}

/// Add project-local lib paths (lib/, local/lib/perl5/) to the front of @INC.
/// Called by the resolver thread, test resolver, and CLI tools.
pub fn add_project_lib_paths(inc_paths: &mut Vec<PathBuf>, workspace_root: &std::path::Path) {
    for local_lib in &["lib", "local/lib/perl5"] {
        let p = workspace_root.join(local_lib);
        if p.is_dir() {
            log::info!("Auto-discovered project lib: {:?}", p);
            inc_paths.insert(0, p);
        }
    }
}


/// Variant that also registers each indexed file in the given
/// `ModuleIndex` under its primary package name. Used so workspace
/// modules participate in cross-file lookups (method resolution,
/// Handler walks, etc.) without waiting for an on-demand `use`
/// resolve. Without this, `->to('Users#list')` couldn't find
/// `test_files/lib/Users.pm` because nothing ever triggers a
/// module_index populate for workspace files.
/// Does this extensionless file start with a Perl shebang
/// (`#!...perl`)? The entrypoint-script test — `jobs`, `login`,
/// Mojo::Lite apps. Peeks 64 bytes; never called on extensioned files.
fn has_perl_shebang(path: &std::path::Path) -> bool {
    if path.extension().is_some() {
        return false;
    }
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let mut buf = [0u8; 64];
    let Ok(n) = f.read(&mut buf) else { return false };
    let head = String::from_utf8_lossy(&buf[..n]);
    let first = head.lines().next().unwrap_or("");
    first.starts_with("#!") && first.contains("perl")
}

/// Extensionless Perl entrypoint scripts, found by a SHALLOW (depth-1)
/// shebang scan over the conventional dirs — repo root, `bin/`,
/// `script/` — plus any `extra` dirs (relative to `root`). Shallow +
/// dir-scoped on purpose: entrypoints are direct files in known
/// places, so this never walks a source tree.
///
/// `extra` is the SEAM for a future workspace-config `entrypoint_dirs`
/// knob: today every caller passes `&[]`; wiring config is one line at
/// the call site, no change here. (The config-file reader itself is
/// deliberately deferred until there's a real config story to design.)
fn scan_entrypoint_scripts(root: &std::path::Path, extra: &[String]) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        vec![root.to_path_buf(), root.join("bin"), root.join("script")];
    dirs.extend(extra.iter().map(|d| root.join(d)));
    let mut out = Vec::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            if std::fs::metadata(&p).map(|m| m.len() < 1_000_000).unwrap_or(false)
                && has_perl_shebang(&p)
            {
                out.push(p);
            }
        }
    }
    out
}

pub fn index_workspace_with_index(
    root: &std::path::Path,
    files: &crate::file_store::FileStore,
    module_index: Option<&crate::module_index::ModuleIndex>,
    // Per-file progress tick (done, total), called from the Rayon workers as
    // files complete. LSP-agnostic: the caller owns any notification / throttle
    // policy. Invoked once per path processed (success OR skip), so `done`
    // reaches `total` at the end.
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
) -> usize {
    use ignore::types::TypesBuilder;
    use ignore::WalkBuilder;
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Extensioned Perl (`*.pm/*.pl/*.t`) — type-pruned at the walk
    // level (cheap; never descends into a JS tree's files).
    let mut types_builder = TypesBuilder::new();
    types_builder.add("perl", "*.pm").unwrap();
    types_builder.add("perl", "*.pl").unwrap();
    types_builder.add("perl", "*.t").unwrap();
    types_builder.select("perl");
    let types = types_builder.build().unwrap();

    let mut paths: Vec<PathBuf> = WalkBuilder::new(root)
        .types(types)
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter(|e| e.metadata().map(|m| m.len() < 1_000_000).unwrap_or(false))
        .map(|e| e.into_path())
        .collect();

    // Extensionless entrypoint SCRIPTS (`#!/usr/bin/env perl` — crm's
    // `jobs`/`login`/… Mojo::Lite apps) carry no glob, so a SHALLOW
    // shebang scan over the conventional entrypoint dirs catches them
    // without enumerating the whole tree. These scripts are exactly
    // where `plugin 'X'` loads live; skipping them blinded the
    // entrypoint-scan lint and goto-def into entrypoint-defined symbols.
    // `&[]` today; the seam for a future workspace-config
    // `entrypoint_dirs` (additive to the built-in root/bin/script).
    paths.extend(scan_entrypoint_scripts(root, &[]));

    let count = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let total = paths.len();

    // Perl workspace persistence (`docs/adr/relational-ref-index.md`,
    // phase 3): blobs + ref rows land in `modules.db` under
    // `source='workspace'` (path-keyed, like the pack tier), warm starts
    // skip re-parsing unchanged files, and — once persisted — the resident
    // copies are refs/bag-stripped like every other index tier. The cache
    // key is the hub's workspace-root spelling, the SAME one the resolver
    // thread and the hub's readers hash, so all three address one DB.
    let cache_key = module_index.and_then(|i| i.workspace_root());
    let conn = module_cache::open_cache_db(cache_key.as_deref(), "perl");
    // Validate-and-stamp the plugin fingerprint BEFORE writing: the resolver
    // thread runs the same (atomic) check concurrently on this DB, and an
    // unstamped fresh DB reads as a mismatch there — it would hard-clear the
    // rows this indexer is about to write.
    if let Some(ref conn) = conn {
        let _ = module_cache::validate_plugin_fingerprint(
            conn,
            &crate::plugin::rhai_host::plugin_fingerprint(),
        );
    }
    // Persistence and eviction are independent: blobs + rows are written
    // whenever a DB exists (the parity harness runs under PERL_LSP_NO_EVICT
    // and still needs the relational side populated); only the resident
    // STRIP obeys the eviction switch.
    let persist = conn.is_some();
    let strip = persist && eviction_enabled();

    // The walk's canonical membership set: warm rows are admitted only for
    // files the CURRENT walk still includes — a path newly gitignored (or
    // newly over the size cap) must not resurrect from its cached row, and
    // its stale generation is dropped.
    let canon_members: std::collections::HashSet<PathBuf> = paths
        .iter()
        .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
        .collect();

    // WARM: stream valid 'workspace' rows — record projections from the
    // full decode, strip, register, drop. Stale/changed rows fall through
    // to the parallel re-parse below.
    let mut warmed: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    if let Some(ref conn) = conn {
        let mut dead_rows: Vec<PathBuf> = Vec::new();
        // Backfill shreds are DEFERRED past the warm scan: writing inside
        // the streaming SELECT's transaction pins a read snapshot that a
        // concurrent resolver-thread commit turns into SQLITE_BUSY_SNAPSHOT
        // (not retried by the busy handler) — silently voiding the whole
        // backfill. Row-less files stay WHOLE this session (refs resident);
        // their rows land below for the next one.
        let mut pending_backfill: Vec<(PathBuf, Vec<crate::file_analysis::RefRowSeed>)> =
            Vec::new();
        let (_n, _stale) =
            module_cache::warm_cache_streaming(conn, Some("workspace"), &mut |_name, path, mut fa| {
                if !canon_members.contains(&path) {
                    dead_rows.push(path);
                    return;
                }
                let path_str = path.to_string_lossy();
                // Refs strip ONLY when their rows are known present — an
                // evicted copy without rows is invisible to the backward
                // walk (rows name candidates; the blob rehydrates).
                let rows_ok = module_cache::has_ref_rows(conn, &path_str);
                if !rows_ok {
                    pending_backfill
                        .push((path.clone(), fa.refs.iter().map(|r| r.row_seed()).collect()));
                }
                if let Some(idx) = module_index {
                    idx.record_workspace_projections(&path, &fa);
                }
                if eviction_enabled() {
                    fa.evict_witness_bag();
                    if rows_ok {
                        fa.evict_refs();
                    }
                }
                let arc = std::sync::Arc::new(fa);
                files.insert_workspace_arc(path.clone(), arc.clone());
                if let Some(idx) = module_index {
                    idx.register_workspace_resident(path.clone(), arc);
                }
                count.fetch_add(1, Ordering::Relaxed);
                warmed.insert(path);
            });
        for chunk in pending_backfill.chunks(128) {
            if conn.execute_batch("BEGIN IMMEDIATE").is_err() {
                log::error!("Workspace backfill txn open failed; rows defer to next warm");
                break;
            }
            for (path, seeds) in chunk {
                if let Err(e) = module_cache::shred_ref_rows(
                    conn,
                    &path.to_string_lossy(),
                    "workspace",
                    seeds,
                ) {
                    log::warn!("Failed to backfill ref rows for {:?}: {}", path, e);
                }
            }
            if let Err(e) = conn.execute_batch("COMMIT") {
                log::error!("Workspace backfill commit failed: {}", e);
                let _ = conn.execute_batch("ROLLBACK");
            }
        }
        for path in dead_rows {
            module_cache::invalidate_generation_tier(
                conn,
                &path.to_string_lossy(),
                "workspace",
            );
        }
    }

    // Fresh entries stream to a dedicated writer over a channel: the writer
    // persists (batched txns) WHILE workers parse, so only a small window of
    // blobs+seeds is ever in flight (never the whole tree's), and a query
    // racing the bulk index sees each file's rows as soon as its chunk
    // commits. Parse-time stamps ride along so a mid-index edit invalidates
    // the row by construction.
    type WsFresh = (
        PathBuf,
        Vec<u8>,
        Vec<crate::file_analysis::RefRowSeed>,
        Vec<std::sync::Arc<str>>,
        (i64, i64),
    );
    let (fresh_tx, fresh_rx) = std::sync::mpsc::channel::<WsFresh>();
    let timing = crate::timings::is_enabled();

    // The Connection moves INTO the writer thread (rusqlite connections are
    // Send, not Sync); nothing after the scope needs it.
    let writer_conn = conn;
    std::thread::scope(|scope| {
        let writer = scope.spawn(move || {
            let Some(conn) = writer_conn.as_ref() else {
                while fresh_rx.recv().is_ok() {}
                return;
            };
            let mut batch: Vec<WsFresh> = Vec::new();
            let mut write_chunk = |batch: &mut Vec<WsFresh>| {
                if batch.is_empty() {
                    return;
                }
                // IMMEDIATE: take the write lock up front — a deferred txn
                // that reads before writing can hit an unretryable
                // SQLITE_BUSY_SNAPSHOT against the resolver thread's writes.
                let txn_open = conn.execute_batch("BEGIN IMMEDIATE").is_ok();
                for (path, blob, seeds, closure, stamp) in batch.iter() {
                    let path_str = path.to_string_lossy();
                    module_cache::save_blob_to_db_stamped(
                        conn, &path_str, path, closure, blob, "workspace", *stamp,
                    );
                    if let Err(e) =
                        module_cache::shred_ref_rows(conn, &path_str, "workspace", seeds)
                    {
                        log::warn!("Failed to shred ref rows for {:?}: {}", path, e);
                    }
                }
                if txn_open {
                    if let Err(e) = conn.execute_batch("COMMIT") {
                        let _ = conn.execute_batch("ROLLBACK");
                        // The chunk rolled back but resident copies were
                        // already stripped. The blob in hand IS the whole
                        // analysis — un-strip by re-registering full copies,
                        // so nothing is lost beyond the persistence itself
                        // (disk full / lock storm stays loud AND self-heals).
                        log::error!(
                            "Workspace persist commit failed ({} files, re-registering whole copies): {}",
                            batch.len(),
                            e
                        );
                        for (path, blob, ..) in batch.iter() {
                            if let Some(fa) = module_cache::decode_analysis(blob) {
                                let arc = std::sync::Arc::new(fa);
                                files.insert_workspace_arc(path.clone(), arc.clone());
                                if let Some(idx) = module_index {
                                    idx.register_workspace_resident(path.clone(), arc);
                                }
                            }
                        }
                    }
                }
                // A racing query may have pinned the previous generation in
                // the rehydration LRU while this chunk was in flight.
                if let Some(idx) = module_index {
                    for (path, ..) in batch.iter() {
                        idx.invalidate_bag_cache(path);
                    }
                }
                batch.clear();
            };
            // A panic anywhere in a chunk must not kill the writer (workers
            // keep stripping copies whose sends would silently fail) — treat
            // it like a failed commit: re-register whole copies and go on.
            let mut safe_chunk = |batch: &mut Vec<WsFresh>| {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    write_chunk(batch)
                }));
                if r.is_err() {
                    log::error!(
                        "workspace writer chunk panicked ({} files) — re-registering whole copies",
                        batch.len()
                    );
                    for (path, blob, ..) in batch.iter() {
                        if let Some(fa) = module_cache::decode_analysis(blob) {
                            let arc = std::sync::Arc::new(fa);
                            files.insert_workspace_arc(path.clone(), arc.clone());
                            if let Some(idx) = module_index {
                                idx.register_workspace_resident(path.clone(), arc);
                            }
                        }
                    }
                    batch.clear();
                }
            };
            while let Ok(entry) = fresh_rx.recv() {
                batch.push(entry);
                while batch.len() < 128 {
                    match fresh_rx.try_recv() {
                        Ok(e) => batch.push(e),
                        Err(_) => break,
                    }
                }
                safe_chunk(&mut batch);
            }
            safe_chunk(&mut batch);
        });

        paths.par_iter().for_each(|path| {
            // Blobs are keyed canonical (matches the warm rows + the CLI's
            // canonicalized origin staging); register under the same spelling
            // so cold and warm runs key the stores identically.
            let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
            if warmed.contains(&canon) {
                if let Some(cb) = progress {
                    let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                    cb(d, total);
                }
                return;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Stamp BEFORE reading: stamp-after-read can bless a stale
                // parse with a fresh mtime when the file changes in between.
                let stamp = module_cache::file_stamp(path).unwrap_or((0, 0));
                let source = std::fs::read_to_string(path).ok()?;
                let mut parser = create_parser();
                let t_parse = if timing { Some(std::time::Instant::now()) } else { None };
                let tree = parser.parse(&source, None)?;
                let parse_dur = t_parse.map(|s| s.elapsed()).unwrap_or_default();
                let t_build = if timing { Some(std::time::Instant::now()) } else { None };
                let analysis = crate::builder::build(&tree, source.as_bytes());
                let build_dur = t_build.map(|s| s.elapsed()).unwrap_or_default();
                if timing {
                    crate::timings::record_built(
                        path.strip_prefix(root).unwrap_or(path).display().to_string(),
                        parse_dur,
                        build_dur,
                    );
                }
                Some((analysis, stamp))
            }));

            match result {
                Ok(Some((mut analysis, stamp))) => {
                    // The file changed while we parsed: the watcher (or next
                    // warm) owns the fresher truth — registering this copy
                    // would overwrite it and re-persist a generation the
                    // watcher may have just invalidated.
                    if module_cache::file_stamp(path) != Some(stamp) {
                        if let Some(cb) = progress {
                            let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                            cb(d, total);
                        }
                        return;
                    }
                    // Projections that read the bag run on the whole
                    // analysis; the persisted generation is encoded whole;
                    // only then is the resident copy stripped.
                    if let Some(idx) = module_index {
                        idx.record_workspace_projections(&canon, &analysis);
                    }
                    if persist && !analysis.degraded {
                        if let Some(blob) = module_cache::encode_analysis(&analysis) {
                            let seeds: Vec<_> =
                                analysis.refs.iter().map(|r| r.row_seed()).collect();
                            let closure = analysis.include_closure.clone();
                            let _ =
                                fresh_tx.send((canon.clone(), blob, seeds, closure, stamp));
                            if strip {
                                analysis.evict_witness_bag();
                                analysis.evict_refs();
                            }
                        }
                    }
                    let arc = std::sync::Arc::new(analysis);
                    files.insert_workspace_arc(canon.clone(), arc.clone());
                    if let Some(idx) = module_index {
                        idx.register_workspace_resident(canon.clone(), arc);
                    }
                    count.fetch_add(1, Ordering::Relaxed);
                }
                Ok(None) => { /* parse failed, skip */ }
                Err(_) => {
                    log::warn!("Panic while indexing {:?}, skipping", path);
                }
            }
            if let Some(cb) = progress {
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                cb(d, total);
            }
        });

        drop(fresh_tx);
        let _ = writer.join();
    });

    count.load(Ordering::Relaxed)
}

/// Index pack-language files (C++/Python/…) into per-language sub-indexes
/// attached to `hub`. GENERIC: registry-driven, so every served pack
/// language gets cross-file from this one walk. Each language keeps its
/// OWN `ModuleIndex` (separate cache — names never comingle across
/// languages), files registered by CLASS name. PERSISTED to a separate
/// `modules-{lang}.db`: warm valid analyses from disk (mtime/size +
/// EXTRACT_VERSION validated), re-analyze only new/changed/stale files,
/// and write the fresh ones back — so a big monorepo doesn't re-analyze
/// every header each launch. `cache_key` is the workspace root the cache
/// dir hashes on (`None` ⇒ no persistence, e.g. tests).
/// Slice-2 eviction off-switch: `PERL_LSP_NO_EVICT` keeps every resident pack
/// bag in memory (the pre-Slice-2 footprint) — an emergency knob and the A/B
/// lever for isolating an eviction-caused regression.
fn eviction_enabled() -> bool {
    std::env::var_os("PERL_LSP_NO_EVICT").is_none()
}

/// Persist one module's generation: blob + its relational ref rows, always
/// together (`docs/adr/relational-ref-index.md` — rows and blob describe the
/// same analysis or neither exists). `save_to_db` skips degraded analyses;
/// mirror that here so no rows exist for an unpersisted blob.
fn save_module_generation(
    conn: &rusqlite::Connection,
    module_name: &str,
    result: &Option<Arc<CachedModule>>,
) {
    module_cache::save_to_db(conn, module_name, result, "import");
    if let Some(m) = result {
        if !m.analysis.degraded {
            let seeds: Vec<_> = m.analysis.refs.iter().map(|r| r.row_seed()).collect();
            if let Err(e) =
                module_cache::shred_ref_rows(conn, &m.path.to_string_lossy(), "import", &seeds)
            {
                log::warn!("Failed to shred ref rows for '{}': {}", module_name, e);
            }
        }
    }
}

pub fn index_pack_languages(
    root: &std::path::Path,
    cache_key: Option<&str>,
    hub: &crate::module_index::ModuleIndex,
    // Per-file progress tick (done, grand_total) across ALL pack languages, so
    // the single pack token's percentage is monotone. Called once per path
    // (warm-skip OR analyzed) — `done` reaches the grand total at the end.
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
    // Slice-2 rehydration LRU byte cap (`maxCacheMb * 1 MiB`). The resident
    // pack analyses are bag-stripped after indexing; a type query into an
    // evicted file rehydrates its exact bag from SQLite into this cap. `0`
    // disables retention (rehydrate-and-drop). See `docs/adr/memory-slice-2-lru.md`.
    bag_cache_bytes: usize,
) -> usize {
    use ignore::types::TypesBuilder;
    use ignore::WalkBuilder;
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // Persist the transitive macro table across sessions (kills the
    // cold-start gather over perl.h's closure) — pointed at this workspace's
    // cache dir.
    crate::cpp_reparse::set_macro_persist_dir(module_cache::cache_dir_for_workspace(cache_key));

    let reg = crate::language_driver::LanguageRegistry::with_enabled();

    // Collect every language's paths UP FRONT so the grand total (the progress
    // denominator) is known before any file is analyzed — a single monotone
    // 0→100% stream across all pack languages on the one shared token.
    let mut lang_paths: Vec<(&'static str, Vec<PathBuf>)> = Vec::new();
    for lang in reg.languages() {
        if lang == "perl" {
            continue;
        }
        let exts: Vec<&'static str> = reg
            .for_id(lang)
            .map(|d| d.extensions().to_vec())
            .unwrap_or_default();
        if exts.is_empty() {
            continue;
        }
        let mut tb = TypesBuilder::new();
        for ext in &exts {
            let _ = tb.add(lang, &format!("*.{ext}"));
        }
        let _ = tb.select(lang);
        let Ok(types) = tb.build() else { continue };
        let paths: Vec<PathBuf> = WalkBuilder::new(root)
            .types(types)
            .build()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .filter(|e| e.metadata().map(|m| m.len() < 2_000_000).unwrap_or(false))
            .map(|e| e.into_path())
            .collect();
        if paths.is_empty() {
            continue;
        }
        lang_paths.push((lang, paths));
    }
    let grand_total: usize = lang_paths.iter().map(|(_, p)| p.len()).sum();

    let total = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    for (lang, paths) in lang_paths {
        // Slice-2 bag-rehydration LRU: a loader that opens THIS lang's SQLite
        // conn on demand (rusqlite `Connection` isn't `Sync`, so we open per
        // rehydration miss — rare, and SQLite handles concurrent readers) and
        // decodes the one requested file's full bag.
        let bag_cache = {
            let cache_key_owned = cache_key.map(|s| s.to_string());
            let loader = move |path: &std::path::Path| -> Option<crate::file_analysis::FileAnalysis> {
                let conn = module_cache::open_cache_db_readonly(cache_key_owned.as_deref(), lang)?;
                // The blob is persisted under the CANONICAL path (both feed
                // paths write `canon`), while the resident copy may be
                // registered under the walk's raw path — canonicalize so the
                // keyed decode matches regardless of which form the caller holds.
                let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
                module_cache::load_one(&conn, &canon.to_string_lossy())
                    .or_else(|| module_cache::load_one(&conn, &path.to_string_lossy()))
            };
            Arc::new(crate::pack_bag_cache::PackBagCache::new(bag_cache_bytes, loader))
        };
        let pack_index = Arc::new(
            crate::module_index::ModuleIndex::new_for_cli().with_bag_cache(bag_cache),
        );
        // This sub-index's relational-ref-index reader — same per-language DB
        // the drain below writes blobs + rows into.
        {
            let cache_key_owned = cache_key.map(|s| s.to_string());
            pack_index.set_ref_rows_opener(Arc::new(move || {
                module_cache::open_cache_db_readonly(cache_key_owned.as_deref(), lang)
            }));
        }
        let conn = module_cache::open_cache_db(cache_key, lang);
        // A generation built under different analysis inputs (toolchain
        // change — or its probe FAILURE, which empties the system include
        // roots) must not be warmed: hard-clear, same as `validate_inc_paths`.
        if let (Some(ref conn), Some(driver)) = (&conn, reg.for_id(lang)) {
            let _ = module_cache::validate_input_fingerprint(
                conn,
                driver.analysis_input_fingerprint(),
            );
        }

        // WARM: stream valid cached analyses (keyed by file path) one row
        // at a time — register a stripped copy, drop the whole decode before
        // the next row, so at most one full analysis is transiently
        // resident. Version-stale rows re-analyze; rows for files the
        // CURRENT walk no longer includes are dropped, not resurrected.
        let canon_members: std::collections::HashSet<PathBuf> = paths
            .iter()
            .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
            .collect();
        let mut warmed: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        if let Some(ref conn) = conn {
            let mut dead_rows: Vec<PathBuf> = Vec::new();
            // Deferred past the warm scan — same SQLITE_BUSY_SNAPSHOT
            // rationale as the workspace indexer's backfill.
            let mut pending_backfill: Vec<(PathBuf, Vec<crate::file_analysis::RefRowSeed>)> =
                Vec::new();
            let (_n, _stale) = module_cache::warm_cache_streaming(conn, None, &mut |_name, path, mut fa| {
                if !canon_members.contains(&path) {
                    dead_rows.push(path);
                    return;
                }
                let path_str = path.to_string_lossy();
                // Refs strip only when their rows are known present — rows
                // name candidates for the backward walk; the blob rehydrates.
                let rows_ok = module_cache::has_ref_rows(conn, &path_str);
                if !rows_ok {
                    pending_backfill
                        .push((path.clone(), fa.refs.iter().map(|r| r.row_seed()).collect()));
                }
                if eviction_enabled() {
                    fa.evict_witness_bag();
                    if rows_ok {
                        fa.evict_refs();
                    }
                }
                pack_index.register_symbols(path.clone(), Arc::new(fa));
                warmed.insert(path);
            });
            for chunk in pending_backfill.chunks(128) {
                if conn.execute_batch("BEGIN IMMEDIATE").is_err() {
                    log::error!("Pack backfill txn open failed; rows defer to next warm");
                    break;
                }
                for (path, seeds) in chunk {
                    if let Err(e) = module_cache::shred_ref_rows(
                        conn,
                        &path.to_string_lossy(),
                        "workspace",
                        seeds,
                    ) {
                        log::warn!("Failed to backfill ref rows for {:?}: {}", path, e);
                    }
                }
                if let Err(e) = conn.execute_batch("COMMIT") {
                    log::error!("Pack backfill commit failed: {}", e);
                    let _ = conn.execute_batch("ROLLBACK");
                }
            }
            for path in dead_rows {
                module_cache::invalidate_generation_tier(
                    conn,
                    &path.to_string_lossy(),
                    "workspace",
                );
            }
        }

        // Analyze only the new/changed/stale files (parallel). Fresh entries
        // stream to a dedicated writer thread over a channel: blobs + rows
        // land in batched txns WHILE workers analyze, so only a bounded
        // window of encoded blobs is in flight and a query racing the bulk
        // index sees each file's rows as soon as its chunk commits.
        // Persistence and eviction are independent: blobs + rows are written
        // whenever a DB exists; only the resident STRIP obeys the eviction
        // switch (the bag/refs are stripped only when recoverable — persisted
        // and non-degraded).
        type FreshEntry = (
            PathBuf,
            Arc<crate::file_analysis::FileAnalysis>,
            Vec<u8>,
            Vec<crate::file_analysis::RefRowSeed>,
            (i64, i64),
        );
        let (fresh_tx, fresh_rx) = std::sync::mpsc::channel::<FreshEntry>();
        let persist = conn.is_some();
        let strip = persist && eviction_enabled();
        let writer_conn = conn;
        let pack_index_writer = Arc::clone(&pack_index);
        std::thread::scope(|scope| {
            let writer = scope.spawn(move || {
                let Some(conn) = writer_conn.as_ref() else {
                    while fresh_rx.recv().is_ok() {}
                    return;
                };
                let mut batch: Vec<FreshEntry> = Vec::new();
                let mut write_chunk = |batch: &mut Vec<FreshEntry>| {
                    if batch.is_empty() {
                        return;
                    }
                    // IMMEDIATE — same snapshot rationale as the workspace writer.
                    let txn_open = conn.execute_batch("BEGIN IMMEDIATE").is_ok();
                    for (path, arc, blob, seeds, stamp) in batch.iter() {
                        let path_str = path.to_string_lossy();
                        module_cache::save_blob_to_db_stamped(
                            conn,
                            &path_str,
                            path,
                            &arc.include_closure,
                            blob,
                            "workspace",
                            *stamp,
                        );
                        if let Err(e) =
                            module_cache::shred_ref_rows(conn, &path_str, "workspace", seeds)
                        {
                            log::warn!("Failed to shred ref rows for {:?}: {}", path, e);
                        }
                    }
                    if txn_open {
                        if let Err(e) = conn.execute_batch("COMMIT") {
                            let _ = conn.execute_batch("ROLLBACK");
                            log::error!(
                                "Pack persist commit failed ({} files, re-registering whole copies): {}",
                                batch.len(),
                                e
                            );
                            for (path, _arc, blob, ..) in batch.iter() {
                                if let Some(fa) = module_cache::decode_analysis(blob) {
                                    pack_index_writer
                                        .register_symbols(path.clone(), Arc::new(fa));
                                }
                            }
                        }
                    }
                    // A racing query may have pinned the previous generation
                    // in the rehydration LRU while this chunk was in flight.
                    for (path, ..) in batch.iter() {
                        pack_index_writer.invalidate_bag_cache(path);
                    }
                    batch.clear();
                };
                let mut safe_chunk = |batch: &mut Vec<FreshEntry>| {
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        write_chunk(batch)
                    }));
                    if r.is_err() {
                        log::error!(
                            "pack writer chunk panicked ({} files) — re-registering whole copies",
                            batch.len()
                        );
                        for (path, _arc, blob, ..) in batch.iter() {
                            if let Some(fa) = module_cache::decode_analysis(blob) {
                                pack_index_writer.register_symbols(path.clone(), Arc::new(fa));
                            }
                        }
                        batch.clear();
                    }
                };
                while let Ok(entry) = fresh_rx.recv() {
                    batch.push(entry);
                    while batch.len() < 128 {
                        match fresh_rx.try_recv() {
                            Ok(e) => batch.push(e),
                            Err(_) => break,
                        }
                    }
                    safe_chunk(&mut batch);
                }
                safe_chunk(&mut batch);
            });

            paths.par_iter().for_each(|path| {
                // Tick before any early-out so warm-cache skips also advance the
                // bar — `done` must reach `grand_total`.
                if let Some(cb) = progress {
                    let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                    cb(d, grand_total);
                }
                let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
                if warmed.contains(&canon) {
                    return; // valid cache hit
                }
                let reg = crate::language_driver::LanguageRegistry::with_enabled();
                let Some(driver) = reg.for_path(path).filter(|d| d.id() == lang) else { return };
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let stamp = module_cache::file_stamp(path).unwrap_or((0, 0));
                    let source = std::fs::read_to_string(path).ok()?;
                    Some((driver.analyze_with_path(&source, Some(path)), stamp))
                }));
                if let Ok(Some((mut analysis, stamp))) = res {
                    // Same changed-under-us guard as the workspace worker.
                    if module_cache::file_stamp(path) != Some(stamp) {
                        return;
                    }
                    // Encode the FULL analysis for the disk write, then strip
                    // the resident copy — one struct, no clone
                    // (`docs/adr/memory-slice-2-lru.md`). Strip only when the
                    // bag/refs are recoverable: persisted and non-degraded
                    // (`save_*` skip degraded rows, so their bag would be lost).
                    let payload = if persist && !analysis.degraded {
                        module_cache::encode_analysis(&analysis).map(|blob| {
                            let seeds: Vec<_> =
                                analysis.refs.iter().map(|r| r.row_seed()).collect();
                            (blob, seeds)
                        })
                    } else {
                        None
                    };
                    if payload.is_some() && strip {
                        analysis.evict_witness_bag();
                        analysis.evict_refs();
                    }
                    let arc = Arc::new(analysis);
                    pack_index.register_symbols(path.clone(), arc.clone());
                    if let Some((blob, seeds)) = payload {
                        let _ = fresh_tx.send((canon.clone(), arc, blob, seeds, stamp));
                    }
                    total.fetch_add(1, Ordering::Relaxed);
                    // Residency: this file's merged/expanded macro tables are a
                    // one-shot build input, now dead weight for the rest of the
                    // bulk index (they'd otherwise accumulate to ~1.6 GB of
                    // per-file duplicates on abseil). Drop them the moment the
                    // analysis is built; the shared `header_cache` stays warm so
                    // an on-edit re-gather is a header-BFS, not a cold gather.
                    // Keyed by the same path analyze got, plus its canonical form.
                    let mut drop_set = std::collections::HashSet::with_capacity(2);
                    drop_set.insert(path.clone());
                    drop_set.insert(canon);
                    crate::cpp_reparse::evict_gather_caches_keep_headers(&drop_set);
                }
            });

            drop(fresh_tx);
            let _ = writer.join();
        });
        hub.attach_pack_index(lang, pack_index);
    }
    if std::env::var_os("PERL_LSP_MEM_REPORT").is_some() {
        eprintln!("[mem-report] {}", crate::cpp_reparse::cache_size_report());
    }
    // Heap-composition of the resident pack `FileAnalysis` set — the Slice-2
    // eviction target (`docs/adr/memory-slice-2-lru.md`). Env-gated, inert by
    // default, no query-path cost.
    if std::env::var_os("PERL_LSP_HEAP_DUMP").is_some() {
        let mut agg = crate::file_analysis::HeapBreakdown::default();
        hub.for_each_pack_registered_file(&mut |_path, fa| agg.add(&fa.heap_estimate()));
        eprintln!("[heap-dump] {agg}");
    }
    total.load(Ordering::Relaxed)
}

/// In-session invalidation for a changed (saved/watched) or deleted pack
/// file — the H1 seam. The include closure is the cross-file visibility
/// key, so it is also the REVERSE-dependency key: a consumer is any
/// registered file whose `include_closure` contains the changed path.
/// Order matters: evict the per-file analysis caches FIRST (macro tables,
/// pre-expanded variants, closures) so the re-analyses here — and the
/// open documents' background refresh after — re-gather instead of
/// serving the frozen tables. Blocking (Rayon inside); callers run it
/// off the message loop.
pub fn pack_file_changed(
    root_uri: Option<&str>,
    hub: &crate::module_index::ModuleIndex,
    path: &std::path::Path,
    deleted: bool,
) {
    use rayon::prelude::*;
    use std::sync::Arc;
    let reg = crate::language_driver::LanguageRegistry::with_enabled();
    let Some(driver) = reg.for_path(path) else { return };
    if driver.id() == "perl" {
        return;
    }
    let lang = driver.id();
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let canon_str = canon.to_string_lossy().into_owned();
    let pack = hub.pack_index(lang);

    let mut consumers: Vec<PathBuf> = Vec::new();
    if let Some(ref pack) = pack {
        pack.for_each_registered_file(&mut |cm| {
            if cm.analysis.include_closure.iter().any(|c| c.as_ref() == canon_str) {
                consumers.push(cm.path.clone());
            }
        });
    }

    let mut evict: std::collections::HashSet<PathBuf> = consumers.iter().cloned().collect();
    evict.insert(canon.clone());
    crate::cpp_reparse::evict_analysis_caches(&evict);

    if deleted {
        if let Some(ref pack) = pack {
            pack.unregister_file(&canon);
        }
    }

    // Re-analyze the changed file (unless deleted) + every consumer
    // (parallel), then swap registrations. Unregister-then-register so names
    // the new version no longer defines don't linger in `all_defs` / the
    // cache winner slot. Consumers re-analyze on delete too — their splices
    // and closures baked the departed header.
    let mut targets: Vec<PathBuf> = Vec::with_capacity(consumers.len() + 1);
    if !deleted {
        targets.push(canon);
    }
    targets.extend(consumers);
    targets.sort();
    targets.dedup();
    let results: Vec<(PathBuf, Arc<crate::file_analysis::FileAnalysis>)> = targets
        .par_iter()
        .filter_map(|p| {
            let reg = crate::language_driver::LanguageRegistry::with_enabled();
            let driver = reg.for_path(p).filter(|d| d.id() == lang)?;
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let source = std::fs::read_to_string(p).ok()?;
                Some(driver.analyze_with_path(&source, Some(p)))
            }));
            match res {
                Ok(Some(analysis)) => Some((p.clone(), Arc::new(analysis))),
                _ => None,
            }
        })
        .collect();
    // Persist the FULL analyses (bag present) FIRST so the on-disk blob can
    // rehydrate, then register bag-STRIPPED resident copies and drop each
    // file's now-stale entry from the rehydration LRU (change #6). `results`
    // holds the full arcs; `save_to_db` encodes them whole. Strip only when we
    // actually persisted — else the bag would be unrecoverable, so keep it.
    let persisted = if let Some(conn) = module_cache::open_cache_db(root_uri, lang) {
        if deleted {
            module_cache::delete_ref_rows(&conn, &canon_str);
        }
        let tx = conn.unchecked_transaction().ok();
        for (p, arc) in &results {
            let p_str = p.to_string_lossy();
            let cached = Arc::new(CachedModule::new(p.clone(), arc.clone()));
            module_cache::save_to_db(&conn, &p_str, &Some(cached), "workspace");
            if !arc.degraded {
                let seeds: Vec<_> = arc.refs.iter().map(|r| r.row_seed()).collect();
                if let Err(e) = module_cache::shred_ref_rows(&conn, &p_str, "workspace", &seeds) {
                    log::warn!("Failed to shred ref rows for {:?}: {}", p, e);
                }
            }
        }
        if let Some(tx) = tx {
            let _ = tx.commit();
        }
        true
    } else {
        false
    };
    if let Some(ref pack) = pack {
        for (p, arc) in &results {
            pack.unregister_file(p);
            if persisted && !arc.degraded && eviction_enabled() {
                let mut resident = (**arc).clone();
                resident.evict_witness_bag();
                resident.evict_refs();
                pack.register_symbols(p.clone(), Arc::new(resident));
            } else {
                pack.register_symbols(p.clone(), arc.clone());
            }
            pack.invalidate_bag_cache(p);
        }
    }
}

/// Scan @INC directories for .pm files, populating the available_modules map.
/// Fast — no file reads, just directory traversal + path→module name conversion.
fn scan_inc_module_names(inc_paths: &[PathBuf], available: &DashMap<String, PathBuf>) {
    for inc in inc_paths {
        if inc.is_dir() {
            scan_dir_recursive(inc, inc, available, 0);
        }
    }
}

fn scan_dir_recursive(base: &std::path::Path, dir: &std::path::Path, available: &DashMap<String, PathBuf>, depth: u32) {
    if depth > 15 { return; } // prevent symlink loops
    let entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            scan_dir_recursive(base, &path, available, depth + 1);
        } else if path.extension().map(|e| e == "pm").unwrap_or(false) {
            if let Ok(rel) = path.strip_prefix(base) {
                let module_name = rel.to_string_lossy()
                    .trim_end_matches(".pm")
                    .replace(std::path::MAIN_SEPARATOR, "::");
                available.insert(module_name, path.clone());
            }
        }
    }
}

fn uri_to_path(uri: &str) -> Option<PathBuf> {
    uri.strip_prefix("file://").map(PathBuf::from)
}

#[cfg(test)]
#[path = "module_resolver_tests.rs"]
mod tests;
