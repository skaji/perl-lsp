//! Bridged-entity enumeration, descendant walks, blocking resolution, and the
//! `CrossFileLookup` trait impl.

use super::*;

impl ModuleIndex {
    /// Every cached module that declares at least one `PluginNamespace`
    /// whose `bridges` list includes `Bridge::Class(class_name)`.
    /// Callers then pull the namespace's entities from the module's
    /// `FileAnalysis` and iterate. Explicit bridges rather than
    /// symbol-package inference.
    /// Is `module` imported by any workspace file (entrypoint scripts
    /// included)? `false` is honest only after the workspace scan has
    /// run — callers on the diagnostics path are post-startup.
    ///
    /// Matching is exact OR last-segment tail: a `plugin 'DataLog'`
    /// load records the default-namespace guess
    /// (`Mojolicious::Plugin::DataLog`) while the resolved provider
    /// may live in an app-custom tree (`Clove::App::Plugin::DataLog`).
    /// The looseness only SUPPRESSES the lint — the honest-quiet
    /// direction.
    pub fn is_module_loaded(&self, module: &str) -> bool {
        if self.loaded_modules.contains_key(module) {
            return true;
        }
        let tail = module.rsplit("::").next().unwrap_or(module);
        self.loaded_modules
            .iter()
            .any(|e| e.key().rsplit("::").next() == Some(tail))
    }

    /// Was `module` registered from the workspace tree (vs @INC)?
    pub fn is_workspace_module(&self, module: &str) -> bool {
        self.workspace_modules.contains_key(module)
    }

    pub fn modules_bridging_to(&self, class_name: &str) -> Vec<String> {
        match self.core.edges.bridges.get(class_name) {
            Some(mods) => {
                let mut result = mods.clone();
                result.sort();
                result.dedup();
                result
            }
            None => Vec::new(),
        }
    }

    /// Every cached module containing a package that directly lists
    /// `class_name` as a parent (`isa` or role composition — the
    /// `children` edge map). Direct children only; transitive walks go
    /// through the graph walk (`walk(INHERITS_INV)`) — this is the
    /// depth-1 edge it composes.
    pub fn modules_with_parent(&self, class_name: &str) -> Vec<String> {
        match self.core.edges.children.get(class_name) {
            Some(mods) => {
                let mut result = mods.clone();
                result.sort();
                result.dedup();
                result
            }
            None => Vec::new(),
        }
    }

    /// Breadth-first walk over the inverse inheritance/composition
    /// graph from `class`: every (package, module) pair whose package
    /// transitively `isa`/composes `class`. Bounded by a package
    /// seen-set (cycles, diamonds) and a fan-out cap; never does I/O.
    ///
    /// The independent BFS oracle the graph-walk descendant test
    /// cross-checks `walk(INHERITS_INV)` against. Production reachability
    /// goes through the graph walk; this stays test-only so the two
    /// implementations can disagree loudly.
    #[cfg(test)]
    pub fn for_each_descendant_package<F>(&self, class: &str, mut visit: F)
    where
        F: FnMut(&str, &Arc<CachedModule>) -> std::ops::ControlFlow<()>,
    {
        const MAX: usize = 512;
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut queue: std::collections::VecDeque<String> =
            std::collections::VecDeque::from([class.to_string()]);
        let mut visited = 0usize;
        while let Some(current) = queue.pop_front() {
            if !seen.insert(current.clone()) {
                continue;
            }
            visited += 1;
            if visited > MAX {
                break;
            }
            for module_name in self.modules_with_parent(&current) {
                let Some(cached) = self.get_cached(&module_name) else { continue };
                // A module can hold several packages; only the ones
                // actually listing `current` as a parent are children.
                for (pkg, parents) in cached.analysis.package_parent_edges() {
                    if !parents.iter().any(|p| p == &current) {
                        continue;
                    }
                    if seen.contains(pkg) {
                        continue;
                    }
                    if visit(pkg, &cached).is_break() {
                        return;
                    }
                    queue.push_back(pkg.clone());
                }
            }
        }
    }

