//! Import-binding classification (`ImportResolution`, `classify_import`)
//! and the import/export candidate gatherers the completion projection and
//! goto-def share, so they can never disagree on importability.
use super::*;

/// Candidates for names a `use` statement makes (or could make) available:
/// explicitly imported symbols, then the imported modules' remaining
/// `@EXPORT`/`@EXPORT_OK` surfaces as auto-add-to-qw candidates. The `seen`
/// set is marked unconditionally so a tier-masked explicit import can never
/// be re-offered by the export walk under the wrong affordance.
pub(super) fn import_candidates(
    origin: &FileAnalysis,
    idx: &dyn CrossFileLookup,
    mask: RoleMask,
    out: &mut Vec<CompletionCandidate>,
) {
    use crate::model::file_analysis::{
        format_inferred_type, SymKind as FaSymKind, PRIORITY_AUTO_ADD_QW, PRIORITY_BARE_IMPORT,
        PRIORITY_EXPLICIT_IMPORT,
    };
    let mut seen = std::collections::HashSet::new();

    for import in &origin.imports {
        let cached = idx.get_cached(&import.module_name);

        // Explicitly imported symbols (from the qw list): origin-file names.
        // Dedup/dispatch by LOCAL name (what the user types); resolve detail
        // against REMOTE name (what exists in the source module) so renaming
        // imports like `del` → `delete` show the real doc.
        for is in &import.imported_symbols {
            let local = &is.local_name;
            if !seen.insert(local.clone()) {
                continue;
            }
            if !origin.symbols_named(local).is_empty() {
                continue;
            }
            if !mask.contains(RoleMask::OPEN) {
                continue;
            }
            let whole = cached.as_ref().map(|c| idx.bag_present(c));
            let detail =
                completion_detail_for_import(is.remote(), whole.as_deref(), &import.module_name);
            out.push(CompletionCandidate {
                label: local.clone(),
                kind: FaSymKind::Sub,
                detail: Some(detail),
                insert_text: None,
                sort_priority: PRIORITY_EXPLICIT_IMPORT,
                additional_edits: vec![],
                import_fact: None,
                display_override: None,
            });
        }

        // The module's remaining export surface: dependency-file names.
        if !mask.contains(RoleMask::DEPENDENCY) {
            continue;
        }
        if let Some(ref cached) = cached {
            let fa = &cached.analysis;
            let all_exported: Vec<&String> = if import.imported_symbols.is_empty() {
                // Bare `use Foo;` — offer @EXPORT
                fa.export.iter().collect()
            } else {
                // `use Foo qw(bar)` — offer remaining @EXPORT + @EXPORT_OK
                let mut all = Vec::new();
                all.extend(fa.export.iter());
                all.extend(fa.export_ok.iter());
                all
            };

            for name in all_exported {
                // Skip already-offered (explicitly imported) and locally defined
                if !seen.insert(name.clone()) {
                    continue;
                }
                if !origin.symbols_named(name).is_empty() {
                    continue;
                }

                let rt_prefix = idx
                    .whole_present(cached)
                    .sub_info_view(name)
                    .and_then(|s| s.return_type(None))
                    .map(|rt| format!("→ {} ", format_inferred_type(&rt)))
                    .unwrap_or_default();

                // The FACT: this name can join the existing qw() list at
                // its close paren. The adapter composes the edit; a bare
                // `use Foo;` has no list to join (no fact, no edit).
                let (detail, priority, import_fact) =
                    if let Some(close_pos) = import.qw_close_paren {
                        (
                            format!("{}{} (auto-import)", rt_prefix, import.module_name),
                            PRIORITY_AUTO_ADD_QW,
                            Some(crate::model::file_analysis::ImportFact::AddToQw {
                                name: name.clone(),
                                qw_close: close_pos,
                            }),
                        )
                    } else {
                        (
                            format!("{}imported from {}", rt_prefix, import.module_name),
                            PRIORITY_BARE_IMPORT,
                            None,
                        )
                    };

                out.push(CompletionCandidate {
                    label: name.clone(),
                    kind: FaSymKind::Sub,
                    detail: Some(detail),
                    insert_text: None,
                    sort_priority: priority,
                    additional_edits: vec![],
                    import_fact,
                    display_override: None,
                });
            }
        }
    }
}

