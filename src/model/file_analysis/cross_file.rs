//! Cross-file capability: `CachedModule`, `SubInfo`, the `CrossFileLookup`
//! trait and `ScopedLookup`, plus the global path-intern table.

use super::*;

// ---- Cross-file lookup capability ----

/// A module in the cache — its filesystem path plus the full FileAnalysis of
/// its source. Shared by reference-count so async handlers don't deep-copy.
#[derive(Debug)]
pub struct CachedModule {
    pub path: std::path::PathBuf,
    pub analysis: std::sync::Arc<FileAnalysis>,
}

impl CachedModule {
    pub fn new(path: std::path::PathBuf, analysis: std::sync::Arc<FileAnalysis>) -> Self {
        CachedModule { path, analysis }
    }

    // Symbol/bag readers deliberately do NOT live on CachedModule: an index
    // copy may be evicted on any axis, so consumers mint the sibling on a
    // present view (`idx.whole_present(&cached).sub_info_view(..)` etc.) —
    // a convenience wrapper here would compile everywhere and silently
    // answer empty at scale.
}

/// A view into a module's metadata for a named sub/method.
///
/// Composed of a primary symbol plus any additional symbols with the same
/// name (for rw accessor setter overloads).
impl FileAnalysis {
    /// The `SubInfo` view over THIS analysis — mint it from a bag-present
    /// copy (`idx.bag_present(&cached)`) when the bag-backed accessors will
    /// be read; an evicted index copy answers those with `None`.
    pub fn sub_info_view(&self, name: &str) -> Option<SubInfo<'_>> {
        // Prefer the first matching Sub/Method symbol. Builder may emit several
        // when rw accessors exist (getter + setter); overloads are collected as
        // additional symbols with the same name.
        let mut syms = self
            .symbols
            .iter()
            .filter(|s| s.name == name && matches!(s.kind, SymKind::Sub | SymKind::Method));
        let primary = syms.next()?;
        let overloads: Vec<&Symbol> = syms.collect();

        // Keys are owned by `Sub { package: primary.package, name }` — the
        // sub's hash keys live under the same package as the sub itself.
        let hash_keys: Vec<String> = self
            .hash_key_defs_for_owner(&HashKeyOwner::Sub {
                package: primary.package.clone(),
                name: name.to_string(),
            })
            .iter()
            .map(|s| s.name.clone())
            .collect();