    /// Run a closure on every `Symbol` reachable from `class_name`
    /// through a plugin bridge. The namespace-wide `Bridge::Class(X)`
    /// identifies which namespaces to walk; per-entity `package`
    /// narrows to "this entity is bound on class X specifically".
    /// Lets one namespace span multiple classes (Mojo helpers on
    /// both Controller and Mojolicious, plus each proxy class in a
    /// dotted chain) without every entity being visible on every
    /// bridged class.
    ///
    /// Single source of truth for plugin-synthesized entity lookup
    /// across files — explicit bridges rather than a per-caller
    /// `symbol.package` scan.
    pub fn for_each_entity_bridged_to(
        &self,
        class_name: &str,
        // `mod_name` is the cache key the bridging module is registered under
        // — the authoritative handle for a follow-up `get_cached(mod_name)`.
        // Don't re-derive it from the analysis: the registration name and the
        // file's first `package` can differ.
        mut visit: impl FnMut(&str, &Arc<CachedModule>, &crate::model::file_analysis::Symbol),
    ) {
        use crate::model::file_analysis::CrossFileLookup;
        for mod_name in self.modules_bridging_to(class_name) {
            let Some(cached) = self.get_cached(&mod_name) else { continue };
            // Entities index into `symbols`, which may be evicted on the
            // resident copy — resolve them against the symbols-axis view
            // (same generation: the LRU is invalidated on every rewrite).
            // Namespaces ride the never-evicted plugin lane, so symbols are
            // the only evictable axis this read touches.
            let whole = self.symbols_present(&cached);
            for ns in &whole.plugin.namespaces {
                let bridges_class = ns.bridges.iter().any(|b|
                    matches!(b, crate::model::file_analysis::Bridge::Class(c) if c == class_name));
                if !bridges_class { continue; }
                // Namespace membership IS the filter — if this namespace
                // bridges to `class_name`, every entity it owns is
                // visible from `class_name`. No `sym.package` gate: the
                // plugin picks ONE canonical home package and the
                // namespace's bridges control visibility, so no
                // per-bridge Method fan-out is needed.
                for sym_id in &ns.entities {
                    let idx = sym_id.0 as usize;
                    let Some(sym) = whole.symbols().get(idx) else { continue };
                    visit(&mod_name, &cached, sym);
                }
            }
        }
    }

    /// Block until `module_name` appears in the cache, or timeout.
    /// (Used by tests and the one-shot CLI import resolution.)
    #[doc(hidden)]
    pub fn wait_resolved(&self, module_name: &str, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut guard = self.core.resolved.mu.lock().unwrap();
        loop {
            if self.core.cache.contains_key(module_name) {
                return true;
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let (g, result) = self.core.resolved.cv.wait_timeout(guard, remaining).unwrap();
            guard = g;
            if result.timed_out() && !self.core.cache.contains_key(module_name) {
                return false;
            }
        }
    }

    /// Get cached module synchronously. WARNING: Does blocking I/O. Only for tests.
    #[cfg(test)]
    pub fn get_cached_blocking(&self, module_name: &str) -> Option<Arc<CachedModule>> {
        if let Some(entry) = self.core.cache.get(module_name) {
            return entry.clone();
        }
        let inc_paths = module_resolver::discover_inc_paths();
        let mut parser = module_resolver::create_parser();
        let result = module_resolver::resolve_and_parse(&inc_paths, module_name, &mut parser);
        self.core.note_shape_change();
        self.core.cache.insert(module_name.to_string(), result.clone());
        result
    }

    #[cfg(test)]
    pub(super) fn inc_paths(&self) -> Vec<PathBuf> {
        module_resolver::discover_inc_paths()
    }

    #[cfg(test)]
    pub fn resolve_module(&self, module_name: &str) -> Option<PathBuf> {
        let inc_paths = module_resolver::discover_inc_paths();
        module_resolver::resolve_module_path(&inc_paths, module_name)
    }
}

/// The capability `file_analysis`/`witnesses` query against. Delegates to
/// the inherent methods (inherent wins name resolution on `self`, so no
/// recursion); the generic inherent iterators accept the `&mut dyn FnMut`
/// trampolines directly.
impl CrossFileLookup for ModuleIndex {
    fn get_cached(&self, module_name: &str) -> Option<Arc<CachedModule>> {
        self.get_cached(module_name)
    }

    fn get_cached_scoped(
        &self,
        module_name: &str,
        visible: &std::collections::HashSet<String>,
    ) -> Option<Arc<CachedModule>> {
        self.get_cached_scoped(module_name, visible)
    }


    fn whole_present(&self, cached: &Arc<CachedModule>) -> Arc<FileAnalysis> {
        if cached.analysis.is_fully_resident() {
            return cached.analysis.clone();
        }
        self.rehydrate_or_resident(cached)
    }

    fn symbols_present(&self, cached: &Arc<CachedModule>) -> Arc<FileAnalysis> {
        // The @INC strip is bag-only, so import-tier copies (the MRO
        // walk's ancestor set) answer resident; workspace copies are
        // symbol-evicted and pay the same rehydration as `whole_present`.
        if !cached.analysis.symbols_are_evicted() {
            return cached.analysis.clone();
        }
        self.rehydrate_or_resident(cached)
    }

