//! Walk infrastructure: scope stack, symbol/ref minting, package-range
//! tracking, flow-edge minting, and call-argument extraction.

use super::*;

impl<'a> Builder<'a> {
    // ---- Scope management ----

    pub(super) fn push_scope(&mut self, kind: ScopeKind, span: Span, package: Option<String>) -> ScopeId {
        let id = ScopeId(self.next_scope_id);
        self.next_scope_id += 1;
        let parent = self.scope_stack.last().copied();
        let pkg = package.or_else(|| {
            // Inherit package from current state or parent
            self.current_package.clone().or_else(|| {
                parent.and_then(|p| self.scopes[p.0 as usize].package.clone())
            })
        });
        self.scopes.push(Scope {
            id,
            parent,
            kind,
            span,
            package: pkg,
        });
        self.scope_stack.push(id);
        id
    }

    pub(super) fn pop_scope(&mut self) -> Option<ScopeId> {
        self.scope_stack.pop()
    }

    pub(super) fn current_scope(&self) -> ScopeId {
        *self.scope_stack.last().expect("scope stack empty")
    }

    /// Package/class name surrounding `node`. Reads the innermost
    /// containing scope's `package` field — set on both
    /// `package Foo;` and `class Foo { … }` entries, so this works
    /// for either flavor of class declaration. Used by
    /// `invocant_type_at_node` for `$self` / `shift` / `__PACKAGE__`
    /// resolution post-walk, where `self.current_package` is stale
    /// (it holds the walk's last-opened package, not the one
    /// containing the node we're querying).
    pub(super) fn package_for_node(&self, node: Node<'a>) -> Option<String> {
        let scope_id = self.scope_at_point(node.start_position());
        let mut cur = Some(scope_id);
        while let Some(sid) = cur {
            let s = &self.scopes[sid.0 as usize];
            if let Some(ref pkg) = s.package {
                return Some(pkg.clone());
            }
            cur = s.parent;
        }
        None
    }

    /// Is a bare `shift` / `$_[0]` here the method invocant (→ enclosing class),
    /// or just `arg[0]`? OO-by-convention is the default (a base class like
    /// `DateTime` types `bless {...}, ref $_[0]` even without declared parents),
    /// EXCEPT in a package that explicitly opted out of class machinery via
    /// `use Mojo::Base -strict`. There the first `@_` element is an ordinary
    /// argument, so typing it as the class produced bogus `unresolved-method`
    /// diagnostics (`$tx = shift; $tx->res` in `Mojo::WebSocket`). (rule #10:
    /// the opt-out is recorded as a package property at the `use` site, not
    /// re-derived from the `shift` shape here.)
    pub(super) fn shift_is_invocant_here(&self, node: Node<'a>) -> bool {
        match self.package_for_node(node) {
            Some(pkg) => !self.non_oo_packages.contains(&pkg),
            None => true,
        }
    }

    /// Innermost scope containing `point`. Mirrors
    /// `FileAnalysis::scope_at` but reads `&self.scopes` directly so
    /// it's callable from within Builder during and after the walk.
    /// Falls back to `ScopeId(0)` (the file scope) if no scope
    /// matches — a defensive default for cases where the walk hasn't
    /// produced any scope containing the point yet.
    pub(super) fn scope_at_point(&self, point: Point) -> ScopeId {
        let mut best: Option<(ScopeId, u64)> = None;
        for scope in &self.scopes {
            if !crate::model::file_analysis::contains_point(&scope.span, point) {
                continue;
            }
            let r = scope.span.end.row.saturating_sub(scope.span.start.row) as u64;
            let c = if scope.span.start.row == scope.span.end.row {
                scope.span.end.column.saturating_sub(scope.span.start.column) as u64
            } else {
                0
            };
            let size = r * 1_000_000 + c;
            if best.is_none() || size <= best.unwrap().1 {
                best = Some((scope.id, size));
            }
        }
        best.map(|(id, _)| id).unwrap_or(ScopeId(0))
    }