        Some(SubInfo { analysis: self, primary, overloads, hash_keys })
    }

    /// Locate a package-global variable declaration (`our $x` / `our @arr`
    /// / `our %h`) by its sigil-bearing name within `package`. Powers
    /// cross-file goto-def for a fully-qualified read (`$Foo::Bar::x`).
    /// `name` includes the sigil (`$x`, `@arr`, `%h`) to match how variable
    /// symbols are keyed.
    pub fn package_var_def_line(&self, name: &str, package: &str) -> Option<u32> {
        self.symbols
            .iter()
            .find(|s| {
                matches!(s.kind, SymKind::Variable | SymKind::Field)
                    && s.name == name
                    && s.package.as_deref() == Some(package)
            })
            .map(|s| s.span.start.row as u32)
    }

    /// True if a sub/method with this name is declared in this module
    /// *attributed to `package`* — not merely declared somewhere in the
    /// file. Cross-package typeglob installs
    /// (`*{'DateTime::'.$sub} = …` inside `package DateTime::PP`)
    /// synthesize a symbol whose `package` (DateTime) differs from the
    /// file's own module name (DateTime::PP), so a class-keyed method
    /// lookup must ask by package, not by module-name match.
    pub fn has_sub_in_package(&self, name: &str, package: &str) -> bool {
        self.symbols.iter().any(|s| {
            s.name == name
                && matches!(s.kind, SymKind::Sub | SymKind::Method)
                && s.package.as_deref() == Some(package)
        })
    }

    /// Completion candidates for `use Module qw(|)` — this module's export
    /// surface, `@EXPORT` first (sort tier 10) then `@EXPORT_OK` (tier 20),
    /// deduped. Detail carries the resolved return type when known. The
    /// adapter projects these; the "still indexing" affordance for a
    /// not-yet-cached module is the adapter's (there's no entity to gather).
    pub fn import_list_candidates(&self) -> Vec<CompletionCandidate> {
        let mut items = Vec::new();
        let mut seen = HashSet::new();
        for name in &self.export {
            if seen.insert(name.clone()) {
                let detail = self
                    .sub_info_view(name)
                    .and_then(|s| s.return_type(None))
                    .map(|rt| format!("@EXPORT → {}", format_inferred_type(&rt)))
                    .or_else(|| Some("@EXPORT".to_string()));
                items.push(CompletionCandidate {
                    label: name.clone(),
                    kind: SymKind::Sub,
                    detail,
                    insert_text: None,
                    sort_priority: 10,
                    additional_edits: vec![],
                    import_fact: None,
                    display_override: None,
                });
            }
        }
        for name in &self.export_ok {
            if seen.insert(name.clone()) {
                let detail = self
                    .sub_info_view(name)
                    .and_then(|s| s.return_type(None))
                    .map(|rt| format!("→ {}", format_inferred_type(&rt)));
                items.push(CompletionCandidate {
                    label: name.clone(),
                    kind: SymKind::Sub,
                    detail,
                    insert_text: None,
                    sort_priority: 20,
                    additional_edits: vec![],
                    import_fact: None,
                    display_override: None,
                });
            }
        }
        items
    }
}

pub struct SubInfo<'a> {
    analysis: &'a FileAnalysis,
    primary: &'a Symbol,
    #[allow(dead_code)] // retained for the `param_counts` / `return_type_for_arity` API surface
    overloads: Vec<&'a Symbol>,
    hash_keys: Vec<String>,
}

impl<'a> SubInfo<'a> {
    pub fn def_line(&self) -> u32 {
        self.primary.span.start.row as u32
    }

    pub fn params(&self) -> &'a [ParamInfo] {
        match &self.primary.detail {
            SymbolDetail::Sub { params, .. } => params,
            _ => &[],
        }
    }

    pub fn is_method(&self) -> bool {
        if self.primary.kind == SymKind::Method {
            return true;
        }
        matches!(
            self.primary.detail,
            SymbolDetail::Sub { is_method: true, .. }
        )
    }

    /// Pass `module_index` so a return type produced by a cross-file method
    /// chain in the sub body resolves; `None` keeps it single-file.
    pub fn return_type(&self, module_index: Option<&dyn CrossFileLookup>) -> Option<InferredType> {
        match &self.primary.detail {
            SymbolDetail::Sub { .. } => {
                self.analysis.symbol_return_type_via_bag_ctx(self.primary.id, None, module_index)
            }
            _ => None,
        }
    }

    pub fn doc(&self) -> Option<&'a str> {
        match &self.primary.detail {
            SymbolDetail::Sub { doc, .. } => doc.as_deref(),
            _ => None,
        }
    }

    pub fn hash_keys(&self) -> &[String] {
        &self.hash_keys
    }

    /// Arity list covering the primary and overloads, in declaration order.
    #[allow(dead_code)] // public SubInfo accessor; consumed by tooling/future cross-file callers
    pub fn param_counts(&self) -> Vec<usize> {
        std::iter::once(self.primary)
            .chain(self.overloads.iter().copied())
            .map(|s| match &s.detail {
                SymbolDetail::Sub { params, .. } => params.len(),
                _ => 0,
            })
            .collect()
    }

    /// Return type for an overload with the given arity, if any matches.
    #[allow(dead_code)] // public SubInfo accessor; consumed by tooling/future cross-file callers
    pub fn return_type_for_arity(&self, arity: usize, module_index: Option<&dyn CrossFileLookup>) -> Option<InferredType> {
        for sym in std::iter::once(self.primary).chain(self.overloads.iter().copied()) {
            if let SymbolDetail::Sub { params, .. } = &sym.detail {
                if params.len() == arity {
                    return self.analysis.symbol_return_type_via_bag_ctx(sym.id, Some(arity), module_index);
                }
            }
        }
        None
    }

    /// SymbolId of the primary (first matching) sym.
    #[allow(dead_code)] // public SubInfo accessor; consumed by tooling/future cross-file callers
    pub fn primary_id(&self) -> SymbolId {
        self.primary.id
    }

    /// SymbolId of the overload whose param count matches `arity`,
    /// if any.
    #[allow(dead_code)] // public SubInfo accessor; consumed by tooling/future cross-file callers
    pub fn id_for_arity(&self, arity: usize) -> Option<SymbolId> {
        for sym in std::iter::once(self.primary).chain(self.overloads.iter().copied()) {
            if let SymbolDetail::Sub { params, .. } = &sym.detail {
                if params.len() == arity {
                    return Some(sym.id);
                }
            }
        }
        None
    }

    /// Inferred type for a param by name (if the analysis resolved one).
    /// Goes through the canonical bag-aware query so framework rules
    /// (Mojo `$self` etc.) apply consistently across every consumer.
    pub fn param_inferred_type(&self, param_name: &str) -> Option<InferredType> {
        self.analysis
            .inferred_type_via_bag(param_name, self.primary.span.end)
    }
}

