//! Module index: public API for cross-file Perl module intelligence.
//!
//! Wraps a concurrent cache (`DashMap`) backed by a background resolver thread.
//! Async LSP handlers only read from the cache (zero I/O). The resolver thread
//! handles @INC discovery, in-process parsing, SQLite persistence, and cpanfile
//! pre-scanning.
//!
//! The cache stores the full `FileAnalysis` (not a lossy summary), so
//! cross-file refs, type constraints, call bindings, and framework context
//! all survive the module boundary.
//!
//! See also:
//! - `module_resolver/` — resolver thread, in-process parsing
//! - `module_cache/` — SQLite persistence (schema v9, bincode+zstd blobs)
//! - `cpanfile.rs` — cpanfile parsing

#[cfg(test)]
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

use dashmap::DashMap;
use tower_lsp::Client;

use crate::model::file_analysis::{CrossFileLookup, FileAnalysis, SymKind};
#[cfg(test)]
use crate::model::file_analysis::InferredType;
use crate::index::module_resolver;

// ---- Public types ----

// `CachedModule` / `SubInfo` are pure views over `FileAnalysis` and live
// there (the index depends on the model, not vice versa); re-exported so
// index consumers keep one import site.
pub use crate::model::file_analysis::{CachedModule, SubInfo};

type InferredTypeOwned = crate::model::file_analysis::InferredType;

/// Rehydration misses on evicted copies this process served degraded
/// (`rehydrate_or_resident`'s invariant-break arm). Process-global: the
/// residency story spans the hub and every pack sub-index, and the flake
/// this polices ("inputs vanished" cold runs) is a per-process verdict.
static REHYDRATION_MISSES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// How many evicted copies failed to rehydrate this process (each was
/// served as a stripped resident — quietly incomplete answers). Zero in a
/// healthy session; the strict gate (`PERL_LSP_STRICT_RESIDENCY`) panics
/// at the first miss instead of counting. Observability hook read by the
/// residency tests; production reacts via the strict gate, not this reader.
#[cfg(test)]
pub fn rehydration_miss_count() -> usize {
    REHYDRATION_MISSES.load(std::sync::atomic::Ordering::Relaxed)
}

/// The linkage-visible feed a registration extracts from a WHOLE analysis:
/// (name, declares-a-Class) per visible symbol. Collected before any strip
/// so the feeds and tie-breaks never read an emptied `symbols`.
fn collect_linkage_feed(analysis: &FileAnalysis) -> Vec<(String, bool)> {
    let mut index: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut feed: Vec<(String, bool)> = Vec::new();
    for sym in &analysis.symbols {
        // The C-linkage surface (`FileAnalysis::is_linkage_visible`) —
        // the same predicate completion gathering uses, so every name
        // registered here is also offerable and vice versa.
        if !analysis.is_linkage_visible(sym) {
            continue;
        }
        let is_class = matches!(sym.kind, SymKind::Class);
        match index.get(sym.name.as_str()) {
            // A file declaring both a value AND a Class under one name
            // ranks as a Class.
            Some(&i) => feed[i].1 |= is_class,
            None => {
                index.insert(sym.name.as_str(), feed.len());
                feed.push((sym.name.clone(), is_class));
            }
        }
    }
    // Class rank is visibility-INDEPENDENT (the old occupant scan matched
    // any Class symbol): a non-linkage-visible Class sharing a visible
    // value's name still ranks the file as declaring that Class.
    for sym in &analysis.symbols {
        if matches!(sym.kind, SymKind::Class) {
            if let Some(&i) = index.get(sym.name.as_str()) {
                feed[i].1 = true;
            }
        }
    }
    feed
}

/// Pick the winner among same-name candidates by the SAME total order
/// `register_symbols` uses for the global cache slot: a TYPE (Class) beats a
/// Sub/value, then the smallest canonical path breaks the tie (order-independent
/// — no reliance on registration order). Factored so the scoped lookup and the
/// registration winner agree by construction.
fn best_candidate<'c>(
    cands: &[&'c Arc<CachedModule>],
    name: &str,
    defines_class: &dyn Fn(&CachedModule, &str) -> bool,
) -> Option<Arc<CachedModule>> {
    cands
        .iter()
        .copied()
        .max_by(|a, b| {
            let (ac, bc) = (defines_class(a, name), defines_class(b, name));
            // Class beats non-class; then SMALLER path wins (reverse for max_by).
            ac.cmp(&bc).then_with(|| b.path.cmp(&a.path))
        })
        .cloned()
}

