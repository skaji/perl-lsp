//! FileAnalysis lifecycle: construction, eviction, index building,
//! post-walk finalization, and the heap-estimate probe.

use super::*;

impl FileAnalysis {
    /// Create a new FileAnalysis with indices built from the raw tables.
    /// `finalize_post_walk` runs on the builder path to seal baseline
    /// counts and resolve text-based MCB; hand-crafted test FAs skip it
    /// and push witnesses directly.
    pub fn new(parts: FileAnalysisParts) -> Self {
        let FileAnalysisParts {
            scopes,
            symbols,
            refs,
            fold_ranges,
            imports,
            call_bindings,
            package_parents,
            method_call_bindings,
            framework_imports,
            export,
            export_ok,
            export_tags,
            reexport_modules,
            plugin_namespaces,
            package_uses,
            type_provenance,
            package_ranges,
            plugin_diagnostics,
            app_surface_consumers,
            mut witnesses,
            package_framework,
            provisional_dispatches,
            gated_emissions,
            guard_sites,
            arrow_deref_sites,
            gated_param_types,
            attr_projections,
            reassigned_scalars,
            key_writes,
            role_requires,
            contract_symbols,
            dynamic_parent_packages,
            role_packages,
            dbic_source_name,
            column_keyed_verbs,
            dynamic_dispatch_sites,
            plugin_loads,
            loader_config_params,
            flow_edges,
            moved_from,
            control_regions,
            param_regions,
            domain_sites,
        } = parts;
        witnesses.rebuild_index();
        let mut fa = FileAnalysis {
            receiver_names: Vec::new(),
            specializes: HashMap::new(),
            template_params: HashMap::new(),
            scopes,
            symbols,
            refs,
            fold_ranges,
            imports,
            call_bindings,
            method_call_bindings,
            package_ranges,
            plugin_diagnostics,
            package_parents,
            app_surface_consumers,
            package_uses,
            framework_imports,
            export,
            export_ok,
            export_tags,
            reexport_modules,
            plugin_namespaces,
            type_provenance,
            witnesses,
            bag_evicted: false,
            refs_evicted: false,
            symbols_evicted: false,
            package_framework,
            base_symbol_count: 0,
            base_witness_count: 0,
            base_ref_count: 0,
            provisional_dispatches,
            gated_emissions,
            guard_sites,
            arrow_deref_sites,
            gated_param_types,
            attr_projections,
            reassigned_scalars,
            key_writes,
            role_requires,
            contract_symbols,
            dynamic_parent_packages,
            role_packages,
            dbic_source_name,
            column_keyed_verbs,
            dynamic_dispatch_sites,
            plugin_loads,
            loader_config_params,
            flow_edges,
            moved_from,
            control_regions,
            param_regions,
            domain_sites,
            // Populated by the pack driver post-construction (macro identity lane).
            macro_defs: Vec::new(),
            // Populated post-construction: `include_directives` from the skeleton,
            // `include_closure` by the driver (it holds the resolving file path).
            include_directives: Vec::new(),
            include_closure: path_intern::ClosureList::default(),
            degraded: false,
            // Pack drivers re-stamp their id post-construction.
            language: super::default_language(),
            scope_starts: Vec::new(),
            symbols_by_name: HashMap::new(),
            symbols_by_scope: HashMap::new(),
            refs_by_name: HashMap::new(),
            refs_by_target: HashMap::new(),
            call_ref_by_start: HashMap::new(),
            export_lookup: HashSet::new(),
        };
        fa.build_indices();
        fa
    }

    /// Run the local-only method-call-binding resolution and seal
    /// baseline counts. Called by `builder::build` after the witness
    /// bag has been moved in.
    ///
    /// `Symbol(sym_id)` and `MethodOnClass{class, name}` return-type
    /// witnesses for every local Sub/Method are already in the bag —
    /// published by `Builder::write_back_sub_return_types` at the
    /// end of the worklist (single emission point for "this sub's
    /// return type is known"). Cross-file imports do not get a local
    /// mirror; they resolve lazily through `query_sub_return_type`.
    /// Drop the witness bag (the build-time type-inference scaffold) from
    /// this resident analysis after the fold baked its conclusions into pinned
    /// fields. The full bag rides the on-disk blob, so this is lossless — a
    /// type query needing it rehydrates the exact persisted bag on demand
    /// (`docs/adr/memory-slice-2-lru.md`). Clears both the `Vec<Witness>` and
    /// its rebuilt index; touches no pinned field (refs, symbols, return_types,
    /// resolved_method_target all survive). Idempotent.
    pub fn evict_witness_bag(&mut self) {
        self.witnesses = crate::model::witnesses::WitnessBag::default();
        self.bag_evicted = true;
    }

