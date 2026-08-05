//! `IndexCore` — the shared index organs as ONE struct.
//!
//! `ModuleIndex` (async side) and the resolver thread (blocking side) hold
//! the same `Arc<IndexCore>`, so every operation on the shared state has
//! exactly one spelling — a method here — and the side-effect set of an
//! operation cannot diverge per entry path (the drift class where an
//! @INC-resolved module fed `edges` but never `loader_config_shapes`
//! because the thread held loose Arcs without the shapes map).

use super::*;

pub(crate) struct IndexCore {
    pub(crate) cache: DashMap<String, Option<Arc<CachedModule>>>,
    /// See `ModuleEdgeIndexes` — names + bridges + children reverse maps.
    pub(crate) edges: ModuleEdgeIndexes,
    /// Loader-config shapes projected at registration: load-name →
    /// (contributor, shape) pairs from each file's `PluginLoad` facts.
    /// Projected HERE because lite entrypoints are PACKAGELESS — they
    /// never enter the cache, so enrichment can't reach their bags;
    /// the config value is a literal, so its shape is final at the
    /// contributor's own build. Fed by `record_workspace_projections`
    /// (before the packageless early-return) AND `insert_resolved`.
    pub(crate) loader_config_shapes: DashMap<String, Vec<(String, InferredTypeOwned)>>,
    /// Modules loaded from cache with an old extract_version.
    /// Eligible for priority re-resolution when requested.
    pub(crate) stale_modules: DashMap<String, ()>,
    /// Perl builtins hover docs, name → rendered markdown. Hydrated
    /// from SQLite by the resolver thread at startup (parsed from
    /// `perlfunc.pod` on first cold-cache miss). Empty until the
    /// resolver has run its warmup path.
    pub(crate) builtins: DashMap<String, String>,
    /// Known module names from @INC scan. Name → path. No exports until resolved.
    pub(crate) available_modules: DashMap<String, std::path::PathBuf>,
    pub(crate) queue: ResolveQueue,
    pub(crate) resolved: ResolveNotify,
    pub(crate) workspace_root: WorkspaceRootChannel,
    /// Monotonic per-path registration generation — the ABA-proof identity
    /// token `enrichment_key` hashes (an Arc pointer can be freed and its
    /// address reused; a counter can't run backwards). Bumped by every
    /// registration front door.
    pub(crate) registration_gen: DashMap<std::path::PathBuf, u64>,
    pub(crate) gen_counter: std::sync::atomic::AtomicU64,
    /// The witness seams' fallback-on-miss enriched retries only pay off
    /// when the process lives long enough to amortize the overlay (each
    /// miss is a whole-analysis deep copy + enrich). Off by default; the
    /// SERVER enables it at initialize. One-shot CLI query modes leave it
    /// off — the bisected cost was 2x warm-gold wall for answers no
    /// one-shot invocation reuses. (`--check`/`--dump-package` consume
    /// `enriched_snapshot` directly and are unaffected by this gate.)
    pub(crate) long_lived: std::sync::atomic::AtomicBool,
    /// Slice-2 rehydration store CELL. Pack sub-indexes get theirs at
    /// construction (keyed to `modules-{lang}.db`); the Perl hub gets its
    /// own in `set_workspace_root` (keyed to `modules.db`). A type query
    /// reaching into an evicted file rehydrates the exact persisted bag
    /// through this LRU (`bag_present`). Kept behind its own `Arc` because
    /// `attach_pack_index` shares the cell itself (not its contents) into
    /// sub-indexes' `foreign_bag_cache`, so a later `set_workspace_root`
    /// install stays visible to them. See `docs/adr/memory-slice-2-lru.md`.
    pub(crate) bag_cache:
        Arc<std::sync::RwLock<Option<Arc<crate::index::pack_bag_cache::PackBagCache>>>>,
}