// ---- Internal sync primitives (pub(crate) for resolver thread) ----

/// Thread-safe queue: Mutex<Vec> + Condvar.
pub(crate) struct ResolveQueue {
    /// High priority: stale modules from open files. Drained first.
    pub priority: Mutex<Vec<String>>,
    /// Normal priority: missing modules.
    pub pending: Mutex<Vec<String>>,
    pub condvar: Condvar,
}

/// Signaled after each module is resolved.
pub(crate) struct ResolveNotify {
    pub mu: Mutex<()>,
    pub cv: Condvar,
}

/// Channel for workspace root from initialize() → resolver thread.
pub(crate) struct WorkspaceRootChannel {
    pub root: Mutex<Option<Option<String>>>,
    pub condvar: Condvar,
}

mod parts;
pub use parts::*;

mod index_core;
pub(crate) use index_core::IndexCore;
#[cfg(test)]
pub(crate) use index_core::strip_import_copy;

mod lookup;
mod queries;
mod registration;

pub struct ModuleIndex {
    /// The shared organs (`IndexCore`): cache, edge indexes, resolve
    /// queue/notify, generation counters, loader shapes, bag-cache cell.
    /// The resolver thread holds the SAME `Arc<IndexCore>`, so both sides
    /// operate through the one method set — an operation's side-effect set
    /// cannot diverge per entry path.
    core: Arc<IndexCore>,
    /// Modules imported (literally or via SyntheticUse) by ANY
    /// workspace file, entrypoint scripts included. Powers the
    /// entrypoint-scan helper lint's "does anything load M" question.
    /// Fed by `register_workspace_module` only — the workspace scan
    /// re-runs every startup, so no warm-rebuild feed is needed.
    loaded_modules: Arc<DashMap<String, ()>>,
    /// Primary package names of workspace-registered files. The lint
    /// fires only for WORKSPACE plugin modules (in-project plugins you
    /// forgot to load); installed CPAN plugins keep the generous
    /// "downloaded = intended" resolution.
    workspace_modules: Arc<DashMap<String, ()>>,
    /// Per-language sub-indexes (`"cpp"`, `"python"`, …) — kept SEPARATE
    /// (own cache, own `modules-{lang}.db`) so names never comingle across
    /// languages. The Perl index is the hub; query routing picks the right
    /// one by the queried file's language. Generic: any pack language.
    pack_indexes: Arc<DashMap<String, Arc<ModuleIndex>>>,
    /// Canonical paths of currently-open docs whose surface record the
    /// open-doc path owns (`SurfaceWrite` — background writes yield).
    /// Marked by the backend on didOpen, cleared + reconciled on didClose.
    /// Perl hub only today: pack languages have no open-doc surface
    /// recorder yet, so guarding their background writes would freeze
    /// records staleward.
    open_doc_paths: Arc<DashMap<std::path::PathBuf, ()>>,
    /// ALL cross-file candidates per name (not just the single winner in
    /// `cache`) — pack languages only. C linkage is globally flat, so two
    /// unrelated files can each define `class Box`; `cache` picks one
    /// deterministic winner, but a query from file F wants the candidate F can
    /// actually SEE (its `#include` closure). `get_cached_scoped` ranks these by
    /// reachability. `docs/adr/macro-handling.md`, "the include-closure lie".
    all_defs: Arc<DashMap<String, Vec<Arc<CachedModule>>>>,
    /// Every pack file registered, keyed by canonical path — including files
    /// that declare NOTHING registrable (a header-only `#include` shim). The
    /// name-keyed views can't reach those, but whole-project sweeps
    /// (`for_each_cached_file`) must.
    all_files: Arc<DashMap<std::path::PathBuf, Arc<CachedModule>>>,
    /// The freshness engine (`docs/adr/storage-engine.md`):
    /// per-file span-free surface records + the reverse-dependency index.
    /// Fed at registration (whole copy, pre-strip) and on open-doc
    /// rebuilds; `dirty_consumers` names who must re-enrich after a
    /// surface CHANGE, and an Unchanged verdict is the early-cutoff.
    freshness: Arc<crate::model::surface::FreshnessIndex>,
    /// The enrichment overlay (R4): derived enriched copies keyed by the
    /// surface fingerprints of the file + its providers. Bounded FIFO —
    /// `enriched_order` is the eviction queue.
    /// `None` payload = a DECLINED build (byte-cap giant / cycle-tainted)
    /// at this key: repeat queries skip the deep-copy entirely until a
    /// provider change moves the key.
    enriched: Arc<DashMap<std::path::PathBuf, (u64, Option<Arc<FileAnalysis>>, usize)>>,
    enriched_order: Arc<std::sync::Mutex<std::collections::VecDeque<std::path::PathBuf>>>,
    /// The linkage-visible (name, declares-a-Class) pairs each file
    /// registered — the exact inverse list `unregister_file` walks AND the
    /// class-rank source for the cache-slot tie-break. Recorded at
    /// registration (pre-strip) because the resident copy's `symbols` may be
    /// evicted, and rehydration after an edit persists would fetch the NEW
    /// generation's names.
    registered_names: Arc<DashMap<std::path::PathBuf, Vec<(String, bool)>>>,
    /// The SIBLING tier's rehydration store, for copies this index does not
    /// own. Sweeps mint `CachedModule`s from FileStore entries and ask
    /// whatever index the query routed to — a cpp query's workspace sweep
    /// hands PERL paths to the cpp sub-index, whose own loader (keyed to
    /// `modules-{lang}.db`) can never serve them. `attach_pack_index`
    /// shares the hub's `bag_cache` cell here so a foreign path routes to
    /// its owner instead of degrading to the stripped resident. The hub's
    /// converse route (a pack path asked of the hub) walks `pack_indexes`.
    foreign_bag_cache: std::sync::RwLock<
        Option<Arc<std::sync::RwLock<Option<Arc<crate::index::pack_bag_cache::PackBagCache>>>>>,
    >,
    /// Read-connection opener for the relational ref index
    /// (`docs/adr/relational-ref-index.md`) — set once per index onto the
    /// per-language DB (`modules.db` for the Perl hub, `modules-{lang}.db`
    /// for pack sub-indexes). Opened per retrieval (WAL readers are cheap
    /// and `rusqlite::Connection` isn't `Sync`); `None` (tests, no cache
    /// dir) contributes no candidates and the resident sweep still covers.
    ref_rows_opener:
        std::sync::RwLock<Option<Arc<dyn Fn() -> Option<rusqlite::Connection> + Send + Sync>>>,
    /// The retained read connection the opener fills lazily — one per index,
    /// so the statement cache amortizes across queries (a heatmap projects
    /// references once per symbol; per-call opens would re-prepare every
    /// statement). WAL readers see each write txn that committed before
    /// their own read txn begins, so retaining it never serves stale rows.
    /// Paired with the DB file's inode at open: `--clear-cache` UNLINKS the
    /// file, and an fd pinning the dead inode would serve frozen rows
    /// forever — an inode change (or missing file) drops the conn so the
    /// next query reopens the recreated DB.
    ref_rows_conn: std::sync::Mutex<Option<(rusqlite::Connection, u64)>>,
}