    /// True when `evict_witness_bag` stripped this copy's bag: an empty bag
    /// here means "on disk, not resident", not "no type facts".
    pub fn bag_is_evicted(&self) -> bool {
        self.bag_evicted
    }

    /// Strip the resident `refs` (and the ref-keyed rebuilt indexes) from an
    /// index copy whose blob + relational rows are persisted — the refs twin
    /// of `evict_witness_bag`. Lossless: the on-disk analysis keeps the full
    /// vec; the backward walk retrieves candidates from the relational index
    /// and rehydrates through `whole_present`. Touches no other pinned field.
    /// Idempotent.
    pub fn evict_refs(&mut self) {
        self.refs = Vec::new();
        self.refs_by_name = std::collections::HashMap::new();
        self.refs_by_target = std::collections::HashMap::new();
        self.call_ref_by_start = std::collections::HashMap::new();
        self.refs_evicted = true;
    }

    /// True when `evict_refs` stripped this copy's refs: empty means "on
    /// disk, not resident", never "no references".
    // Asserted by the eviction tests and read by the (currently unwired)
    // `refs_present` seam; keep in step with `symbols_are_evicted`.
    #[allow(dead_code)]
    pub fn refs_are_evicted(&self) -> bool {
        self.refs_evicted
    }

    /// Strip the resident `symbols` (and the symbol-keyed rebuilt indexes)
    /// from an index copy whose blob + `syms` rows are persisted — the third
    /// eviction axis. Lossless: the on-disk analysis keeps the full vec;
    /// enumeration (workspace/symbol) reads rows, detail reads rehydrate.
    /// Registration feeds were extracted BEFORE this runs. Touches no other
    /// pinned field (`export`/`export_ok`/`export_lookup` derive from export
    /// lists, not symbols, and stay). Idempotent.
    pub fn evict_symbols(&mut self) {
        self.symbols = Vec::new();
        self.symbols_by_name = std::collections::HashMap::new();
        self.symbols_by_scope = std::collections::HashMap::new();
        self.symbols_evicted = true;
    }

    /// True when `evict_symbols` stripped this copy's symbols: empty means
    /// "on disk, not resident", never "no symbols".
    pub fn symbols_are_evicted(&self) -> bool {
        self.symbols_evicted
    }

    /// Whole on EVERY evictable axis — the property `whole_present` gates
    /// on. New eviction axes extend THIS conjunction (and their `evict_*`
    /// setter), so multi-axis consumers stay whole-covered by construction
    /// instead of each spelling its own flag list.
    pub fn is_fully_resident(&self) -> bool {
        !self.bag_evicted && !self.refs_evicted && !self.symbols_evicted
    }

    /// The ONE speller of the registration strip: `strip_bag` drops the
    /// witness bag; `strip_rows` drops the row-backed axes (refs AND
    /// symbols — they persist as one generation and evict as one). Every
    /// registration path routes here so a new eviction axis is added in
    /// exactly one place; a site spelling `evict_*` calls directly is
    /// re-stating this pairing by convention.
    pub fn evict_axes(&mut self, strip_bag: bool, strip_rows: bool) {
        if strip_bag {
            self.evict_witness_bag();
        }
        if strip_rows {
            self.evict_refs();
            self.evict_symbols();
        }
    }

    pub(crate) fn finalize_post_walk(&mut self) {
        self.resolve_method_call_types(None);
        // Fill HashKeyAccess owners that are resolvable in-file
        // via the invocant ladder (`method_call_invocant_type`).
        // Cross-file gaps stay None until
        // `enrich_imported_types_with_keys` re-runs the same
        // routine with `module_index`.
        self.fix_chain_receiver_hash_key_owners(None);
        // Stamp the build-time-resolved dispatch target on MethodCall
        // refs (local-only here; enrichment re-stamps with the index
        // for OPEN docs). Mutates existing refs in place, so it must run
        // before sealing base_ref_count — the seal counts the refs, the
        // stamp only sets a field on them.
        self.stamp_method_call_targets(None);
        self.base_symbol_count = self.symbols.len();
        self.base_witness_count = self.witnesses.len();
        self.base_ref_count = self.refs.len();
    }

