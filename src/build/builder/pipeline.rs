//! Build entry points and the fixed-order phase driver
//! (`build_with_plugins_inner`), plus the walk-time `TypeConstraint`
//! push helpers and the one-shot witness-bag seed.

use super::*;

/// Walk the tree once, indexing the three node kinds the chain-typing
/// reducer cares about. Pure: reads only tree-sitter structural data,
/// no Builder state. Same recursion shape (depth-first via
/// `named_child(i)`) the three former independent walks all used.
pub(super) fn build_chain_typing_index<'a>(tree: &'a Tree) -> ChainTypingIndex<'a> {
    let mut idx = ChainTypingIndex {
        assignment_nodes: Vec::new(),
        return_nodes: std::collections::HashMap::new(),
        invocant_nodes: std::collections::HashMap::new(),
        method_call_args: std::collections::HashMap::new(),
        method_call_nodes: Vec::new(),
        chained_hash_elements: Vec::new(),
    };
    fn walk<'t>(node: Node<'t>, idx: &mut ChainTypingIndex<'t>) {
        match node.kind() {
            "assignment_expression" => {
                idx.assignment_nodes.push(node);
            }
            "return_expression" => {
                idx.return_nodes
                    .insert((node.start_position(), node.end_position()), node);
            }
            "method_call_expression" => {
                idx.method_call_nodes.push(node);
                if let Some(inv) = node.child_by_field_name("invocant") {
                    idx.invocant_nodes
                        .insert((inv.start_position(), inv.end_position()), inv);
                }
                if let Some(args) = node.child_by_field_name("arguments") {
                    idx.method_call_args
                        .insert((node.start_position(), node.end_position()), args);
                }
            }
            "hash_element_expression" => {
                // Container is the first named child; index only the
                // chained shape where it's a method call — plain
                // `$var->{key}` is handled by the walk.
                if node
                    .named_child(0)
                    .map(|c| c.kind() == "method_call_expression")
                    .unwrap_or(false)
                {
                    idx.chained_hash_elements.push(node);
                }
            }
            _ => {}
        }
        for i in 0..node.named_child_count() {
            if let Some(c) = node.named_child(i) {
                walk(c, idx);
            }
        }
    }
    walk(tree.root_node(), &mut idx);
    idx
}

pub fn build(tree: &Tree, source: &[u8]) -> FileAnalysis {
    build_with_plugins(tree, source, default_plugin_registry())
}

/// The compiled `@flow` query (`queries/perl/flow.scm`), compiled once.
/// `Query::new` is expensive and `build` runs per file — see `warm_flow_query`
/// for why this is warmed at startup rather than lazily on the first build.
pub(super) fn flow_query() -> Option<&'static tree_sitter::Query> {
    use std::sync::OnceLock;
    static FLOW_SCM: &str = include_str!("../../../queries/perl/flow.scm");
    static FLOW_QUERY: OnceLock<Option<tree_sitter::Query>> = OnceLock::new();
    FLOW_QUERY
        .get_or_init(|| {
            let lang: tree_sitter::Language = ts_parser_perl::LANGUAGE.into();
            tree_sitter::Query::new(&lang, FLOW_SCM).ok()
        })
        .as_ref()
}

/// Force the flow query to compile now, off the parallel per-file path.
pub(crate) fn warm_flow_query() {
    let _ = flow_query();
}

/// Build with a caller-provided plugin registry. Tests use this to swap in
/// deterministic plugin sets; the global default is otherwise shared.
pub fn build_with_plugins(
    tree: &Tree,
    source: &[u8],
    plugins: Arc<PluginRegistry>,
) -> FileAnalysis {
    build_with_plugins_inner(tree, source, plugins, false)
}