    /// Run the declarative `@flow` query (`queries/perl/flow.scm`) and mint a
    /// FlowEdge per `(target, source)` with the builder's OWN scope. The
    /// assignment shapes are captured in the `.scm`; the minting + scope live
    /// here — the same FlowEdge concept the cpp pack produces. The forcing-
    /// function start of Perl-on-the-query-engine. Provenance-only for now
    /// (no lowering): the shapes' types still come from the walk.
    pub(super) fn mint_flow_edges_via_query(&mut self, tree: &'a Tree) {
        use tree_sitter::{QueryCursor, StreamingIterator};
        let query = match flow_query() {
            Some(q) => q,
            None => return,
        };
        let cap_names: Vec<String> = query.capture_names().iter().map(|s| s.to_string()).collect();
        // Collect captures per match FIRST — the cursor borrows `self.source`,
        // so we can't mutate until it drops. `source` is optional: bind shapes
        // (`@flow.bare`) carry no inflowing value.
        struct FlowCaps<'t> {
            lhs: Option<Node<'t>>,
            target: Option<Node<'t>>,
            bare: Option<Node<'t>>,
            loopvar: Option<Node<'t>>,
            source: Option<Node<'t>>,
        }
        let mut pending: Vec<FlowCaps> = Vec::new();
        {
            let mut cursor = QueryCursor::new();
            let mut matches = cursor.matches(query, tree.root_node(), self.source);
            while let Some(m) = matches.next() {
                let mut caps = FlowCaps {
                    lhs: None,
                    target: None,
                    bare: None,
                    loopvar: None,
                    source: None,
                };
                for c in m.captures {
                    match cap_names[c.index as usize].as_str() {
                        "flow.lhs" => caps.lhs = Some(c.node),
                        "flow.target" => caps.target = Some(c.node),
                        "flow.bare" => caps.bare = Some(c.node),
                        "flow.loopvar" => caps.loopvar = Some(c.node),
                        "flow.source" => caps.source = Some(c.node),
                        _ => {}
                    }
                }
                if caps.lhs.or(caps.target).or(caps.bare).or(caps.loopvar).is_some() {
                    pending.push(caps);
                }
            }
        }
        for caps in pending {
            // Bind shapes: no inflowing value. A bare `my`/`local` CLEARS to
            // undef (`Cleared`); a `foreach` var rebinds per element (`Rebind`,
            // type TBD). Both record the rebind for the narrowing cutoff.
            if let Some(bare) = caps.bare {
                let at = bare.start_position();
                for name in self.bare_bind_names(bare) {
                    // Record the rebind (for the narrowing cutoff). A scalar
                    // clears to undef — but that `Undef` is a REGION assertion
                    // truncated at the next rebind (`my $x; $x->[0]` autoviv
                    // ends it), so it lands with the narrowing tier (where
                    // region+cutoff compose), not as a plain bag witness here.
                    self.push_flow_edge(
                        name,
                        at,
                        node_to_span(bare),
                        crate::model::file_analysis::Extraction::Rebind,
                    );
                }
                continue;
            }
            if let Some(loopvar) = caps.loopvar {
                if let (Ok(name), Some(src)) = (loopvar.utf8_text(self.source), caps.source) {
                    self.push_flow_edge(
                        name.to_string(),
                        loopvar.start_position(),
                        node_to_span(src),
                        crate::model::file_analysis::Extraction::Rebind,
                    );
                }
                continue;
            }
            let Some(src) = caps.source else { continue };
            let source_span = node_to_span(src);
            if let Some(lhs_node) = caps.lhs {
                if let Some(targets) = self.lhs_list_targets(lhs_node) {
                    // List/destructuring: each slot edges to its literal element
                    // (Whole) or a Positional projection — the logic that used
                    // to live in `visit_assignment`'s paren arm, now driven by
                    // the declarative capture.
                    let elem_nodes = self.list_element_nodes(src);
                    let at = lhs_node.start_position();
                    for (vt, extraction) in targets {
                        let (source, extraction) = match (&elem_nodes, &extraction) {
                            (Some(nodes), crate::model::file_analysis::Extraction::Positional(n))
                                if *n < nodes.len() =>
                            {
                                self.emit_expr_witness(nodes[*n]);
                                (node_to_span(nodes[*n]), crate::model::file_analysis::Extraction::Whole)
                            }
                            _ => (source_span, extraction),
                        };
                        self.push_flow_edge(vt, at, source, extraction);
                    }
                } else if let Some(vt) = self.get_var_text_from_lhs(lhs_node) {
                    self.push_flow_edge(
                        vt,
                        lhs_node.start_position(),
                        source_span,
                        crate::model::file_analysis::Extraction::Whole,
                    );
                }
            } else if let Some(tnode) = caps.target {
                if let Ok(vt) = tnode.utf8_text(self.source) {
                    let vt = vt.to_string();
                    self.push_flow_edge(
                        vt,
                        tnode.start_position(),
                        source_span,
                        crate::model::file_analysis::Extraction::Whole,
                    );
                }
            }
        }
    }

    /// The variable name(s) a bare bind (`@flow.bare`) targets — a
    /// `variable_declaration` (single or paren list) or a `localization`
    /// scalar.
    pub(super) fn bare_bind_names(&self, bare: Node<'a>) -> Vec<String> {
        if bare.kind() == "variable_declaration" {
            if let Some(targets) = self.lhs_list_targets(bare) {
                return targets.into_iter().map(|(n, _)| n).collect();
            }
            return self.get_var_text_from_lhs(bare).into_iter().collect();
        }
        bare.utf8_text(self.source)
            .ok()
            .map(|s| s.to_string())
            .into_iter()
            .collect()
    }

    /// Mint a FlowEdge + lower it as a FALLBACK: a refined eager TC (a direct
    /// InferredType witness, resolvable pre-fold) wins; the query Edge fills in
    /// only when the walk left the variable untyped. The single mint+lower for
    /// the query pass.
    pub(super) fn push_flow_edge(
        &mut self,
        name: String,
        at: Point,
        source: Span,
        extraction: crate::model::file_analysis::Extraction,
    ) {
        let scope = self.scope_at_point(at);
        let already_typed = self.bag_query_variable(&name, scope, at).is_some();
        let fe = crate::model::file_analysis::FlowEdge {
            target_name: name,
            target_scope: scope,
            target_at: at,
            source,
            extraction,
        };
        if !already_typed {
            if let Some(w) = fe.lower_to_witness() {
                self.bag.push(w);
            }
        }
        self.flow_edges.push(fe);
    }

    // ---- Symbol/Ref creation ----

    pub(super) fn add_symbol(&mut self, name: String, kind: SymKind, span: Span, selection_span: Span, detail: SymbolDetail) -> SymbolId {
        self.add_symbol_ns(name, kind, span, selection_span, detail, Namespace::Language)
    }

    pub(super) fn add_symbol_ns(
        &mut self,
        name: String,
        kind: SymKind,
        span: Span,
        selection_span: Span,
        detail: SymbolDetail,
        namespace: Namespace,
    ) -> SymbolId {
        let pkg = self.current_package.clone();
        self.add_symbol_in_package(name, kind, span, selection_span, detail, namespace, pkg)
    }

    /// `add_symbol` with an explicit package override. Cross-package
    /// glob installs (`*{'DateTime::'.$sub} = …`) name a target package
    /// in the glob string that differs from `current_package` (the file
    /// declares e.g. `package DateTime::PP`). The synthesized tail
    /// (`_ymd2rd`) must be keyed under the *named* package so
    /// `MethodOnClass{DateTime, _ymd2rd}` resolves — not the file's
    /// own package. Every other caller keeps `current_package` via
    /// `add_symbol_ns`.
    pub(super) fn add_symbol_in_package(
        &mut self,
        name: String,
        kind: SymKind,
        span: Span,
        selection_span: Span,
        detail: SymbolDetail,
        namespace: Namespace,
        package: Option<String>,
    ) -> SymbolId {
        let id = SymbolId(self.next_symbol_id);
        self.next_symbol_id += 1;
        // Every symbol attaches to the current lexical scope. Package
        // context lives separately in `package_ranges`; the variable
        // resolver gates `our` decls by package match at lookup time
        // (so bare `$version` from a sibling `package main;` doesn't
        // reach a Calculator-package `our $version`).
        self.symbols.push(Symbol {
            id,
            name,
            kind,
            span,
            selection_span,
            scope: self.current_scope(),
            package,
            detail,
            namespace,
            outline_label: None,
            attributes: Vec::new(),
            deref_stack: Vec::new(),
            // Perl carries params in `SymbolDetail::Sub`; `param_arity()`
            // reads them. No pack-minted arity here.
            arity: None,
        });
        id
    }

    // ---- Package-range tracking ----

    /// Record a `package Foo;` / `class Foo;` (statement form). Trims
    /// the previously-open statement range to end at `start`, then
    /// pushes a new range whose end is seeded with the file end —
    /// trimmed in turn when a successor appears, or left at file end
    /// if none does.
    pub(super) fn open_statement_package_range(&mut self, name: String, start: Point) {
        use crate::model::file_analysis::{PackageKind, PackageRange};
        if let Some(idx) = self.open_statement_package.take() {
            self.package_ranges[idx].span.end = start;
        }
        let file_end = self
            .scope_stack
            .first()
            .map(|id| self.scopes[id.0 as usize].span.end)
            .unwrap_or(start);
        self.package_ranges.push(PackageRange {
            package: name,
            span: Span { start, end: file_end },
            kind: PackageKind::Statement,
        });
        self.open_statement_package = Some(self.package_ranges.len() - 1);
    }

    /// Record a `package Foo { … }` / `class Foo { … }` (block form).
    /// Span is the node's own span — no successor-trimming required.
    /// Block forms do NOT supplant any statement-form range that
    /// brackets them: `package Foo; package Bar { … }` leaves Foo
    /// covering everything outside the Bar block.
    pub(super) fn push_block_package_range(&mut self, name: String, span: Span) {
        use crate::model::file_analysis::{PackageKind, PackageRange};
        self.package_ranges.push(PackageRange {
            package: name,
            span,
            kind: PackageKind::Block,
        });
    }

    /// Build-time mirror of `FileAnalysis::package_at`. Used by the
    /// variable resolver to gate `our` decls by package context — the
    /// builder can't call into FileAnalysis (it hasn't been
    /// constructed yet).
    pub(super) fn package_at_pos(&self, point: Point) -> Option<&str> {
        let mut best: Option<&crate::model::file_analysis::PackageRange> = None;
        for r in &self.package_ranges {
            if !crate::model::file_analysis::contains_point(&r.span, point) {
                continue;
            }
            let win = match best {
                None => true,
                Some(prev) => {
                    let cur_start = (r.span.start.row, r.span.start.column);
                    let prev_start = (prev.span.start.row, prev.span.start.column);
                    let cur_size = (
                        r.span.end.row - r.span.start.row,
                        r.span.end.column.saturating_sub(r.span.start.column),
                    );
                    let prev_size = (
                        prev.span.end.row - prev.span.start.row,
                        prev.span.end.column.saturating_sub(prev.span.start.column),
                    );
                    cur_start > prev_start || (cur_start == prev_start && cur_size < prev_size)
                }
            };
            if win {
                best = Some(r);
            }
        }
        best.map(|r| r.package.as_str())
    }

    pub(super) fn add_ref(&mut self, kind: RefKind, span: Span, target_name: String, access: AccessKind) {
        self.refs.push(Ref {
            kind,
            span,
            scope: self.current_scope(),
            target_name,
            access,
            resolves_to: None,
            resolved_method_target: None,
            folded_from: None,
            arg_count: None,
        });
    }

    // ---- Plugin dispatch helpers ----

    /// Normalize a call's `arguments` field into a flat list of argument
    /// nodes. Tree-sitter-perl wraps multi-arg lists in `list_expression`;
    /// single-arg calls present the arg directly.
    pub(super) fn extract_call_args(&self, call_node: Node<'a>) -> Vec<Node<'a>> {
        crate::cst::call_args(call_node)
    }

    /// The FLAT positional arg sequence plugins see — all grouping peeled.
    /// `has 'x' => (is => 'ro')`, `has 'x', is => 'ro'`, and the lisp-y
    /// `has(('x' => (is => ('ro'))))` are the same keyval sequence; only the
    /// parenthesization differs, and `list_expression`/`parenthesized_expression`
    /// are pure grouping in Perl. Delegates the recursive splice to the one
    /// `cst::flatten_list` primitive (shared with hash/array literals and
    /// pair walking); a non-group arg passes through whole. Plugin-facing
    /// view ONLY — arity stays on the un-peeled `extract_call_args`.
    pub(super) fn flat_call_args(&self, args_raw: Vec<Node<'a>>) -> Vec<Node<'a>> {
        let mut out = Vec::new();
        for n in args_raw {
            if matches!(n.kind(), "list_expression" | "parenthesized_expression") {
                crate::cst::flatten_list(n, &mut out);
            } else {
                out.push(n);
            }
        }
        out.into_iter().filter(|n| n.is_named()).collect()
    }

    /// Build an `ArgInfo` for a plugin. Constant-folds literals, barewords,
    /// and `$var` references that accumulate in `constant_strings`. When the
    /// arg is an anonymous sub, also extracts its param list so plugins
    /// registering handlers (`->on('ready', sub ($s, $m) {})`) can preserve
    /// the handler signature for later sig-help lookup.
    ///
    /// `&mut self` because the inferred-type derivation emits the arg's
    /// `Expr(span)` witness onto the bag before querying it — the order
    /// matters: emit first, then query. Reversing yields `None` from the
    /// query (no witness on the attachment yet) and the caller would
    /// silently skip the `callable_return_edge` projection.
    pub(super) fn arg_info_for(&mut self, arg: Node<'a>) -> plugin::ArgInfo {
        let text = arg.utf8_text(self.source).unwrap_or("").to_string();
        let mut content_span: Option<Span> = None;
        let string_value = match arg.kind() {
            "string_literal" | "interpolated_string_literal" => {
                // Read the string_content child — quote-flavor-agnostic
                // (handles q{}, qq!!, heredocs, etc.). An empty literal
                // has no content child, so default to "".
                // Also capture the content span so plugins can address
                // positions inside the string without hardcoding
                // quote-length offsets into the outer node's span.
                for i in 0..arg.named_child_count() {
                    if let Some(c) = arg.named_child(i) {
                        if c.kind() == "string_content" {
                            content_span = Some(node_to_span(c));
                            break;
                        }
                    }
                }
                Some(self.extract_string_content(arg).unwrap_or_default())
            }
            // `autoquoted_bareword` is a fat-comma key (`key => value`)
            // — its text IS the value, never const-folded (a key that
            // happens to match a constant name is still that key).
            "autoquoted_bareword" => Some(text.clone()),
            // A positional `bareword` arg may be a constant — fold it
            // through the constant table (`$app->plugin(EXTRA)` where
            // `use constant EXTRA => 'Gizmos'`). Falls back to the raw
            // token when it names no constant.
            "bareword" => self
                .resolve_constant_strings(&text, 0)
                .and_then(|f| f.into_iter().next())
                .or_else(|| Some(text.clone())),
            "scalar" | "array" | "hash" => {
                self.resolve_constant_strings(&text, 0).and_then(|f| f.into_iter().next())
            }
            _ => None,
        };
        // `string_values` is the multi-value channel: a loop registration
        // (`$app->helper("get_$name" => …) for my $name (qw(a b))`) folds to
        // every candidate. The general enumeration owns literal / interpolated
        // / constant-ref / concat folding; an undecidable arg yields empty and
        // falls back to the single `string_value` (a fat-comma bareword key,
        // an unfolded interpolation the plugin then skips).
        let mut string_values = self.enumerate_string_values(arg);
        if string_values.is_empty() {
            string_values.extend(string_value.clone());
        }
        self.emit_expr_witness(arg);
        let inferred_type = self.bag_query_expr_span(node_to_span(arg));
        let sub_params = if arg.kind() == "anonymous_subroutine_expression" {
            self.extract_anonymous_sub_params(arg)
        } else {
            Vec::new()
        };
        // `callable_return_edge` flows from whichever
        // `InferredType::CodeRef { return_edge }` is reachable for
        // this arg. Three reachability paths covered uniformly:
        //
        //   helper(name => sub { … })             (anon literal)
        //   my $sub = sub { … }; helper(_, $sub)   (rebound anon)
        //   helper(name => \&Foo::bar)             (named ref)
        //
        // The literal paths (anon-sub + refgen) flow through
        // `emit_expr_witness`'s closed-syntax arms in `expr_payload`;
        // the rebind path goes through `invocant_type_at_node`'s
        // `scalar` arm, which `bag_query_variable`-resolves the
        // variable's TC. Either yields the right `CodeRef` shape;
        // the projection extracts the attachment whatever its target
        // shape (`Expr(span)` for anon, `MethodOnClass{...}` for refgen).
        let callable_return_edge = inferred_type
            .as_ref()
            .and_then(InferredType::callable_return_edge)
            .cloned()
            .or_else(|| {
                self.invocant_type_at_node(arg)
                    .as_ref()
                    .and_then(InferredType::callable_return_edge)
                    .cloned()
            });
        // `\&name` refgen — the named sub a registration plugin may want to
        // type the first param of. Same name extraction the return-edge path
        // uses; bare names stay bare so the deferred resolver scopes them to
        // the current package.
        let ref_sub_name = if arg.kind() == "refgen_expression" {
            self.extract_names_from_refgen(arg).into_iter().next()
        } else {
            None
        };
        let value_shape = self.classify_value_shape(arg);
        plugin::ArgInfo {
            text,
            string_value,
            string_values,
            span: node_to_span(arg),
            content_span,
            inferred_type,
            value_shape,
            sub_params,
            callable_return_edge,
            ref_sub_name,
        }
    }

    /// Span of the body's last expression on an
    /// `anonymous_subroutine_expression`. Mirrors
    /// `infer_anonymous_sub_return_type`'s body-walk — the last
    /// statement, unwrapped from `expression_statement` /
    /// `return_expression` if necessary, gives us the expression
    /// whose type IS the sub's return when called. Plugins use
    /// this to emit a back-edge from the synthesized Method's
    /// Symbol to that Expr, deferring return-type inference to
    /// query time.
    pub(super) fn anonymous_sub_body_last_expr_span(&self, node: Node<'a>) -> Option<Span> {
        if node.kind() != "anonymous_subroutine_expression" {
            return None;
        }
        let body = node.child_by_field_name("body")?;
        let mut node = body.named_child(body.named_child_count().checked_sub(1)?)?;
        // Peel through `expression_statement` and `return_expression`
        // wrappers (an explicit `return $expr;` shows up as
        // `expression_statement → return_expression → $expr` in
        // tree-sitter-perl). One unwrap isn't enough.
        loop {
            match node.kind() {
                "expression_statement" | "return_expression" => {
                    node = node.named_child(0)?;
                }
                _ => break,
            }
        }
        Some(node_to_span(node))
    }

    /// Single derivation site for a CodeRef-shaped value's
    /// `return_edge`, given the source node. Used by `expr_payload`
    /// when emitting the bag witness for `anonymous_subroutine_expression`
    /// / `refgen_expression` — the bag is canonical and there's no
    /// second consumer that bypasses it.
    ///
    /// Two recognized shapes:
    ///   - `anonymous_subroutine_expression` → `Symbol(sub_id)`,
    ///     looked up in `anon_sub_symbol_by_span`. The bag's
    ///     symbol-keyed reducers (`ReturnExprReducer`,
    ///     `SubReturnReducer`) all see anon subs the same way they
    ///     see named subs — uniform attachment shape, no
    ///     special-case for "this is anonymous."
    ///     Falls back to `Expr(body_last_expr_span)` only when the
    ///     symbol stash misses, which would mean a parse-error /
    ///     ERROR-recovery path where `visit_anonymous_sub` didn't
    ///     run; the body-span chase is still meaningful in that case.
    ///   - `refgen_expression` (`\&foo`, `\&Foo::bar`,
    ///     `\&$const_folded`) → `MethodOnClass { class, name }`.
    ///     Bag's MRO + `module_index` machinery resolves it,
    ///     including cross-file. Bare names default `class` to
    ///     the current package; qualified names split at the
    ///     last `::`. `\&$var` with a non-const-foldable name
    ///     returns `None`.
    ///
    /// Other node kinds return `None` (caller decides whether to
    /// wrap the result in `CodeRef { return_edge: None }` for
    /// opaque-coderef sources or fall through entirely).
    pub(super) fn coderef_return_edge_for(
        &self,
        node: Node<'a>,
    ) -> Option<crate::model::witnesses::WitnessAttachment> {
        match node.kind() {
            "anonymous_subroutine_expression" => {
                let span = node_to_span(node);
                if let Some(sym_id) = self.anon_sub_symbol_by_span.get(&span) {
                    return Some(crate::model::witnesses::WitnessAttachment::Symbol(*sym_id));
                }
                self.anonymous_sub_body_last_expr_span(node)
                    .map(crate::model::witnesses::WitnessAttachment::Expr)
            }
            "refgen_expression" => {
                let names = self.extract_names_from_refgen(node);
                let raw = names.into_iter().next()?;
                let (class, name) = match crate::model::file_analysis::split_qualified(&raw) {
                    (Some(c), n) => (c.to_string(), n.to_string()),
                    (None, _) => (self.current_package.clone()?, raw),
                };
                Some(crate::model::witnesses::WitnessAttachment::MethodOnClass { class, name })
            }
            _ => None,
        }
    }

    /// Extract params from an anonymous sub. Delegates to the builder's
    /// shared named-sub extractor (signature syntax + `my (...) = @_` +
    /// `shift`/`$_[N]` unpacks, all via tree walking) so the two codepaths
    /// can't diverge.
    pub(super) fn extract_anonymous_sub_params(&self, sub_node: Node<'a>) -> Vec<plugin::EmittedParam> {
        self.extract_params(sub_node)
            .into_iter()
            .map(|p| plugin::EmittedParam {
                name: p.name,
                default: p.default,
                is_slurpy: p.is_slurpy,
                is_invocant: false,
            })
            .collect()
    }

    /// Resolve a bare `foo()` call to the package whose `sub foo` it
    /// refers to. Order mirrors Perl's name-lookup rule:
    ///
    ///   1. Explicit qualifier (`Foo::bar()` → `Foo`).
    ///   2. Enclosing package that declares `sub <name>` locally (so
    ///      `package Foo { sub bar {} bar(); }` resolves to `Foo`).
    ///   3. Most-recent import whose `imported_symbols` lists this
    ///      name (`use Bler qw/hi/` → `Bler`). Later imports win —
    ///      Perl's later `use` shadows earlier one.
    ///
    /// Returns `None` when none of those pin a package. Downstream
    /// class/package-scoped queries treat `None` as no-match rather
    /// than falling back to name-only union.
    pub(super) fn resolve_call_package(&self, call_name: &str) -> Option<String> {
        // (1) Qualified: `Foo::bar` → `Foo`.
        if let (Some(pkg), _) = crate::model::file_analysis::split_qualified(call_name) {
            return Some(pkg.to_string());
        }
        // (2) Enclosing package defines the sub locally.
        if let Some(ref pkg) = self.current_package {
            if self.symbols.iter().any(|s| {
                s.name == call_name
                    && matches!(s.kind, SymKind::Sub | SymKind::Method)
                    && s.package.as_deref() == Some(pkg.as_str())
            }) {
                return Some(pkg.clone());
            }
        }
        // (3) Imports — walk in reverse order so later `use` wins.
        for imp in self.imports.iter().rev() {
            if let Some(sym) = imp.imported_symbols.iter().find(|s| s.local_name == *call_name) {
                // A renaming import (`use Exp beta => { -as => 'rb' }`) binds a
                // LOCAL alias the module doesn't define under that name. The
                // alias belongs to the CONSUMING package — keying its calls
                // there keeps rename/references local (and off the exporter's
                // unrelated symbols, e.g. a stray `Exp::rb`). goto-def still
                // reaches `Exp::beta` via the import binding's remote name,
                // which `resolve_imported_function` reads independently.
                if sym.remote_name.is_some() {
                    return self.current_package.clone();
                }
                return Some(imp.module_name.clone());
            }
        }
        None
    }
}