/// The store `ModuleIndex::lookup_for` routed a language to. Owning enum:
/// a pack sub-index is an `Arc` out of the hub's registry and must stay
/// alive for the query's lifetime (the caller holds this value), while the
/// hub case stays borrow-only.
pub enum RoutedIndex<'a> {
    Hub(&'a ModuleIndex),
    Pack(Arc<ModuleIndex>),
}

impl RoutedIndex<'_> {
    /// The routed store as the lookup trait `resolve()` and the slot-shaped
    /// pre-set lanes (include tokens, raw-word fallbacks) consume.
    pub fn as_lookup(&self) -> &dyn crate::model::file_analysis::CrossFileLookup {
        match self {
            RoutedIndex::Hub(h) => *h,
            RoutedIndex::Pack(p) => p.as_ref(),
        }
    }
}

// ---- Module-level helpers ----

/// Return the parents of the primary package of a module, preferring the
/// package with the same name as `module_name` and falling back to the
/// single-package case if only one package exists in the file.
/// First `package X;` declaration in a FileAnalysis. Used to decide
/// under what name a workspace file should be registered in the
/// module index so cross-file method resolution (which keys on
/// package name, e.g. "Users" for `->to('Users#list')`) can find it.
/// Returns `None` for scripts with no explicit package declaration.
pub fn first_package_name(analysis: &FileAnalysis) -> Option<String> {
    for sym in &analysis.symbols {
        if matches!(sym.kind, SymKind::Package | SymKind::Class) {
            return Some(sym.name.clone());
        }
    }
    None
}

pub fn primary_package_parents(analysis: &FileAnalysis, module_name: &str) -> Vec<String> {
    analysis.declared_parents(module_name).to_vec()
}


#[cfg(test)]
#[path = "module_index_tests.rs"]
mod tests;