impl IndexCore {
    pub(crate) fn new() -> Self {
        IndexCore {
            cache: DashMap::new(),
            edges: ModuleEdgeIndexes::new(),
            loader_config_shapes: DashMap::new(),
            stale_modules: DashMap::new(),
            builtins: DashMap::new(),
            available_modules: DashMap::new(),
            queue: ResolveQueue {
                priority: Mutex::new(Vec::new()),
                pending: Mutex::new(Vec::new()),
                condvar: Condvar::new(),
            },
            resolved: ResolveNotify { mu: Mutex::new(()), cv: Condvar::new() },
            workspace_root: WorkspaceRootChannel {
                root: Mutex::new(None),
                condvar: Condvar::new(),
            },
            registration_gen: DashMap::new(),
            gen_counter: std::sync::atomic::AtomicU64::new(1),
            long_lived: std::sync::atomic::AtomicBool::new(false),
            bag_cache: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Mint a fresh monotonic registration generation for `path`. The
    /// enrichment key's ABA-proof identity token: a re-registration (or an
    /// @INC re-resolve) bumps the gen, moving every consumer's key — where a
    /// bare Arc pointer could be freed and its address reused.
    pub(crate) fn mint_registration_gen(&self, path: &std::path::Path) {
        let g = self
            .gen_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.registration_gen.insert(path.to_path_buf(), g);
    }

    /// Stamp a generation for every name-keyed cache entry that lacks one.
    /// The @INC warm scan (`warm_cache`) writes blobs straight into the
    /// cache without a registration front door, so those providers would
    /// otherwise read gen 0 in `enrichment_key`. `or_insert` so a warm entry
    /// racing a workspace front-door registration keeps the front-door
    /// generation.
    pub(crate) fn stamp_missing_import_gens(&self) {
        for entry in self.cache.iter() {
            if let Some(ref cm) = *entry.value() {
                self.registration_gen.entry(cm.path.clone()).or_insert_with(|| {
                    self.gen_counter
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                });
            }
        }
    }

    /// THE one spelling of "a module resolution landed in the name-keyed
    /// cache slot" — the resolver thread and the CLI both route here.
    /// `result` is the WHOLE parsed copy; `persisted` says its blob landed
    /// (the strip license) and `strip` is the eviction switch. On a resolved
    /// copy, in order: stale-pin clear BEFORE the copy is reachable (a
    /// re-resolve replaced the blob; a query racing this insert must not
    /// rehydrate the prior generation), a fresh registration generation
    /// (moves every consumer's enrichment key), then the projections — edge
    /// feeds and loader-config shapes — on the WHOLE analysis (the shape
    /// projection resolves config literals through the witness bag the strip
    /// drops: reads-whole-before-evict), then the registration-owned strip,
    /// then the store. Returns the stored copy (the caller's memo value).
    ///
    /// A `None` miss never clobbers an already-indexed copy: on-demand @INC
    /// resolution can miss a module the workspace indexer already built (a
    /// project module under a relative `use lib` the resolver's @INC doesn't
    /// cover), and clobbering would leave the reverse index pointing at a
    /// module the cache no longer holds (the orphan that broke cross-file
    /// Handler / dispatch lookup).
    pub(crate) fn insert_resolved(
        &self,
        module_name: &str,
        result: Option<Arc<CachedModule>>,
        persisted: bool,
        strip: bool,
    ) -> Option<Arc<CachedModule>> {
        if let Some(ref m) = result {
            if let Some(bc) = self.bag_cache.read().ok().and_then(|g| g.clone()) {
                bc.invalidate(&m.path);
            }
            self.mint_registration_gen(&m.path);
            self.edges.feed(module_name, &m.analysis);
            self.record_loader_shapes(module_name, &m.analysis);
        } else if matches!(self.cache.get(module_name).as_deref(), Some(Some(_))) {
            return None;
        }
        let stored = strip_import_copy(&result, persisted, strip);
        self.cache.insert(module_name.to_string(), stored.clone());
        stored
    }

    /// Project each `PluginLoad` fact's config value into a stored
    /// shape under its load-name. The value is a literal in the
    /// contributor's file, so `expr_type_at_span` with no index is
    /// already final — this is a registration-time projection of
    /// local facts (the same tier as export names), not a cached
    /// cross-file resolution.
    pub(crate) fn record_loader_shapes(&self, contributor: &str, analysis: &FileAnalysis) {
        // re-registration: drop this contributor's old entries
        self.loader_config_shapes.retain(|_n, v| {
            v.retain(|(c, _)| c != contributor);
            !v.is_empty()
        });
        for f in &analysis.plugin.loads {
            let Some(span) = f.config_span else { continue };
            if let Some(t) = analysis.expr_type_at_span(span, None) {
                self.loader_config_shapes
                    .entry(f.name.clone())
                    .or_default()
                    .push((contributor.to_string(), t));
            }
        }
    }

    /// Rebuild the edge indexes (`func → modules`, bridges, children, specs)
    /// from the current cache. The warm path writes blobs straight into the
    /// cache without touching the indexes, so a warm start that skips this
    /// leaves every reverse lookup blind (cold/warm attribution, the B6
    /// class).
    pub(crate) fn rebuild_reverse_index(&self) {
        self.edges.clear();
        for entry in self.cache.iter() {
            if let Some(ref cached) = *entry.value() {
                self.edges.feed(entry.key(), &cached.analysis);
            }
        }
    }
}

/// The @INC tier's registration-owned strip: once the blob is persisted,
/// the resident copy drops its witness bag (the dominant share of a CPAN
/// module's payload; `bag_present` rehydrates through the hub's LRU).
/// Symbols and refs stay resident this slice — their reader routing for
/// the import tier is the follow-up in
/// `docs/prompt-storage-residuals.md`. Degraded
/// analyses keep the bag (their rows never persist).
pub(crate) fn strip_import_copy(
    result: &Option<Arc<CachedModule>>,
    persisted: bool,
    strip: bool,
) -> Option<Arc<CachedModule>> {
    match result {
        Some(m) if persisted && strip && !m.analysis.degraded => {
            let mut fa = (*m.analysis).clone();
            fa.evict_axes(true, false);
            Some(Arc::new(CachedModule::new(m.path.clone(), Arc::new(fa))))
        }
        _ => result.clone(),
    }
}
