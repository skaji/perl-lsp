//! Completion items: candidate conversion, in-scope + native paths, auto-import edits.

use super::*;

pub(crate) fn fa_completion_kind(kind: &FaSymKind) -> CompletionItemKind {
    match kind {
        FaSymKind::Sub => CompletionItemKind::FUNCTION,
        FaSymKind::Method => CompletionItemKind::METHOD,
        FaSymKind::Variable => CompletionItemKind::VARIABLE,
        FaSymKind::Field => CompletionItemKind::FIELD,
        FaSymKind::Enumerator => CompletionItemKind::ENUM_MEMBER,
        FaSymKind::Package => CompletionItemKind::CLASS,
        FaSymKind::Class => CompletionItemKind::CLASS,
        FaSymKind::Module => CompletionItemKind::MODULE,
        FaSymKind::HashKeyDef => CompletionItemKind::PROPERTY,
        FaSymKind::Handler => CompletionItemKind::EVENT,
        FaSymKind::Namespace => CompletionItemKind::MODULE,
    }
}

/// Rank scope-variable candidates whose inferred type matches `expected`
/// first, keeping every other candidate in place (never prunes). A matching
/// variable keeps its `PRIORITY_LOCAL` slot while the non-matching locals it
/// leads are nudged one tier down, so the client's sort_text agrees with the
/// stable reorder the CLI/gold sees. Exact `InferredType` equality, or same
/// class name (a `ClassName` matches by class).
fn rank_candidates_by_expected_type(
    candidates: &mut Vec<CompletionCandidate>,
    expected: &InferredType,
    analysis: &FileAnalysis,
    point: Point,
) {
    use crate::model::file_analysis::PRIORITY_LOCAL;
    let is_match = |c: &CompletionCandidate| -> bool {
        matches!(c.kind, FaSymKind::Variable)
            && analysis
                .inferred_type_via_bag(&c.label, point)
                .is_some_and(|t| inferred_type_matches(expected, &t))
    };
    let mut tagged: Vec<(bool, CompletionCandidate)> =
        candidates.drain(..).map(|c| (is_match(&c), c)).collect();
    for (m, c) in tagged.iter_mut() {
        if !*m && matches!(c.kind, FaSymKind::Variable) && c.sort_priority == PRIORITY_LOCAL {
            c.sort_priority = PRIORITY_LOCAL + 1;
        }
    }
    tagged.sort_by_key(|(m, _)| !*m); // stable: matches (key false) lead
    *candidates = tagged.into_iter().map(|(_, c)| c).collect();
}

/// Does `actual` satisfy the `expected` slot type — exact enum equality, or
/// (for object types) the same class name.
fn inferred_type_matches(expected: &InferredType, actual: &InferredType) -> bool {
    expected == actual
        || matches!(
            (expected.class_name(), actual.class_name()),
            (Some(a), Some(b)) if a == b
        )
}

pub(crate) fn candidate_to_completion_item(c: CompletionCandidate) -> CompletionItem {
    let additional_text_edits = if c.additional_edits.is_empty() {
        None
    } else {
        Some(
            c.additional_edits
                .iter()
                .map(|(span, text)| TextEdit {
                    range: span_to_range(*span),
                    new_text: text.clone(),
                })
                .collect(),
        )
    };
    // `filter_text` is what LSP clients match the typed prefix against
    // when narrowing the completion list client-side. By default it's
    // the label. But when `insert_text` differs (e.g. dispatch-target
    // candidates insert `'connect'` while the label is just `connect`),
    // some clients fall back to `insert_text` for filtering — then
    // typing `c` after `(` stops matching because insert_text starts
    // with `'`. Set filter_text explicitly to the bare label so
    // client-side filtering keys on the name regardless.
    let filter_text = Some(c.label.clone());

    // Sort text places dispatch handlers ABOVE anything
    // complete_general can produce. Both default to sort_priority 0;
    // tied at "000" they interleave alphabetically (connect, fire,
    // message, wire) which makes handlers look like they're mixed
    // into noise. Prefixing with a space character ensures the
    // handler group sorts first as a block — space (0x20) < digit
    // (0x30) lexicographically.
    //
    // The label is the intra-priority tie-break in every case (module /
    // import-list / qualified-path candidates carry it explicitly, and
    // it's what a client falls back to for equal sortText anyway — so
    // spelling it here is ranking-neutral for the identifier/member/key
    // arms and lets this one projection reproduce those arms byte-for-byte).
    let sort_text = if matches!(c.kind, FaSymKind::Handler) {
        Some(format!(" {:03}{}", c.sort_priority, c.label))
    } else {
        Some(format!("{:03}{}", c.sort_priority, c.label))
    };
    let kind = if let Some(ref d) = c.display_override {
        handler_display_to_completion_kind(d)
    } else {
        fa_completion_kind(&c.kind)
    };
    CompletionItem {
        label: c.label,
        kind: Some(kind),
        detail: c.detail,
        insert_text: c.insert_text,
        filter_text,
        sort_text,
        additional_text_edits,
        ..Default::default()
    }
}

