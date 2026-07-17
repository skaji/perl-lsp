//! Post-walk query-pattern dispatch — SPIKE of `docs/prompt-plugin-queries.md`.
//!
//! Plugins declare their items of interest as tree-sitter queries
//! (`FrameworkPlugin::patterns`); this driver runs them once per file
//! after the live walk, gates each match by the plugin's triggers at
//! the match site's package, computes the declared projections for
//! actual matches only, and dispatches `on_match`. Emissions flow
//! through the same `apply_emit_action` path as the emit hooks.
//!
//! Runs post-walk (scopes, package ranges, constant folds complete) but
//! BEFORE the deferred `VarType` / named-sub-param flushes, so pattern
//! emissions land in the same downstream machinery as hook emissions.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use tree_sitter::{
    CaptureQuantifier, Node, Query, QueryCursor, QueryPredicateArg, StreamingIterator,
};

use crate::file_analysis::{DispatchCandidate, HandlerOwner, InferredType, ReceiverGated, Span};
use crate::plugin::{self, CaptureData, CaptureValue, MatchContext, PatternSpec};

use super::{node_to_span, Builder};

/// A `#receiver-isa?` deferred predicate on a pattern: NOT a match-time
/// filter (receiver isa is a cross-file, query-time question — see
/// `docs/adr/receiver-gated-dispatch.md`). It tags the match so its
/// `DispatchCall` emissions are recorded as `ReceiverGated` candidates
/// instead of applied directly; `FileAnalysis::applicable_dispatches`
/// resolves them against the receiver's actual class at query time,
/// exactly like the `dispatch_verbs()` manifest path.
struct ReceiverGate {
    capture_index: u32,
    target_class: String,
}

/// Extract a pattern's `#receiver-isa? @cap "Class"` predicate, if any.
/// Unknown predicate names land in `general_predicates` unevaluated —
/// the binding's reservation this tier is built on.
fn receiver_gate_for(query: &Query, pattern_index: usize) -> Option<ReceiverGate> {
    for p in query.general_predicates(pattern_index) {
        if &*p.operator != "receiver-isa?" {
            continue;
        }
        let mut cap = None;
        let mut class = None;
        for a in &p.args {
            match a {
                QueryPredicateArg::Capture(ix) => cap = Some(*ix),
                QueryPredicateArg::String(s) => class = Some(s.to_string()),
            }
        }
        match (cap, class) {
            (Some(capture_index), Some(target_class)) => {
                return Some(ReceiverGate {
                    capture_index,
                    target_class,
                })
            }
            _ => {
                log::error!(
                    "#receiver-isa? needs a capture and a class string; got {:?}",
                    p.args
                );
            }
        }
    }
    None
}

/// Compile a pattern query once per unique source text, process-wide.
/// `Query::new` is expensive; patterns are static per plugin load, so
/// the leak is bounded (one per distinct pattern source). Compile
/// errors are cached too — a broken pattern logs once per build, not
/// once per match attempt.
fn cached_pattern_query(source: &str) -> Result<&'static Query, String> {
    static CACHE: OnceLock<Mutex<HashMap<u64, Result<&'static Query, String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        source.hash(&mut h);
        h.finish()
    };
    if let Some(q) = cache.lock().unwrap().get(&key) {
        return q.clone();
    }
    let language: tree_sitter::Language = ts_parser_perl::LANGUAGE.into();
    let compiled: Result<&'static Query, String> = Query::new(&language, source)
        .map_err(|e| e.to_string())
        .and_then(|q| {
            // A pattern with zero captures is the top-level-predicate
            // trap: `[alts] (#pred …)` does NOT attach the predicate to
            // the alternation — it becomes its own degenerate pattern
            // (matching everywhere, capturing nothing) and the
            // alternation runs UNFILTERED. Hard error so the author
            // fixes the spelling to `([alts] (#pred …))` instead of
            // shipping a dead filter. (This exact trap shipped in
            // `query_cache::cpanfile_requires`.)
            for i in 0..q.pattern_count() {
                let quants = q.capture_quantifiers(i);
                if quants.iter().all(|qt| matches!(qt, CaptureQuantifier::Zero)) {
                    return Err(format!(
                        "pattern #{} captures nothing — a predicate after a bracketed \
                         alternation attaches to NOTHING (the alternation runs \
                         unfiltered). Wrap them in a group: ([…] (#pred …))",
                        i
                    ));
                }
            }
            let leaked: &'static Query = Box::leak(Box::new(q));
            Ok(leaked)
        });
    cache.lock().unwrap().insert(key, compiled.clone());
    compiled
}

