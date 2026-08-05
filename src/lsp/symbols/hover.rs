//! Hover rendering for Perl and pack languages.

use super::*;

/// Hover for pack languages: a presentation of the CandidateSet's hover
/// projection (`docs/adr/resolution-candidate-set.md` — hover presents the
/// top-ranked candidate goto-def would jump to, so the two verbs answer one
/// resolution and can't disagree). Presentation stays here: the member
/// drill-downs (domain headline, storage leaf, template substitution) run
/// first over the same invocant resolution the set's member goto-def lane
/// uses; everything else renders the projection's candidate.
pub fn pack_hover_markdown(
    cs: &crate::index::resolve::CandidateSet,
    language: &str,
) -> Option<String> {
    let analysis = cs.origin_analysis();
    let source = cs.origin_source()?;
    let point = cs.cursor();
    let module_index = cs.scoped_index();
    // Member access (`obj->field` / `obj->method()`): resolve the EXACT member
    // via the invocant class + ancestor walk — the SAME resolution the set's
    // member goto-def lane uses — so a same-file field def (or a same-named
    // symbol on another class) can't hijack it with the wrong scope.
    // A data field shows `field: type` (member_hover, keyed on the field's own
    // scope); a method shows its signature.
    if let Some(r) = analysis.ref_at(point).filter(|r| matches!(r.kind, RefKind::MethodCall { .. })) {
        if let Some(midx) = module_index {
            if let Some(cn) = analysis.method_call_invocant_class(r, Some(midx)) {
                let field = r.unqualified_target_name();
                // The receiver's full VALUE (not just its dispatch class):
                // a template instance's args refine a param-shaped member
                // type (`T get()` on a `Box<int>` receiver → `int`) — shown
                // only when the substitution actually changed the answer,
                // so non-template hovers stay byte-identical.
                let recv_ty = match &r.kind {
                    RefKind::MethodCall { invocant_span: Some(sp), .. } => {
                        analysis.expr_type_at_span(*sp, Some(midx))
                    }
                    _ => None,
                };
                let substituted = |raw: Option<InferredType>| -> Option<InferredType> {
                    let sub = recv_ty
                        .as_ref()
                        .and_then(|t| analysis.member_value_type(t, field, Some(midx), None))?;
                    (raw.as_ref() != Some(&sub)).then_some(sub)
                };
                if let Some(crate::model::file_analysis::MethodResolution::Local { sym_id, .. }) =
                    analysis.resolve_method_in_ancestors(&cn, field, Some(midx))
                {
                    let sym = analysis.symbol(sym_id);
                    if matches!(sym.kind, FaSymKind::Method | FaSymKind::Sub) {
                        let mut text = render_symbol_hover(
                            sym, source, &sym.span.start, language, analysis, sym.span.start, Some(midx),
                        );
                        if let Some(rt) = substituted(
                            analysis.find_method_return_type(&cn, field, Some(midx), None),
                        ) {
                            text.push_str(&format!(
                                "\n\n*returns: {}*",
                                crate::model::file_analysis::format_inferred_type(&rt)
                            ));
                        }
                        return Some(text);
                    }
                }
                // A param-typed member substitutes the same way (`T v_;` on
                // `Box<int>` reads `v_: int`; a cross-file method's return
                // lands here too, so the label stays kind-agnostic).
                if let Some(sub) =
                    substituted(analysis.field_type_on_class(&cn, field, Some(midx)))
                {
                    return Some(format!(
                        "```{}\n{}: {}\n```\n\n*member*",
                        language,
                        field,
                        crate::model::file_analysis::format_inferred_type(&sub)
                    ));
                }
                // The member's declared type may be a config-variant macro whose
                // flow type is the join abstraction (`Numeric`); display the
                // concrete leaf from the config-active variant's alias chain.
                let storage_leaf = analysis
                    .member_type_spelling(&cn, field, Some(midx))
                    .and_then(|sp| config_variant_leaf_display(analysis, &sp, midx));
                // Domain typing: the slot's storage type (`uint16_t`) discards
                // its DOMAIN (`opcode`), recoverable from usage. When the
                // usage-fold recovers one, it headlines with the storage leaf
                // as a drill-down: `op_type: opcode (stored as uint16_t)`. The
                // domain never overrides storage for correctness — a human
                // surface only.
                if let Some(dom) = analysis.field_domain(&cn, field, Some(midx)) {
                    let stored = storage_leaf
                        .clone()
                        .map(|s| format!(" *(stored as `{}`)*", s))
                        .unwrap_or_default();
                    return Some(format!(
                        "```{}\n{}: {}\n```\n\n*field*{}",
                        language, field, dom.domain, stored
                    ));
                }
                if let Some(leaf) = storage_leaf {
                    return Some(format!("```{}\n{}: {}\n```\n\n*field*", language, field, leaf));
                }
                if let Some(h) = analysis.member_hover(&cn, field, Some(midx)) {
                    return Some(format!("```{}\n{}\n```\n\n*field*", language, h));
                }
            }
        }
    }
    // The projection's answer: present the top-ranked definition candidate —
    // what goto-def would jump to — wherever it lives (macro variants,
    // template/spec ladders, locals, cross-file functions all arrive here).
    if let Some(loc) = cs.hover_candidate() {
        if let Some(text) = render_candidate_hover(cs, &loc, language) {
            return Some(text);
        }
    }
    // Cursor on a decl the forward walk didn't self-resolve: render the
    // symbol under the cursor directly (its own type point + scope).
    if let Some(sym) = analysis.symbol_at(point) {
        return Some(render_symbol_hover(
            sym, source, &sym.span.start, language, analysis, point, module_index,
        ));
    }
    None
}