/// Auto-import candidates: every cached exporter's `@EXPORT`/`@EXPORT_OK`
/// surface, each carrying the importable-from FACT (`ImportFact::NewUse`);
/// the adapter composes the `use Module qw(func);` edit at the slot's
/// affordance.
pub(super) fn unimported_export_candidates(
    origin: &FileAnalysis,
    idx: &dyn CrossFileLookup,
    out: &mut Vec<CompletionCandidate>,
) {
    use crate::model::file_analysis::{SymKind as FaSymKind, PRIORITY_UNIMPORTED};
    let mut candidates = Vec::new();

    // Already-imported modules are the import walk's job, not this one's.
    let imported_modules: std::collections::HashSet<&str> = origin
        .imports
        .iter()
        .map(|i| i.module_name.as_str())
        .collect();

    idx.for_each_cached(&mut |module_name, cached| {
        if imported_modules.contains(module_name) {
            return;
        }

        let fa = &cached.analysis;
        let all_exported = fa.export.iter().chain(fa.export_ok.iter());
        for name in all_exported {
            // Skip functions already defined locally
            if !origin.symbols_named(name).is_empty() {
                continue;
            }
            candidates.push(CompletionCandidate {
                label: name.clone(),
                kind: FaSymKind::Sub,
                detail: Some(format!("{} (auto-import)", module_name)),
                insert_text: None,
                sort_priority: PRIORITY_UNIMPORTED,
                additional_edits: vec![],
                import_fact: Some(crate::model::file_analysis::ImportFact::NewUse {
                    module: module_name.to_string(),
                    name: name.clone(),
                }),
                display_override: None,
            });
        }
    });

    // Sort for deterministic order
    candidates.sort_by(|a, b| a.label.cmp(&b.label).then(a.detail.cmp(&b.detail)));
    out.extend(candidates);
}

pub(super) fn completion_detail_for_import(
    name: &str,
    // The bag-present analysis (`idx.bag_present`) — return types read the
    // bag, and the resident index copy may be evicted.
    whole: Option<&crate::model::file_analysis::FileAnalysis>,
    module_name: &str,
) -> String {
    use crate::model::file_analysis::format_inferred_type;
    if let Some(whole) = whole {
        if let Some(sub_info) = whole.sub_info_view(name) {
            if let Some(rt) = sub_info.return_type(None) {
                return format!("→ {} ({})", format_inferred_type(&rt), module_name);
            }
        }
    }
    format!("imported from {}", module_name)
}

/// All `Handler` definitions matching `(owner, name)` across cached modules.
/// A dispatch (`$emitter->emit('ready')`) can target stacked registrations
/// in different files; every hit surfaces so the editor can show a picker.
/// Shared by the materialized-ref path and the query-time `dispatch_at` path
/// so both resolve handlers identically.
pub(super) fn dispatch_handler_locations(
    owner: &HandlerOwner,
    name: &str,
    module_index: &dyn CrossFileLookup,
) -> Vec<RefLocation> {
    use crate::model::file_analysis::SymbolDetail;
    let mut locs: Vec<RefLocation> = Vec::new();
    for module_name in module_index.modules_with_symbol(name) {
        let Some(cached) = module_index.get_cached(&module_name) else { continue };
        let whole = module_index.whole_present(&cached);
        for sym in &whole.symbols {
            if sym.name != name {
                continue;
            }
            if let SymbolDetail::Handler { owner: o, .. } = &sym.detail {
                if o == owner {
                    locs.push(RefLocation {
                        key: FileKey::Path(cached.path.clone()),
                        span: sym.selection_span,
                        access: AccessKind::Declaration,
                        rewritable: true,
                        label: None
                    });
                }
            }
        }
    }
    locs
}

/// How a function name relates to an importing `use` statement. Both
/// goto-def and the unresolved-function diagnostic read this one verdict so
/// they can never disagree on whether a name is resolvable as imported
/// (NAV § (c): the divergent-export-surface root cause).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImportResolution {
    /// The name is brought into the caller's namespace: named in `qw(...)`,
    /// pulled in by a `:tag` selector against the producer surface, or
    /// auto-imported by a bare `use Foo;`. Goto-def jumps; the diagnostic
    /// stays silent (the name is genuinely available here).
    Brought,
    /// The name is exported by the imported module but this `use` didn't
    /// bring it in (e.g. a named `qw(other)` that omits it). Goto-def can
    /// still jump to the def; the diagnostic offers the "exported but not
    /// imported" hint.
    ExportedNotBrought,
}