/// Compile every Perl pattern query once, up front, at plugin-registry load.
///
/// `cached_pattern_query`'s memo is process-wide but compiles OUTSIDE its
/// lock and is populated lazily on first dispatch. Under the parallel cold
/// workspace index (`par_iter` over `build()`), that lets each Rayon worker
/// recompile the whole ~14-query set on the first file it touches — ~750ms of
/// `Query::new` charged to a handful of files' build phase (H7-14). Warming
/// the memo here, single-threaded before any parallel build starts, makes
/// every per-file dispatch a pure cache hit and removes the race entirely.
pub(crate) fn warm_pattern_queries<'a>(specs: impl Iterator<Item = &'a PatternSpec>) {
    for spec in specs {
        if spec.language != "perl" {
            continue;
        }
        let _ = cached_pattern_query(&spec.query);
    }
}

/// Verify a pattern's `expect` snippets against the real grammar:
/// parse each snippet, run the query, assert the match count and any
/// declared capture texts. This is the pattern author's guard against
/// the query medium's silent-match-nothing failure mode (field names
/// that print in the CST but don't match in the query engine, anchor
/// subtleties, …). Run by `--plugin-check` and by
/// `bundled_pattern_expects_hold` over every bundled pattern.
pub(crate) fn verify_pattern_expects(spec: &PatternSpec) -> Result<(), String> {
    if spec.language != "perl" {
        return Ok(());
    }
    let query = cached_pattern_query(&spec.query)
        .map_err(|e| format!("pattern `{}`: query compile failed: {}", spec.name, e))?;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&ts_parser_perl::LANGUAGE.into())
        .map_err(|e| e.to_string())?;
    for ex in &spec.expect {
        let tree = parser
            .parse(&ex.src, None)
            .ok_or_else(|| format!("pattern `{}` expect `{}`: parse failed", spec.name, ex.src))?;
        let mut count = 0usize;
        let mut texts: HashMap<String, String> = HashMap::new();
        {
            let mut cursor = QueryCursor::new();
            let mut it = cursor.matches(query, tree.root_node(), ex.src.as_bytes());
            while let Some(m) = it.next() {
                count += 1;
                for c in m.captures {
                    let name = query.capture_names()[c.index as usize];
                    texts.insert(
                        name.to_string(),
                        c.node.utf8_text(ex.src.as_bytes()).unwrap_or("").to_string(),
                    );
                }
            }
        }
        if count != ex.matches {
            return Err(format!(
                "pattern `{}` expect `{}`: {} matches, expected {}",
                spec.name, ex.src, count, ex.matches
            ));
        }
        for (cap, want) in &ex.captures {
            match texts.get(cap) {
                Some(got) if got == want => {}
                other => {
                    return Err(format!(
                        "pattern `{}` expect `{}`: capture @{} = {:?}, expected {:?}",
                        spec.name, ex.src, cap, other, want
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Union of the match's capture spans — the match extent handed to the
/// plugin. A pattern with a root capture (`… ) @call`) gets that node's
/// span, since it encloses every other capture.
fn union_span(caps: &[(u32, Node<'_>)]) -> Span {
    let mut it = caps.iter().map(|(_, n)| node_to_span(*n));
    let first = it.next().expect("non-empty capture list");
    it.fold(first, |acc, s| Span {
        start: acc.start.min(s.start),
        end: acc.end.max(s.end),
    })
}

impl<'a> Builder<'a> {
    /// Innermost package at a point, from the walk's `package_ranges`
    /// (latest-starting containing range wins — same rule as
    /// `FileAnalysis::package_at`), defaulting to the implicit `main`
    /// before any explicit package statement.
    fn package_at_point(&self, point: tree_sitter::Point) -> String {
        let mut best: Option<&crate::file_analysis::PackageRange> = None;
        for r in &self.package_ranges {
            if !crate::file_analysis::contains_point(&r.span, point) {
                continue;
            }
            let win = match best {
                None => true,
                Some(prev) => {
                    (r.span.start.row, r.span.start.column)
                        > (prev.span.start.row, prev.span.start.column)
                }
            };
            if win {
                best = Some(r);
            }
        }
        best.map(|r| r.package.clone())
            .unwrap_or_else(|| "main".to_string())
    }

    /// Run every plugin's declared patterns over the tree and dispatch
    /// matches. Fixed point over trigger gating: emissions can add
    /// package parents / uses that make more gates true, so rounds
    /// repeat until nothing new dispatches. Monotone gate inputs +
    /// per-(plugin, pattern, span) dedup ⇒ termination; the cap is a
    /// debug-only net, mirroring the worklist fold's discipline.
    pub(super) fn dispatch_pattern_plugins(&mut self, root: Node<'a>) {
        if self.plugins.is_empty() {
            return;
        }
        let plugins = self.plugins.clone();
        let mut dispatched: HashSet<(String, String, Span)> = HashSet::new();
        for round in 0..16 {
            debug_assert!(round < 15, "pattern dispatch failed to reach a fixed point");
            let mut progressed = false;
            for p in plugins.all() {
                for spec in p.patterns() {
                    if spec.language != "perl" || spec.phase != "walk" {
                        continue;
                    }
                    let query = match cached_pattern_query(&spec.query) {
                        Ok(q) => q,
                        Err(e) => {
                            log::error!(
                                "plugin `{}` pattern `{}`: query compile failed: {}",
                                p.id(),
                                spec.name,
                                e
                            );
                            continue;
                        }
                    };
                    // Collect matches first: the cursor borrows the tree
                    // immutably, the projection pass needs `&mut self`.
                    // Text predicates (#eq?, #any-of?, …) are evaluated
                    // by the query engine here, since `matches` gets the
                    // source text; unknown predicate names pass through
                    // unfiltered (the deferred-predicate reservation).
                    let mut collected: Vec<(usize, Vec<(u32, Node<'a>)>)> = Vec::new();
                    {
                        let mut cursor = QueryCursor::new();
                        let mut it = cursor.matches(query, root, self.source);
                        while let Some(m) = it.next() {
                            let caps: Vec<(u32, Node<'a>)> =
                                m.captures.iter().map(|c| (c.index, c.node)).collect();
                            if !caps.is_empty() {
                                collected.push((m.pattern_index, caps));
                            }
                        }
                    }
                    // Raw counts recorded on the FIRST round only (later
                    // rounds re-run the same query over the same tree);
                    // zero-match runs record too so a never-matching
                    // pattern shows up at 0 in the stats report.
                    if round == 0 {
                        crate::timings::record_pattern_matches(
                            p.id(),
                            &spec.name,
                            collected.len(),
                        );
                    }
                    for (pattern_index, caps) in collected {
                        let mspan = union_span(&caps);
                        let key = (p.id().to_string(), spec.name.clone(), mspan);
                        if dispatched.contains(&key) {
                            continue;
                        }
                        let pkg = self.package_at_point(mspan.start);
                        let uses = self.package_uses.get(&pkg).cloned().unwrap_or_default();
                        let parents = self.transitive_parents(&pkg);
                        let tq = plugin::TriggerQuery {
                            package_uses: &uses,
                            package_parents: &parents,
                        };
                        let fires = plugin::trigger_fires(p.triggers(), &tq);
                        // Trigger didn't fire locally, but a `ClassIsa` gate may
                        // still hold CROSS-FILE (the package has ancestry the
                        // index-free builder can't resolve). Run `on_match` and
                        // DEFER the emission — enrichment re-fires it once the
                        // module index confirms the gate. No parents ⇒ no
                        // cross-file ancestor possible, so nothing to defer.
                        let gate_prefixes = if fires {
                            Vec::new()
                        } else {
                            Self::cross_file_gate_prefixes(p.triggers())
                        };
                        let defer = !fires && !gate_prefixes.is_empty() && !parents.is_empty();
                        if !fires && !defer {
                            continue;
                        }
                        dispatched.insert(key);
                        progressed = true;
                        crate::timings::record_pattern_dispatch(p.id(), &spec.name);
                        // Projections that consult package-relative walk
                        // state (constant folds via the current package,
                        // `__PACKAGE__` receivers) see the match site's
                        // package, exactly as the walk would have.
                        let pkg_for_gate = pkg.clone();
                        let saved =
                            std::mem::replace(&mut self.current_package, Some(pkg.clone()));
                        let mctx = self.build_match_context(
                            spec,
                            query,
                            pattern_index,
                            &caps,
                            mspan,
                            pkg,
                            uses,
                            parents,
                            None,
                        );
                        let actions = p.on_match(&spec.name, &mctx);
                        if defer {
                            self.record_gated_pattern_emission(
                                p.id(),
                                gate_prefixes,
                                pkg_for_gate,
                                mspan.start,
                                actions,
                            );
                            self.current_package = saved;
                            continue;
                        }
                        // A #receiver-isa? gate defers DispatchCall
                        // emissions to query time. The build-time
                        // receiver type is a HINT on the candidate
                        // (same role as record_provisional_dispatch's),
                        // never the verdict.
                        let gate = receiver_gate_for(query, pattern_index);
                        let receiver_hint = gate.as_ref().and_then(|g| {
                            let node = caps
                                .iter()
                                .find(|(ix, _)| *ix == g.capture_index)
                                .map(|(_, n)| *n)?;
                            match self.invocant_type_at_node(node) {
                                Some(InferredType::ClassName(c)) => Some(c),
                                _ => None,
                            }
                        });
                        // Emissions attach to the scope AND package open
                        // at the match site — the same context a
                        // walk-time hook emission would have gotten
                        // (apply_emit_action stamps `current_package`
                        // onto symbols, so it must still be the match
                        // site's package here).
                        let match_scope = self.scope_at_point(mspan.start);
                        self.scope_stack.push(match_scope);
                        for a in actions {
                            // A loader's config value must carry an Expr
                            // witness at `config_span` so a cross-file
                            // `expr_type_at_span` (the `$conf` join in
                            // `record_loader_shapes`) resolves its shape.
                            // The captured node lives in `caps`; emit for
                            // it, mirroring the method-form recorder.
                            if let plugin::EmitAction::PluginLoad {
                                config_span: Some(cfg),
                                ..
                            } = &a
                            {
                                if let Some((_, node)) = caps
                                    .iter()
                                    .find(|(_, n)| node_to_span(*n) == *cfg)
                                {
                                    self.emit_expr_witness(*node);
                                }
                            }
                            if let (
                                Some(g),
                                plugin::EmitAction::DispatchCall {
                                    name,
                                    dispatcher,
                                    owner,
                                    span,
                                    ..
                                },
                            ) = (&gate, &a)
                            {
                                let HandlerOwner::Class(owner_class) = owner;
                                self.provisional_dispatches.push(ReceiverGated::new(
                                    g.target_class.clone(),
                                    DispatchCandidate {
                                        name: name.clone(),
                                        span: *span,
                                        dispatcher: dispatcher.clone(),
                                        owner_class: owner_class.clone(),
                                        receiver_class: receiver_hint.clone(),
                                        call_span: mspan,
                                    },
                                ));
                                continue;
                            }
                            self.apply_emit_action(p.id().to_string(), a);
                        }
                        self.scope_stack.pop();
                        self.current_package = saved;
                    }
                }
            }
            if !progressed {
                break;
            }
        }
    }


    /// Fold-phase dispatch: patterns declared `phase: "fold"` run after
    /// the worklist fold's PostFold pass, when chain typing has settled
    /// (route brands, resolved invocants). Differences from the walk
    /// phase, all deliberate:
    ///
    ///   - Matches from ALL fold patterns dispatch in DOCUMENT order,
    ///     because `SetRouteBase` emissions from earlier matches feed
    ///     later matches' `route_defaults` projections.
    ///   - The topic-route base is REPLAYED: the walk recorded group
    ///     scopes (`topic_group_spans`); a base set inside a group
    ///     restores when the replay passes the group's end — the
    ///     group-scoped push/pop semantics of a topic-DSL base.
    ///   - `SetRouteBase` emissions update the replay base instead of
    ///     the (stale) walk stack.
    ///   - Single pass, no gating fixed point: fold emissions don't
    ///     grow trigger inputs today. Revisit if one ever does.
    ///
    /// The deferred `VarType` / named-sub-param flushes ran long before
    /// this phase — fold patterns must not emit those actions.
    pub(super) fn dispatch_pattern_plugins_fold(&mut self, root: Node<'a>) {
        if self.plugins.is_empty() {
            return;
        }
        let plugins = self.plugins.clone();
        type Collected<'p, 'a> = (
            &'p dyn plugin::FrameworkPlugin,
            &'p PatternSpec,
            &'static Query,
            usize,
            Vec<(u32, Node<'a>)>,
            Span,
        );
        let mut collected: Vec<Collected<'_, 'a>> = Vec::new();
        for p in plugins.all() {
            for spec in p.patterns() {
                if spec.language != "perl" || spec.phase != "fold" {
                    continue;
                }
                let query = match cached_pattern_query(&spec.query) {
                    Ok(q) => q,
                    Err(e) => {
                        log::error!(
                            "plugin `{}` pattern `{}`: query compile failed: {}",
                            p.id(),
                            spec.name,
                            e
                        );
                        continue;
                    }
                };
                let mut count = 0usize;
                {
                    let mut cursor = QueryCursor::new();
                    let mut it = cursor.matches(query, root, self.source);
                    while let Some(m) = it.next() {
                        let caps: Vec<(u32, Node<'a>)> =
                            m.captures.iter().map(|c| (c.index, c.node)).collect();
                        if !caps.is_empty() {
                            let span = union_span(&caps);
                            collected.push((p, spec, query, m.pattern_index, caps, span));
                            count += 1;
                        }
                    }
                }
                crate::timings::record_pattern_matches(p.id(), &spec.name, count);
            }
        }
        collected.sort_by_key(|(_, _, _, _, _, s)| (s.start.row, s.start.column));

        let groups = self.topic_group_spans.clone();
        let mut gi = 0usize;
        let mut base_stack: Vec<(Span, Option<String>)> = Vec::new();
        let mut current_base: Option<String> = None;
        let mut dispatched: HashSet<(String, String, Span)> = HashSet::new();

        for (p, spec, query, pattern_index, caps, mspan) in collected {
            let point = mspan.start;
            // Leave group frames the replay has passed (inner frames
            // sit on top, so inner-first restore order is automatic).
            while let Some((gspan, _)) = base_stack.last() {
                if (point.row, point.column) > (gspan.end.row, gspan.end.column) {
                    let (_, saved) = base_stack.pop().expect("checked non-empty");
                    current_base = saved;
                } else {
                    break;
                }
            }
            // Enter group frames that contain this match.
            while gi < groups.len() {
                let g = groups[gi];
                if (g.start.row, g.start.column) > (point.row, point.column) {
                    break;
                }
                if crate::file_analysis::contains_point(&g, point) {
                    base_stack.push((g, current_base.clone()));
                }
                gi += 1;
            }

            let key = (p.id().to_string(), spec.name.clone(), mspan);
            if dispatched.contains(&key) {
                continue;
            }
            let pkg = self.package_at_point(point);
            let uses = self.package_uses.get(&pkg).cloned().unwrap_or_default();
            let parents = self.transitive_parents(&pkg);
            let tq = plugin::TriggerQuery {
                package_uses: &uses,
                package_parents: &parents,
            };
            let fires = plugin::trigger_fires(p.triggers(), &tq);
            // Cross-file `ClassIsa` deferral, same rule as the walk phase.
            let gate_prefixes = if fires {
                Vec::new()
            } else {
                Self::cross_file_gate_prefixes(p.triggers())
            };
            let defer = !fires && !gate_prefixes.is_empty() && !parents.is_empty();
            if !fires && !defer {
                continue;
            }
            dispatched.insert(key);
            crate::timings::record_pattern_dispatch(p.id(), &spec.name);

            let pkg_for_gate = pkg.clone();
            let saved = std::mem::replace(&mut self.current_package, Some(pkg.clone()));
            let mctx = self.build_match_context(
                spec,
                query,
                pattern_index,
                &caps,
                mspan,
                pkg,
                uses,
                parents,
                current_base.as_deref(),
            );
            let actions = p.on_match(&spec.name, &mctx);
            if defer {
                self.record_gated_pattern_emission(
                    p.id(),
                    gate_prefixes,
                    pkg_for_gate,
                    mspan.start,
                    actions,
                );
                self.current_package = saved;
                continue;
            }
            let gate = receiver_gate_for(query, pattern_index);
            let receiver_hint = gate.as_ref().and_then(|g| {
                let node = caps
                    .iter()
                    .find(|(ix, _)| *ix == g.capture_index)
                    .map(|(_, n)| *n)?;
                match self.invocant_type_at_node(node) {
                    Some(InferredType::ClassName(c)) => Some(c),
                    _ => None,
                }
            });
            let match_scope = self.scope_at_point(mspan.start);
            self.scope_stack.push(match_scope);
            for a in actions {
                // Same loader-config witness rule as the walk phase: a
                // PluginLoad's config value must carry an Expr witness at
                // `config_span` or `record_loader_shapes`' cross-file join
                // silently loses the shape — the phases must not diverge.
                if let plugin::EmitAction::PluginLoad {
                    config_span: Some(cfg),
                    ..
                } = &a
                {
                    if let Some((_, node)) =
                        caps.iter().find(|(_, n)| node_to_span(*n) == *cfg)
                    {
                        self.emit_expr_witness(*node);
                    }
                }
                if let plugin::EmitAction::SetRouteBase { controller } = &a {
                    current_base = Some(controller.clone());
                    continue;
                }
                if let (
                    Some(g),
                    plugin::EmitAction::DispatchCall {
                        name,
                        dispatcher,
                        owner,
                        span,
                        ..
                    },
                ) = (&gate, &a)
                {
                    let HandlerOwner::Class(owner_class) = owner;
                    self.provisional_dispatches.push(ReceiverGated::new(
                        g.target_class.clone(),
                        DispatchCandidate {
                            name: name.clone(),
                            span: *span,
                            dispatcher: dispatcher.clone(),
                            owner_class: owner_class.clone(),
                            receiver_class: receiver_hint.clone(),
                            call_span: mspan,
                        },
                    ));
                    continue;
                }
                self.apply_emit_action(p.id().to_string(), a);
            }
            self.scope_stack.pop();
            self.current_package = saved;
        }
    }

    /// A pattern matched syntactically but its `ClassIsa` trigger did NOT
    /// fire against LOCAL ancestry (rule #1: the builder is index-free, so
    /// `transitive_parents` sees only in-file parents). The match may still
    /// belong to the framework via a CROSS-FILE ancestor — the DBIC result
    /// class reaching `DBIx::Class` through an intermediate base in another
    /// file. Record the already-computed `on_match` output, translated to
    /// file-analysis-native symbols/refs, as a [`GatedEmission`] the
    /// enrichment pass re-fires once the module index can confirm the gate
    /// (`class_isa_prefix`). Trigger semantics are OR, and only `ClassIsa`
    /// triggers can newly-fire cross-file — `gate_prefixes` is exactly that
    /// subset of the plugin's triggers.
    ///
    /// Symbol-emitting actions (`Method`/`HashKeyDef`/`Handler`/`Symbol`) and
    /// the reference actions that link call sites to them
    /// (`DispatchCall`/`HashKeyAccess`) are captured; other kinds under a
    /// deferred gate are logged and skipped (out of scope — none are emitted
    /// by the bundled `ClassIsa` plugins on this path).
    fn record_gated_pattern_emission(
        &mut self,
        plugin_id: &str,
        gate_prefixes: Vec<String>,
        package: String,
        scope_point: tree_sitter::Point,
        actions: Vec<plugin::EmitAction>,
    ) {
        use crate::file_analysis::{
            GatedEmission, GatedRef, GatedSymbol, RefKind, SymKind, SymbolDetail,
        };
        use plugin::EmitAction;
        let mut symbols: Vec<GatedSymbol> = Vec::new();
        let mut refs: Vec<GatedRef> = Vec::new();
        for a in actions {
            match a {
                EmitAction::Method {
                    name, span, selection_span, params, is_method, return_type, doc,
                    on_class, display, hide_in_outline, opaque_return, ..
                } => {
                    symbols.push(GatedSymbol {
                        name,
                        kind: SymKind::Method,
                        span,
                        selection_span,
                        detail: SymbolDetail::Sub {
                            params: params.into_iter().map(Into::into).collect(),
                            is_method,
                            doc,
                            display,
                            hide_in_outline,
                            opaque_return,
                            is_constant: false,
                            lexical: false,
                        },
                        on_class,
                        return_type,
                    });
                }
                EmitAction::HashKeyDef { name, owner, span, selection_span } => {
                    symbols.push(GatedSymbol {
                        name,
                        kind: SymKind::HashKeyDef,
                        span,
                        selection_span,
                        detail: SymbolDetail::HashKeyDef { owner, is_dynamic: false },
                        on_class: None,
                        return_type: None,
                    });
                }
                EmitAction::Handler {
                    name, owner, dispatchers, params, span, selection_span,
                    display, hide_in_outline, ..
                } => {
                    symbols.push(GatedSymbol {
                        name,
                        kind: SymKind::Handler,
                        span,
                        selection_span,
                        detail: SymbolDetail::Handler {
                            owner,
                            dispatchers,
                            params: params.into_iter().map(Into::into).collect(),
                            display,
                            hide_in_outline,
                        },
                        on_class: None,
                        return_type: None,
                    });
                }
                EmitAction::Symbol { name, kind, span, selection_span, detail, return_type } => {
                    symbols.push(GatedSymbol {
                        name,
                        kind,
                        span,
                        selection_span,
                        detail,
                        on_class: None,
                        return_type,
                    });
                }
                EmitAction::DispatchCall { name, dispatcher, owner, span, .. } => {
                    refs.push(GatedRef {
                        kind: RefKind::DispatchCall { dispatcher, owner: Some(owner) },
                        span,
                        target_name: name,
                        access: crate::file_analysis::AccessKind::Read,
                    });
                }
                EmitAction::HashKeyAccess { name, owner, var_text, span, access } => {
                    refs.push(GatedRef {
                        kind: RefKind::HashKeyAccess { var_text, owner: Some(owner) },
                        span,
                        target_name: name,
                        access,
                    });
                }
                other => {
                    log::debug!(
                        "plugin `{}`: deferred cross-file ClassIsa emission drops \
                         unsupported action {:?}",
                        plugin_id,
                        std::mem::discriminant(&other),
                    );
                }
            }
        }
        if symbols.is_empty() && refs.is_empty() {
            return;
        }
        self.gated_emissions.push(GatedEmission {
            gate_prefixes,
            package,
            scope_point,
            plugin_id: plugin_id.to_string(),
            symbols,
            refs,
        });
    }

    /// The `ClassIsa` trigger prefixes of `triggers` — the only trigger
    /// shape that can newly-fire once cross-file ancestry is known (a
    /// `UsesModule` / `Always` verdict is settled locally at build).
    fn cross_file_gate_prefixes(triggers: &[plugin::Trigger]) -> Vec<String> {
        triggers
            .iter()
            .filter_map(|t| match t {
                plugin::Trigger::ClassIsa(prefix) => Some(prefix.clone()),
                _ => None,
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn build_match_context(
        &mut self,
        spec: &PatternSpec,
        query: &Query,
        pattern_index: usize,
        caps: &[(u32, Node<'a>)],
        span: Span,
        package: String,
        package_uses: Vec<String>,
        package_parents: Vec<String>,
        topic_base: Option<&str>,
    ) -> MatchContext {
        let names = query.capture_names();
        let quants = query.capture_quantifiers(pattern_index);
        // Group nodes per capture index, preserving first-seen order.
        let mut order: Vec<u32> = Vec::new();
        let mut grouped: HashMap<u32, Vec<Node<'a>>> = HashMap::new();
        for (idx, node) in caps {
            if !grouped.contains_key(idx) {
                order.push(*idx);
            }
            grouped.entry(*idx).or_default().push(*node);
        }
        let mut captures = HashMap::new();
        for idx in order {
            let nodes = &grouped[&idx];
            let Some(name) = names.get(idx as usize) else {
                continue;
            };
            let projections: &[String] = spec
                .projections
                .get(*name)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let datas: Vec<CaptureData> = nodes
                .iter()
                .map(|n| self.project_capture(*n, projections, topic_base))
                .collect();
            let many = matches!(
                quants.get(idx as usize),
                Some(CaptureQuantifier::ZeroOrMore) | Some(CaptureQuantifier::OneOrMore)
            );
            let value = if many {
                CaptureValue::Many(datas)
            } else {
                // Scalar position: last node wins (there is normally
                // exactly one). An optional capture that didn't match
                // simply isn't present in the map — Rhai reads `()`.
                match datas.into_iter().next_back() {
                    Some(d) => CaptureValue::One(Box::new(d)),
                    None => continue,
                }
            };
            captures.insert((*name).to_string(), value);
        }
        MatchContext {
            pattern: spec.name.clone(),
            span,
            package: Some(package),
            package_parents,
            package_uses,
            captures,
        }
    }

    /// Compute the declared projections for one captured node. `text`
    /// and `span` are free and always present; everything else routes
    /// through the SAME extractors the emit-hook pre-capture uses
    /// (`arg_info_for`, `invocant_type_at_node`) — laziness comes from
    /// only being here for actual matches.
    fn project_capture(
        &mut self,
        node: Node<'a>,
        projections: &[String],
        topic_base: Option<&str>,
    ) -> CaptureData {
        let wants = |k: &str| projections.iter().any(|p| p == k);
        let mut data = CaptureData {
            text: node.utf8_text(self.source).unwrap_or("").to_string(),
            span: node_to_span(node),
            string_value: None,
            string_values: Vec::new(),
            content_span: None,
            inferred_type: None,
            value_shape: None,
            sub_params: Vec::new(),
            callable_return_edge: None,
            list: Vec::new(),
            is_package_receiver: None,
            args: Vec::new(),
            isa: None,
            ref_sub_name: None,
            call_name: None,
            route_defaults: Vec::new(),
        };
        if wants("str")
            || wants("strs")
            || wants("content_span")
            || wants("sub_params")
            || wants("callable_edge")
            || wants("shape")
            || wants("ref_sub_name")
        {
            let ai = self.arg_info_for(node);
            if wants("str") {
                data.string_value = ai.string_value;
            }
            if wants("strs") {
                data.string_values = ai.string_values;
            }
            if wants("content_span") {
                data.content_span = ai.content_span;
            }
            if wants("sub_params") {
                data.sub_params = ai.sub_params;
            }
            if wants("callable_edge") {
                data.callable_return_edge = ai.callable_return_edge;
            }
            if wants("shape") {
                data.value_shape = Some(ai.value_shape);
            }
            if wants("ref_sub_name") {
                data.ref_sub_name = ai.ref_sub_name;
            }
        }
        if wants("ty") {
            data.inferred_type = self.invocant_type_at_node(node);
        }
        if wants("list") {
            data.list = self.extract_arg_name_list(node);
        }
        if wants("args") {
            let flat = self.flat_call_args(vec![node]);
            data.args = flat.iter().map(|n| self.arg_info_for(*n)).collect();
        }
        if wants("isa") {
            data.isa = self.isa_type_in_option_tail(node);
        }
        if wants("call_name") {
            data.call_name = self.invocant_call_name(node);
        }
        if wants("route_defaults") {
            // Same flattening as the legacy CallContext fill: the
            // fold-settled brand's stash + controller, then — for a
            // topic-DSL verb CALL receiver still missing a controller
            // — the replayed topic base (`under(...)->to('ctrl#')`'s
            // SetRouteBase, scoped by group frames).
            let mut defaults: Vec<(String, String)> = Vec::new();
            if let Some(InferredType::BrandedRoute { controller, stash, .. }) =
                self.invocant_type_at_node(node)
            {
                defaults = stash;
                if let Some(c) = controller {
                    defaults.push(("controller".to_string(), c));
                }
            }
            if defaults.iter().all(|(k, _)| k != "controller") {
                if let (Some(dsl), Some(callee)) =
                    (self.active_topic_dsl(), self.invocant_call_name(node))
                {
                    if dsl.verbs.iter().any(|v| *v == callee) {
                        if let Some(c) = topic_base {
                            defaults.push(("controller".to_string(), c.to_string()));
                        }
                    }
                }
            }
            data.route_defaults = defaults;
        }
        if wants("is_package_receiver") {
            // Same rule as the emit-hook path's `is_pkg_call`:
            // `__PACKAGE__` (any spelling conventions certifies) or a
            // bareword naming the match site's own package.
            let is_pkg = crate::conventions::is_current_package_token(&data.text)
                || (node.kind() == "package"
                    && Some(data.text.as_str()) == self.current_package.as_deref());
            data.is_package_receiver = Some(is_pkg);
        }
        data
    }
}