/// Render the hover projection's candidate: the symbol declared at the
/// location — in the origin (fresh text in hand) or a cached pack module
/// (read from disk, suffixed with the defining file's name) — through the
/// same renderer decl-site hovers use. A location no Symbol sits at (a
/// macro def whose Symbol was claimed under another lane, a top-of-file
/// landing) renders its source line, which for a `#define` IS the def.
fn render_candidate_hover(
    cs: &crate::index::resolve::CandidateSet,
    loc: &crate::index::resolve::RefLocation,
    language: &str,
) -> Option<String> {
    let module_index = cs.scoped_index();
    let sym_at = |a: &FileAnalysis| -> Option<usize> {
        a.symbols
            .iter()
            .position(|s| s.selection_span.start == loc.span.start)
            .or_else(|| {
                a.symbols
                    .iter()
                    .position(|s| s.selection_span.start.row == loc.span.start.row
                        && crate::model::file_analysis::contains_point(&s.selection_span, loc.span.start))
            })
    };
    if crate::index::resolve::file_key_eq(&loc.key, cs.origin_file_key()) {
        let analysis = cs.origin_analysis();
        let source = cs.origin_source()?;
        if let Some(i) = sym_at(analysis) {
            let sym = &analysis.symbols[i];
            return Some(render_symbol_hover(
                sym, source, &sym.span.start, language, analysis, cs.cursor(), module_index,
            ));
        }
        let line = source.lines().nth(loc.span.start.row)?.trim();
        return (!line.is_empty()).then(|| format!("```{}\n{}\n```", language, line));
    }
    let path = crate::index::resolve::key_for_sort(&loc.key);
    let text = std::fs::read_to_string(&path).ok()?;
    let fname = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
    // The candidate's own analysis: the scoped index caches every pack file
    // a projection can answer from.
    let mut found: Option<std::sync::Arc<crate::model::file_analysis::CachedModule>> = None;
    if let Some(midx) = module_index {
        midx.for_each_cached_file(&mut |cached| {
            if found.is_none() && cached.path == path {
                found = Some(std::sync::Arc::clone(cached));
            }
        });
    }
    if let Some(cached) = &found {
        let whole = module_index
            .map(|midx| midx.whole_present(cached))
            .unwrap_or_else(|| cached.analysis.clone());
        if let Some(i) = sym_at(&whole) {
            let sym = &whole.symbols[i];
            let mut out = render_symbol_hover(
                sym, &text, &sym.span.start, language, &whole, sym.span.start,
                module_index,
            );
            out.push_str(&format!("\n\n— `{}`", fname));
            return Some(out);
        }
    }
    let line = text.lines().nth(loc.span.start.row)?.trim();
    (!line.is_empty()).then(|| format!("```{}\n{}\n```\n\n— `{}`", language, line, fname))
}

/// Render a symbol's hover. Variables/fields show `name: type` (the inferred
/// type — exact class for objects, generic for primitives) rather than the
/// raw decl line, which for a PARAM is the whole function signature. Other
/// kinds show their declaration line + kind (+ class attribute signals).
/// The hover/label word for `sym` — one mapping shared by every render path
/// below (the typed-variable early return AND the declaration-line
/// fallback), so a kind never gets a different label depending on which
/// branch happened to serve it. A `#define`-backed callable is a real
/// `SymKind::Sub` everywhere else (dispatch/completion/goto-def), but its
/// `"macro"` attribute (stamped at extraction) overrides the label here —
/// the attribute is the value-borne "this Sub is macro-shaped" fact,
/// checked before the kind match rather than re-deriving it from the name.
fn hover_kind_label(sym: &crate::model::file_analysis::Symbol) -> &'static str {
    if sym.attributes.iter().any(|a| a == "macro") {
        return "macro";
    }
    match sym.kind {
        FaSymKind::Sub => "function",
        FaSymKind::Method => "method",
        FaSymKind::Class => "class",
        FaSymKind::Package => "namespace",
        FaSymKind::Variable => "variable",
        FaSymKind::Field => "field",
        FaSymKind::Enumerator => "enumerator",
        _ => "symbol",
    }
}