/// Language-agnostic in-scope completion: every symbol visible from
/// `point` — top-level definitions (functions / classes / packages,
/// globally addressable) plus locals / params / methods / fields whose
/// declaring scope encloses the cursor — as plain CompletionItems. The
/// client filters by the typed prefix (sigils and all). This is the
/// pack-language completion path (half 1): no cursor context, no member
/// resolution — the `.`/`->` receiver seam is a separate design.
pub fn in_scope_completion(analysis: &FileAnalysis, point: Point) -> Vec<CompletionItem> {
    use std::collections::HashSet;
    let chain: HashSet<_> = analysis
        .scope_at(point)
        .map(|s| analysis.scope_chain(s).into_iter().collect())
        .unwrap_or_default();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut items = Vec::new();
    for sym in analysis.symbols() {
        // Top-level defs are addressable anywhere; everything else
        // (params, locals, a class's methods/fields) only where the
        // declaring scope is on the cursor's scope chain.
        let top_level = matches!(
            sym.kind,
            FaSymKind::Sub | FaSymKind::Class | FaSymKind::Package
        );
        if !top_level && !chain.contains(&sym.scope) {
            continue;
        }
        if sym.name.is_empty() || !seen.insert(sym.name.as_str()) {
            continue;
        }
        items.push(CompletionItem {
            label: sym.name.clone(),
            kind: Some(fa_completion_kind(&sym.kind)),
            ..Default::default()
        });
    }
    items
}

pub fn completion_items(
    files: &crate::index::file_store::FileStore,
    origin_key: &crate::index::file_store::FileKey,
    analysis: &FileAnalysis,
    tree: &Tree,
    source: &str,
    pos: Position,
    module_index: &ModuleIndex,
    stable_packages: Option<&[(String, usize)]>,
) -> Vec<CompletionItem> {
    let point = position_to_point(pos);

    // Plugin query hook — runs BEFORE the native path. A plugin can
    // contribute items and optionally claim exclusivity for the slot
    // (e.g. Minion's arg-0 task-name completion: pure tasks, no
    // Minion instance-method firehose).
    if let Some(qctx) = cursor_context::build_plugin_query_context(analysis, tree, source.as_bytes(), point) {
        let registry = crate::build::plugin::default_plugin_registry();
        let (uses, parents) = analysis.trigger_view_at(point);
        let query = crate::build::plugin::TriggerQuery {
            package_uses: &uses,
            package_parents: &parents,
        };
        let mut plugin_items: Vec<CompletionItem> = Vec::new();
        let mut exclusive = false;
        for p in registry.applicable(&query) {
            if let Some(answer) = p.on_completion(&qctx) {
                if answer.exclusive { exclusive = true; }
                for c in answer.items {
                    plugin_items.push(plugin_completion_to_item(c));
                }
                // Plugin-delegated dispatch-target completion: walk
                // Handler symbols whose owner matches and contribute
                // their names as items. Saves each plugin from
                // reimplementing the symbol-table scan.
                if let Some(req) = answer.dispatch_targets_for {
                    plugin_items.extend(dispatch_target_items_for(
                        analysis, module_index, &req.owner_class, &req.dispatcher_names,
                    ));
                }
            }
        }
        if exclusive {
            return plugin_items;
        }
        if !plugin_items.is_empty() {
            let native = completion_items_native(files, origin_key, analysis, tree, source, pos, module_index, stable_packages);
            let mut out = plugin_items;
            out.extend(native);
            return out;
        }
    }

    completion_items_native(files, origin_key, analysis, tree, source, pos, module_index, stable_packages)
}

