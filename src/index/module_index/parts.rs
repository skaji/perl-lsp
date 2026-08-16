//! Reverse-edge index bundle plus the registration tokens: `ModuleEdgeIndexes`,
//! the pre-strip Pack/Workspace registration parts, and the surface-write types.

use super::*;

/// Concurrent module cache with background resolution.
///
/// The reverse-edge maps over the module cache, bundled so every feed
/// site updates all of them in lockstep. Every map answers "which
/// modules…" for a different edge:
///
/// - `names`: symbol/export name → modules declaring or exporting it.
///   The single generic "find me modules with symbol X" primitive —
///   hover, signature help, goto-def, auto-import, and the
///   unimported-completion path all route through it instead of
///   reinventing per-feature cache walks. Covers every module-visible
///   symbol kind (Sub, Method, Package, Class, Module, HashKeyDef,
///   Handler) plus the export/export_ok lists (XS exporters name
///   functions with no Perl body). Callers wanting narrower semantics
///   filter via per-module inspection.
/// - `bridges`: class → modules declaring a `PluginNamespace` whose
///   `bridges` list contains `Bridge::Class(class)`. The one reverse
///   index for plugin-synthesized content; queried through
///   `for_each_entity_bridged_to`.
/// - `children`: parent class/role → modules containing a package
///   that `isa`/composes it (inverse `PackageFacts::parents`). The
///   long-distance primitive: "who composes this role" /
///   "who subclasses this class" in O(1).
///
/// The bundle exists because the feeds must never diverge across the
/// resolve insert path, the SQLite warm rebuild, and workspace
/// registration — a map fed on insert but not on rebuild serves cold
/// sessions and starves warm ones (the twice-paid B6 lesson). One
/// `feed()` per site makes a missed map unrepresentable.
pub struct ModuleEdgeIndexes {
    pub(super) names: DashMap<String, Vec<String>>,
    pub(super) bridges: DashMap<String, Vec<String>>,
    pub(super) children: DashMap<String, Vec<String>>,
    /// primary template → modules declaring a specialization of it (inverse
    /// `FileAnalysis.pack.specializes`). The `Specializes` family edge's
    /// cross-file half; member resolution never reads it.
    pub(super) specs: DashMap<String, Vec<String>>,
    /// The indexable-name list each FILE last fed — the symbols-derived
    /// half of `feed`, recorded from the WHOLE analysis so a re-feed over
    /// symbol-EVICTED cache copies (`rebuild_reverse_index*` after the
    /// workspace indexer strips, sibling replay after a same-name purge)
    /// replays the names instead of reading empty vecs and silently
    /// blinding `modules_with_symbol`/`find_exporters`. Keyed by PATH, not
    /// module name: several files can feed under one package name (Perl
    /// reopens packages anywhere), and a name-keyed record would replay one
    /// file's names for its siblings. `clear()` and `purge_module` keep it
    /// (re-feeds are exactly when it's needed); `remove_path_record` drops
    /// it when the file itself goes.
    name_records: DashMap<std::path::PathBuf, Vec<String>>,
}

impl ModuleEdgeIndexes {
    pub fn new() -> Self {
        ModuleEdgeIndexes {
            names: DashMap::new(),
            bridges: DashMap::new(),
            children: DashMap::new(),
            specs: DashMap::new(),
            name_records: DashMap::new(),
        }
    }

    /// Register every edge `analysis` contributes under `module_name`.
    /// The ONLY write path besides `purge_module`/`clear` — new edge
    /// maps get their extraction added here and nowhere else. Eviction-
    /// aware: a symbol-stripped copy replays `path`'s recorded name list;
    /// a whole copy recomputes and re-records it. Idempotent per
    /// (bucket, module_name): re-feeding never grows a bucket, so the
    /// candidate-set rebuilds (purge + one feed per candidate) and the
    /// warm rebuild can overlap without accumulation.
    pub fn feed(&self, module_name: &str, path: &std::path::Path, analysis: &FileAnalysis) {
        let names: Vec<String> = if analysis.symbols_are_evicted() {
            match self.name_records.get(path) {
                Some(rec) => rec.clone(),
                // No record (a stripped copy fed without ever being fed
                // whole — shouldn't happen, but degrade to the pinned
                // export names rather than nothing).
                None => Self::indexable_names(analysis),
            }
        } else {
            let names = Self::indexable_names(analysis);
            self.name_records.insert(path.to_path_buf(), names.clone());
            names
        };
        let push_unique = |map: &DashMap<String, Vec<String>>, key: String| {
            let mut v = map.entry(key).or_default();
            if !v.iter().any(|m| m == module_name) {
                v.push(module_name.to_string());
            }
        };
        for name in names {
            push_unique(&self.names, name);
        }
        for class in Self::bridge_classes(analysis) {
            push_unique(&self.bridges, class);
        }
        for parent in Self::parent_classes(analysis) {
            push_unique(&self.children, parent);
        }
        for primary in Self::spec_primaries(analysis) {
            push_unique(&self.specs, primary);
        }
    }