/// What query-time cross-file resolution needs from the dependency
/// index. `ModuleIndex` implements this; `file_analysis`/`witnesses`
/// depend on the capability, not the index — the inversion that breaks
/// the FA ↔ index cycle (dependency inversion; the index implements it).
///
/// Object-safe by design: a `&dyn CrossFileLookup` rides
/// `witnesses::BagContext`, hence the `&mut dyn FnMut` callback params.
/// Process-global path interner (`docs/adr/relational-ref-index.md`,
/// residency phases): closure paths repeat across nearly every file in a
/// tree (abseil shares ~90% of its header universe per TU), so resident
/// copies share ONE allocation per unique path instead of one per
/// (file × path). Serialized form stays a plain string sequence — blob
/// layout unchanged, interning happens on the way in.
pub mod path_intern {
    use std::sync::{Arc, OnceLock};

    // ---- Global path-id table (the ClosureList substrate) ----
    //
    // Closures at scale are the largest resident bucket as 16-byte
    // `Arc<str>` pointer vecs (chromium: 2.8 GB / 41% of the floor). A
    // sorted `Arc<[u32]>` over one process-global id table is 4× smaller
    // per entry and turns the hot membership gate into id-compare binary
    // search. IDs are process-local (never serialized — the blob keeps
    // `Vec<String>`), so the table only ever grows within a session.

    use std::collections::HashMap;
    use std::sync::RwLock;

    struct PathIds {
        by_str: HashMap<Arc<str>, u32>,
        by_id: Vec<Arc<str>>,
    }

    static IDS: OnceLock<RwLock<PathIds>> = OnceLock::new();