    /// Stamp `resolved_method_target` on every `MethodCall` ref — the NAV
    /// unification edge (build pipeline phase 6 `PostFold`, then re-stamped
    /// at enrichment). The invocant class is resolved ONCE here (via the
    /// bag-routed `method_call_invocant_class`) and frozen on the ref;
    /// `refs_to` / `find_definition` / hover read the frozen edge instead of
    /// re-deriving the class at query time, so they can never diverge.
    ///
    /// Contract: if the invocant class does not infer, store `None` (honest
    /// miss). No name-only fallback — that re-introduces the `->new` flood.
    pub(crate) fn stamp_method_call_targets(&mut self, module_index: Option<&dyn CrossFileLookup>) {
        // Collect resolutions first; `method_call_invocant_class` /
        // `resolve_method_in_ancestors` borrow `&self`, so we can't hold a
        // `&mut self.refs[i]` while calling them.
        let mut stamped: Vec<(usize, Option<MethodTarget>)> = Vec::new();
        for (i, r) in self.refs.iter().enumerate() {
            // A plugin-bridged invocant must NEVER freeze as a class:
            // its resolution needs the index + the owning plugin, absent
            // at build time. Leaving the edge `None` makes `refs_to` /
            // goto-def re-consult the plugin at query time (with the
            // index in hand) instead of trusting a guessed token.
            if !matches!(r.kind, RefKind::MethodCall { .. })
                || matches!(&r.kind, RefKind::MethodCall { invocant, .. } if invocant.is_bridged())
            {
                continue;
            }
            let target = self
                .method_call_invocant_class(r, module_index)
                .map(|cn| {
                    match self.resolve_method_in_ancestors(&cn, r.unqualified_target_name(), module_index) {
                        Some(MethodResolution::Local { sym_id, .. }) => MethodTarget::Local {
                            sym_id,
                            invocant_class: cn,
                        },
                        // Method found cross-file, OR the invocant class is
                        // known but the method isn't found on it locally and
                        // the class has cross-file parents / a cross-file
                        // body the index may carry. Either way the invocant
                        // froze, so keep the edge (CrossFile); the rename
                        // chain still gates which targets it matches. A class
                        // with no method and no parents still resolved its
                        // invocant — the edge records that fact; find_def's
                        // method-not-found arm returns None honestly.
                        _ => MethodTarget::CrossFile { invocant_class: cn },
                    }
                });
            stamped.push((i, target));
        }
        for (i, target) in stamped {
            // Monotone: a re-stamp that can't re-derive the invocant class must
            // not ERASE an authoritative freeze. Witnesses only accrue (finalize
            // → enrichment adds the index), so a class never legitimately
            // retracts; the only Some→None here is a synthesized member ref
            // whose class was frozen from the field decl (a macro-body
            // `->field` whose receiver is an untypeable macro parameter). Keep it.
            if target.is_some() {
                self.refs[i].resolved_method_target = target;
            }
        }
    }

