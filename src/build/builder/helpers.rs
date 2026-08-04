//! Shared walk helpers: fold ranges, LHS/key extraction, access
//! classification, bless handling, hash-key emission, POD doc capture.

use super::*;

impl<'a> Builder<'a> {
    // ---- Fold ranges ----

    pub(super) fn add_fold_range(&mut self, node: Node<'a>) {
        let start = node.start_position().row;
        let end = node.end_position().row;
        if end > start {
            self.fold_ranges.push(FoldRange {
                start_line: start,
                end_line: end,
                kind: FoldKind::Region,
            });
        }
    }

    // ---- Helpers ----

    pub(super) fn get_decl_keyword(&self, var_decl: Node<'a>) -> Option<String> {
        for i in 0..var_decl.child_count() {
            if let Some(child) = var_decl.child(i) {
                let k = child.kind();
                if matches!(k, "my" | "our" | "state" | "field") {
                    return Some(k.to_string());
                }
            }
        }
        None
    }

    pub(super) fn collect_vars_from_decl(&self, node: Node<'a>) -> Vec<(String, Span)> {
        let mut vars = Vec::new();
        self.collect_vars_walk(node, &mut vars);
        vars
    }

    pub(super) fn collect_vars_walk(&self, node: Node<'a>, out: &mut Vec<(String, Span)>) {
        match node.kind() {
            "scalar" | "array" | "hash" => {
                if let Some(name) = self.build_var_name(node) {
                    out.push((name, node_to_span(node)));
                }
            }
            _ => {
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        self.collect_vars_walk(child, out);
                    }
                }
            }
        }
    }

    /// Build a variable's canonical name by reading the tree: sigil
    /// comes from the node kind (`scalar` → `$`, `array` → `@`, `hash`
    /// → `%`), bare name comes from the `varname` child. This keeps us
    /// correct for edge cases where the full node text isn't just
    /// `sigil + identifier` — e.g. `${foo}` (text `${foo}`, varname
    /// `foo`), `$:field` (whatever TSP aliases into varname), or any
    /// future TSP-added sigil-bearing syntax. The previous
    /// `node.utf8_text()` + caller-side sigil-stripping broke on every
    /// one of those shapes (`{foo}`, `:field`, etc.).
    ///
    /// Falls back to the full node text when the varname child is
    /// missing (ERROR recovery, partial parses).
    pub(super) fn build_var_name(&self, node: Node<'a>) -> Option<String> {
        let sigil = match node.kind() {
            "scalar" => '$',
            "array" => '@',
            "hash" => '%',
            _ => return node.utf8_text(self.source).ok().map(|s| s.to_string()),
        };
        let varname = find_varname_child(node).and_then(|v| v.utf8_text(self.source).ok());
        match varname {
            Some(name) => Some(format!("{}{}", sigil, name)),
            None => node.utf8_text(self.source).ok().map(|s| s.to_string()),
        }
    }

    pub(super) fn first_var_child(&self, node: Node<'a>) -> Option<String> {
        for i in 0..node.named_child_count() {
            if let Some(child) = node.named_child(i) {
                if matches!(child.kind(), "scalar" | "array" | "hash") {
                    return self.build_var_name(child);
                }
            }
        }
        None
    }

    /// Record a `$var->{key} = …` write for the mutation-extension
    /// pass. Walks subscript chains down to the scalar base: the FIRST
    /// hop's key is the one the write autovivifies onto the variable's
    /// own shape (`$v->{a}{b} = …` adds `a`); only a direct single-hop
    /// write types the key's value from the RHS. Plain `%foo` elements
    /// (`$foo{k}`, no arrow) are a different variable — skipped.
    /// Record an escape as an open-switching `KeyWrite` (`key: None`)
    /// at the escape span — the modeled form of escape widening: once
    /// the reference is out of our sight, any key may have been
    /// written, which is exactly what a dynamic-key write claims.
    pub(super) fn record_escape_write(&mut self, var_text: String, node: Node<'a>) {
        let span = node_to_span(node);
        self.key_writes.push(crate::model::file_analysis::KeyWrite {
            var_text,
            key: crate::model::file_analysis::WriteKey::Unknown,
            scope: self
                .scope_stack
                .last()
                .copied()
                .unwrap_or(crate::model::file_analysis::ScopeId(0)),
            span,
            rhs_span: None,
            conditional: true,
        });
    }

    pub(super) fn record_key_write(&mut self, left: Node<'a>, rhs: Option<Node<'a>>) {
        let mut innermost = left;
        loop {
            let Some(c) = innermost.named_child(0) else { return };
            match c.kind() {
                "hash_element_expression" | "array_element_expression" => innermost = c,
                "scalar" | "container_variable" => break,
                _ => return,
            }
        }
        // Direct `$v->[N] = …` — a Sequence slot write. Only the
        // direct, static-index, arrow-deref form is modeled (the pass
        // retypes the slot / appends at len); container `$arr[N]` and
        // nested array hops stay unmodeled — there's no open flag on
        // Sequence to widen into, and no array-index diagnostic to
        // protect.
        if innermost.kind() == "array_element_expression" {
            if innermost != left {
                return;
            }
            if !crate::cst::element_arrow_deref(innermost, self.source) {
                return;
            }
            let Some(container) = innermost.named_child(0) else { return };
            if container.kind() != "scalar" {
                return;
            }
            let Ok(t) = container.utf8_text(self.source) else { return };
            if !t.starts_with('$') {
                return;
            }
            let Some(idx_node) = innermost.child_by_field_name("index") else { return };
            let Ok(Ok(idx)) = idx_node.utf8_text(self.source).map(|s| s.parse::<i32>())
            else {
                return;
            };
            let span = node_to_span(idx_node);
            self.key_writes.push(crate::model::file_analysis::KeyWrite {
                var_text: t.to_string(),
                key: crate::model::file_analysis::WriteKey::Index(idx),
                scope: self
                    .scope_stack
                    .last()
                    .copied()
                    .unwrap_or(crate::model::file_analysis::ScopeId(0)),
                span,
                rhs_span: rhs.map(node_to_span),
                conditional: crate::cst::is_conditionally_executed(left),
            });
            return;
        }
        if innermost.kind() != "hash_element_expression" {
            return;
        }
        let Some(container) = innermost.named_child(0) else { return };
        // Container form `$h{k}` writes `%h` (canonical name); deref
        // form `$v->{k}` writes through scalar `$v` — arrow required
        // (`$foo{k}` without one would be `%foo` mis-keyed to `$foo`).
        let var_text: String = if container.kind() == "container_variable" {
            match crate::cst::canonical_container_name(container, self.source) {
                Some(n) => n,
                None => return,
            }
        } else {
            if !crate::cst::element_arrow_deref(innermost, self.source) {
                return;
            }
            let Ok(t) = container.utf8_text(self.source) else { return };
            if !t.starts_with('$') {
                return;
            }
            t.to_string()
        };
        let key_node = innermost.child_by_field_name("key");
        let key = key_node
            .and_then(|k| self.extract_key_text(k))
            .and_then(|(t, dynamic)| (!dynamic).then_some(t))
            .map_or(
                crate::model::file_analysis::WriteKey::Unknown,
                crate::model::file_analysis::WriteKey::Hash,
            );
        let direct = innermost == left;
        let span = key_node
            .map(node_to_span)
            .unwrap_or_else(|| node_to_span(innermost));
        self.key_writes.push(crate::model::file_analysis::KeyWrite {
            var_text: var_text.to_string(),
            key,
            scope: self
                .scope_stack
                .last()
                .copied()
                .unwrap_or(crate::model::file_analysis::ScopeId(0)),
            span,
            rhs_span: if direct { rhs.map(node_to_span) } else { None },
            conditional: crate::cst::is_conditionally_executed(left),
        });
    }

    pub(super) fn determine_access(&self, node: Node<'a>) -> AccessKind {
        if let Some(parent) = node.parent() {
            match parent.kind() {
                "variable_declaration" => return AccessKind::Declaration,
                "assignment_expression" => {
                    // Check if we're on the left side
                    if let Some(left) = parent.child_by_field_name("left") {
                        if node.start_byte() >= left.start_byte()
                            && node.end_byte() <= left.end_byte()
                        {
                            return AccessKind::Write;
                        }
                    }
                }
                _ => {}
            }
            // Check grandparent for assignment
            if let Some(grandparent) = parent.parent() {
                if grandparent.kind() == "assignment_expression" {
                    if let Some(left) = grandparent.child_by_field_name("left") {
                        if node.start_byte() >= left.start_byte()
                            && node.end_byte() <= left.end_byte()
                        {
                            return AccessKind::Write;
                        }
                    }
                }
            }
        }
        AccessKind::Read
    }

    /// Map a container/slice/keyval access node to the name of the
    /// variable it actually reads. The sigil on the access site is
    /// NOT the declared sigil:
    ///
    ///   $foo[0]         → @foo   (array element, under array_element_expression)
    ///   $foo{hi}        → %foo   (hash element, under hash_element_expression)
    ///   @foo[0..1]      → @foo   (array slice — parent `slice_expression` field `array:`)
    ///   @foo{qw/.../}   → %foo   (hash slice — parent `slice_expression` field `hash:`)
    ///   %foo[0..1]      → @foo   (KV slice of array — `keyval_expression` field `array:`)
    ///   %foo{a}         → %foo   (KV slice of hash — `keyval_expression` field `hash:`)
    ///
    /// For slice/keyval we ask the parent which *field* this node is
    /// filling, because the sigil on the child is always `@` (slice)
    /// or `%` (keyval) regardless of the underlying container. Bare
    /// name comes from the `varname` child so forms like `@{$ref}[0]`
    /// (ERROR/block-varname) don't produce garbage.
    pub(super) fn canonicalize_container(&self, node: Node<'a>, text: &str) -> String {
        crate::cst::canonical_container_name(node, self.source)
            .unwrap_or_else(|| text.to_string())
    }

    /// Extract the function name from a call expression (function_call or ambiguous_function_call).
    pub(super) fn extract_call_name(&self, node: Node<'a>) -> Option<String> {
        // Only match actual function calls, not method calls
        // (method calls are handled by MethodCallBinding).
        //
        // Qualified names (`Pkg::Sub::foo()`) pass through whole — the
        // downstream fixup strips the package prefix before the
        // `return_types` lookup, so rejecting them here only loses bindings.
        //
        // Dynamic calls like `my $fn = 'get_config'; $fn->()` — the parser
        // yields a function_call_expression with function="$fn". Mirror the
        // method-call path: try constant folding to recover the concrete
        // sub name; fall through to None when the variable isn't a known
        // compile-time constant.
        match node.kind() {
            "function_call_expression" | "ambiguous_function_call_expression" => {
                let name = crate::cst::extract_call_name(node, self.source)?;
                if let Some(stripped) = name.strip_prefix('&') {
                    // `&$fn()` syntax — same deal.
                    if stripped.starts_with('$') {
                        return self.resolve_constant_strings(stripped, 0)
                            .and_then(|names| names.into_iter().next());
                    }
                    return Some(stripped.to_string());
                }
                if name.starts_with('$') {
                    return self.resolve_constant_strings(&name, 0)
                        .and_then(|names| names.into_iter().next());
                }
                Some(name)
            }
            _ => None,
        }
    }

    pub(super) fn extract_constructor_class(&self, node: Node<'a>) -> Option<String> {
        let inv = crate::cst::constructor_invocant(node, self.source)?;
        if crate::model::conventions::is_current_package_token(inv) {
            return self.current_package.clone();
        }
        Some(inv.to_string())
    }

    /// `recv->resultset('Foo')` → `Parametric(ResultSet { base,
    /// row })`. `base` is discovered via the
    /// `<NS>::Result::<X>` ↔ `<NS>::ResultSet::<X>` convention if
    /// that class exists in this file; otherwise falls back to
    /// `DBIx::Class::ResultSet` (the universal DBIC default that
    /// runtime DBIC creates dynamically when no custom resultset
    /// class is defined). The fallback hardcode is core-resident
    /// for now — the DBIC plugin (queued, see
    /// `docs/prompt-dbic-as-plugin.md`) will own it once the port
    /// lands.
    ///
    /// First arg must be a string literal — a non-literal
    /// (variable, computed) means the row class is dynamic and
    /// we don't claim.
    pub(super) fn extract_resultset_parametric(&self, node: Node<'a>) -> Option<InferredType> {
        use crate::model::file_analysis::ParametricType;
        if node.kind() != "method_call_expression" {
            return None;
        }
        let method = node.child_by_field_name("method")?;
        let mtext = method.utf8_text(self.source).ok()?;
        // `recv->resultset('Foo')` — row class from the string arg.
        if mtext == "resultset" {
            let args = node.child_by_field_name("arguments")?;
            let row_class = self.first_string_or_constfold_arg(args)?;
            let base = self
                .discover_resultset_class(&row_class)
                .unwrap_or_else(|| "DBIx::Class::ResultSet".to_string());
            return Some(InferredType::Parametric(ParametricType::ResultSet { base, row: row_class }));
        }
        None
    }

    /// Is this call a plugin-declared FLUENT verb (`$rs->search`)? A fluent verb
    /// returns its invocant's type UNCHANGED — whatever it is — so the call site
    /// edges the call's type to the invocant's rather than minting one (DBIC's
    /// `search`/`search_rs` keep a resultset a resultset; this stays generic).
    /// The verb list is the plugin's (#10/#8).
    pub(super) fn is_fluent_verb_call(&self, node: Node<'a>) -> bool {
        node.kind() == "method_call_expression"
            && node
                .child_by_field_name("method")
                .and_then(|m| m.utf8_text(self.source).ok())
                .is_some_and(|m| self.plugins.fluent_verbs().any(|v| v == m))
    }

    /// Push `ReturnExpr` declarations on `MethodOnClass{base, m}`
    /// for every projection method the flavor declares. Called
    /// after each `extract_resultset_parametric` hit so the chain
    /// typer's coderef-edge / dynamic-method / inheritance routes
    /// all see the same answer through the bag's standard
    /// `MethodOnClass` chase. Latest-wins among duplicates: same
    /// `(base, method)` pair seen twice (two `resultset(...)` calls
    /// in one file) re-publishes the same ReturnExpr — the
    /// reducer's content equality on Operator(RowOf(Receiver))
    /// makes that a no-op.
    ///
    /// Idempotent across re-runs: `EXTRACT_VERSION` bumps when
    /// `WitnessPayload::ReturnExpr` lands, so cached blobs from
    /// before this phase don't carry stale declarations. Within
    /// one build, the worklist driver doesn't call this — emission
    /// happens once during the live walk.
    pub(super) fn emit_parametric_return_expr_decls(&mut self, ty: &InferredType) {
        let Some(p) = ty.as_parametric() else { return };
        // The flavor's own dispatch class pins the slot — no per-variant
        // match, so a flavor with no declarations (empty vec) is a no-op.
        let Some(base_class) = p.class_name().map(|s| s.to_string()) else { return };
        let zero = Span {
            start: Point { row: 0, column: 0 },
            end: Point { row: 0, column: 0 },
        };
        for (method_name, return_expr) in p.return_method_declarations() {
            self.bag.push(crate::model::witnesses::Witness {
                attachment: crate::model::witnesses::WitnessAttachment::MethodOnClass {
                    class: base_class.clone(),
                    name: method_name.to_string(),
                },
                source: crate::model::witnesses::WitnessSource::Builder(
                    "parametric_return_expr".into(),
                ),
                payload: crate::model::witnesses::WitnessPayload::ReturnExpr(return_expr),
                span: zero,
            });
        }
    }

    /// Like `first_string_literal_arg` but additionally const-folds
    /// a scalar arg (`$sner` where `my $sner = 'Foo'` exists in
    /// scope). Used by `extract_resultset_parametric` so
    /// `$schema->resultset($sner)` types the same as
    /// `$schema->resultset('Foo')` when `$sner` is a known
    /// compile-time constant. Multi-value const-folding (a scalar
    /// that resolves to multiple strings) bails — Parametric's
    /// `row` is a single class, not a sum type yet.
    pub(super) fn first_string_or_constfold_arg(&self, args: Node<'a>) -> Option<String> {
        if let Some(s) = self.first_string_literal_arg(args) {
            return Some(s);
        }
        // Try the first arg as a `scalar` node with const-foldable
        // value. Same one-level unwrap rule as the literal helper.
        let arg_node = match args.kind() {
            "scalar" => Some(args),
            "parenthesized_expression" | "list_expression" => {
                let mut found: Option<Node<'a>> = None;
                for i in 0..args.named_child_count() {
                    if let Some(c) = args.named_child(i) {
                        found = Some(c);
                        break;
                    }
                }
                found
            }
            _ => None,
        }?;
        if arg_node.kind() != "scalar" {
            return None;
        }
        let var_text = arg_node.utf8_text(self.source).ok()?;
        let folded = self.resolve_constant_strings(var_text, 0)?;
        // Single-value fold only — multi-value (loop variable that
        // takes several strings) doesn't have a single row class to
        // emit. Future sum-types work could lift this.
        if folded.len() == 1 {
            folded.into_iter().next()
        } else {
            None
        }
    }

    /// Discover the resultset class for a given row class via the
    /// `<NS>::Result::<X>` ↔ `<NS>::ResultSet::<X>` convention.
    /// Returns `Some(class)` only when the discovered class is
    /// declared in the file's symbols (`SymKind::Package` or
    /// `SymKind::Class`). Cross-file discovery (looking up
    /// `<NS>::ResultSet::<X>` in `module_index`) is queued with
    /// the DBIC plugin port — most projects keep the row class
    /// and resultset class in sibling files of the same dist,
    /// so this in-file discovery covers the common case until
    /// the plugin lands.
    pub(super) fn discover_resultset_class(&self, row_class: &str) -> Option<String> {
        if !row_class.contains("::Result::") {
            return None;
        }
        let candidate = row_class.replacen("::Result::", "::ResultSet::", 1);
        let exists = self.symbols.iter().any(|s| {
            s.name == candidate
                && matches!(s.kind, SymKind::Package | SymKind::Class)
        });
        if exists { Some(candidate) } else { None }
    }

    /// First named child of a call-arg node that's a string literal,
    /// returning its content. Returns None for non-literal first
    /// args (variables, computed expressions). Shared between the
    /// resultset emission and any future parametric-by-literal
    /// emission rule.
    pub(super) fn first_string_literal_arg(&self, args: Node<'a>) -> Option<String> {
        // Single-arg method calls land here with `args` itself a
        // `string_literal` node (no surrounding paren / list
        // expression). Multi-arg calls wrap in
        // `list_expression`/`parenthesized_expression`. Handle the
        // bare-string case first, then recurse into wrappers.
        match args.kind() {
            "string_literal" | "interpolated_string_literal" => {
                return self.extract_string_content(args);
            }
            "parenthesized_expression" | "list_expression" => {
                for i in 0..args.named_child_count() {
                    let child = args.named_child(i)?;
                    return match child.kind() {
                        "string_literal" | "interpolated_string_literal" => {
                            self.extract_string_content(child)
                        }
                        "parenthesized_expression" | "list_expression" => {
                            self.first_string_literal_arg(child)
                        }
                        _ => None,
                    };
                }
            }
            _ => {}
        }
        None
    }

    /// The scalar variable names in a paren-list LHS (`my ($a, $b)` /
    /// `($a, $b) = ...`) — the list-context binding sites. Empty for a
    /// scalar LHS (`my $x`) or a non-declaration. Arrays/hashes in the list
    /// are skipped (they slurp, not bind a single row).
    pub(super) fn paren_list_scalars(&self, lhs: Node<'a>) -> Vec<String> {
        let mut out = Vec::new();
        // `my ($a, $b)` parses as a `variable_declaration` with one
        // `variables` FIELD PER scalar (not a single list node), so walk the
        // fielded children. A bare paren/list expression on the LHS holds its
        // scalars as named children directly.
        let mut cursor = lhs.walk();
        match lhs.kind() {
            "variable_declaration" => {
                for c in lhs.children_by_field_name("variables", &mut cursor) {
                    if c.kind() == "scalar" {
                        if let Ok(t) = c.utf8_text(self.source) {
                            out.push(t.to_string());
                        }
                    }
                }
            }
            "parenthesized_expression" | "list_expression" => {
                for i in 0..lhs.named_child_count() {
                    if let Some(c) = lhs.named_child(i) {
                        if c.kind() == "scalar" {
                            if let Ok(t) = c.utf8_text(self.source) {
                                out.push(t.to_string());
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        out
    }

    /// Innermost scope id containing `point` (the same smallest-span rule
    /// `apply_chain_typing_assignments` uses), `ScopeId(0)` when none.
    pub(super) fn innermost_scope_id_at(&self, point: Point) -> ScopeId {
        self.scopes
            .iter()
            .filter(|s| crate::model::file_analysis::contains_point(&s.span, point))
            .min_by_key(|s| {
                let r = (s.span.end.row.saturating_sub(s.span.start.row)) as u64;
                let c = if s.span.start.row == s.span.end.row {
                    s.span.end.column.saturating_sub(s.span.start.column) as u64
                } else {
                    0
                };
                r * 1_000_000 + c
            })
            .map(|s| s.id)
            .unwrap_or(ScopeId(0))
    }

    pub(super) fn get_var_text_from_lhs(&self, lhs: Node<'a>) -> Option<String> {
        if lhs.kind() == "variable_declaration" {
            if let Some(var) = lhs.child_by_field_name("variable") {
                return var.utf8_text(self.source).ok().map(|s| s.to_string());
            }
            // Paren list: my ($x) = ...
            if let Some(vars) = lhs.child_by_field_name("variables") {
                for i in 0..vars.named_child_count() {
                    if let Some(child) = vars.named_child(i) {
                        if matches!(child.kind(), "scalar" | "array" | "hash") {
                            return child.utf8_text(self.source).ok().map(|s| s.to_string());
                        }
                    }
                }
            }
        }
        if matches!(lhs.kind(), "scalar" | "array" | "hash") {
            return lhs.utf8_text(self.source).ok().map(|s| s.to_string());
        }
        None
    }

    /// The targets of a LIST/destructuring assignment LHS (`my ($a, $b) =
    /// …`) with each one's positional extraction — `Some` ONLY for a paren
    /// list (the `variables` field); `None` for a single `my $x`, leaving the
    /// existing single-var path untouched. A scalar at slot N is `Positional(N)`,
    /// a slurpy `@rest`/`%opts` is `Slurpy(N)` (consumes the tail).
    pub(super) fn lhs_list_targets(
        &self,
        lhs: Node<'a>,
    ) -> Option<Vec<(String, crate::model::file_analysis::Extraction)>> {
        use crate::model::file_analysis::Extraction;
        // `my ($a, $b)` (variable_declaration) OR a bare `($a, $b) = …`
        // reassignment (a `list_expression` LHS — no `my`).
        if !matches!(lhs.kind(), "variable_declaration" | "list_expression") {
            return None;
        }
        // A single `my $x` uses the `variable` field; a list `my ($a, $b)` uses
        // the (repeated) `variables` field. The former is not a list. (A
        // `list_expression` has no `variable` field, so it falls through.)
        if lhs.child_by_field_name("variable").is_some() {
            return None;
        }
        let mut out = Vec::new();
        let mut cursor = lhs.walk();
        let mut pos = 0usize;
        for child in lhs.named_children(&mut cursor) {
            let extraction = match child.kind() {
                "scalar" => Extraction::Positional(pos),
                "array" | "hash" => Extraction::Slurpy(pos),
                _ => continue,
            };
            if let Ok(t) = child.utf8_text(self.source) {
                out.push((t.to_string(), extraction));
            }
            pos += 1;
        }
        (!out.is_empty()).then_some(out)
    }

    /// The per-element NODES of a literal-list RHS (`(10, "str")` → [10,
    /// "str"]), so a list assignment can edge each LHS var straight to its
    /// element (emit its witness + use its own span — no container projection).
    /// `None` when the RHS isn't a literal list (`@arr`, a call) — that path
    /// needs the source typed as a Positional container instead.
    pub(super) fn list_element_nodes(&self, node: Node<'a>) -> Option<Vec<Node<'a>>> {
        let inner = if node.kind() == "parenthesized_expression" {
            node.named_child(0)?
        } else {
            node
        };
        if inner.kind() != "list_expression" {
            return None;
        }
        let mut cursor = inner.walk();
        Some(inner.named_children(&mut cursor).collect())
    }

    pub(super) fn get_hash_var_from_element(&self, node: Node<'a>) -> Option<String> {
        // hash_element_expression: first named child is the container variable
        for i in 0..node.named_child_count() {
            if let Some(child) = node.named_child(i) {
                if matches!(child.kind(), "container_variable" | "keyval_container_variable" | "scalar" | "hash") {
                    return child.utf8_text(self.source).ok().map(|s| s.to_string());
                }
            }
        }
        None
    }

    pub(super) fn extract_key_text(&self, key_node: Node<'a>) -> Option<(String, bool)> {
        match key_node.kind() {
            "autoquoted_bareword" => {
                key_node.utf8_text(self.source).ok().map(|s| (s.to_string(), false))
            }
            "string_literal" | "interpolated_string_literal" => {
                // Simple string: 'key' or "key"
                let text = key_node.utf8_text(self.source).ok()?;
                // Strip quotes
                if text.len() >= 2 {
                    let inner = &text[1..text.len()-1];
                    // Dynamic if interpolated and contains $/@
                    let is_dynamic = key_node.kind() == "interpolated_string_literal"
                        && (inner.contains('$') || inner.contains('@'));
                    Some((inner.to_string(), is_dynamic))
                } else {
                    None
                }
            }
            _ => {
                // Dynamic key (variable, expression)
                key_node.utf8_text(self.source).ok().map(|s| (s.to_string(), true))
            }
        }
    }

    pub(super) fn detect_anon_hash_owner(&self, anon_hash: Node<'a>) -> Option<HashKeyOwner> {
        let mut ancestor = anon_hash.parent()?;
        for _ in 0..5 {
            // Check if this is inside a bless call
            if self.is_bless_call(ancestor) {
                let pkg = self.current_package.clone()
                    .or_else(|| self.scopes[self.current_scope().0 as usize].package.clone());
                if let Some(pkg) = pkg {
                    // Inside a sub: register to `Sub{C, sub_name}` —
                    // that's the actual constructor (or whatever sub
                    // does the blessing). `has`-emitted HashKeyDefs
                    // use the same shape, so a single owner encoding
                    // covers both registration paths and call-site
                    // HashKeyAccess refs (Sub{C, method}) match
                    // strict. Top-level blesses (rare) keep the
                    // coarse `Class(C)` form.
                    return Some(match self.enclosing_sub_name() {
                        Some(name) => HashKeyOwner::Sub { package: Some(pkg), name },
                        None => HashKeyOwner::Class(pkg),
                    });
                }
            }
            // Check if this is inside a return expression of a sub
            if ancestor.kind() == "return_expression" {
                if let Some(name) = self.enclosing_sub_name() {
                    return Some(HashKeyOwner::Sub {
                        package: self.current_package.clone(),
                        name,
                    });
                }
            }
            // Check if this is the last expression in a sub body (implicit return)
            if ancestor.kind() == "expression_statement" {
                if self.is_last_statement_in_sub(ancestor) {
                    if let Some(name) = self.enclosing_sub_name() {
                        return Some(HashKeyOwner::Sub {
                            package: self.current_package.clone(),
                            name,
                        });
                    }
                }
            }
            ancestor = ancestor.parent()?;
        }
        None
    }

    pub(super) fn is_bless_call(&self, node: Node<'a>) -> bool {
        let kind = node.kind();
        if kind == "function_call_expression" || kind == "ambiguous_function_call_expression" {
            if let Some(func) = node.child_by_field_name("function") {
                return func.utf8_text(self.source).ok() == Some("bless");
            }
        }
        false
    }

    /// `bless $ref, $class` promotes `$ref`'s type from its hashref/
    /// arrayref rep to `ClassName($class)` — after the bless the value IS
    /// an instance, so `$self->method` resolves. We push a `ClassName` TC
    /// on the first arg scoped at the bless statement; the temporal fold
    /// makes it win for queries past this point. Honest-miss when the class
    /// isn't statically determinable (a computed `$class` that doesn't
    /// resolve to a class name) — no TC, the value keeps its rep type.
    ///
    /// Class resolution for the 2nd arg: `$class`/`shift`/`$_[0]` →
    /// enclosing class via the bag's FirstParam projection; `__PACKAGE__`
    /// → current package; a string/bareword literal → its text; a missing
    /// 2nd arg (`bless {}`) → current package (Perl's one-arg bless blesses
    /// into the caller's package).
    pub(super) fn visit_bless_call(&mut self, node: Node<'a>) {
        let args = self.extract_call_args(node);
        let obj = match args.first() {
            Some(n) => *n,
            None => return,
        };
        // Receiver-polymorphic statement bless — the Bugzilla::Object idiom
        // (`bless($object, $class) if $object; return $object;`). The class
        // is the CALL SITE's receiver, not a name resolvable here, so a
        // concrete TC can't carry it: push the deferred witness on the
        // VARIABLE (see `push_receiver_bless_witness`). The concrete TC
        // below still fires when the class ALSO resolves statically
        // (`$class` from `shift` → enclosing class via FirstParam): that
        // concrete witness is the in-body baseline; the deferred one wins
        // at real call sites via reducer order.
        if obj.kind() == "scalar" {
            if let Ok(text) = obj.utf8_text(self.source) {
                let text = text.to_string();
                self.push_receiver_bless_witness(&text, node);
            }
        }
        let class = match args.get(1) {
            Some(class_node) => match self.bless_class_of(*class_node) {
                Some(c) => c,
                None => return, // honest miss — class undeterminable
            },
            // One-arg `bless {}` blesses into the current package.
            None => match self.current_package.clone() {
                Some(p) => p,
                None => return,
            },
        };
        self.push_var_type_constraint(obj, node, InferredType::ClassName(class));
    }

    /// If `bless_node` is a bless whose class argument denotes the RECEIVER
    /// (`bless X, $class` / `ref $x || $x` — `bless_class_is_receiver`),
    /// push the deferred `ReturnExpr::ReceiverOr(enclosing)` witness on
    /// `var_name` — the receiver-polymorphic ctor's variable half. A
    /// concrete TC can't carry "the call site's class"; the deferred
    /// payload rides the variable, the return-arm chase reaches it through
    /// `Edge(Variable)` with the caller's receiver threaded
    /// (`query_variable_with_visited`), and `ReturnExprReducer` substitutes
    /// at each call site — so an inherited `$class->new` types to the
    /// subclass it was called on, cross-file included. Temporal: the
    /// witness carries the bless span; pre-bless queries keep the rep type.
    /// Returns whether a witness was pushed.
    pub(super) fn push_receiver_bless_witness(&mut self, var_name: &str, bless_node: Node<'a>) -> bool {
        if !self.is_bless_call(bless_node) {
            return false;
        }
        let args = self.extract_call_args(bless_node);
        let Some(class_node) = args.get(1) else { return false };
        if !self.bless_class_is_receiver(*class_node) {
            return false;
        }
        let re = match self.current_package.clone() {
            Some(pkg) => {
                crate::model::witnesses::ReturnExpr::ReceiverOr(InferredType::ClassName(pkg))
            }
            None => crate::model::witnesses::ReturnExpr::Receiver,
        };
        self.bag.push(crate::model::witnesses::Witness {
            attachment: crate::model::witnesses::WitnessAttachment::Variable {
                name: var_name.to_string(),
                scope: self.current_scope(),
            },
            source: crate::model::witnesses::WitnessSource::Builder("bless_receiver".into()),
            payload: crate::model::witnesses::WitnessPayload::ReturnExpr(re),
            span: node_to_span(bless_node),
        });
        true
    }

    /// Resolve a `bless`'s class argument to a class name. String/bareword
    /// literals read directly; everything else routes through the same
    /// invocant-class resolver used for method receivers (so `$class` from
    /// `shift`, `__PACKAGE__`, etc. resolve identically).
    /// Does this `bless`-class argument denote the RECEIVER (the call-site
    /// invocant) rather than a fixed class? Matches the receiver-polymorphic
    /// constructor idiom: a bare invocant (`shift` / `$self` / `$class` /
    /// `$_[0]`), `ref <invocant>`, or `<a> || <b>` / `<a> // <b>` where a side
    /// denotes the receiver (`ref $class || $class`). A string / bareword /
    /// `__PACKAGE__` is NOT the receiver — it blesses into a fixed class. When
    /// true the bless returns `ReturnExpr::ReceiverOr`, so an inherited ctor /
    /// `SUPER::new` chain types to whatever subclass it was called on.
    pub(super) fn bless_class_is_receiver(&self, node: Node<'a>) -> bool {
        if self.is_shift_call(node) {
            return true;
        }
        match node.kind() {
            "scalar" => crate::cst::is_conventional_invocant_scalar(node, self.source),
            "array_element_expression" => self.is_positional_receiver(node),
            // `ref <invocant>`
            "func1op_call_expression" => {
                node.child(0).and_then(|c| c.utf8_text(self.source).ok()) == Some("ref")
                    && node
                        .named_child(0)
                        .map_or(false, |op| self.bless_class_is_receiver(op))
            }
            // `<a> || <b>` / `<a> // <b>` — receiver if either side is.
            "binary_expression" => {
                matches!(self.get_operator_text(node).as_deref(), Some("||") | Some("//"))
                    && (0..node.named_child_count())
                        .filter_map(|i| node.named_child(i))
                        .any(|c| self.bless_class_is_receiver(c))
            }
            _ => false,
        }
    }

    pub(super) fn bless_class_of(&self, class_node: Node<'a>) -> Option<String> {
        // `__PACKAGE__` parses as a func0op call here (not a bareword), so
        // `invocant_type_at_node`'s bareword arm doesn't catch it — resolve
        // it to the enclosing package directly.
        if class_node.utf8_text(self.source).ok().is_some_and(crate::model::conventions::is_current_package_token) {
            return self.package_for_node(class_node);
        }
        // `bless $r, ref $x` (the clone idiom) blesses into `$x`'s class: the
        // 2nd arg `ref EXPR` yields EXPR's class *name*, so the bless target is
        // EXPR's resolved class. `ref EXPR` as a general value is a String, so
        // this unwrap lives here (the class slot) rather than in the generic
        // invocant resolver.
        if class_node.kind() == "func1op_call_expression"
            && class_node.child(0).and_then(|c| c.utf8_text(self.source).ok()) == Some("ref")
        {
            return class_node
                .named_child(0)
                .and_then(|operand| self.resolve_invocant_class_tree(operand));
        }
        if let Some(s) = self.literal_arg_string(class_node) {
            if !s.is_empty() {
                return Some(s);
            }
        }
        self.resolve_invocant_class_tree(class_node)
    }

    /// Emit a `HashKeyAccess` ref at every odd-indexed (1st, 3rd, …)
    /// stringy arg inside a call's args node, owned by `owner`. In
    /// Perl, `foo(a => 1, "b", 2, c => 3)` is `foo("a", 1, "b", 2,
    /// "c", 3)` — `=>` is just an autoquoting comma. The keys are
    /// the even-position named args, regardless of which separator
    /// comes after them. Mirrors `collect_pair_keys` (callee
    /// side, emits HashKeyDef symbols); this is the caller side, so
    /// `ref_at` on the key token picks the narrow span over the
    /// broad MethodCall/FunctionCall ref. Without this, cursor on
    /// the key in `MooApp->new(name => 'alice')` lands on the
    /// method ref and rename clobbers the wrong token.
    ///
    /// Gated on a matching HashKeyDef already being registered for
    /// `owner`. Otherwise we'd shadow the broader MethodCall ref
    /// for cases the caller-side has no def to anchor on (`class
    /// Foo { field $x :param }` — Point->new(x => 3, …) needs the
    /// MethodCall ref's `find_param_field` fallback in
    /// `find_definition`).
    /// Emit `HashKeyAccess` refs for the keys of a `my %h = (k => …)` literal,
    /// keyed by the hash variable (`var_text = %h`, `owner: None`). The post-walk
    /// owner fixup resolves them to `Variable{%h, def_scope}` — the same owner
    /// the `$h{k}` accesses get — so they group with the accesses for rename.
    pub(super) fn emit_lexical_hash_literal_keys(&mut self, hash_name: &str, rhs: Node<'a>) {
        for (key_node, _value) in crate::cst::pair_nodes(rhs) {
            if !matches!(
                key_node.kind(),
                "bareword" | "autoquoted_bareword" | "string_literal" | "interpolated_string_literal"
            ) {
                continue;
            }
            let Some((key, is_dynamic)) = self.extract_key_text(key_node) else { continue };
            if is_dynamic {
                continue;
            }
            let span = if matches!(key_node.kind(), "string_literal" | "interpolated_string_literal") {
                self.string_content_span(key_node)
            } else {
                node_to_span(key_node)
            };
            self.refs.push(Ref {
                kind: RefKind::HashKeyAccess { var_text: hash_name.to_string(), owner: None },
                span,
                scope: self.scope_at_point(span.start),
                target_name: key,
                access: AccessKind::Write,
                resolves_to: None,
                resolved_method_target: None,
                folded_from: None,
                arg_count: None,
            });
        }
    }

    pub(super) fn emit_call_arg_key_accesses(&mut self, args_node: Node<'a>, gate: Gate) {
        // Unwrap one level into a hash literal / paren wrapper —
        // search's `{KEY=>...}` is `anonymous_hash_expression`
        // wrapping a `list_expression` of pairs; constructors are
        // paren-wrapped. Even-position iteration expects a flat
        // pair list. Constructor-style args without the wrapper
        // pass through unchanged.
        let mut effective = args_node;
        // A column-keyed verb's column hash is POSITIONALLY the first arg
        // (`search(\%cond, \%attrs)` → `\%cond`; `create(\%cols)`). Narrow to it
        // only when arg 0 is itself a hash literal — a scalar/arrayref cond
        // (`search($cond, \%attrs)`) carries no inline keys, and the trailing
        // `\%attrs` hash (`order_by`/`rows`/…) must never be mistaken for it
        // (picking "first hash among args" would walk it).
        if matches!(gate, Gate::ColumnKeyed(_)) {
            let arg0 = if effective.kind() == "anonymous_hash_expression" {
                Some(effective)
            } else {
                effective.named_child(0)
            };
            match arg0 {
                // Hashref cond/cols (`search({…})`, `create({…})`): narrow to it.
                Some(h) if h.kind() == "anonymous_hash_expression" => effective = h,
                // Flat constructor pairs (`new(name => 1)`): arg 0 is a stringy
                // key — walk the top-level pair list (effective unchanged).
                Some(k) if matches!(
                    k.kind(),
                    "bareword" | "autoquoted_bareword" | "string_literal" | "interpolated_string_literal"
                ) => {}
                // A positional non-key first arg (`search($cond, \%attrs)`): the
                // cond is a prebuilt ref with no inline keys, and the trailing
                // `\%attrs` hash is not column-keyed. Walk nothing.
                _ => return,
            }
        }
        // Pair-walk via the shared `cst::pair_nodes`: Perl tucks the tail pairs
        // of `a => 1, b => 2` into a right-nested `list_expression`, so manual
        // even/odd iteration over the top-level children sees only the FIRST
        // pair. `pair_nodes` flattens the nesting (and skips separators), so
        // every key is walked. (CLAUDE.md: don't re-derive pair-walking.)
        for (child, _value) in crate::cst::pair_nodes(effective) {
            if !matches!(
                child.kind(),
                "bareword" | "autoquoted_bareword" | "string_literal" | "interpolated_string_literal"
            ) {
                continue;
            }
            let Some((key, is_dynamic)) = self.extract_key_text(child) else { continue };
            if is_dynamic { continue; }
            // Gate decides whether to emit + what owner to record.
            // Strict checks the local symbol table for a matching
            // HashKeyDef (prevents `Foo::bar(name=>1)` from latching
            // onto unrelated `Sub{Foo,new}` keys). Open trusts the
            // caller (the receiver's flavor pinned the owner).
            // Deferred emits with `owner: None`; the post-walk
            // fixup in `fix_chain_receiver_hash_key_owners` fills
            // the owner once the receiver type resolves
            // (in-file or cross-file).
            let owner_to_emit: Option<HashKeyOwner> = match &gate {
                Gate::Strict(owner) => {
                    if !self.has_hash_key_def(&key, owner) { continue; }
                    Some(owner.clone())
                }
                Gate::ColumnKeyed(sub_owner) => {
                    // First-hashref narrowing already happened in `effective`.
                    // A key that's an actual column → the column owner; else fall
                    // back to the `Sub{class,verb}` owner if the verb declares it
                    // (Moo/Corinna ctor keys under a generic-named `new`); else
                    // skip (so `order_by` and friends never latch on).
                    let class = match sub_owner {
                        HashKeyOwner::Sub { package: Some(c), .. } => c.clone(),
                        _ => continue,
                    };
                    let col = HashKeyOwner::Bridged { class };
                    if self.has_hash_key_def(&key, &col) {
                        Some(col)
                    } else if self.has_hash_key_def(&key, sub_owner) {
                        Some(sub_owner.clone())
                    } else {
                        continue;
                    }
                }
                Gate::StrictOrDefer(owner) => {
                    if self.has_hash_key_def(&key, owner) {
                        Some(owner.clone())
                    } else {
                        None
                    }
                }
                Gate::Open(owner) => Some(owner.clone()),
                Gate::Deferred => None,
            };
            let access = self.determine_access(child);
            // `scope_at_point` instead of `current_scope` so this
            // works both inside the walk (function-call args path)
            // and post-walk (method-call args path; scope_stack is
            // empty by then). Equivalent at walk-time because a
            // node's innermost containing scope IS what
            // current_scope would return.
            //
            // A quoted key (`{ "name", 2 }`) must rename its CONTENT, not the
            // whole literal — rewriting the quotes turns it into a bareword
            // (`{ fullname, 2 }`), a `strict subs` error in a plain-comma list.
            let span = if matches!(
                child.kind(),
                "string_literal" | "interpolated_string_literal"
            ) {
                self.string_content_span(child)
            } else {
                node_to_span(child)
            };
            self.refs.push(Ref {
                kind: RefKind::HashKeyAccess {
                    var_text: String::new(),
                    owner: owner_to_emit,
                },
                span,
                scope: self.scope_at_point(span.start),
                target_name: key,
                access,
                resolves_to: None,
                resolved_method_target: None,
                folded_from: None,
                arg_count: None,
            });
        }
    }

    /// Strict (no `found_by` broadening) check: is a HashKeyDef
    /// registered with this exact `owner` and `name`? Used by
    /// `emit_call_arg_key_accesses` to gate emission — broadening
    /// would let `Foo::bar(name => 1)` latch onto `name` keys
    /// registered to `Sub{Foo, new}`, which they don't logically
    /// belong to.
    pub(super) fn has_hash_key_def(&self, name: &str, owner: &HashKeyOwner) -> bool {
        self.symbols.iter().any(|s| {
            if s.name != name { return false; }
            matches!(&s.detail, SymbolDetail::HashKeyDef { owner: o, .. } if o == owner)
        })
    }

    /// Emit a `HashKeyDef` symbol for every key in a hash literal owned by
    /// `owner` (e.g. a blessed `{ ... }`). Keys are the even-position elements
    /// of the flat pair sequence — `{ a => 1, 'b', 2 }` is `{ 'a', 1, 'b', 2 }`,
    /// so the separator (`,` vs `=>`) is irrelevant; we pair positionally via
    /// the shared node-level walker and take each key node.
    pub(super) fn collect_pair_keys(&mut self, node: Node<'a>, owner: &HashKeyOwner) -> Vec<String> {
        let mut defs: Vec<(String, Span)> = Vec::new();
        for (k_node, _val) in crate::cst::pair_nodes(node) {
            if matches!(
                k_node.kind(),
                "autoquoted_bareword" | "string_literal" | "interpolated_string_literal"
            ) {
                if let Some((key, is_dynamic)) = self.extract_key_text(k_node) {
                    if !is_dynamic {
                        defs.push((key, node_to_span(k_node)));
                    }
                }
            }
        }
        let keys: Vec<String> = defs.iter().map(|(k, _)| k.clone()).collect();
        for (key, span) in defs {
            self.add_symbol(
                key,
                SymKind::HashKeyDef,
                span,
                span,
                SymbolDetail::HashKeyDef { owner: owner.clone(), is_dynamic: false },
            );
        }
        keys
    }

    /// Synthesize `Sub` symbols for AutoLoader / SelfLoader packages whose
    /// real sub definitions live in the file's `data_section` (the text after
    /// `__END__` / `__DATA__`). Those subs are loaded on demand at runtime by
    /// `AUTOLOAD`, so they are live code — but tree-sitter parks the whole
    /// region in one opaque `data_section` node, leaving them un-navigable.
    ///
    /// Gate: only packages that actually use AutoLoader/SelfLoader (via `use`
    /// or `@ISA`/`use base`/`use parent`) get this treatment, so genuine
    /// `__DATA__` payloads and trailing POD on ordinary modules synthesize
    /// nothing. This is a framework semantic (like Moo/Mojo detection), not a
    /// shape-branch: the property "package is autoload-backed" rides on the
    /// package's recorded uses/parents, and the data section is only mined when
    /// the owning package answers yes.
    ///
    /// Re-parsing the data-section text as Perl is the single-build-entry way
    /// to recover the sub shapes (rule #1: all CST traversal stays in build()).
    /// Spans are offset back to real file coordinates so goto-def lands.
    pub(super) fn synthesize_autoloader_data_subs(&mut self, tree: &Tree) {
        let Some(data) = find_data_section(tree.root_node()) else { return };

        // Which package is in effect at the data section? `__END__`/`__DATA__`
        // terminates compilation, so the owning package is whichever range
        // brackets the marker — for the typical single-package AutoLoader
        // module that's the only package in the file.
        let data_start = data.start_position();
        let owner_pkg = self
            .package_ranges
            .iter()
            .rev()
            .find(|r| crate::model::file_analysis::contains_point(&r.span, data_start))
            .map(|r| r.package.clone())
            .or_else(|| self.current_package.clone());

        let Some(pkg) = owner_pkg else { return };
        if !self.is_autoload_backed(&pkg) {
            return;
        }

        let section_text = match data.utf8_text(self.source) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        };

        let mut parser = tree_sitter::Parser::new();
        if parser.set_language(&ts_parser_perl::LANGUAGE.into()).is_err() {
            return;
        }
        let Some(sub_tree) = parser.parse(&section_text, None) else { return };
        let sub_src = section_text.as_bytes();

        // tree-sitter reports re-parse positions relative to the section text;
        // map them back to file coordinates. Row N of the section is file row
        // (data_start.row + N); the section's first row continues the
        // `__END__` line, so its column carries the marker offset.
        let offset = |p: Point| -> Point {
            if p.row == 0 {
                Point { row: data_start.row, column: data_start.column + p.column }
            } else {
                Point { row: data_start.row + p.row, column: p.column }
            }
        };
        let offset_span = |s: Span| -> Span {
            Span { start: offset(s.start), end: offset(s.end) }
        };

        let prev_pkg = self.current_package.take();
        self.current_package = Some(pkg.clone());

        let mut found: Vec<Node> = Vec::new();
        collect_data_section_subs(sub_tree.root_node(), &mut found);
        let mut synth_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for sub_node in found {
            let Some(name_node) = sub_node.child_by_field_name("name") else { continue };
            let Ok(name) = name_node.utf8_text(sub_src) else { continue };
            let params = extract_data_section_params(sub_node, sub_src);
            self.add_symbol(
                name.to_string(),
                SymKind::Sub,
                offset_span(node_to_span(sub_node)),
                offset_span(node_to_span(name_node)),
                SymbolDetail::Sub {
                    params,
                    is_method: false,
                    doc: None,
                    display: None,
                    hide_in_outline: false,
                    opaque_return: false,
                    is_constant: false,
                    lexical: false,
                },
            );
            synth_names.insert(name.to_string());
        }

        self.current_package = prev_pkg;

        // Bareword calls to these subs were visited during the walk before the
        // synthesized symbols existed, so `resolve_call_package` left their
        // `FunctionCall` ref unresolved (`resolved_package: None`). Pin any such
        // ref — within the owning package's source region and naming a sub we
        // just synthesized — to `pkg`, so goto-def lands on the data-section
        // definition. Scoped to autoload-backed packages: we only touch refs
        // whose target is one of the freshly minted names.
        for r in self.refs.iter_mut() {
            if let RefKind::FunctionCall { resolved_package } = &mut r.kind {
                if resolved_package.is_none() && synth_names.contains(&r.target_name) {
                    let in_pkg = self
                        .package_ranges
                        .iter()
                        .rev()
                        .find(|pr| crate::model::file_analysis::contains_point(&pr.span, r.span.start))
                        .is_some_and(|pr| pr.package == pkg);
                    if in_pkg {
                        *resolved_package = Some(pkg.clone());
                    }
                }
            }
        }
    }

    /// True when `pkg` pulls in AutoLoader or SelfLoader — either via a `use`
    /// line or by inheriting from it (`@ISA`/`use base`/`use parent`). Both
    /// reach `package_parents`; `use` lines reach `package_uses`.
    pub(super) fn is_autoload_backed(&self, pkg: &str) -> bool {
        let is_loader = |m: &str| m == "AutoLoader" || m == "SelfLoader";
        self.package_uses
            .get(pkg)
            .map_or(false, |us| us.iter().any(|m| is_loader(m)))
            || self
                .package_parents
                .get(pkg)
                .map_or(false, |ps| ps.iter().any(|p| is_loader(p)))
    }

    /// Find the nearest enclosing Sub or Method scope from the current scope stack.
    pub(super) fn enclosing_sub_scope(&self) -> Option<ScopeId> {
        for &scope_id in self.scope_stack.iter().rev() {
            match &self.scopes[scope_id.0 as usize].kind {
                ScopeKind::Sub { .. } | ScopeKind::Method { .. } => return Some(scope_id),
                _ => {}
            }
        }
        None
    }

    /// Get the name of the enclosing sub/method, if any.
    /// Extract POD or comment documentation immediately preceding a sub node.
    /// Walks prev_sibling chain (tree-sitter CST traversal stays in builder).
    pub(super) fn extract_preceding_doc(&self, sub_node: Node<'a>, sub_name: &str) -> Option<String> {
        let source_str = std::str::from_utf8(self.source).ok()?;
        let mut prev = sub_node.prev_sibling();
        let mut comment_lines: Vec<String> = Vec::new();

        while let Some(node) = prev {
            match node.kind() {
                "pod" => {
                    let text = &source_str[node.byte_range()];
                    // Try to extract just the section for this sub (=head2 or =item)
                    // extract_head2_section/extract_item_section return rendered markdown
                    if let Some(md) = crate::build::pod::extract_head2_section(sub_name, text) {
                        if !md.is_empty() {
                            return Some(md);
                        }
                    }
                    if let Some(md) = crate::build::pod::extract_item_section(sub_name, text) {
                        if !md.is_empty() {
                            return Some(md);
                        }
                    }
                    // Fallback: convert entire POD block (e.g. single-section pod)
                    let md = crate::build::pod::pod_to_markdown(text);
                    if !md.is_empty() {
                        return Some(md);
                    }
                    break;
                }
                "comment" => {
                    let text = source_str[node.byte_range()].trim();
                    let stripped = text.strip_prefix('#').unwrap_or(text).trim();
                    if !stripped.is_empty() {
                        comment_lines.push(stripped.to_string());
                    }
                }
                _ => break, // hit code, stop
            }
            prev = node.prev_sibling();
        }

        if !comment_lines.is_empty() {
            comment_lines.reverse(); // collected bottom-up
            return Some(comment_lines.join("\n"));
        }

        None
    }

    /// Post-pass: for subs with no preceding doc, scan collected pod_texts
    /// for a =head2 section matching the sub name (tail POD style).
    pub(super) fn resolve_tail_pod_docs(&mut self) {
        if self.pod_texts.is_empty() {
            return;
        }
        if crate::model::timings::phases_enabled() {
            let nsubs = self.symbols.iter().filter(|s| matches!(s.kind, SymKind::Sub | SymKind::Method)).count();
            let podbytes: usize = self.pod_texts.iter().map(|p| p.len()).sum();
            eprintln!(
                "[PHASE] {:<32} {} subs, {} pod_texts, {} pod_bytes",
                "build::tail_pod_inputs",
                nsubs,
                self.pod_texts.len(),
                podbytes
            );
        }
        // One name → doc map across every POD block. Within a block `=head2`
        // wins over `=item`; across blocks the earlier `pod_text` wins
        // (`or_insert`), so a sub resolves to the first matching section in
        // source order.
        let mut sections: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for pod_text in &self.pod_texts {
            for (name, md) in crate::build::pod::extract_sub_doc_sections(pod_text) {
                sections.entry(name).or_insert(md);
            }
        }
        if sections.is_empty() {
            return;
        }
        for sym in &mut self.symbols {
            if !matches!(sym.kind, SymKind::Sub | SymKind::Method) {
                continue;
            }
            if let SymbolDetail::Sub { ref mut doc, .. } = sym.detail {
                if doc.is_some() {
                    continue; // already has preceding doc
                }
                if let Some(md) = sections.get(&sym.name) {
                    *doc = Some(md.clone());
                }
            }
        }
    }

    pub(super) fn enclosing_sub_name(&self) -> Option<String> {
        let scope_id = self.enclosing_sub_scope()?;
        match &self.scopes[scope_id.0 as usize].kind {
            ScopeKind::Sub { ref name } | ScopeKind::Method { ref name } => Some(name.clone()),
            _ => None,
        }
    }

    /// Check if a node is the last statement in a sub/method body block.
    pub(super) fn is_last_statement_in_sub(&self, node: Node<'a>) -> bool {
        let parent = match node.parent() {
            Some(p) => p,
            None => return false,
        };
        // The parent should be a block that is a sub/method body
        if parent.kind() != "block" {
            return false;
        }
        if let Some(grandparent) = parent.parent() {
            if !matches!(grandparent.kind(),
                "subroutine_declaration_statement" | "method_declaration_statement"
                | "anonymous_subroutine_expression"
            ) {
                return false;
            }
        } else {
            return false;
        }
        // Check this is the last named child in the block
        if let Some(last) = parent.named_child(parent.named_child_count().saturating_sub(1)) {
            last.id() == node.id()
        } else {
            false
        }
    }
}