/// Test-only entry: build the file, then re-run the worklist fold
/// driver (`fold_to_fixed_point`) one extra time before finalizing.
///
/// The fold is fully idempotent: the resulting
/// `FileAnalysis` is byte-identical to a plain `build_with_plugins(...)`
/// call — same `type_provenance`, same `sub_return_type_at_arity`
/// answers, same witness counts. The two re-emittable passes inside
/// `resolve_return_types` (arity-return emission, call-binding
/// propagator) clear their prior outputs before re-emitting, so each
/// fact lands in the bag exactly once regardless of iteration count.
/// The `post_walk_fold_is_observably_idempotent` invariant test
/// asserts the answer-level guarantee directly.
#[cfg(test)]
pub(crate) fn build_with_plugins_extra_re_fold(
    tree: &Tree,
    source: &[u8],
    plugins: Arc<PluginRegistry>,
) -> FileAnalysis {
    build_with_plugins_inner(tree, source, plugins, true)
}

pub(super) fn build_with_plugins_inner(
    tree: &Tree,
    source: &[u8],
    plugins: Arc<PluginRegistry>,
    extra_re_fold: bool,
) -> FileAnalysis {
    let topic_dsls: Vec<plugin::TopicRouteDsl> =
        plugins.all().filter_map(|pl| pl.topic_route_dsl()).collect();
    let mut b = Builder {
        source,
        scopes: Vec::new(),
        symbols: Vec::new(),
        refs: Vec::new(),
        deferred_var_types: Vec::new(),
        deferred_named_sub_param_types: Vec::new(),
        fold_ranges: Vec::new(),
        imports: Vec::new(),
        return_infos: Vec::new(),
        pending_array_pushes: Vec::new(),
        last_expr_span: std::collections::HashMap::new(),
        slot_write_rhs_span: std::collections::HashMap::new(),
        call_bindings: Vec::new(),
        method_call_bindings: Vec::new(),
        pod_texts: Vec::new(),
        package_parents: std::collections::HashMap::new(),
        package_uses: std::collections::HashMap::new(),
        use_dedup: std::collections::HashSet::new(),
        dispatch_dedup: std::collections::HashSet::new(),
        sub_return_delegations: std::collections::HashMap::new(),
        framework_modes: std::collections::HashMap::new(),
        framework_imports: std::collections::HashSet::new(),
        constant_strings: std::collections::HashMap::new(),
        constant_string_source: std::collections::HashMap::new(),
        declared_constants: std::collections::HashMap::new(),
        export_member_sites: Vec::new(),
        export: Vec::new(),
        export_ok: Vec::new(),
        export_tags: std::collections::HashMap::new(),
        reexport_modules: Vec::new(),
        plugin_namespaces: Vec::new(),
        type_provenance: std::collections::HashMap::new(),
        bag: crate::model::witnesses::WitnessBag::new(),
        unresolved_expr_nodes: Vec::new(),
        package_framework: std::collections::HashMap::new(),
        non_oo_packages: std::collections::HashSet::new(),
        scope_stack: Vec::new(),
        // Perl's implicit top-level package. Without this seed,
        // top-level scripts (`Mojolicious::Lite` apps, one-off
        // `.pl` files) have `current_package = None` until they
        // hit an explicit `package` statement — which means
        // `package_uses` never records the file's `use` lines and
        // `Trigger::UsesModule` plugin triggers don't fire. Same
        // as Perl's own runtime: every script starts in `main`.
        current_package: Some("main".to_string()),
        next_scope_id: 0,
        next_symbol_id: 0,
        package_ranges: Vec::new(),
        open_statement_package: None,
        plugins,
        dispatch_manifest: std::collections::HashMap::new(),
        load_manifest: std::collections::HashMap::new(),
        type_constraint_names: std::collections::HashSet::new(),
        app_surface_consumers: Vec::new(),
        param_type_manifest: std::collections::HashMap::new(),
        param_type_wildcards: Vec::new(),
        plugin_loads: Vec::new(),
        loader_config_params: Vec::new(),
        flow_edges: Vec::new(),
        any_requires_action_attr: false,
        provisional_dispatches: Vec::new(),
        gated_emissions: Vec::new(),
        gated_param_types: Vec::new(),
        method_call_invocant: std::collections::HashMap::new(),
        attr_projections: Vec::new(),
        escape_recorded: std::collections::HashSet::new(),
        role_requires: std::collections::HashMap::new(),
        contract_symbols: std::collections::HashSet::new(),
        dynamic_parent_packages: std::collections::HashSet::new(),
        dynamic_dispatch_sites: 0,
        role_maker_modules: std::collections::HashSet::new(),
        role_packages: std::collections::HashSet::new(),
        dbic_source_name: None,
        topic_group_spans: Vec::new(),
        plugin_diagnostics: Vec::new(),
        topic_dsls,
        reassigned_scalars: std::collections::HashSet::new(),
        key_writes: Vec::new(),
        method_call_arity: std::collections::HashMap::new(),
        parametric_emitted_refs: std::collections::HashSet::new(),
        method_call_ref_dedup: std::collections::HashSet::new(),
        route_branded_refs: std::collections::HashSet::new(),
        defined_narrowings: Vec::new(),
        pending_narrowings: Vec::new(),
        guard_sites: Vec::new(),
        arrow_deref_sites: Vec::new(),
        anon_sub_symbol_by_span: std::collections::HashMap::new(),
        modifier_invocant_pos: None,
    };
    b.dispatch_manifest = b
        .plugins
        .dispatch_verbs()
        .map(|d| (d.verb.clone(), d.clone()))
        .collect();
    b.load_manifest = b
        .plugins
        .load_verbs()
        .map(|d| (d.verb.clone(), d.clone()))
        .collect();
    b.type_constraint_names = b
        .plugins
        .type_constraint_names()
        .map(|s| s.to_string())
        .collect();
    b.app_surface_consumers = b
        .plugins
        .app_surface_consumers()
        .map(|s| s.to_string())
        .collect();
    b.role_maker_modules
        .extend(b.plugins.role_makers().map(|s| s.to_string()));
    for pt in b.plugins.param_types() {
        match &pt.method {
            Some(name) => {
                b.param_type_manifest
                    .entry(name.clone())
                    .or_default()
                    .push(pt.clone());
            }
            None => b.param_type_wildcards.push(pt.clone()),
        }
    }
    b.any_requires_action_attr = b
        .param_type_manifest
        .values()
        .flatten()
        .any(|r| r.requires_action_attr)
        || b.param_type_wildcards
            .iter()
            .any(|r| r.requires_action_attr);

    // Create file-level scope and walk
    let file_scope = b.push_scope(ScopeKind::File, node_to_span(tree.root_node()), None);
    bphase!("walk(visit_children)", b.visit_children(tree.root_node()));
    // Still inside the file scope: synthesize Sub symbols for AutoLoader /
    // SelfLoader packages whose real definitions live in the `data_section`
    // after `__END__` (or `__DATA__`). Runs here so `package_uses` /
    // `package_parents` (the AutoLoader-backed gate) are fully populated and
    // the synthesized symbols attach to the file scope, like every other
    // top-level sub.
    b.synthesize_autoloader_data_subs(tree);

    // Query-declared plugin capture (SPIKE): dispatch plugin patterns
    // against the finished tree. Post-walk so package ranges, uses,
    // and constant folds are complete; still inside the file scope
    // (emissions need an open scope stack) and BEFORE the VarType /
    // named-sub flushes below so pattern emissions ride the same
    // machinery as walk-interleaved hook emissions.
    bphase!("pattern_dispatch", b.dispatch_pattern_plugins(tree.root_node()));

    b.pop_scope();
    let _ = file_scope;

    // Flush plugin-emitted VarType constraints now that every scope
    // has been pushed. Each uses scope_at on the declared anchor point
    // so a `$app->helper(... sub { my ($c) = @_; ... })` emission
    // lands inside the callback body rather than the outer file scope.
    let deferred = std::mem::take(&mut b.deferred_var_types);
    for d in deferred {
        let scope = b
            .scopes
            .iter()
            .rev()
            .find(|s| crate::model::file_analysis::contains_point(&s.span, d.at.start))
            .map(|s| s.id)
            .unwrap_or(ScopeId(0));
        b.push_plugin_type_constraint(
            TypeConstraint {
                variable: d.variable,
                scope,
                constraint_span: d.at,
                inferred_type: d.inferred_type,
            },
            d.plugin_id,
        );
    }

    // Named-sub param typing (`->helper(_ => \&sub)`): same flush window —
    // every sub scope + its params now exist, including forward-declared
    // ones.
    b.flush_deferred_named_sub_param_types();

    // Post-pass 1: resolve variable refs -> resolves_to
    bphase!("resolve_variable_refs", b.resolve_variable_refs());

    // Value-flow capture: run the declarative `@flow` query (the assignment
    // SHAPES) and mint FlowEdges with the builder's own scope. Provenance-only
    // for now (no lowering) — the shapes' types still come from the walk; this
    // proves the query path before it subsumes the manual minting.
    bphase!("flow_query", b.mint_flow_edges_via_query(tree));

    // Narrowing cutoffs: now that the FlowEdges exist, truncate each recognized
    // narrowed region at the first edge that rebinds its subject (the
    // edge-driven replacement for the `cst::rebinds_scalar` walk) and emit.
    bphase!("narrowing_cutoffs", b.apply_narrowing_cutoffs());

    // Export-list member refs: a `@EXPORT` / `@EXPORT_OK` / `%EXPORT_TAGS`
    // member naming a local sub gets a FunctionCall ref back to it. Runs
    // post-walk because subs are usually declared after the export list.
    b.emit_export_member_refs();

    // Pin forward-reference calls (call above its `sub`) to the local def's
    // package — order-independent, so goto-def/references/rename match them
    // like backward calls (the walk-time pin only saw subs declared earlier).
    b.pin_unresolved_call_packages();

    // Post-pass 2: resolve hash key owners from type constraints
    b.resolve_hash_key_owners();

    // Compute per-package framework facts BEFORE return-type fold so
    // the bag-aware reducer has the right context. Mirrors the data
    // the framework-accessor synthesis already consumed during the walk.
    b.package_framework = b
        .framework_modes
        .iter()
        .map(|(pkg, mode)| {
            let ff = match mode {
                FrameworkMode::Moo => crate::model::witnesses::FrameworkFact::Moo,
                FrameworkMode::Moose => crate::model::witnesses::FrameworkFact::Moose,
                FrameworkMode::MojoBase => crate::model::witnesses::FrameworkFact::MojoBase,
            };
            (pkg.clone(), ff)
        })
        .collect();

    // Plugin `overrides()` manifests run first. They pin return
    // types inference can't reach (`Mojolicious::Routes::Route::_route`
    // returning $self via an array-slice idiom). Provenance is
    // recorded in `type_provenance` (PluginOverride) so
    // `--dump-package` can answer "why does this return X?".
    bphase!("apply_type_overrides", b.apply_type_overrides());

    // Post-walk bag-population pass: ref-derived facts that don't
    // need walk-time visibility — `HashRefAccess` observations from
    // `$v->{k}` refs and invocant-mutation facts from hash-key
    // writes. Variable witnesses for TCs and walk-time idiom witnesses
    // (branch arms, arity gating) are already in `b.bag` — pushed
    // live during the walk.
    bphase!("populate_witness_bag", b.populate_witness_bag());

    // Forward-reference resolution: walk-time `expr_payload` arms for
    // `function_call_expression` / `bareword` / `scoped_identifier` did
    // a `self.symbols.iter().find` against a partial symbol table.
    // Forward-defined callees (Perl's `sub a { b() } sub b {…}` pattern,
    // canonically Carp's `longmess` → `longmess_heavy`) silently
    // produced no witness. The walk queued each missing lookup; resolve
    // them now against the final symbol table and push the
    // `Expr(span) → Edge(Symbol(sid))` witness the walk would have.
    bphase!("resolve_fwd_expr_witnesses", b.resolve_forward_expr_witnesses());

    // Worklist driver: one fixed-point loop over chain typing +
    // reducer dispatch (rather than a manually-ordered
    // `fold → chain → fold → chain` sequence). Each iteration runs
    // `ChainPassMode::PreFold` (assignment + return-arm refresh)
    // followed by `resolve_return_types`; the loop exits when the
    // snapshot of Sub/Method return types and bag length stops
    // moving. Invocant-class refresh runs once after the lattice
    // settles.
    //
    // The two re-emittable passes inside `resolve_return_types`
    // (arity-return witnesses, call-binding propagator) became
    // clear-and-emit in this same commit, so the bag stays canonical
    // regardless of how many iterations the loop runs — each fact
    // lands exactly once at the end. Chain typing's TC-existence
    // check keeps it idempotent on the same assignment span.
    //
    // For shallow files (no through-chain dependencies on inferred
    // sub return types) the loop terminates in two iterations: one
    // to derive the initial fold answer, one to confirm stability.
    // Deeper chains take more iterations; `MAX_FOLD_ITERATIONS`
    // (debug-only) catches dependency-tracking bugs that would
    // otherwise spin forever.
    let chain_idx = bphase!("build_chain_typing_index", build_chain_typing_index(tree));
    bphase!("fold_to_fixed_point", b.fold_to_fixed_point(&chain_idx));
    // PostFold filled `invocant_class` on MethodCall refs after the
    // worklist exited; re-emit method-call return edges so
    // Expression(refidx) chases resolve through to
    // MethodOnClass{class, method} for any invocant freshly known.
    // Then push array contributions: spans queryable through the
    // freshly-published edges.
    bphase!("emit_mc_return_edges", b.emit_method_call_return_edges());
    bphase!("emit_array_push_witns", b.emit_array_push_witnesses());
    // Record each method-call invocant's resolved type at its span so
    // the tree-free query entry (`FileAnalysis::expr_type_at_span`) can
    // answer "what is this expression?" without a CST. Runs after array
    // pushes so `$arr[N]` invocants project against the settled
    // `Variable{@arr}` Sequence. The build-time symbolic executor
    // (`invocant_type_at_node`) is the single structure-discovery site;
    // this pass records its answer.
    bphase!("emit_invocant_expr_witns", b.emit_invocant_expr_witnesses(&chain_idx));

    // Fold-phase pattern dispatch: patterns declared `phase: "fold"`
    // run HERE — after PostFold, so their projections read settled
    // chain typing (route brands, resolved invocants). Matches
    // dispatch in document order with the topic-route base replayed
    // from the walk's recorded group spans; `SetRouteBase` emissions
    // update the replay base instead of the (stale) walk stack.
    bphase!("pattern_dispatch_fold", b.dispatch_pattern_plugins_fold(tree.root_node()));

    // Test-only: re-run the worklist fold one more time to pin
    // idempotency. Production callers always pass `false`; only
    // `build_with_plugins_extra_re_fold` flips this on. Re-running
    // `fold_to_fixed_point` against a settled state should land in
    // 1 iteration (loop sees `prev == cur` immediately) and produce
    // a byte-identical FileAnalysis — including witness counts,
    // unlike the pre-Phase-6 pipeline.
    if extra_re_fold {
        b.fold_to_fixed_point(&chain_idx);
    }

    // Post-pass: emit `HashKeyAccess` refs for even-position stringy
    // args on every resolved `MethodCall` ref (`MooApp->new(name => 'alice')`,
    // helper-emitted controllers, etc.). Runs after `fold_to_fixed_point`
    // so `invocant_class` is canonical against the bag — was a walk-time
    // emission gated on the partially-resolved walk-time class, now it's
    // a single post-walk pass that joins refs to args via the chain
    // typing index.
    bphase!("emit_mc_arg_keys", b.emit_method_call_arg_keys(&chain_idx));

    // Post-pass: chained hashref-key accesses (`$obj->get_config->{host}`).
    // Runs post-fold so the method's return type is canonical — the
    // owner class is the chain receiver's type, unknowable until then.
    bphase!("emit_chained_hk_refs", b.emit_chained_hash_key_refs(&chain_idx));

    // Post-pass: upgrade Variable-owned hash-key derefs whose variable's
    // type settled to a class DURING the fold (`my $row = $rs->find(1);
    // $row->{name}` — the RowOf projection lands mid-fold, after
    // resolve_hash_key_owners ran). A Class owner routes the key to the
    // class's defs (DBIC columns, Moo slots); variables without a class
    // type keep their lexical grouping.
    bphase!("upgrade_var_hk_owners", b.upgrade_variable_hash_key_owners());

    // Post-pass 5: fill in tail POD docs for subs that didn't get preceding doc
    bphase!("resolve_tail_pod_docs", b.resolve_tail_pod_docs());

    let mut fa = FileAnalysis::new(crate::model::file_analysis::FileAnalysisParts {
        scopes: b.scopes,
        symbols: b.symbols,
        refs: b.refs,
        fold_ranges: b.fold_ranges,
        imports: b.imports,
        call_bindings: b.call_bindings,
        package_parents: b.package_parents,
        method_call_bindings: b.method_call_bindings,
        framework_imports: b.framework_imports,
        export: b.export,
        export_ok: b.export_ok,
        export_tags: b.export_tags,
        reexport_modules: b.reexport_modules,
        plugin_namespaces: b.plugin_namespaces,
        package_uses: b.package_uses,
        type_provenance: b.type_provenance,
        package_ranges: b.package_ranges,
        plugin_diagnostics: b.plugin_diagnostics,
        app_surface_consumers: b.app_surface_consumers,
        witnesses: b.bag,
        package_framework: b.package_framework,
        provisional_dispatches: b.provisional_dispatches,
        gated_emissions: b.gated_emissions,
        guard_sites: b.guard_sites,
        arrow_deref_sites: b.arrow_deref_sites,
        attr_projections: b.attr_projections,
        gated_param_types: b.gated_param_types,
        reassigned_scalars: b.reassigned_scalars,
        key_writes: b.key_writes,
        role_requires: b.role_requires,
        contract_symbols: b.contract_symbols,
        dynamic_parent_packages: b.dynamic_parent_packages,
        dynamic_dispatch_sites: b.dynamic_dispatch_sites,
        role_packages: b.role_packages,
        dbic_source_name: b.dbic_source_name,
        column_keyed_verbs: b.plugins.column_keyed_verbs().map(|s| s.to_string()).collect(),
        plugin_loads: b.plugin_loads,
        loader_config_params: b.loader_config_params,
        flow_edges: b.flow_edges,
        // use-after-move is a cpp-pack fact (`std::move`); Perl mints none.
        moved_from: Vec::new(),
        control_regions: Vec::new(),
        param_regions: Vec::new(),
        // domain-typing sites are a pack-language fact; Perl mints none here.
        domain_sites: Vec::new(),
    });
    // Finalize: run the legacy text-based MCB resolver as a fallback.
    // For every assignment the unified typer (run before
    // `resolve_return_types` above) couldn't handle, MCB fills in.
    // Cross-file enrichment also reuses MCB resolution without a tree.
    bphase!("finalize_post_walk", fa.finalize_post_walk());

    fa
}