    /// Set the `owner` on `HashKeyAccess { owner: None, .. }` refs
    /// whose enclosing `MethodCall`'s receiver types as a
    /// `Parametric` flavor that claims this method's args (DBIC's
    /// `search`/`find`/`update`/...). Build emits these refs
    /// eagerly with `owner: None` for chain receivers it can't
    /// resolve at walk time; this routine fills them once the
    /// receiver's type is resolvable.
    ///
    /// `module_index = None` resolves only in-file chains. The
    /// same routine runs from enrichment with `module_index =
    /// Some(_)` to fill cross-file gaps. Idempotent — only None-
    /// owner refs are touched, so a second run leaves them alone.
    pub(super) fn fix_chain_receiver_hash_key_owners(&mut self, module_index: Option<&dyn CrossFileLookup>) {
        let mut owner_fixes: Vec<(usize, HashKeyOwner)> = Vec::new();
        for (i, r) in self.refs.iter().enumerate() {
            if !matches!(r.kind, RefKind::HashKeyAccess { owner: None, .. }) {
                continue;
            }
            // Find the enclosing MethodCall ref by span
            // containment — smallest-span containing MethodCall
            // wins (innermost call's args).
            let mut enclosing: Option<&Ref> = None;
            let mut enclosing_area: u64 = u64::MAX;
            for other in &self.refs {
                if !matches!(other.kind, RefKind::MethodCall { .. }) {
                    continue;
                }
                if !contains_point(&other.span, r.span.start) {
                    continue;
                }
                let area = (other.span.end.row.saturating_sub(other.span.start.row)) as u64
                    * 10_000
                    + other.span.end.column as u64;
                if area < enclosing_area {
                    enclosing = Some(other);
                    enclosing_area = area;
                }
            }
            let Some(call) = enclosing else { continue };
            let Some(ty) = self.method_call_invocant_type(call, module_index) else {
                continue;
            };
            let Some(p) = ty.as_parametric() else { continue };
            // Bare method name: a qualified spelling (`SUPER::search`,
            // `Foo::search`) claims args exactly like the bare one — the
            // flavor's vocabulary is unqualified.
            let Some(o) = p.method_arg_owner(call.unqualified_target_name()) else { continue };
            owner_fixes.push((i, o));
        }
        for (i, o) in owner_fixes {
            if let RefKind::HashKeyAccess { ref mut owner, .. } = self.refs[i].kind {
                *owner = Some(o);
            }
        }
    }


    /// Rebuild all derived indices after deserialization.
    /// Idempotent: safe to call on a freshly deserialized `FileAnalysis` whose
    /// index fields were zeroed by `#[serde(skip, default)]`.
    pub fn after_deserialize(&mut self) {
        // Clear first in case this is called on a populated FileAnalysis.
        self.scope_starts.clear();
        self.symbols_by_name.clear();
        self.symbols_by_scope.clear();
        self.refs_by_name.clear();
        self.refs_by_target.clear();
        self.call_ref_by_start.clear();
        self.export_lookup.clear();
        self.build_indices();
    }