fn render_symbol_hover(
    sym: &crate::model::file_analysis::Symbol,
    source: &str,
    line_at: &Point,
    language: &str,
    analysis: &FileAnalysis,
    type_point: Point,
    module_index: Option<&dyn crate::model::file_analysis::CrossFileLookup>,
) -> String {
    if matches!(sym.kind, FaSymKind::Variable | FaSymKind::Field | FaSymKind::Enumerator) {
        if let Some(ty) = analysis.inferred_type_via_bag_ctx(&sym.name, type_point, module_index) {
            // Config-variant macro type → display the concrete leaf recovered
            // from the config-active variant's alias chain, not the join
            // abstraction the type flows as.
            let display = module_index
                .and_then(|midx| {
                    analysis
                        .type_name_edge_of(&sym.name, sym.scope)
                        .and_then(|sp| config_variant_leaf_display(analysis, &sp, midx))
                })
                .unwrap_or_else(|| sym.display_type(&ty));
            // A union member's def-site hover carries the storage overlay,
            // same as the member-access path (`FileAnalysis::member_hover`).
            let overlay = match analysis.union_overlay(sym) {
                Some(sibs) if !sibs.is_empty() => {
                    format!(" — union member (overlays {})", sibs.join(", "))
                }
                _ => String::new(),
            };
            return format!(
                "```{}\n{}: {}{}\n```\n\n*{}*",
                language, sym.name, display, overlay, hover_kind_label(sym)
            );
        }
    }
    let line = source.lines().nth(line_at.row).unwrap_or("").trim();
    let sig = line.trim_end_matches([' ', '{', ';']).trim();
    let mut out = format!("```{}\n{}\n```\n\n*{}*", language, sig, hover_kind_label(sym));
    if matches!(sym.kind, FaSymKind::Class) {
        for attr in &sym.attributes {
            out.push_str(&format!("\n\n*{}*", attr));
        }
    }
    out
}

pub fn pack_hover(cs: &crate::index::resolve::CandidateSet, language: &str) -> Option<Hover> {
    let value = pack_hover_markdown(cs, language)?;
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: None,
    })
}

pub fn hover_info(
    analysis: &FileAnalysis,
    source: &str,
    pos: Position,
    module_index: &ModuleIndex,
) -> Option<Hover> {
    let point = position_to_point(pos);

    // Try local hover first
    if let Some(markdown) = analysis.hover_info(point, source, Some(module_index)) {
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: markdown,
            }),
            range: None,
        });
    }

    // Check if cursor is on an imported function call or a Perl
    // builtin. Builtin docs come from `module_index.builtin_doc`,
    // which the resolver thread hydrates from SQLite (parsed from
    // `perlfunc.pod` only on cold-cache miss).
    if let Some(r) = analysis.ref_at(point) {
        if matches!(r.kind, RefKind::FunctionCall { .. }) {
            if crate::model::builtins::is_builtin(&r.target_name) {
                if let Some(markdown) = module_index.builtin_doc(&r.target_name) {
                    return Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: markdown,
                        }),
                        range: None,
                    });
                }
            }
            if let Some((import, _path, remote_name)) =
                resolve_imported_function(analysis, &r.target_name, module_index)
            {
                let mut parts = Vec::new();

                // Show signature if available. Cross-file lookup uses
                // the REMOTE name — for a renaming import (`del` →
                // `delete`), cursor is on `del` but sub_info lives
                // under `delete` in the cached module.
                if let Some(cached) = module_index
                    .defining_module_cached(&import.module_name, &remote_name)
                    .or_else(|| module_index.get_cached(&import.module_name))
                {
                    let whole = module_index.bag_present(&cached);
                    if let Some(sub_info) = whole.sub_info_view(&remote_name) {
                        // Present the sig under the LOCAL name — that's
                        // what the user typed and what hover should lead
                        // with; the remote name is just how we fetched it.
                        let sig = format_imported_signature(&r.target_name, &sub_info);
                        parts.push(format!("```perl\n{}\n```", sig));
                        if let Some(doc) = sub_info.doc() {
                            parts.push(doc.to_string());
                        }
                    }
                }

                if remote_name != r.target_name {
                    parts.push(format!(
                        "*imported from `{}` (as `{}`)*",
                        import.module_name, remote_name
                    ));
                } else {
                    parts.push(format!("*imported from `{}`*", import.module_name));
                }
                let markdown = parts.join("\n\n");
                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: None,
                });
            }

            // Fully-qualified call with no import: resolve the sub in the
            // package named by the qualifier (the `Function` binding).
            if let (RefKind::FunctionCall, Some(pkg)) = (&r.kind, r.resolved_package()) {
                let bare = r.unqualified_target_name();
                if let Some(cached) = module_index.get_cached(pkg) {
                    let whole = module_index.bag_present(&cached);
                    if let Some(sub_info) = whole.sub_info_view(bare) {
                        let sig = format_imported_signature(bare, &sub_info);
                        let mut parts = vec![format!("```perl\n{}\n```", sig)];
                        if let Some(doc) = sub_info.doc() {
                            parts.push(doc.to_string());
                        }
                        parts.push(format!("*from `{}`*", pkg));
                        return Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: parts.join("\n\n"),
                            }),
                            range: None,
                        });
                    }
                }
            }
        }
    }

    None
}