    fn ref_candidate_paths(&self, keys: &[String]) -> Vec<std::path::PathBuf> {
        self.with_rows_conn(|conn| {
            crate::index::module_cache::ref_candidate_files(conn, keys)
                .into_iter()
                .map(std::path::PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
    }

    fn ref_indexed_paths(&self) -> std::collections::HashSet<std::path::PathBuf> {
        self.with_rows_conn(|conn| {
            crate::index::module_cache::paths_with_ref_rows(conn)
                .into_iter()
                .map(std::path::PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
    }

    fn cached_by_path(&self, path: &std::path::Path) -> Option<Arc<CachedModule>> {
        // Pack sub-indexes: the per-path registry is O(1). The Perl hub's
        // cache is name-keyed with unique paths — linear fallback (hub
        // candidate sets are @INC-sized, not tree-sized).
        if let Some(cm) = self.all_files.get(path) {
            return Some(cm.value().clone());
        }
        self.core.cache.iter().find_map(|entry| {
            entry
                .value()
                .as_ref()
                .filter(|cm| cm.path == path)
                .cloned()
        })
    }

    fn enriched_present(&self, cached: &Arc<CachedModule>) -> Arc<FileAnalysis> {
        if !self.core.long_lived.load(std::sync::atomic::Ordering::Relaxed) {
            return self.bag_present(cached);
        }
        self.enriched_snapshot(cached)
            .unwrap_or_else(|| self.bag_present(cached))
    }
    fn bag_present(&self, cached: &Arc<CachedModule>) -> Arc<FileAnalysis> {
        // Never-evicted copy (open docs, degraded files kept whole): a cheap
        // Arc bump, no I/O.
        if !cached.analysis.bag_is_evicted() {
            return cached.analysis.clone();
        }
        self.rehydrate_or_resident(cached)
    }

    fn parents_cached(&self, module_name: &str) -> Vec<String> {
        self.parents_cached(module_name)
    }

    fn module_path_cached(&self, module_name: &str) -> Option<std::path::PathBuf> {
        self.module_path_cached(module_name)
    }

    fn modules_with_symbol(&self, name: &str) -> Vec<String> {
        self.modules_with_symbol(name)
    }

    fn find_exporters(&self, func_name: &str) -> Vec<String> {
        self.find_exporters(func_name)
    }

    fn defining_module_cached(&self, entry: &str, name: &str) -> Option<Arc<CachedModule>> {
        self.defining_module_cached(entry, name)
    }

    fn module_declaring_method_in_package(&self, name: &str, class: &str) -> Option<String> {
        self.module_declaring_method_in_package(name, class)
    }

    fn for_each_cached(&self, f: &mut dyn FnMut(&str, &Arc<CachedModule>)) {
        self.for_each_cached(f)
    }

    fn def_candidates(&self, name: &str) -> Vec<Arc<CachedModule>> {
        match self.all_defs.get(name) {
            Some(cands) if !cands.is_empty() => cands.clone(),
            // Perl hub: `all_defs` is pack-only, fall back to the winner.
            _ => self.get_cached(name).into_iter().collect(),
        }
    }

    fn for_each_cached_file(&self, f: &mut dyn FnMut(&Arc<CachedModule>)) {
        // `all_files` is the complete per-path registry (pack indexes, fed by
        // `register_symbols`) — the name-keyed views can't see a file that
        // lost all its name ties OR declares nothing registrable. The Perl
        // hub's cache is written directly (module-name keys, unique per
        // file), so union it in for both worlds.
        let mut seen: std::collections::HashSet<std::path::PathBuf> =
            std::collections::HashSet::new();
        for entry in self.all_files.iter() {
            if seen.insert(entry.key().clone()) {
                f(entry.value());
            }
        }
        for entry in self.core.cache.iter() {
            if let Some(ref cached) = *entry.value() {
                if seen.insert(cached.path.clone()) {
                    f(cached);
                }
            }
        }
    }

    fn for_each_reexport_module(
        &self,
        start: Vec<String>,
        visit: &mut dyn FnMut(&Arc<CachedModule>) -> std::ops::ControlFlow<()>,
    ) {
        self.for_each_reexport_module(start, visit)
    }

    fn for_each_entity_bridged_to(
        &self,
        class_name: &str,
        f: &mut dyn FnMut(&str, &Arc<CachedModule>, &crate::model::file_analysis::Symbol),
    ) {
        self.for_each_entity_bridged_to(class_name, f)
    }

    fn direct_children_of(&self, class: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for module in self.modules_with_parent(class) {
            let Some(cached) = self.get_cached(&module) else { continue };
            for (pkg, parents) in cached.analysis.package_parent_edges() {
                if parents.iter().any(|p| p == class) {
                    out.push((pkg.clone(), module.clone()));
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    fn direct_specializations_of(&self, primary: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let modules: Vec<String> = self
            .core
            .edges
            .specs
            .get(primary)
            .map(|v| v.clone())
            .unwrap_or_default();
        for module in modules {
            let Some(cached) = self.get_cached(&module) else { continue };
            for (spec, prim) in &cached.analysis.pack.specializes {
                if prim == primary {
                    out.push((spec.clone(), module.clone()));
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    fn for_each_loader_shape(&self, f: &mut dyn FnMut(&str, &crate::model::file_analysis::InferredType)) {
        for entry in self.core.loader_config_shapes.iter() {
            for (_contributor, t) in entry.value() {
                f(entry.key(), t);
            }
        }
    }

    fn complete_module_names(&self, prefix: &str) -> Vec<(String, bool)> {
        self.complete_module_names(prefix)
    }

    fn visible_defs_with_prefix(
        &self,
        prefix: &str,
        visible: &std::collections::HashSet<String>,
    ) -> Vec<(String, Arc<CachedModule>)> {
        self.visible_defs_with_prefix(prefix, visible)
    }
}