impl<'a> Builder<'a> {
    /// Push a `TypeConstraint` shape into the witness bag — Variable
    /// `InferredType` + class-assertion / FirstParam observation when
    /// the type is a class identity. Walk-time and worklist callers go
    /// through here so `bag_query_variable` sees seeded types
    /// immediately. Mirrors `FileAnalysis::push_type_constraint`'s
    /// shape (the FA helper handles enrichment-time pushes after the
    /// builder's bag has been moved into the FA).
    pub(crate) fn push_type_constraint(&mut self, tc: TypeConstraint) {
        self.push_type_constraint_from(tc, crate::model::witnesses::WitnessSource::Builder("type_constraint".into()));
    }

    /// `push_type_constraint` with a plugin source so the witness carries
    /// `Plugin` priority. A plugin that knows a variable's type
    /// (`->helper(_ => sub/\&sub)` → `$c` is a controller) must dominate
    /// builder heuristics for that variable — the `my $c = shift` idiom
    /// otherwise types `$c` as the enclosing class. `FrameworkAwareTypeFold`
    /// prefers the higher-priority class assertion (source-priority axis,
    /// CLAUDE.md "Source priority breaks ties").
    pub(crate) fn push_plugin_type_constraint(&mut self, tc: TypeConstraint, plugin_id: String) {
        self.push_type_constraint_from(tc, crate::model::witnesses::WitnessSource::Plugin(plugin_id));
    }