    fn ids() -> &'static RwLock<PathIds> {
        IDS.get_or_init(|| {
            RwLock::new(PathIds { by_str: HashMap::new(), by_id: Vec::new() })
        })
    }

    /// The id for `s`, minting one if unseen.
    fn id_intern(s: &str) -> u32 {
        {
            let g = ids().read().unwrap();
            if let Some(&id) = g.by_str.get(s) {
                return id;
            }
        }
        let mut g = ids().write().unwrap();
        if let Some(&id) = g.by_str.get(s) {
            return id;
        }
        let a: Arc<str> = Arc::from(s);
        let id = g.by_id.len() as u32;
        g.by_id.push(a.clone());
        g.by_str.insert(a, id);
        id
    }

    /// The id for `s` ONLY if some closure already interned it — a miss
    /// means no closure can contain it (lookups must not grow the table).
    fn id_lookup(s: &str) -> Option<u32> {
        ids().read().unwrap().by_str.get(s).copied()
    }

    fn str_of(id: u32) -> Arc<str> {
        ids().read().unwrap().by_id[id as usize].clone()
    }

    /// Process-wide table cost (counted ONCE, not per file): unique paths
    /// and their string bytes across both the Arc pool and the id table.
    pub fn table_stats() -> (usize, usize) {
        let g = ids().read().unwrap();
        let bytes: usize = g
            .by_id
            .iter()
            .map(|a| a.len() + std::mem::size_of::<Arc<str>>() * 2 + 8)
            .sum();
        (g.by_id.len(), bytes)
    }

    /// The id for `s` if any closure has interned it — `None` means no
    /// closure anywhere contains it. The one-per-query half of the
    /// `contains_id` fast path.
    pub fn lookup_id(s: &str) -> Option<u32> {
        id_lookup(s)
    }

    /// A file's `#include` closure as sorted path-ids over the global
    /// table. Semantically a set of path strings; consumers ask membership
    /// (`contains`) or iterate the strings — the representation is private
    /// so it can keep shrinking (`docs/forks-resolved.md`, closure
    /// representation fork).
    #[derive(Debug, Clone, Default)]
    pub struct ClosureList(Arc<[u32]>);

    impl ClosureList {
        pub fn from_iter<'a>(items: impl Iterator<Item = &'a str>) -> Self {
            let mut v: Vec<u32> = items.map(id_intern).collect();
            v.sort_unstable();
            v.dedup();
            ClosureList(v.into())
        }

        pub fn contains(&self, s: &str) -> bool {
            match id_lookup(s) {
                Some(id) => self.0.binary_search(&id).is_ok(),
                None => false,
            }
        }

        /// Membership by pre-resolved id — hot loops (the backward walk's
        /// visibility gate runs once per candidate file) resolve the query
        /// string to an id ONCE via `lookup_id` and test lock-free here.
        pub fn contains_id(&self, id: u32) -> bool {
            self.0.binary_search(&id).is_ok()
        }

        pub fn is_empty(&self) -> bool {
            self.0.is_empty()
        }

        /// The member paths as shared strings (save path, visibility sets).
        pub fn iter_strs(&self) -> impl Iterator<Item = Arc<str>> + '_ {
            self.0.iter().map(|&id| str_of(id))
        }

        /// Per-file resident bytes (the id array; the global table is
        /// counted once process-wide, not per file).
        pub fn heap_bytes(&self) -> usize {
            self.0.len() * std::mem::size_of::<u32>()
        }
    }

    impl serde::Serialize for ClosureList {
        fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            s.collect_seq(self.iter_strs().map(|a| a.as_ref().to_owned()))
        }
    }

    impl<'de> serde::Deserialize<'de> for ClosureList {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            let raw = <Vec<String> as serde::Deserialize>::deserialize(d)?;
            Ok(ClosureList::from_iter(raw.iter().map(|s| s.as_str())))
        }
    }
}