/// Test-only convenience: completion against a bare analysis with an empty
/// store (gathering still routes through the CandidateSet; visibility
/// defaults to the full VISIBLE universe).
#[cfg(test)]
pub fn completion_items_for_test(
    analysis: &FileAnalysis,
    tree: &Tree,
    source: &str,
    pos: Position,
    module_index: &ModuleIndex,
    stable_packages: Option<&[(String, usize)]>,
) -> Vec<CompletionItem> {
    let files = crate::index::file_store::FileStore::new();
    let key = crate::index::file_store::FileKey::Path(std::path::PathBuf::from("/test/origin.pl"));
    completion_items(&files, &key, analysis, tree, source, pos, module_index, stable_packages)
}

/// The native completion path — the plugin-aware `completion_items`
/// wrapper above falls through to it.
#[allow(clippy::too_many_arguments)]
fn completion_items_native(
    files: &crate::index::file_store::FileStore,
    origin_key: &crate::index::file_store::FileKey,
    analysis: &FileAnalysis,
    tree: &Tree,
    source: &str,
    pos: Position,
    module_index: &ModuleIndex,
    stable_packages: Option<&[(String, usize)]>,
) -> Vec<CompletionItem> {
    let point = position_to_point(pos);
    // Candidate GATHERING routes through the resolution CandidateSet — the
    // same visible universe references/rename/goto-def project from
    // (docs/adr/resolution-candidate-set.md). The cursor-context matching
    // below decides which slot the cursor is in; the set decides where the
    // identifier names come from.
    let cs = crate::index::resolve::resolve(
        files,
        analysis,
        origin_key.clone(),
        point,
        Some(module_index),
        crate::index::resolve::OverrideScope::default(),
    );

    // The slot verdict (`docs/adr/cursor-slots.md`) — Perl's detector
    // wraps `cursor_context`'s tree-then-text chain unchanged.
    let crate::lsp::cursor_slot::DetectedSlot { slot, arm: slot_arm } = crate::lsp::cursor_slot::detect_slot(
        analysis, tree, source, point, "perl", Some(module_index));
    // Bare-sigil trigger (`$|`/`@|`/`%|`) decoded once so the match below
    // doesn't need a second borrow of `slot` inside its own arm.
    let sigil_trigger = slot.sigil();

    // Mid-string completion for plugin-emitted MethodCallRefs. When the
    // cursor sits inside the span of a MethodCallRef emitted by a plugin
    // (e.g. `->to('Users#lis|')` in mojo-routes), offer methods on the
    // target class — prefix-filtered by whatever's been typed since the
    // `#` (or the whole prefix if none). This generalizes: any plugin
    // that drops a MethodCallRef at a string span gets scoped method
    // completion for free. Runs first so it preempts the generic paths.
    if let Some(refs) = refs_at_point_matching(analysis, point, |r|
        matches!(r.kind, RefKind::MethodCall { .. })
    ) {
        for r in &refs {
            if let RefKind::MethodCall { invocant, .. } = &r.kind {
                let early = mid_string_methodref_completions(
                    analysis, module_index, invocant.text(), source, point, r.span,
                );
                if !early.is_empty() {
                    return early;
                }
            }
        }
    }

    // Dispatch-target completions are orthogonal to the context match:
    // inside `$obj->emit(^)` the cursor is both after a `->` (tree
    // detects `Method`) and inside call args. Pull the call context out
    // once, prepend handler completions at arg-0, and SUPPRESS the global
    // sub/module firehose at arg-N>0 so comma-triggered completion in a
    // dispatch call doesn't dump hundreds of unrelated symbols (sig help
    // is the right affordance past arg-0).
    //
    // Dispatch items go in a separate vec so we can retarget their
    // textEdit range to the string-content span mid-string, without
    // having to filter the shared `candidates` buffer by kind later.
    let mut dispatch_items: Vec<CompletionItem> = Vec::new();
    let mut candidates: Vec<CompletionCandidate> = Vec::new();
    let mut suppress_firehose = false;
    if let Some(call_ctx) = cursor_context::find_call_context(tree, source.as_bytes(), point) {
        if call_ctx.is_method {
            let dispatch_class = analysis.invocant_text_to_class(call_ctx.invocant.as_deref(), point);
            let has_any_handlers = dispatch_class.as_ref().is_some_and(|c|
                class_has_dispatch_handlers(analysis, module_index, c, &call_ctx.name)
            );
            // Debug line for dispatch completion — one-shot diagnoses
            // every "starting to type kills completion" / "no routes
            // offered" report. Includes the four values that together
            // determine whether dispatch fires and which handlers pass
            // the ancestor-walk filter: call name, invocant text,
            // resolved class (None = inferred_type miss), active_param
            // (>0 short-circuits to vars-only), and has_any_handlers
            // (false = bridges empty or filter mismatch).
            log::debug!(
                "completion dispatch: method={:?} invocant={:?} class={:?} active_param={} has_handlers={}",
                call_ctx.name, call_ctx.invocant, dispatch_class,
                call_ctx.active_param, has_any_handlers,
            );

            if call_ctx.active_param == 0 && has_any_handlers {
                // arg-0 of a known dispatcher: handlers at the top,
                // suppress the global sub/module firehose that would
                // otherwise drown them.
                let dispatch_cands = dispatch_target_completions(
                    analysis,
                    module_index,
                    call_ctx.invocant.as_deref(),
                    &call_ctx.name,
                    point,
                    tree,
                );
                dispatch_items.extend(
                    dispatch_cands.into_iter().map(candidate_to_completion_item),
                );
                // When the cursor is inside the string arg
                // (`url_for('/us|ers/profile')`) pin each item's
                // textEdit to the string-content span. The client's
                // default word-at-cursor (nvim's `iskeyword` default
                // excludes `/`, `#`, `:`) can't see across those
                // chars, so filter_text alone is dropped for labels
                // like `/users/profile` or `Users#list`. textEdit.range
                // tells the client "filter by the whole in-range
                // text" — works regardless of keyword class.
                if let Some(span) = string_content_span_at(tree, point) {
                    retarget_items_to_span(&mut dispatch_items, span);
                }
                suppress_firehose = true;
            } else if call_ctx.active_param > 0 && has_any_handlers
                && !matches!(slot, Slot::Key { .. })
            {
                // Past arg-0 in a known dispatcher: the only sensible
                // completion is variables-in-scope (candidates for
                // passing as the next arg). Sig help handles shape
                // guidance. Short-circuit the context match entirely.
                //
                // EXCEPT when the cursor is sitting inside a nested
                // hash literal — that's a HashKey context and the
                // callee (or a plugin) has real keys to offer for it
                // (Minion's `enqueue(..., [...], { | })` options).
                // Skipping the short-circuit there lets the HashKey
                // match run and populate `priority`/`queue`/etc.
                let vars_only: Vec<CompletionCandidate> = cs.complete("", false)
                    .into_iter()
                    .filter(|c| matches!(c.kind, FaSymKind::Variable | FaSymKind::Field))
                    .collect();
                candidates.extend(vars_only);
                return candidates.drain(..).map(candidate_to_completion_item).collect();
            }
        }
    }

    candidates.extend::<Vec<CompletionCandidate>>(match slot {
        Slot::Member { ref receiver, .. } => {
            if let Some(ref ty) = receiver.receiver_type {
                // `class_name_lenient` peels `Optional<Foo>` to `Foo` so an
                // unguarded optional receiver still offers its methods — the
                // same lenient receiver projection goto/hover/refs now use.
                if let Some(cn) = ty.class_name_lenient() {
                    analysis.complete_methods_for_class(cn, Some(module_index))
                } else {
                    // Ref types get deref snippet completions (handled below)
                    Vec::new()
                }
            } else {
                let invocant_text = receiver.receiver_text.as_deref().unwrap_or("");
                analysis.complete_methods(invocant_text, point, Some(module_index))
            }
        }
        Slot::Key { ref owner } => {
            // Keys already written in the enclosing hash literal —
            // they shouldn't re-appear in the suggestions. Scoped to
            // the hash_expression directly so unrelated nearby calls
            // don't interfere. Works for both class-typed hashes and
            // sub-owned ones.
            let used = cursor_context::used_keys_in_enclosing_hash(tree, source.as_bytes(), point);
            let class_name = owner.owner_type.as_ref().and_then(|t| t.class_name());
            let candidates = if let Some(cn) = class_name {
                analysis.complete_hash_keys_for_class(cn, point, Some(module_index))
            } else if let Some(ref sub_name) = owner.source_sub {
                // Routes to HashKeyOwner::Sub { name } — catches both
                // plugin-emitted HashKeyDefs (minion enqueue options)
                // AND body-derived keys from `$opts->{...}` accesses
                // in a final-hashref param. Previously this branch
                // was skipped when owner_type was None, so real hash
                // literals at a call-arg position returned nothing.
                analysis.complete_hash_keys_for_sub(sub_name, point, Some(module_index))
            } else {
                analysis.complete_hash_keys(&owner.var_text, point, Some(module_index))
            };
            candidates.into_iter().filter(|c| !used.contains(&c.label)).collect()
        }
        Slot::Import { ref module } => {
            if let Some(ref name) = module {
                // The export surface is entity content on `CachedModule`;
                // the "still indexing" placeholder is a slot affordance
                // (no entity to gather yet), so it stays adapter-side.
                return match module_index.get_cached(name) {
                    Some(cached) => module_index
                        .whole_present(&cached)
                        .import_list_candidates()
                        .into_iter()
                        .map(candidate_to_completion_item)
                        .collect(),
                    None => vec![import_list_loading_placeholder(name)],
                };
            }
            Vec::new()
        }
        Slot::ModulePath { ref prefix } => {
            // `use Foo::<cursor>` → the loadable-module half; `Foo::<cursor>`
            // mid-expression → the qualified-path drill (subs + sub-packages).
            // Both are candidate-level on the set; this branch is the answer,
            // so it returns directly (the global firehose is suppressed). The
            // arm (not a local field) tells the two renders apart.
            let candidates = if slot_arm == crate::lsp::cursor_slot::DetectorArm::UseModule {
                cs.complete_module_candidates(prefix)
            } else {
                cs.complete_qualified_path(module_index, prefix)
            };
            return candidates.into_iter().map(candidate_to_completion_item).collect();
        }
        Slot::Identifier { .. } if sigil_trigger.is_some() => {
            analysis.complete_variables(point, sigil_trigger.expect("checked by guard"))
        }
        Slot::Identifier { .. } => {
            let mut items = Vec::new();
            // Keyval arg completions if inside a call at key position.
            // (Dispatch-target completions are handled above the match
            // regardless of context, so they apply whether the slot
            // resolves to Member, Identifier, or anything else.)
            if let Some(call_ctx) =
                cursor_context::find_call_context(tree, source.as_bytes(), point)
            {
                if call_ctx.at_key_position {
                    items.extend(analysis.complete_keyval_args(
                        &call_ctx.name,
                        call_ctx.is_method,
                        call_ctx.invocant.as_deref(),
                        point,
                        &call_ctx.used_keys,
                        Some(module_index),
                    ));
                }
            }
            // Identifier universe from the CandidateSet: in-scope names,
            // plus the import-sourced firehose when the slot has an
            // import affordance. The firehose is useful at top-level
            // positions, harmful when we just offered dispatch handlers
            // at arg-0 (they'd drown in it) — `suppress_firehose` is set
            // above when the cursor is at arg-0 of a known dispatcher
            // call, and withholds the affordance. The candidates carry the
            // importable-from FACT; the edit is composed HERE, fact + slot
            // affordance (`auto_import_span` needs the LSP-side stable
            // outline) — placement is the adapter's, not the model's.
            let mut import_sourced = cs.complete("", !suppress_firehose);
            if !suppress_firehose {
                let insert_at = auto_import_span(analysis, point, stable_packages);
                for c in &mut import_sourced {
                    match &c.import_fact {
                        Some(crate::model::file_analysis::ImportFact::AddToQw { name, qw_close }) => {
                            let at = crate::model::file_analysis::Span { start: *qw_close, end: *qw_close };
                            c.additional_edits.push((at, format!(" {}", name)));
                        }
                        Some(crate::model::file_analysis::ImportFact::NewUse { module, name }) => {
                            c.additional_edits
                                .push((insert_at, format!("use {} qw({});\n", module, name)));
                        }
                        None => {}
                    }
                }
            }
            items.extend(import_sourced);

            items
        }
        // Perl's slot detector never produces these — ArgPosition is
        // `detect_call_slot`'s question (sig-help's), TypePosition has no
        // Perl detector at all.
        Slot::TypePosition { .. } | Slot::ArgPosition { .. } => Vec::new(),
    });

    // Type-constrained ranking: when the cursor sits at a call arg whose
    // callee has a typed param, scope variables whose inferred type matches
    // rank first (`Slot::expected_type` — the seam's Perl consumer). Purely
    // a reorder + priority boost on the gathered candidates; nothing is
    // pruned (a mid-refactor mismatch stays visible).
    if let Some(expected) = crate::lsp::cursor_slot::detect_call_slot(tree, source.as_bytes(), point)
        .and_then(|s| s.slot.expected_type(analysis, point, Some(module_index)))
    {
        rank_candidates_by_expected_type(&mut candidates, &expected, analysis, point);
    }

    let mut items: Vec<CompletionItem> = candidates
        .drain(..)
        .map(candidate_to_completion_item)
        .collect();
    // Dispatch items stay at the top — their sort_text already leads
    // with a space so they group above the priority-numbered rest,
    // but the authoritative ordering is "dispatch first" so they're
    // prepended explicitly.
    if !dispatch_items.is_empty() {
        let mut with_dispatch = dispatch_items;
        with_dispatch.extend(items);
        items = with_dispatch;
    }

    // Ref-type deref snippets when completing after ->
    if let Slot::Member { ref receiver, .. } = slot {
        if let Some(ref ty) = receiver.receiver_type {
            if !ty.is_object() {
                items.extend(ref_type_snippet_completions(ty));
            }
        }
    }

    items
}