    fn build_indices(&mut self) {
        // Scope starts — sorted for binary search
        self.scope_starts = self.scopes.iter()
            .map(|s| (s.span.start, s.id))
            .collect();
        self.scope_starts.sort_by_key(|(p, _)| (p.row, p.column));

        // Symbols by name
        for sym in &self.symbols {
            self.symbols_by_name
                .entry(sym.name.clone())
                .or_default()
                .push(sym.id);
        }

        // Symbols by scope
        for sym in &self.symbols {
            self.symbols_by_scope
                .entry(sym.scope)
                .or_default()
                .push(sym.id);
        }

        // Link HashKeyAccess refs to their HashKeyDef symbols whenever the
        // owner is already resolved (the builder's pre-pass handled type
        // constraints + variable identity + call-binding fixups). With this
        // link, `refs_to_symbol(def_id)` returns all accesses in O(1), which
        // is what references, rename, and highlights consume.
        let hashkey_defs: HashMap<(&str, &HashKeyOwner), SymbolId> = self.symbols.iter()
            .filter_map(|sym| {
                if let SymbolDetail::HashKeyDef { owner, .. } = &sym.detail {
                    Some(((sym.name.as_str(), owner), sym.id))
                } else {
                    None
                }
            })
            .collect();
        let mut hashkey_resolutions: Vec<(usize, SymbolId)> = Vec::new();
        for (i, r) in self.refs.iter().enumerate() {
            if r.resolves_to.is_some() {
                continue;
            }
            if let RefKind::HashKeyAccess { owner: Some(owner), .. } = &r.kind {
                if let Some(&sid) = hashkey_defs.get(&(r.target_name.as_str(), owner)) {
                    hashkey_resolutions.push((i, sid));
                }
            }
        }
        for (idx, sid) in hashkey_resolutions {
            self.refs[idx].resolves_to = Some(sid);
        }

        // Link DispatchCall refs → Handler symbols by (owner, name). A
        // DispatchCall whose owner couldn't be resolved at build time (e.g.
        // `$obj->emit('x')` where `$obj` type isn't known yet) stays
        // unlinked here and may be re-resolved by enrichment when the
        // cross-file receiver type becomes known.
        //
        // Unlike hash keys, multiple Handlers with the same (owner, name)
        // legitimately coexist (stacked registrations) — we link the ref
        // to the *first* def found so `resolves_to` has a single target,
        // and rely on `refs_to_symbol` walking all stacked defs separately
        // for features like references/rename.
        let handler_defs: HashMap<(&str, &HandlerOwner), SymbolId> = self.symbols.iter()
            .filter_map(|sym| {
                if let SymbolDetail::Handler { owner, .. } = &sym.detail {
                    Some(((sym.name.as_str(), owner), sym.id))
                } else {
                    None
                }
            })
            .collect();
        let mut handler_resolutions: Vec<(usize, SymbolId)> = Vec::new();
        for (i, r) in self.refs.iter().enumerate() {
            if r.resolves_to.is_some() { continue; }
            if let RefKind::DispatchCall { owner: Some(owner), .. } = &r.kind {
                if let Some(&sid) = handler_defs.get(&(r.target_name.as_str(), owner)) {
                    handler_resolutions.push((i, sid));
                }
            }
        }
        for (idx, sid) in handler_resolutions {
            self.refs[idx].resolves_to = Some(sid);
        }

        // Refs by target name, and refs by resolved target SymbolId.
        // Same loop populates the start-point → call-ref-idx index
        // used by `method_call_invocant_class` to chase chain
        // receivers. Only MethodCall (whose span covers the whole
        // call expression) and FunctionCall (whose span covers just
        // the function-name node, but whose start point still
        // matches the outer call's invocant_span.start) refs go in.
        for (i, r) in self.refs.iter().enumerate() {
            self.refs_by_name
                .entry(r.target_name.clone())
                .or_default()
                .push(i);
            if let Some(sym_id) = r.resolves_to {
                self.refs_by_target.entry(sym_id).or_default().push(i);
            }
            if matches!(r.kind, RefKind::MethodCall { .. } | RefKind::FunctionCall { .. }) {
                // Smaller span (closer to the actual receiver) wins;
                // a tie keeps the earlier insertion. Method-call refs
                // are visited outer-first, so for a chain like
                // `Foo->new->m` the outer `m` and inner `Foo->new`
                // share a start point — keeping the smaller-span ref
                // points the index at the inner receiver. FunctionCall
                // refs (just the function-name span) are naturally
                // narrower than the enclosing MethodCall, so they win
                // the same way.
                let cur = self.call_ref_by_start.get(&r.span.start).copied();
                let take = match cur {
                    None => true,
                    Some(prev) => {
                        let prev_span = self.refs[prev].span;
                        // Smaller span (closer to the receiver) wins.
                        // Tie-breaker: prefer FunctionCall over MethodCall
                        // when at the same start, since FunctionCall is
                        // narrower (just the function-name span).
                        let new_smaller = (r.span.end.row, r.span.end.column)
                            < (prev_span.end.row, prev_span.end.column);
                        new_smaller
                    }
                };
                if take {
                    self.call_ref_by_start.insert(r.span.start, i);
                }
            }
        }

        // Export membership set — union of export + export_ok for O(1) lookup.
        self.export_lookup = self.export.iter()
            .chain(self.export_ok.iter())
            .cloned()
            .collect();
    }