pub trait CrossFileLookup {
    fn get_cached(&self, module_name: &str) -> Option<std::sync::Arc<CachedModule>>;
    /// `get_cached` scoped to a querying file's VISIBILITY set (its own path +
    /// its `#include` closure) — see `ModuleIndex::get_cached_scoped`. Default:
    /// ignore the scope (identical to `get_cached`), so non-index impls and
    /// languages with no include model are unaffected. `ScopedLookup` and the
    /// pack `ModuleIndex` override it to rank same-name candidates by reachability.
    fn get_cached_scoped(
        &self,
        module_name: &str,
        _visible: &std::collections::HashSet<String>,
    ) -> Option<std::sync::Arc<CachedModule>> {
        self.get_cached(module_name)
    }
    /// EVERY cached file defining `name` (the pack index's full candidate
    /// table), not just the one-winner `get_cached` view — for consumers that
    /// must weigh candidates themselves (definition-over-prototype). Default:
    /// the winner alone.
    fn def_candidates(&self, name: &str) -> Vec<std::sync::Arc<CachedModule>> {
        self.get_cached(name).into_iter().collect()
    }
    /// A cached module's analysis with its witness bag GUARANTEED present.
    /// Slice 2 evicts the bag from resident pack-index copies; every TYPE
    /// query that reads a foreign file's bag (the `MethodOnClass` / `SlotType`
    /// / `TypeName` cross-file chases, `def_candidates` return-type folds,
    /// cross-file field types) routes through here so the exact persisted bag
    /// rehydrates on demand. Default (Perl hub, tests, non-pack impls): a cheap
    /// `Arc` bump — those copies are never evicted. The pack `ModuleIndex`
    /// overrides it to rehydrate from its `PackBagCache` when the bag is
    /// evicted. See `docs/adr/memory-slice-2-lru.md`.
    fn bag_present(
        &self,
        cached: &std::sync::Arc<CachedModule>,
    ) -> std::sync::Arc<FileAnalysis> {
        cached.analysis.clone()
    }
    /// A cached WORKSPACE module's analysis with cross-file ENRICHMENT
    /// applied (`docs/adr/storage-engine.md`, the always-enriched
    /// tier): imported return types propagated, synthetic hash-key defs
    /// injected — derived through the overlay, never in-place. Consumers
    /// are FALLBACK-ON-MISS: call this only after the raw bag answered
    /// None (a miss pays one deep-copy+enrich, then the overlay caches by
    /// dep-surface fingerprint). Default: the bag-present view — impls
    /// without an overlay answer unenriched, never wrongly.
    fn enriched_present(
        &self,
        cached: &std::sync::Arc<CachedModule>,
    ) -> std::sync::Arc<FileAnalysis> {
        self.bag_present(cached)
    }
    /// A cached module's analysis whole on EVERY evictable axis — bag, refs,
    /// AND symbols present. Consumers that read more than one axis from the
    /// same copy (the diagnostics sweep, the `refs_to` matcher, `sub_info`
    /// readers, heatmap/parity enumeration) route here: a single-axis view
    /// returns the resident copy when its own axis survived but a sibling
    /// was evicted (the shred-failure degradation path), silently dropping
    /// the other axis's answers.
    fn whole_present(
        &self,
        cached: &std::sync::Arc<CachedModule>,
    ) -> std::sync::Arc<FileAnalysis> {
        cached.analysis.clone()
    }
    /// Every indexed file holding at least one ref row keyed by one of
    /// `keys` — the relational reverse index's candidate-file retrieval
    /// (`SELECT DISTINCT path … WHERE name_id IN keys`). The backward walk
    /// rehydrates these and runs the one matcher over them. Default: empty
    /// (impls without a row store contribute no candidates; the resident
    /// sweep still covers their files).
    fn ref_candidate_paths(&self, _keys: &[String]) -> Vec<std::path::PathBuf> {
        Vec::new()
    }
    /// Every path this index has SHREDDED into the relational row store
    /// (the `files` table — the single "rows present" marker). A file in
    /// this set but ABSENT from `ref_candidate_paths(keys)` has no ref or
    /// sym row for those names, so — rows over-approximate references — it
    /// provably matches nothing and the backward walk can skip rehydrating
    /// it. Empty (default, or no row store) ⇒ no narrowing; the resident
    /// sweep whole-views every gate-passing file as before. `docs/adr/
    /// relational-ref-index.md`.
    fn ref_indexed_paths(&self) -> std::collections::HashSet<std::path::PathBuf> {
        std::collections::HashSet::new()
    }
    /// Path-keyed cached-module lookup — the retrieval above hands back
    /// paths; this maps them onto the resident registration (for the
    /// visibility gate + whole-copy rehydration). Default `None`.
    fn cached_by_path(
        &self,
        _path: &std::path::Path,
    ) -> Option<std::sync::Arc<CachedModule>> {
        None
    }
    fn parents_cached(&self, module_name: &str) -> Vec<String>;
    fn modules_with_symbol(&self, name: &str) -> Vec<String>;
    fn find_exporters(&self, func_name: &str) -> Vec<String>;
    fn defining_module_cached(&self, entry: &str, name: &str) -> Option<std::sync::Arc<CachedModule>>;
    fn module_declaring_method_in_package(&self, name: &str, class: &str) -> Option<String>;
    /// The on-disk path a module name resolves to (Perl module goto-def).
    /// Default `None` for impls without a path map.
    fn module_path_cached(&self, _module_name: &str) -> Option<std::path::PathBuf> {
        None
    }
    /// The querying file's visibility scope when this lookup is bound to one
    /// (`ScopedLookup`): its own canonical path + the visible set (self path ∪
    /// include closure, canonical strings). `None` for unscoped indexes.
    /// The backward reference gate mints a pack target's `def_paths` from this
    /// so def→uses matching runs under the SAME visibility forward resolution
    /// uses (`resolve::pack_def_paths`).
    fn visibility_scope(
        &self,
    ) -> Option<(&std::path::Path, &std::collections::HashSet<String>)> {
        None
    }
    fn for_each_cached(&self, f: &mut dyn FnMut(&str, &std::sync::Arc<CachedModule>));
    /// Visit every distinct cached FILE exactly once. `for_each_cached` is
    /// keyed by NAME with one winner per key, so a pack file that loses every
    /// name tie (two fixtures both declaring `is_scope`) is invisible there —
    /// any whole-project sweep (find-references, macro variants, include
    /// reverse) must use this instead. Default: path-dedup over
    /// `for_each_cached` (correct for the Perl hub, whose module-name keys
    /// are unique per file); the pack index overrides with its complete
    /// per-file candidate table.
    fn for_each_cached_file(&self, f: &mut dyn FnMut(&std::sync::Arc<CachedModule>)) {
        let mut seen: std::collections::HashSet<std::path::PathBuf> =
            std::collections::HashSet::new();
        self.for_each_cached(&mut |_n, cached| {
            if seen.insert(cached.path.clone()) {
                f(cached);
            }
        });
    }
    fn for_each_reexport_module(
        &self,
        start: Vec<String>,
        visit: &mut dyn FnMut(&std::sync::Arc<CachedModule>) -> std::ops::ControlFlow<()>,
    );
    fn for_each_entity_bridged_to(
        &self,
        class_name: &str,
        f: &mut dyn FnMut(&str, &std::sync::Arc<CachedModule>, &Symbol),
    );
    /// Direct children/composers of `class` as (package, module) pairs
    /// — the `children_index` inverse, depth 1 (the graph walker
    /// supplies transitivity).
    fn direct_children_of(&self, class: &str) -> Vec<(String, String)>;
    /// Template specializations of `primary` as (spec, module) pairs — the
    /// cross-file half of the graph's `Specializes` family edge (the local
    /// half reads `FileAnalysis.pack.specializes`). Default: none (the Perl hub
    /// and language-less impls have no specialization index).
    fn direct_specializations_of(&self, _primary: &str) -> Vec<(String, String)> {
        Vec::new()
    }
    /// Registration-time loader-config shapes: every (load_name, shape)
    /// projected from `PluginLoad` facts across the workspace —
    /// INCLUDING packageless entrypoint scripts, which never enter the
    /// module cache.
    fn for_each_loader_shape(&self, f: &mut dyn FnMut(&str, &InferredType));
    /// Loadable module names matching `prefix` for completion, as
    /// (name, is_resolved) — resolved modules have full analysis, the rest
    /// are @INC-scanned availability. Defaults empty so lookups that have
    /// no module universe stay honest without stubbing.
    fn complete_module_names(&self, _prefix: &str) -> Vec<(String, bool)> {
        Vec::new()
    }
    /// Completion-GATHERING mirror of `get_cached_scoped`: every registered
    /// name starting with `prefix` that has a definition candidate inside
    /// `visible` (canonical paths — the querying file's `#include` closure).
    /// No global fallback — a file is never offered symbols from headers it
    /// doesn't include. Defaults empty (the Perl hub has no closure model).
    fn visible_defs_with_prefix(
        &self,
        _prefix: &str,
        _visible: &std::collections::HashSet<String>,
    ) -> Vec<(String, std::sync::Arc<CachedModule>)> {
        Vec::new()
    }
}