    /// Record `path`'s indexable-name list from a WHOLE analysis so a later
    /// `feed` of its stripped copy replays it — the pre-strip half of the
    /// split workspace registration, where the feed itself waits for the
    /// blob COMMIT but only the whole analysis can spell the names.
    pub fn record_names(&self, path: &std::path::Path, analysis: &FileAnalysis) {
        debug_assert!(!analysis.symbols_are_evicted());
        self.name_records
            .insert(path.to_path_buf(), Self::indexable_names(analysis));
    }

    /// Remove `module_name` from every bucket of every map. Runs
    /// before re-registration so stale edges from a prior version of
    /// the same module don't accumulate (phantom-module lookups).
    /// KEEPS `name_records` — they are per-PATH, and a same-name sibling
    /// file's replay source must survive this file's re-registration.
    pub fn purge_module(&self, module_name: &str) {
        for map in [&self.names, &self.bridges, &self.children, &self.specs] {
            map.retain(|_key, mods| {
                mods.retain(|m| m != module_name);
                !mods.is_empty()
            });
        }
    }

    /// Drop `path`'s recorded name list (the file itself is gone).
    pub fn remove_path_record(&self, path: &std::path::Path) {
        self.name_records.remove(path);
    }

    /// Wipe the edge maps for a rebuild. Deliberately KEEPS `name_records`
    /// — the rebuild re-feeds from cache copies that may be symbol-evicted,
    /// and the records are their only complete name source.
    pub fn clear(&self) {
        self.names.clear();
        self.bridges.clear();
        self.children.clear();
        self.specs.clear();
    }

    /// Every name `find_exporters` might need to locate a module by:
    /// declared module-visible symbols plus the export/export_ok lists.
    /// Variables and fields are skipped — file-local, not queryable
    /// across files.
    fn indexable_names(analysis: &FileAnalysis) -> Vec<String> {
        let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for sym in analysis.symbols() {
            if matches!(
                sym.kind,
                SymKind::Sub | SymKind::Method | SymKind::Package | SymKind::Class
                    | SymKind::Module | SymKind::HashKeyDef | SymKind::Handler,
            ) {
                names.insert(sym.name.clone());
            }
        }
        names.extend(analysis.export.iter().cloned());
        names.extend(analysis.export_ok.iter().cloned());
        names.into_iter().collect()
    }

    /// The bridge classes an analysis' plugin namespaces declare, deduped.
    fn bridge_classes(analysis: &FileAnalysis) -> Vec<String> {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for ns in &analysis.plugin.namespaces {
            for crate::model::file_analysis::Bridge::Class(c) in &ns.bridges {
                seen.insert(c.clone());
            }
        }
        seen.into_iter().collect()
    }

    /// Every primary a specialization in the analysis names — the values of
    /// `specializes`, deduped.
    fn spec_primaries(analysis: &FileAnalysis) -> Vec<String> {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for primary in analysis.pack.specializes.values() {
            seen.insert(primary.clone());
        }
        seen.into_iter().collect()
    }

    /// Every parent class/role any package in the analysis records —
    /// the values of `PackageFacts::parents`, deduped. `use parent`/`use
    /// base`/`@ISA`/`class :isa`/`:does`/`with` all land here, so the
    /// `children` map covers inheritance and role composition alike.
    fn parent_classes(analysis: &FileAnalysis) -> Vec<String> {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (_pkg, parents) in analysis.package_parent_edges() {
            for p in parents {
                seen.insert(p.clone());
            }
        }
        seen.into_iter().collect()
    }
}

/// Async LSP handlers read from `cache` (zero I/O). The background resolver
/// thread populates the cache by parsing `.pm` files in-process.
/// The pack registration TOKEN: the (possibly stripped) arc to register
/// plus the whole-analysis halves — feed, specialization edges, projected
/// surface — all extracted BEFORE the strip. Fields are PRIVATE and the
/// struct is minted ONLY by the choke points in this module
/// (`prepare_pack_parts` = the reads-whole-before-evict strip, `whole` =
/// a deliberate whole-copy door, `from_warm_stub` = a persisted token
/// rehydrated). Holding one is the compile-time proof that a resident
/// `FileAnalysis` reached registration through one of those seams — a new
/// caller cannot hand `register_symbols_inner` a loose whole arc.
pub(crate) struct PackRegistrationParts {
    pub(super) arc: Arc<FileAnalysis>,
    pub(super) feed: Vec<(String, bool)>,
    pub(super) specs: Vec<(String, String)>,
    pub(super) surface: crate::model::surface::Surface,
}