    /// All refs that resolve to this symbol — O(1) lookup via the index.
    /// Callers typically combine this with a kind filter.
    pub fn refs_to_symbol(&self, sym_id: SymbolId) -> &[usize] {
        self.refs_by_target.get(&sym_id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// True if `name` appears in `@EXPORT` or `@EXPORT_OK` for this module.
    /// O(1) via `export_lookup` (built by `build_indices`).
    pub fn exports_name(&self, name: &str) -> bool {
        self.export_lookup.contains(name)
    }

    /// A producer module's export surface — the names a consumer's `use` can
    /// bring into scope, split into the default set (`@EXPORT`, auto-imported by
    /// a bare `use M;`), the optional set (`@EXPORT_OK`, opt-in only), and tags
    /// (`%EXPORT_TAGS`, with `:DEFAULT` synthesized as `@EXPORT`). This is the
    /// single structure `imported_names` evaluates a consumer's import spec
    /// against, so diagnostics and nav share one notion of "what does this
    /// module export, and what does this `use` bind."
    pub fn export_surface(&self) -> ExportSurface<'_> {
        ExportSurface {
            analysis: self,
            default_set: None,
            optional_set: None,
            tags: None,
            all_names: None,
        }
    }

    /// Like `export_surface`, but resolves `reexport_modules` transitively
    /// through `module_index`: the materialized surface includes every
    /// re-exported module's surface (default ∪ optional ∪ tags), walked
    /// cross-file via `ModuleIndex::for_each_reexport_module` (seen-set for
    /// cycles, fan-out cap). When this module has no re-export edges the
    /// result is identical to `export_surface` (own-only, zero extra storage).
    /// This is the one transitive-closure site — the consumer evaluator
    /// (`imported_names`) is untouched; it binds whatever the surface reports.
    pub fn export_surface_with_index(
        &self,
        module_index: &dyn CrossFileLookup,
    ) -> ExportSurface<'_> {
        if self.reexport_modules.is_empty() {
            return self.export_surface();
        }

        let mut default_set: Vec<String> = self.export.clone();
        let mut optional_set: Vec<String> = self.export_ok.clone();
        let mut tags: HashMap<String, Vec<String>> = self.export_tags.clone();

        // Merge every re-exported module's surface, walking the edges through the
        // one shared traversal (cycle-bounded + fan-out-capped). Own surface is
        // already seeded above, so we seed the queue with `reexport_modules`.
        module_index.for_each_reexport_module(
            self.reexport_modules.to_vec(),
            &mut |cached| {
                let a = &cached.analysis;
                for n in &a.export {
                    if !default_set.contains(n) {
                        default_set.push(n.clone());
                    }
                }
                for n in &a.export_ok {
                    if !optional_set.contains(n) {
                        optional_set.push(n.clone());
                    }
                }
                for (tag, members) in &a.export_tags {
                    let bucket = tags.entry(tag.clone()).or_default();
                    for m in members {
                        if !bucket.contains(m) {
                            bucket.push(m.clone());
                        }
                    }
                }
                std::ops::ControlFlow::Continue(())
            },
        );

        let mut all_names: HashSet<String> = HashSet::new();
        all_names.extend(default_set.iter().cloned());
        all_names.extend(optional_set.iter().cloned());
        for members in tags.values() {
            all_names.extend(members.iter().cloned());
        }

        ExportSurface {
            analysis: self,
            default_set: Some(default_set),
            optional_set: Some(optional_set),
            tags: Some(tags),
            all_names: Some(all_names),
        }
    }

}

/// Per-bucket resident-heap estimate for one or many `FileAnalysis`es, summed
/// by `add`. Measurement support for the bounded-memory work
/// (`docs/adr/memory-slice-2-lru.md`); NOT on any query path, wired only behind
/// the `PERL_LSP_HEAP_DUMP` env gate at the end of pack indexing.
///
/// Methodology: flat `size_of` of each collection's element footprint times its
/// `capacity` (so `Vec`/`HashMap` backing slack is counted), plus the deep
/// `String` capacities of the dominant string-bearing buckets (ref target
/// names, symbol names, the include closure, the reverse-index keys). Deep
/// strings inside the long-tail structs are NOT drilled — a deliberate,
/// documented undercount that keeps the probe cheap; the dominant buckets it
/// drills are what the eviction design turns on.
#[derive(Default, Clone, Debug)]
pub struct HeapBreakdown {
    pub files: usize,
    /// `refs` vec + every ref's `target_name`.
    pub refs: usize,
    /// `symbols` vec + names/packages/attributes.
    pub symbols: usize,
    /// Witness-bag `witnesses` vec.
    pub witness_vec: usize,
    /// Witness-bag rebuilt attachment index (serde-skip, rebuilt on load).
    pub witness_index: usize,
    /// `include_closure` + `include_directives` strings — the abseil
    /// header-path duplication.
    pub include: usize,
    /// `scopes` vec + package names.
    pub scopes: usize,
    /// The serde-skip reverse indices rebuilt on load
    /// (`refs_by_name`/`refs_by_target`/`symbols_by_name`/… ).
    pub rebuilt_indices: usize,
    /// `imports` + `call_bindings` + `method_call_bindings` + `fold_ranges`.
    pub bindings: usize,
    /// The pack/cpp flat fact vectors (domain sites, flow edges, macro defs,
    /// guard/deref sites, projections, moved-from, regions, …).
    pub cpp_extras: usize,
    /// The per-package small maps/sets (parents, uses, frameworks, exports,
    /// role/dynamic sets, provenance, template params, …).
    pub misc: usize,
    /// `size_of::<FileAnalysis>()` — the inline struct shell, once per file.
    pub shell: usize,
}