/// A `CrossFileLookup` decorator scoped to ONE querying file's include-closure
/// visibility. Every cross-file resolution routed through it ranks same-name
/// candidates by reachability (`get_cached` → `inner.get_cached_scoped`), so a
/// file resolves `class Box` to the `Box` it can actually see — not an unrelated
/// file's same-named class (C's flat linkage). Wrap the pack index once per
/// request at the LSP/CLI entry point; every downstream `get_cached` inherits
/// the scope with no threaded parameter. `visible` empty ⇒ transparent
/// (Perl / unwarmed on-open). `docs/adr/macro-handling.md`.
pub struct ScopedLookup<'a> {
    inner: &'a dyn CrossFileLookup,
    visible: std::collections::HashSet<String>,
    self_path: Option<std::path::PathBuf>,
}

impl<'a> ScopedLookup<'a> {
    /// Build the visibility set from a querying file's include closure plus its
    /// own path (a file always sees the classes it declares itself). Canonicalize
    /// the self path so it matches the candidates' canonical `CachedModule.path`.
    pub fn new(
        inner: &'a dyn CrossFileLookup,
        include_closure: &path_intern::ClosureList,
        self_path: Option<&std::path::Path>,
    ) -> Self {
        let mut visible: std::collections::HashSet<String> =
            include_closure.iter_strs().map(|a| a.as_ref().to_owned()).collect();
        let self_path = self_path.map(|p| {
            let canon = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
            visible.insert(canon.to_string_lossy().into_owned());
            canon
        });
        ScopedLookup { inner, visible, self_path }
    }
}