impl PackRegistrationParts {
    /// The arc registration stores (read for persistence — `include_closure`
    /// — and for stub encoding).
    pub(crate) fn arc(&self) -> &Arc<FileAnalysis> {
        &self.arc
    }
    pub(crate) fn feed(&self) -> &[(String, bool)] {
        &self.feed
    }
    pub(crate) fn specs(&self) -> &[(String, String)] {
        &self.specs
    }
    pub(crate) fn surface(&self) -> &crate::model::surface::Surface {
        &self.surface
    }

    /// A whole-copy token minted from an already-`Arc`'d analysis: the feed
    /// reads the whole `symbols`, the surface projects from the whole bag.
    /// The deliberate whole-copy front door (`register_symbols`) — bounded,
    /// tripwire-counted at its call sites.
    pub(crate) fn whole(arc: Arc<FileAnalysis>) -> Self {
        let (feed, specs) = ModuleIndex::prepare_pack_feed(&arc);
        let surface = crate::model::surface::Surface::project(&arc);
        PackRegistrationParts { arc, feed, specs, surface }
    }

    /// Rehydrate a token from a warm stub — the persisted form of a prior
    /// `prepare_pack_parts` output (`encode_stub` was fed exactly these
    /// halves). The proof-of-strip is the persistence itself: a stub only
    /// exists because a fully-stripped copy was written.
    pub(crate) fn from_warm_stub(stub: crate::index::module_cache::WarmStub) -> Self {
        PackRegistrationParts {
            arc: Arc::new(stub.skeleton),
            feed: stub.feed,
            specs: stub.specs,
            surface: stub.surface,
        }
    }

    /// Record this file's span-free surface (the freshness write half).
    /// Separate from registration so the deferred-writer path can record
    /// pre-COMMIT (session-local) while the residency half waits for the
    /// commit; the sync front doors record then register in sequence.
    pub(crate) fn record_surface(
        &self,
        idx: &ModuleIndex,
        path: &std::path::Path,
    ) -> crate::model::surface::SurfaceVerdict {
        idx.record_surface_value(path, self.surface.clone())
    }
}

/// The workspace registration TOKEN — the Perl twin of
/// `PackRegistrationParts`. Same private-field / choke-point-mint discipline:
/// minted only by `prepare_workspace_parts` (strip) in this module.
pub(crate) struct WorkspaceRegistrationParts {
    pub(super) arc: Arc<FileAnalysis>,
    /// EVERY package name the file declares (name, is-class), extracted
    /// pre-strip — Perl allows any number of packages per file, and each
    /// one must be reachable by name (`docs/adr/file-store-and-resolve.md`).
    pub(super) names: Vec<(String, bool)>,
    pub(super) surface: crate::model::surface::Surface,
}

impl WorkspaceRegistrationParts {
    pub(crate) fn arc(&self) -> &Arc<FileAnalysis> {
        &self.arc
    }

    /// See `PackRegistrationParts::record_surface`.
    pub(crate) fn record_surface(
        &self,
        idx: &ModuleIndex,
        path: &std::path::Path,
    ) -> crate::model::surface::SurfaceVerdict {
        idx.record_surface_value(path, self.surface.clone())
    }
}

/// Who is recording a surface. While a doc is OPEN, cross-file consumers
/// read its BUFFER analysis (query priority: open docs shadow the indexed
/// disk copy), so the freshness baseline must track the buffer: a
/// `Background` write (bulk indexer, watcher tick, save re-register) for an
/// open path describes a disk state consumers cannot see and is SUPPRESSED
/// — otherwise an edit reverting the buffer to the disk state reads
/// Unchanged against the wrong baseline and skips the consumer refresh.
/// `did_close` reconciles: consumers flip back to the disk copy, so the
/// close path re-records it (and refreshes whoever the flip dirtied).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SurfaceWrite {
    /// The open-doc editor path — owns the record while the doc is open.
    OpenDoc,
    /// Everything else (indexers, watcher, warm lanes) — yields to an open
    /// doc's record, wins otherwise.
    Background,
}

/// The freshness gate's answer: the surface verdict plus, on `Changed`, the
/// transitive dirty consumer set. Returned by `ModuleIndex::record_and_dirty`
/// (and by `register_workspace_resident`, which routes through it) so a
/// caller that records a surface always holds the consumer answer from the
/// same path.
pub struct SurfaceDirty {
    /// Rides the answer for callers that gate on FirstSeen vs Unchanged vs
    /// Changed; today's consumers act only on `dirty` (empty ⇒ nothing to do).
    #[allow(dead_code)]
    pub verdict: crate::model::surface::SurfaceVerdict,
    pub dirty: std::collections::HashSet<std::path::PathBuf>,
}