impl HeapBreakdown {
    pub fn add(&mut self, o: &HeapBreakdown) {
        self.files += o.files;
        self.refs += o.refs;
        self.symbols += o.symbols;
        self.witness_vec += o.witness_vec;
        self.witness_index += o.witness_index;
        self.include += o.include;
        self.scopes += o.scopes;
        self.rebuilt_indices += o.rebuilt_indices;
        self.bindings += o.bindings;
        self.cpp_extras += o.cpp_extras;
        self.misc += o.misc;
        self.shell += o.shell;
    }

    pub fn total(&self) -> usize {
        self.refs
            + self.symbols
            + self.witness_vec
            + self.witness_index
            + self.include
            + self.scopes
            + self.rebuilt_indices
            + self.bindings
            + self.cpp_extras
            + self.misc
            + self.shell
    }
}

impl std::fmt::Display for HeapBreakdown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mb = |b: usize| b as f64 / 1_048_576.0;
        let t = self.total().max(1);
        let row = |f: &mut std::fmt::Formatter<'_>, name: &str, b: usize| {
            writeln!(
                f,
                "  {name:<20} {:>9.1} MB  ({:>4.1}%)",
                mb(b),
                b as f64 / t as f64 * 100.0
            )
        };
        writeln!(
            f,
            "FileAnalysis heap composition ({} files, ~{:.1} MB estimated payload):",
            self.files,
            mb(self.total())
        )?;
        row(f, "refs", self.refs)?;
        row(f, "rebuilt_indices", self.rebuilt_indices)?;
        row(f, "witness_vec", self.witness_vec)?;
        row(f, "witness_index", self.witness_index)?;
        row(f, "symbols", self.symbols)?;
        row(f, "include_closure", self.include)?;
        row(f, "scopes", self.scopes)?;
        row(f, "bindings", self.bindings)?;
        row(f, "cpp_extras", self.cpp_extras)?;
        row(f, "misc_maps", self.misc)?;
        row(f, "struct_shell", self.shell)?;
        write!(f, "  {:-<20} {:>9.1} MB", "TOTAL ", mb(self.total()))
    }
}

impl FileAnalysis {
    /// Estimate this analysis's resident heap by bucket. See `HeapBreakdown`.
    pub fn heap_estimate(&self) -> HeapBreakdown {
        fn vcap<T>(v: &Vec<T>) -> usize {
            v.capacity() * std::mem::size_of::<T>()
        }
        fn mcap<K, V>(m: &HashMap<K, V>) -> usize {
            // hashbrown: ~1 control byte per slot on top of the (K,V) pair.
            m.capacity() * (std::mem::size_of::<(K, V)>() + 1)
        }
        fn scap<T>(s: &HashSet<T>) -> usize {
            s.capacity() * (std::mem::size_of::<T>() + 1)
        }
        fn strcaps<'a>(it: impl Iterator<Item = &'a String>) -> usize {
            it.map(|s| s.capacity() + std::mem::size_of::<String>()).sum()
        }
        // HashMap<String, Vec<V>>: flat table + deep key strings + value vecs.
        fn map_str_vec<V>(m: &HashMap<String, Vec<V>>) -> usize {
            let mut b = mcap(m);
            for (k, v) in m {
                b += k.capacity() + v.capacity() * std::mem::size_of::<V>();
            }
            b
        }

        let mut h = HeapBreakdown {
            files: 1,
            shell: std::mem::size_of::<FileAnalysis>(),
            ..Default::default()
        };

        // refs — the dominant bucket for a big-fan-in TU.
        h.refs = vcap(&self.refs) + strcaps(self.refs.iter().map(|r| &r.target_name));