/// Classify a name against a single import. Routes through the consumer
/// evaluator (`imported_names`) so the verdict is exactly "is this name in the
/// bound set this `use` produces" — the single notion of import binding that
/// diagnostics, goto-def, and references all read (NAV § (c)). Returns the
/// resolved verdict plus the REMOTE (origin) name for the matched local name.
///
/// `cached` is the producer's `FileAnalysis` when known; its `export_surface`
/// expands `:tag` selectors and supplies the `@EXPORT` defaults for a bare
/// `use`. When absent (module not yet cached), the evaluator still binds
/// explicitly-named `qw()` imports — those don't need the surface — so an
/// explicit named import is never spuriously flagged while the resolver warms.
fn classify_import(
    import: &crate::model::file_analysis::Import,
    func_name: &str,
    cached: Option<&crate::model::file_analysis::CachedModule>,
    module_index: &dyn CrossFileLookup,
) -> Option<(ImportResolution, String)> {
    if let Some(cached) = cached {
        let surface = cached.analysis.export_surface_with_index(module_index);
        let bound = crate::model::file_analysis::imported_names(import, &surface);
        if let Some((_local, remote)) = bound.iter().find(|(local, _)| local == func_name) {
            return Some((ImportResolution::Brought, remote.clone()));
        }
        // Not bound by this `use`, but on the producer surface → the actionable
        // "exported but not imported" hint (a named `qw(other)` omitting it, or
        // an `@EXPORT_OK` name reached only by a bare `use` — GATE-5).
        if surface.exports(func_name) {
            return Some((ImportResolution::ExportedNotBrought, func_name.to_string()));
        }
        return None;
    }
    // Module not cached yet: only an explicitly-named import can be judged
    // `Brought` without the producer surface (tags / bare-use defaults need it).
    // This keeps a `qw(foo)` import from being flagged while the resolver warms,
    // and never resolves a bare/tagged name it can't actually verify.
    if let Some(sym) = import.imported_symbols.iter().find(|s| s.local_name == *func_name) {
        return Some((ImportResolution::Brought, sym.remote().to_string()));
    }
    None
}

/// Best resolution of `func_name` across all imports: the matched import, its
/// remote name, the resolvability verdict, and — when known — the module path
/// for navigation. `Brought` wins over `ExportedNotBrought` when several
/// imports relate. The single resolvability query goto-def, the diagnostic, and
/// references all read, so they can never disagree on the bound set.
pub(crate) fn resolve_imported_function_classified<'b>(
    analysis: &'b FileAnalysis,
    func_name: &str,
    module_index: &dyn CrossFileLookup,
) -> Option<(&'b crate::model::file_analysis::Import, Option<PathBuf>, String, ImportResolution)> {
    let mut best: Option<(
        &'b crate::model::file_analysis::Import,
        Option<PathBuf>,
        String,
        ImportResolution,
    )> = None;
    for import in &analysis.imports {
        let cached = module_index.get_cached(&import.module_name);
        let Some((res, remote)) = classify_import(import, func_name, cached.as_deref(), module_index) else { continue };
        let path = cached.as_ref().map(|c| c.path.clone());
        // `Brought` is the strongest verdict; once found, keep it.
        if matches!(best, Some((_, _, _, ImportResolution::Brought))) {
            continue;
        }
        best = Some((import, path, remote, res));
    }
    best
}

/// Find which import provides a given function name, with a concrete module
/// path to jump to. Returns the matched Import, the module's path, and the
/// REMOTE name (the sub's actual name in the source module — differs from the
/// caller's `func_name` only for renaming imports like `del` → `delete`).
/// Callers use the remote name for whole-view `sub_info_view(...)` lookups so
/// hover/gd/sig-help reach the real sub.
pub(crate) fn resolve_imported_function<'b>(
    analysis: &'b FileAnalysis,
    func_name: &str,
    module_index: &dyn CrossFileLookup,
) -> Option<(&'b crate::model::file_analysis::Import, PathBuf, String)> {
    // Goto-def needs a concrete module path to jump to.
    resolve_imported_function_classified(analysis, func_name, module_index)
        .and_then(|(import, path, remote, _)| path.map(|p| (import, p, remote)))
}