impl<'a> CrossFileLookup for ScopedLookup<'a> {
    fn get_cached(&self, module_name: &str) -> Option<std::sync::Arc<CachedModule>> {
        self.inner.get_cached_scoped(module_name, &self.visible)
    }
    fn get_cached_scoped(
        &self,
        module_name: &str,
        _visible: &std::collections::HashSet<String>,
    ) -> Option<std::sync::Arc<CachedModule>> {
        self.inner.get_cached_scoped(module_name, &self.visible)
    }
    fn def_candidates(&self, name: &str) -> Vec<std::sync::Arc<CachedModule>> {
        // Unscoped by design: consumers of the full candidate table weigh
        // definition-ness themselves, and a definition legitimately lives
        // OUTSIDE the querying file's closure (a `.c` body nobody includes).
        self.inner.def_candidates(name)
    }
    fn bag_present(
        &self,
        cached: &std::sync::Arc<CachedModule>,
    ) -> std::sync::Arc<FileAnalysis> {
        // MUST delegate: the inner pack index owns the `PackBagCache`. Without
        // this, cpp cross-file type queries (which thread a `ScopedLookup`)
        // hit the trait default and read the evicted bag — silent Slice-2
        // type regressions while goto/refs stay green.
        self.inner.bag_present(cached)
    }
    fn enriched_present(
        &self,
        cached: &std::sync::Arc<CachedModule>,
    ) -> std::sync::Arc<FileAnalysis> {
        // Same delegation rule as `bag_present` — the inner index owns the
        // enrichment overlay.
        self.inner.enriched_present(cached)
    }
    fn whole_present(
        &self,
        cached: &std::sync::Arc<CachedModule>,
    ) -> std::sync::Arc<FileAnalysis> {
        // Same delegation rule as `bag_present` — the inner index owns the LRU.
        self.inner.whole_present(cached)
    }
    fn ref_candidate_paths(&self, keys: &[String]) -> Vec<std::path::PathBuf> {
        // Unscoped by design, like `def_candidates`: the backward walk applies
        // its own per-file closure gate; pre-narrowing here would hide sites
        // in files the textual-inclusion extension admits.
        self.inner.ref_candidate_paths(keys)
    }
    fn ref_indexed_paths(&self) -> std::collections::HashSet<std::path::PathBuf> {
        self.inner.ref_indexed_paths()
    }
    fn cached_by_path(
        &self,
        path: &std::path::Path,
    ) -> Option<std::sync::Arc<CachedModule>> {
        self.inner.cached_by_path(path)
    }
    fn parents_cached(&self, module_name: &str) -> Vec<String> {
        self.inner.parents_cached(module_name)
    }
    fn modules_with_symbol(&self, name: &str) -> Vec<String> {
        self.inner.modules_with_symbol(name)
    }
    fn find_exporters(&self, func_name: &str) -> Vec<String> {
        self.inner.find_exporters(func_name)
    }
    fn defining_module_cached(&self, entry: &str, name: &str) -> Option<std::sync::Arc<CachedModule>> {
        self.inner.defining_module_cached(entry, name)
    }
    fn module_declaring_method_in_package(&self, name: &str, class: &str) -> Option<String> {
        self.inner.module_declaring_method_in_package(name, class)
    }
    fn module_path_cached(&self, module_name: &str) -> Option<std::path::PathBuf> {
        // Scope-aware: the path must name the same candidate the scoped
        // `get_cached` answers with, or a consumer that pairs this path with a
        // scoped range splices two different files (wrong file at a
        // nonexistent position). Fall back to the raw path map only when no
        // analysis is cached at all.
        self.inner
            .get_cached_scoped(module_name, &self.visible)
            .map(|c| c.path.clone())
            .or_else(|| self.inner.module_path_cached(module_name))
    }
    fn visibility_scope(
        &self,
    ) -> Option<(&std::path::Path, &std::collections::HashSet<String>)> {
        self.self_path.as_deref().map(|p| (p, &self.visible))
    }
    fn for_each_cached(&self, f: &mut dyn FnMut(&str, &std::sync::Arc<CachedModule>)) {
        self.inner.for_each_cached(f)
    }
    fn for_each_cached_file(&self, f: &mut dyn FnMut(&std::sync::Arc<CachedModule>)) {
        self.inner.for_each_cached_file(f)
    }
    fn for_each_reexport_module(
        &self,
        start: Vec<String>,
        visit: &mut dyn FnMut(&std::sync::Arc<CachedModule>) -> std::ops::ControlFlow<()>,
    ) {
        self.inner.for_each_reexport_module(start, visit)
    }
    fn for_each_entity_bridged_to(
        &self,
        class_name: &str,
        f: &mut dyn FnMut(&str, &std::sync::Arc<CachedModule>, &Symbol),
    ) {
        self.inner.for_each_entity_bridged_to(class_name, f)
    }
    fn direct_children_of(&self, class: &str) -> Vec<(String, String)> {
        self.inner.direct_children_of(class)
    }
    fn direct_specializations_of(&self, primary: &str) -> Vec<(String, String)> {
        self.inner.direct_specializations_of(primary)
    }
    fn for_each_loader_shape(&self, f: &mut dyn FnMut(&str, &InferredType)) {
        self.inner.for_each_loader_shape(f)
    }
    fn visible_defs_with_prefix(
        &self,
        prefix: &str,
        visible: &std::collections::HashSet<String>,
    ) -> Vec<(String, std::sync::Arc<CachedModule>)> {
        self.inner.visible_defs_with_prefix(prefix, visible)
    }
}