        // symbols + their deep strings.
        h.symbols = vcap(&self.symbols)
            + self
                .symbols
                .iter()
                .map(|s| {
                    s.name.capacity()
                        + s.package.as_ref().map_or(0, |p| p.capacity())
                        + vcap(&s.attributes)
                        + strcaps(s.attributes.iter())
                        + vcap(&s.deref_stack)
                })
                .sum::<usize>();

        // witness bag.
        let (wv, wi) = self.witnesses.heap_bytes_estimate();
        h.witness_vec = wv;
        h.witness_index = wi;

        // include closure — the shared-header-path duplication.
        // Sorted path-ids over the global table: 4 bytes per entry; the
        // table's string bytes are process-wide, counted once, not per file.
        h.include = self.include_closure.heap_bytes()
            + vcap(&self.include_directives)
            + self
                .include_directives
                .iter()
                .map(|(_, s)| s.capacity())
                .sum::<usize>();

        // scopes.
        h.scopes = vcap(&self.scopes)
            + self
                .scopes
                .iter()
                .map(|s| s.package.as_ref().map_or(0, |p| p.capacity()))
                .sum::<usize>();

        // The serde-skip reverse indices (rebuilt on load, resident-only).
        h.rebuilt_indices = {
            fn mcap<K, V>(m: &HashMap<K, V>) -> usize {
                m.capacity() * (std::mem::size_of::<(K, V)>() + 1)
            }
            let mut b = self.scope_starts.capacity()
                * std::mem::size_of::<(Point, ScopeId)>()
                + self.export_lookup.capacity()
                    * (std::mem::size_of::<String>() + 1)
                + self
                    .export_lookup
                    .iter()
                    .map(|s| s.capacity())
                    .sum::<usize>()
                + mcap(&self.symbols_by_scope)
                + mcap(&self.refs_by_target)
                + mcap(&self.call_ref_by_start);
            for (k, v) in &self.symbols_by_name {
                b += k.capacity() + v.capacity() * std::mem::size_of::<SymbolId>();
            }
            b += mcap(&self.symbols_by_name);
            for (k, v) in &self.refs_by_name {
                b += k.capacity() + v.capacity() * std::mem::size_of::<usize>();
            }
            b += mcap(&self.refs_by_name);
            for v in self.symbols_by_scope.values() {
                b += v.capacity() * std::mem::size_of::<SymbolId>();
            }
            for v in self.refs_by_target.values() {
                b += v.capacity() * std::mem::size_of::<usize>();
            }
            b
        };

        // bindings / imports.
        h.bindings = vcap(&self.imports)
            + vcap(&self.call_bindings)
            + vcap(&self.method_call_bindings)
            + vcap(&self.fold_ranges);

        // pack/cpp flat fact vectors.
        h.cpp_extras = vcap(&self.provisional_dispatches)
            + vcap(&self.guard_sites)
            + vcap(&self.arrow_deref_sites)
            + vcap(&self.gated_param_types)
            + vcap(&self.attr_projections)
            + vcap(&self.key_writes)
            + vcap(&self.flow_edges)
            + vcap(&self.moved_from)
            + vcap(&self.control_regions)
            + vcap(&self.param_regions)
            + vcap(&self.macro_defs)
            + vcap(&self.domain_sites)
            + vcap(&self.plugin_loads)
            + vcap(&self.loader_config_params)
            + vcap(&self.package_ranges);

        // per-package small maps/sets + export lists.
        h.misc = map_str_vec(&self.package_parents)
            + map_str_vec(&self.package_uses)
            + map_str_vec(&self.role_requires)
            + map_str_vec(&self.template_params)
            + map_str_vec(&self.export_tags)
            + mcap(&self.specializes)
            + mcap(&self.type_provenance)
            + mcap(&self.package_framework)
            + scap(&self.framework_imports)
            + scap(&self.reassigned_scalars)
            + scap(&self.dynamic_parent_packages)
            + scap(&self.role_packages)
            + scap(&self.column_keyed_verbs)
            + vcap(&self.export)
            + vcap(&self.export_ok)
            + vcap(&self.reexport_modules)
            + vcap(&self.receiver_names)
            + vcap(&self.app_surface_consumers)
            + vcap(&self.plugin_namespaces);

        h
    }
}