/// The `use Module qw(|)` "still indexing" affordance — shown while the
/// named module's export surface (the entity) isn't cached yet. Not an
/// entity candidate, so it's built here rather than via the projection.
fn import_list_loading_placeholder(module_name: &str) -> CompletionItem {
    CompletionItem {
        label: format!("loading {}...", module_name),
        kind: Some(CompletionItemKind::TEXT),
        detail: Some("Module is being indexed".to_string()),
        insert_text: Some(String::new()),
        sort_text: Some("999".to_string()),
        ..Default::default()
    }
}

/// Returns snippet completions for ref-type dereference after `->`.
fn ref_type_snippet_completions(ty: &InferredType) -> Vec<CompletionItem> {
    match ty {
        InferredType::ArrayRef => vec![CompletionItem {
            label: "[index]".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some("array dereference".to_string()),
            insert_text: Some("[$0]".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some("000".to_string()),
            ..Default::default()
        }],
        InferredType::CodeRef { .. } => vec![CompletionItem {
            label: "(args)".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some("code dereference".to_string()),
            insert_text: Some("($0)".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some("000".to_string()),
            ..Default::default()
        }],
        InferredType::HashRef => vec![CompletionItem {
            label: "{key}".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some("hash dereference".to_string()),
            insert_text: Some("{$0}".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some("000".to_string()),
            ..Default::default()
        }],
        _ => Vec::new(),
    }
}

/// Language-agnostic hover for pack languages: the symbol's declaration
/// line in a language-appropriate code fence + its kind. Resolves a
/// cursor on a def directly, or on a call/ref to the local def it names.
/// The Perl `hover_info` renderer is Perl-specific (```perl fences,
/// method-resolution prose); pack languages get this instead.
/// Member completion for a pack language: the members of `class` (the
/// type of the `.`/`->` receiver, resolved by the sentinel) as items. The
/// tree work (sentinel reparse → receiver → type, incl. chains) happens in
/// the backend; this is the tree-free class → members → items half.
/// `op_fix = Some((operator_span, correct_operator))` attaches an
/// `additionalTextEdit` to every item that swaps the typed `.`/`->` for the
/// one the receiver's pointer depth requires (Mode A — accepting `width` on
/// `p.` yields `p->width`). `None` leaves the items untouched (operator
/// already correct, or DEEP receiver shown-only).
pub fn member_completion_for_class(
    analysis: &FileAnalysis,
    class: &str,
    module_index: &dyn crate::model::file_analysis::CrossFileLookup,
    op_fix: Option<(crate::model::file_analysis::Span, String)>,
    point: Point,
) -> Option<Vec<CompletionItem>> {
    // The access-specifier gate needs to know whether the
    // CURSOR itself is lexically inside `class`'s own body — self-access
    // sees non-public members, an external receiver doesn't.
    let requesting_class = analysis
        .scope_at(point)
        .and_then(|sc| analysis.enclosing_class_for_scope(sc));
    let candidates = analysis.complete_members_for_class(
        class, Some(module_index), requesting_class.as_deref(),
    );
    if candidates.is_empty() {
        return None;
    }
    Some(
        candidates
            .into_iter()
            .map(|mut c| {
                if let Some((span, text)) = &op_fix {
                    c.additional_edits.push((*span, text.clone()));
                }
                candidate_to_completion_item(c)
            })
            .collect(),
    )
}

// ---- Import resolution helpers ----

/// Where a completion-accepted auto-import `use` edit lands: the standard
/// insertion position for the package under `point`, clamped to fall at or
/// above the cursor — an edit below the cursor would import after the call
/// being completed.
fn auto_import_span(
    analysis: &FileAnalysis,
    point: Point,
    stable_packages: Option<&[(String, usize)]>,
) -> crate::model::file_analysis::Span {
    let mut insert_pos = find_use_insertion_position(analysis, point, stable_packages);

    // If the computed position is after the cursor, fall back to inserting
    // after the nearest import or package statement ABOVE the cursor.
    if insert_pos.line as usize > point.row {
        // Find the last import above the cursor
        let last_import_above = analysis.imports.iter().rev()
            .find(|imp| imp.span.start.row < point.row);
        if let Some(imp) = last_import_above {
            insert_pos = Position { line: imp.span.end.row as u32 + 1, character: 0 };
        } else {
            // Find the last package statement above the cursor
            let last_pkg_above = analysis.symbols().iter().rev()
                .find(|s| matches!(s.kind, FaSymKind::Package | FaSymKind::Class) && s.selection_span.start.row < point.row);
            if let Some(pkg) = last_pkg_above {
                insert_pos = Position { line: pkg.selection_span.start.row as u32 + 1, character: 0 };
            }
            // else: keep original position (top of file)
        }
    }

    let p = tree_sitter::Point {
        row: insert_pos.line as usize,
        column: insert_pos.character as usize,
    };
    crate::model::file_analysis::Span { start: p, end: p }
}

pub(super) fn format_imported_signature(name: &str, sub_info: &SubInfo<'_>) -> String {
    let params_str = sub_info
        .params()
        .iter()
        .map(|p| p.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut sig = format!("sub {}({})", name, params_str);
    if let Some(rt) = sub_info.return_type(None) {
        sig.push_str(&format!(" → {}", format_inferred_type(&rt)));
    }
    sig
}