    pub(super) fn push_type_constraint_from(
        &mut self,
        tc: TypeConstraint,
        source: crate::model::witnesses::WitnessSource,
    ) {
        use crate::model::witnesses::{
            TypeObservation, Witness, WitnessAttachment, WitnessPayload,
        };
        let TypeConstraint { variable, scope, constraint_span: span, inferred_type: ty } = tc;
        self.bag.push(Witness {
            attachment: WitnessAttachment::Variable { name: variable.clone(), scope },
            source: source.clone(),
            payload: WitnessPayload::InferredType(ty.clone()),
            span: Span { start: span.start, end: span.start },
        });
        match ty {
            InferredType::ClassName(n) => {
                self.bag.push(Witness {
                    attachment: WitnessAttachment::Variable { name: variable, scope },
                    source,
                    payload: WitnessPayload::Observation(TypeObservation::ClassAssertion(n)),
                    span,
                });
            }
            InferredType::FirstParam { package } => {
                self.bag.push(Witness {
                    attachment: WitnessAttachment::Variable { name: variable, scope },
                    source,
                    payload: WitnessPayload::Observation(TypeObservation::FirstParamInMethod {
                        package,
                    }),
                    span,
                });
            }
            _ => {}
        }
    }


    /// Post-walk pass: ref-derived facts that don't need walk-time
    /// visibility — `HashRefAccess` observations from `$v->{k}` refs
    /// and invocant-mutation facts on hash-key writes. Variable
    /// witnesses for TCs and walk-time idiom witnesses (branch arms,
    /// arity gating) are already in the bag — pushed live during the
    /// walk via `push_type_constraint` and `bag.push` from the emit
    /// sites.
    ///
    /// Method-call return edges (`Expression(refidx) → Edge(MethodOnClass{class, method})`)
    /// are emitted later — by `emit_method_call_return_edges` from
    /// inside the worklist, once `invocant_class` is filled.
    pub(super) fn populate_witness_bag(&mut self) {
        use crate::model::witnesses::{
            TypeObservation, Witness, WitnessAttachment, WitnessPayload, WitnessSource,
        };

        // Rep observations from `$v->{k}` access. Method-call return
        // edges on `Expression(refidx)` are emitted later — by the
        // chain-typing PostFold pass once `invocant_class` is filled —
        // as `Edge(MethodOnClass{class, method})`. Without a known
        // class there's no class-keyed answer to chase to, so the
        // emission is gated by chain-typing's own progress.
        let mut hash_obs: Vec<(String, ScopeId, Span)> = Vec::new();
        for r in self.refs.iter() {
            if let RefKind::HashKeyAccess { var_text, .. } = &r.kind {
                if var_text.starts_with('$') {
                    hash_obs.push((var_text.clone(), r.scope, r.span));
                }
            }
        }
        for (var, scope, span) in hash_obs {
            self.bag.push(Witness {
                attachment: WitnessAttachment::Variable { name: var, scope },
                source: WitnessSource::Builder("hash_ref_access".into()),
                payload: WitnessPayload::Observation(TypeObservation::HashRefAccess),
                span,
            });
        }

        // Invocant mutations on hash keys.
        //
        // Two seeds per typed-owner write: the untyped `mutation` Fact
        // (key-name completion via `mutated_keys_on_class`) and — when
        // the owner resolves to a CLASS and the RHS has a recorded span
        // and a bag-resolved type — a typed `SlotType{class, key} →
        // Edge(Expr(rhs_span))`. The edge routes through the same
        // canonical chase as implicit-return chains; `SlotTypeFold`
        // agrees the per-write arms. Honest-skip if the owner is a
        // `Sub` (not a class), or the RHS is unknown — never a bare
        // SlotType seed.
        let mut mutations: Vec<(HashKeyOwner, String, Span)> = Vec::new();
        let mut slot_writes: Vec<(String, String, Span, Span)> = Vec::new();
        for r in &self.refs {
            if let (RefKind::HashKeyAccess { owner, var_text }, AccessKind::Write) =
                (&r.kind, r.access)
            {
                let resolved_owner = match owner {
                    Some(o @ (HashKeyOwner::Class(_) | HashKeyOwner::Sub { .. })) => Some(o.clone()),
                    _ => {
                        if var_text == "$self" {
                            let scope = &self.scopes[r.scope.0 as usize];
                            scope.package.clone().map(HashKeyOwner::Class)
                        } else {
                            None
                        }
                    }
                };
                if let Some(o) = resolved_owner {
                    if let HashKeyOwner::Class(class) = &o {
                        if let Some(rhs_span) = self.slot_write_rhs_span.get(&r.span) {
                            slot_writes.push((
                                class.clone(),
                                r.target_name.clone(),
                                r.span,
                                *rhs_span,
                            ));
                        }
                    }
                    mutations.push((o, r.target_name.clone(), r.span));
                }
            }
        }
        for (owner, key, span) in mutations {
            self.bag.push(Witness {
                attachment: WitnessAttachment::HashKey { owner, name: key.clone() },
                source: WitnessSource::Builder("invocant_mutation".into()),
                payload: WitnessPayload::Fact {
                    family: "mutation".into(),
                    key: "written_at".into(),
                    value: crate::model::witnesses::FactValue::Str(key),
                },
                span,
            });
        }
        for (class, key, span, rhs_span) in slot_writes {
            // Only seed when the RHS actually resolves to a type — a bare
            // `Edge(Expr(rhs_span))` to an unresolved span folds to None,
            // which is honest, but emitting nothing for `= shift` / `= $param`
            // keeps the attachment absent entirely (no guess).
            if self.bag_query_expr_span(rhs_span).is_none() {
                continue;
            }
            self.bag.push(Witness {
                attachment: WitnessAttachment::SlotType { class, key },
                source: WitnessSource::Builder("slot_type".into()),
                payload: WitnessPayload::Edge(WitnessAttachment::Expr(rhs_span)),
                span,
            });
        }

        // Implicit-last-statement return edges. For each user-defined
        // sub/method scope with NO explicit `return` statements, push
        // `Symbol(sid) → Edge(Expr(last_expr_span))` so registry
        // queries on `Symbol(sid)` materialize the implicit return
        // through the canonical edge-chase path. Subs with explicit
        // returns route via the `Edge(SymbolReturnArm(sid))` chain
        // `publish_return_arm_witnesses` pushes — those claim the
        // same attachment shape first via `SymbolReturnArmFold`.
        // Framework / plugin-synthesized syms have no Scope and thus
        // no entry in `last_expr_span`; they're invisible to this
        // loop, which is the right behavior (their answer comes from
        // the synth-pushed Symbol witness directly).
        //
        // Invariant: `return_infos` is walk-final by the time
        // `populate_witness_bag` runs — it's populated only by
        // `visit_node`'s `return_expression` arm during the live walk
        // and never mutated after. No clear-and-emit tag on the implicit-return
        // edge is therefore needed; the gate `return_infos.is_empty()
        // for this scope` is a one-shot decision.
        let mut implicit_edges: Vec<(SymbolId, Span, Span)> = Vec::new();
        for scope in &self.scopes {
            if !matches!(scope.kind, ScopeKind::Sub { .. } | ScopeKind::Method { .. }) {
                continue;
            }
            if self.return_infos.iter().any(|ri| ri.scope == scope.id) {
                continue;
            }
            let Some(span) = self.last_expr_span.get(&scope.id).copied() else { continue };
            let Some(sym_id) = self.find_sub_symbol_for_scope(scope.id) else { continue };
            implicit_edges.push((sym_id, span, scope.span));
        }
        for (sym_id, expr_span, sym_span) in implicit_edges {
            self.bag.push(Witness {
                attachment: WitnessAttachment::Symbol(sym_id),
                source: WitnessSource::Builder("implicit_return".into()),
                payload: WitnessPayload::Edge(WitnessAttachment::Expr(expr_span)),
                span: sym_span,
            });
        }
    }

}
