//! The cursor/query CLI commands and their single source of truth,
//! `run_one` — shared verbatim by the single-mode wrappers and `--batch`.

use super::*;

// ---- CLI Commands ----

/// --check [<root>] [--format json|human] [--severity error|warning|info|hint] — Batch diagnostics
pub(crate) fn cli_check(args: &[String]) {
    let root = args.iter()
        .find(|a| !a.starts_with("--") && !["json", "human", "error", "warning", "info", "hint"].contains(&a.as_str()))
        .map(|s| s.as_str())
        .unwrap_or(".");
    let json_mode = is_json_format(args);
    let min_severity = get_arg_value(args, "--severity").unwrap_or("warning");
    let min_rank = severity_rank(min_severity);
    // Opt-in QA channel, mirrors the LSP `initializationOptions` toggle.
    let options = symbols::DiagnosticOptions::from_cli_args(args);

    if args.iter().any(|a| a == "--timings") {
        timings::enable();
    }
    timings::enable_from_env();

    let (ws, module_index) = cli_full_startup(root);

    // Dump the per-module breakdown before diagnostics output so the table
    // isn't buried under (and the early `exit(1)` below doesn't swallow it).
    timings::report();
    timings::report_pattern_stats();

    let mut all_diagnostics = Vec::new();

    for (file, d) in enriched_tree_diagnostics(&ws, &module_index, options) {
        {
            let sev = match d.severity {
                Some(s) if s == tower_lsp::lsp_types::DiagnosticSeverity::ERROR => "error",
                Some(s) if s == tower_lsp::lsp_types::DiagnosticSeverity::WARNING => "warning",
                Some(s) if s == tower_lsp::lsp_types::DiagnosticSeverity::INFORMATION => "info",
                Some(s) if s == tower_lsp::lsp_types::DiagnosticSeverity::HINT => "hint",
                _ => "warning",
            };
            // Filter by minimum severity
            if severity_rank(sev) > min_rank {
                continue;
            }
            all_diagnostics.push(serde_json::json!({
                "file": file,
                "line": d.range.start.line,
                "col": d.range.start.character,
                "severity": sev,
                "code": d.code.map(|c| match c {
                    tower_lsp::lsp_types::NumberOrString::String(s) => s,
                    tower_lsp::lsp_types::NumberOrString::Number(n) => n.to_string(),
                }).unwrap_or_default(),
                "message": d.message,
            }));
        }
    }

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&all_diagnostics).unwrap());
    } else {
        for d in &all_diagnostics {
            let severity = d["severity"].as_str().unwrap_or("warning");
            let file = d["file"].as_str().unwrap_or("");
            let line = d["line"].as_u64().unwrap_or(0) + 1;
            let col = d["col"].as_u64().unwrap_or(0) + 1;
            let code = d["code"].as_str().unwrap_or("");
            let msg = d["message"].as_str().unwrap_or("");
            eprintln!("{}:{}:{}: {}[{}] {}", file, line, col, severity, code, msg);
        }
        let total = all_diagnostics.len();
        let files = ws.workspace_len();
        eprintln!("{} diagnostics in {} files", total, files);
    }

    if !all_diagnostics.is_empty() {
        std::process::exit(1);
    }
}

/// --outline <file> — Document symbol outline
pub(crate) fn cli_outline(file: &str) {
    let (_source, _tree, analysis) = parse_file(file);
    println!("{}", outline_json(&analysis));
}

/// --hover <file> <line> <col> — single-file type info and docs (no index).
/// The cross-file form (`--hover <root> ...`) routes through `cli_cursor` so it
/// can never drift from `--batch`; this no-root form has no index, so it keeps
/// its own path.
pub(crate) fn cli_hover_single_file(file: &str, line_str: &str, col_str: &str) {
    let point = parse_point(line_str, col_str);
    let (source, _tree, analysis) = parse_file(file);
    // Pack languages get the language-agnostic renderer (matches the LSP); no
    // index here, so cross-file function hover is unavailable in this form (use
    // the root form for that). Perl keeps hover_info.
    let reg = language_driver::LanguageRegistry::with_enabled();
    let pack_lang = reg
        .for_path_sniffed(std::path::Path::new(file), &source)
        .map(|d| d.id())
        .filter(|id| *id != "perl");
    let markdown = match pack_lang {
        Some(lang) => {
            let files = file_store::FileStore::new();
            let cs = resolve::resolve(
                &files,
                &analysis,
                file_store::FileKey::Path(std::path::PathBuf::from(file)),
                point,
                None,
                resolve::OverrideScope::default(),
            )
            .with_source(&source);
            symbols::pack_hover_markdown(&cs, lang)
        }
        None => analysis.hover_info(point, &source, None),
    };
    if let Some(markdown) = markdown {
        println!("{}", markdown);
    } else {
        eprintln!("No hover info at {}:{}", line_str, col_str);
        std::process::exit(1);
    }
}

/// --type-at <file> <line> <col> — Single type query
pub(crate) fn cli_type_at(file: &str, line_str: &str, col_str: &str) {
    let (_source, _tree, analysis) = parse_file(file);
    let point = parse_point(line_str, col_str);

    // Check refs for inferred type — route through the witness bag
    // so framework / branch / arity rules refine the answer.
    if let Some(r) = analysis.ref_at(point) {
        if let Some(ty) = analysis.inferred_type_via_bag(&r.target_name, point) {
            println!("{}", file_analysis::format_inferred_type(&ty));
            return;
        }
    }
    // Check symbols
    if let Some(sym) = analysis.symbol_at(point) {
        if let Some(ty) = analysis.inferred_type_via_bag(&sym.name, point) {
            println!("{}", file_analysis::format_inferred_type(&ty));
            return;
        }
    }
    eprintln!("No type info at {}:{}", line_str, col_str);
    std::process::exit(1);
}

/// Run `run_one` and reproduce the single-mode contract: `Ok` to stdout,
/// `Err` to stderr with exit-1 (preserves the "miss → exit 1" behavior every
/// single-mode command had). `fmt` selects the output coordinate dialect.
fn print_run_one(
    ws: &file_store::FileStore,
    idx: &module_index::ModuleIndex,
    req: &BatchReq,
    fmt: CoordFmt,
) {
    match run_one(ws, idx, req, fmt) {
        Ok(s) => println!("{}", s),
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

/// Every uniform cursor query (`--definition` / `--references` /
/// `--implementations` / `--completion` / `--signature-help` /
/// `--document-highlight` / `--linked-editing` / cross-file `--hover`). `q` is
/// the `run_one` query string; `rest` is the cursor args after `<root>` (either
/// positional `<file> <line> <col>` or `--at <file>:<line>:<col>`). The input
/// dialect selects the output `CoordFmt`, and a `[pos]` annotation goes to
/// stderr so the 0-vs-1-based interpretation is never silent.
pub(crate) fn cli_cursor(q: &str, root: &str, rest: &[String]) {
    let target = parse_cursor_target(rest, root).unwrap_or_else(|| {
        eprintln!(
            "perl-lsp --{q}: expected `<root> <file> <line> <col>` or `<root> --at <file>:<line>:<col>`"
        );
        std::process::exit(2);
    });
    emit_pos_annotation(&target);
    let (ws, idx) = cli_full_startup(root);
    let req = BatchReq {
        id: String::new(),
        q: q.to_string(),
        file: target.file.clone(),
        line: target.point.row,
        col: target.point.column,
        query: None,
        newname: None,
    };
    print_run_one(&ws, &idx, &req, target.fmt);
}

/// --semantic-tokens <root> <file> — token classification for the file.
pub(crate) fn cli_semantic_tokens(root: &str, file: &str) {
    let (ws, idx) = cli_full_startup(root);
    let req = BatchReq {
        id: String::new(),
        q: "semantic-tokens".into(),
        file: file.to_string(),
        line: 0,
        col: 0,
        query: None,
        newname: None,
    };
    // semantic-tokens output is independent of the coordinate seam; the batch
    // dialect keeps it unchanged.
    print_run_one(&ws, &idx, &req, CoordFmt::EditorOneBasedChar);
}

/// Build an open `Document` (tree + analysis + stable_outline) and enrich it
/// against the index exactly like the server's open-file path. Shared by the
/// interactive CLI modes.
fn cli_open_document(file: &str, idx: &module_index::ModuleIndex) -> document::Document {
    let text = std::fs::read_to_string(file).unwrap_or_else(|e| {
        eprintln!("Cannot read {}: {}", file, e);
        std::process::exit(1);
    });
    // Route a pack language (cpp, ...) through its driver so the CLI
    // cursor handlers (definition/references/highlight/…) match the LSP
    // server. Perl + truly-unrecognized files keep Document::new + enrichment.
    let reg = language_driver::LanguageRegistry::with_enabled();
    let pack = reg
        .for_path_sniffed(std::path::Path::new(file), &text)
        .filter(|d| d.id() != "perl");
    if let Some(driver) = pack {
        return tphase!("Document::new_routed", document::Document::new_routed(text, driver, Some(std::path::PathBuf::from(file))).unwrap_or_else(|| {
            eprintln!("Parse failed: {}", file);
            std::process::exit(1);
        }));
    }
    let mut doc = tphase!("Document::new (parse+build)", document::Document::new(text).unwrap_or_else(|| {
        eprintln!("Parse failed: {}", file);
        std::process::exit(1);
    }));
    tphase!("enrich_imported_types", std::sync::Arc::make_mut(&mut doc.analysis).enrich_imported_types_with_keys(Some(idx)));
    doc
}

#[derive(serde::Deserialize)]
struct BatchReq {
    id: String,
    q: String,
    #[serde(default)] file: String,
    #[serde(default)] line: usize,
    #[serde(default)] col: usize,
    #[serde(default)] query: Option<String>,
    #[serde(default)] newname: Option<String>,
}

/// Single source of truth for every cursor/query CLI capability. Both the
/// single-mode `cli_*` wrappers and `--batch` call this, so their stdout can
/// never drift. The expensive startup state (`ws` + `idx`) is built once by the
/// caller and shared; this re-parses only the one target file per call. Returns
/// Stage an analysis in the workspace store for the lifetime of one cross-file
/// query (references/rename need `refs_to` to see the freshly-enriched target),
/// restoring the prior entry on drop. `--batch` shares one FileStore across all
/// requests; without this restore a references/rename request would leave its
/// enrichment-synthesized symbols in the store, so a later workspace-symbol or
/// diagnostics request in the same batch would see different results depending
/// on ordering.
struct ScopedWorkspaceEntry<'a> {
    ws: &'a file_store::FileStore,
    path: std::path::PathBuf,
    prior: Option<std::sync::Arc<file_analysis::FileAnalysis>>,
}

impl<'a> ScopedWorkspaceEntry<'a> {
    fn insert(
        ws: &'a file_store::FileStore,
        path: std::path::PathBuf,
        analysis: file_analysis::FileAnalysis,
    ) -> Self {
        let prior = ws.workspace_raw().get(&path).map(|r| r.value().clone());
        ws.insert_workspace(path.clone(), analysis);
        Self { ws, path, prior }
    }

    fn insert_arc(
        ws: &'a file_store::FileStore,
        path: std::path::PathBuf,
        analysis: std::sync::Arc<file_analysis::FileAnalysis>,
    ) -> Self {
        let prior = ws.workspace_raw().get(&path).map(|r| r.value().clone());
        ws.insert_workspace_arc(path.clone(), analysis);
        Self { ws, path, prior }
    }
}

impl Drop for ScopedWorkspaceEntry<'_> {
    fn drop(&mut self) {
        match self.prior.take() {
            Some(arc) => self.ws.insert_workspace_arc(self.path.clone(), arc),
            None => self.ws.remove_workspace(&self.path),
        }
    }
}

/// Resolve the query file's direct imports before answering. The LSP
/// backend does this on didOpen (request_resolve per import); a CLI
/// query is one-shot, so it requests AND waits, bounded per module.
/// Without it, modules nothing in the workspace literally `use`s —
/// framework-implied SyntheticUses like Mojolicious::Plugin::
/// DefaultHelpers — resolve in the editor but stay invisible to CLI
/// probes and the gold harness.
fn resolve_imports_blocking(
    idx: &module_index::ModuleIndex,
    analysis: &file_analysis::FileAnalysis,
) {
    for imp in &analysis.imports {
        idx.request_resolve(&imp.module_name);
    }
    // Global budget, not per-import: a cold cache resolving a
    // framework's whole dependency tree must not stall the CLI for
    // minutes. Each resolved module persists to the SQLite cache, so
    // repeated runs converge to fully warm; within one run we answer
    // with whatever resolved in time (the editor is eventually-
    // consistent here too — it refreshes when resolution lands).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    for imp in &analysis.imports {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            break;
        }
        let _ = idx.wait_resolved(&imp.module_name, left);
    }
}

fn run_one(
    ws: &file_store::FileStore,
    idx: &module_index::ModuleIndex,
    req: &BatchReq,
    fmt: CoordFmt,
) -> Result<String, String> {
    use tower_lsp::lsp_types::Position;
    let file = req.file.as_str();
    let point = tree_sitter::Point { row: req.line, column: req.col };
    let pos = Position { line: req.line as u32, character: req.col as u32 };

    // Graceful miss instead of process exit — a bad path must not kill --batch.
    let needs_file = !matches!(req.q.as_str(), "workspace-symbol" | "diagnostics");
    if needs_file && (file.is_empty() || !std::path::Path::new(file).exists()) {
        return Err(format!("file not found: {}", file));
    }

    match req.q.as_str() {
        "definition" => {
            let (source, _tree, mut analysis) = parse_file(file);
            resolve_imports_blocking(idx, &analysis);
            analysis.enrich_imported_types_with_keys(Some(idx));
            let abs = std::fs::canonicalize(file).unwrap_or_else(|_| std::path::PathBuf::from(file));
            let uri = tower_lsp::lsp_types::Url::from_file_path(&abs)
                .unwrap_or_else(|_| tower_lsp::lsp_types::Url::parse("file:///unknown").unwrap());
            // Pack languages resolve cross-file through their sub-index
            // (matches the LSP server); Perl uses the hub. `lookup_for` is
            // the one speller of that store selection.
            let reg = language_driver::LanguageRegistry::with_enabled();
            let lang_id = reg.for_path_sniffed(std::path::Path::new(file), &source)
                .map(|d| d.id()).filter(|id| *id != "perl");
            let routed = idx.lookup_for(lang_id.unwrap_or("perl"));
            let base_idx = routed.as_lookup();
            // `#include "x.h"` path → the resolved header (`#include` = `use`).
            // A path token, not a name — slot-shaped, stays ahead of the set.
            // The pack declares whether it has include tokens; asked, never
            // named (rule #10) — the same gate the LSP handler asks.
            if language_driver::LanguageRegistry::has_include_tokens(lang_id.unwrap_or("perl")) {
                if let Some(loc) = symbols::pack_include_definition(&analysis, point, Some(abs.as_path())) {
                    let path = loc.uri.to_file_path().map(|p| p.display().to_string())
                        .unwrap_or_else(|_| loc.uri.to_string());
                    let (line, col) = fmt.render_pos(
                        loc.range.start.line as usize, loc.range.start.character as usize);
                    return Ok(format!("{}:{}:{}", path, line, col));
                }
            }
            let _ = &uri;
            // Forward projection of the set (mirrors the LSP handler): the
            // source text unlocks the macro variant lane for pack routing.
            // Print EVERY offered location (one per line), ranked as
            // returned — macro variants config-active first (labels shown),
            // a domain-typed field decl FIRST then its domain enum def.
            let _staged = ScopedWorkspaceEntry::insert(ws, abs.clone(), analysis);
            let origin = ws.workspace_raw().get(&abs).map(|r| r.value().clone())
                .expect("origin staged above");
            let cs = resolve::resolve(
                ws, &origin, file_store::FileKey::Path(abs), point,
                Some(base_idx), resolve::OverrideScope::default(),
            )
            .with_source(&source);
            let locs = cs.definitions();
            if !locs.is_empty() {
                let mut sources = SourceCache::new(fmt);
                let mut lines = Vec::new();
                for loc in locs {
                    let path = match &loc.key {
                        file_store::FileKey::Path(p) => p.display().to_string(),
                        file_store::FileKey::Url(u) => u.to_file_path()
                            .map(|p| p.display().to_string()).unwrap_or_else(|_| u.to_string()),
                    };
                    let (line, col) = sources.display(
                        &path, loc.span.start.row, loc.span.start.column);
                    let label = loc.label.map(|l| format!("  ({l})")).unwrap_or_default();
                    lines.push(format!("{}:{}:{}{}", path, line, col, label));
                }
                return Ok(lines.join("\n"));
            }
            Err(format!("No definition found at {}:{}", req.line, req.col))
        }
        "references" => {
            let (s, _t, mut analysis) = parse_file(file);
            resolve_imports_blocking(idx, &analysis);
            analysis.enrich_imported_types_with_keys(Some(idx));
            let file_path = std::path::Path::new(file).canonicalize()
                .unwrap_or_else(|_| std::path::PathBuf::from(file));
            let mut sources = SourceCache::new(fmt);
            let mut results = Vec::new();
            // Pack languages route through their sub-index (matches goto-def
            // and the LSP server) — the hub only knows Perl modules, so
            // resolving/collecting against it silently misses every
            // cross-file cpp use. Perl keeps the hub (empty closure = no-op).
            let reg = language_driver::LanguageRegistry::with_enabled();
            let lang_id = reg.for_path_sniffed(std::path::Path::new(file), &s)
                .map(|d| d.id()).filter(|id| *id != "perl");
            let routed = idx.lookup_for(lang_id.unwrap_or("perl"));
            let base_idx = routed.as_lookup();
            // `#include` reverse — "who includes this header" — owns the path
            // token exclusively (its backward mirror of include goto-def).
            // The pack declares whether it has include tokens; asked, never
            // named (rule #10) — the same gate the LSP handler asks.
            if language_driver::LanguageRegistry::has_include_tokens(lang_id.unwrap_or("perl")) {
                if let Some(incs) = symbols::pack_include_references(
                    &analysis, point, Some(file_path.as_path()), base_idx)
                {
                    for (path, span) in incs {
                        let ps = path.display().to_string();
                        let (line, col) = sources.display(&ps, span.start.row, span.start.column);
                        results.push(serde_json::json!({"file": ps, "line": line, "col": col}));
                    }
                    return Ok(serde_json::to_string_pretty(&results).unwrap());
                }
            }
            // Stage the enriched origin, then construct the set from the
            // staged snapshot — the same one-construction/one-projection
            // shape as the LSP handler, so CLI and editor answers can't
            // diverge. Pack routing + the origin's closure scope are
            // construction facts on the set.
            let _staged = ScopedWorkspaceEntry::insert(ws, file_path.clone(), analysis);
            let origin = ws.workspace_raw().get(&file_path).map(|r| r.value().clone())
                .expect("origin staged above");
            let cs = resolve::resolve(
                ws, &origin, file_store::FileKey::Path(file_path), point,
                Some(base_idx), override_scope_from_env(),
            );
            for loc in cs.references() {
                let path = match &loc.key {
                    file_store::FileKey::Path(p) => p.display().to_string(),
                    file_store::FileKey::Url(u) => u.to_file_path()
                        .map(|p| p.display().to_string()).unwrap_or_else(|_| u.to_string()),
                };
                let (line, col) = sources.display(&path, loc.span.start.row, loc.span.start.column);
                results.push(serde_json::json!({"file": path, "line": line, "col": col}));
            }
            Ok(serde_json::to_string_pretty(&results).unwrap())
        }
        "implementations" => {
            let (s, _t, mut analysis) = parse_file(file);
            resolve_imports_blocking(idx, &analysis);
            analysis.enrich_imported_types_with_keys(Some(idx));
            let file_path = std::path::Path::new(file).canonicalize()
                .unwrap_or_else(|_| std::path::PathBuf::from(file));
            let mut sources = SourceCache::new(fmt);
            let mut results = Vec::new();
            // Same store routing as the LSP handler, so the CLI mirror can't
            // diverge: the domain bridge (enum def → field-slot sites) and
            // the family/spec walks are one projection.
            let reg = language_driver::LanguageRegistry::with_enabled();
            let lang_id = reg
                .for_path_sniffed(std::path::Path::new(file), &s)
                .map(|d| d.id())
                .filter(|id| *id != "perl");
            let routed = idx.lookup_for(lang_id.unwrap_or("perl"));
            let _staged = ScopedWorkspaceEntry::insert(ws, file_path.clone(), analysis);
            let origin = ws.workspace_raw().get(&file_path).map(|r| r.value().clone())
                .expect("origin staged above");
            let cs = resolve::resolve(
                ws, &origin, file_store::FileKey::Path(file_path), point,
                Some(routed.as_lookup()), resolve::OverrideScope::default(),
            );
            for loc in cs.implementations() {
                let path = match &loc.key {
                    file_store::FileKey::Path(p) => p.display().to_string(),
                    file_store::FileKey::Url(u) => u.to_file_path()
                        .map(|p| p.display().to_string()).unwrap_or_else(|_| u.to_string()),
                };
                let (line, col) = sources.display(&path, loc.span.start.row, loc.span.start.column);
                results.push(serde_json::json!({"file": path, "line": line, "col": col}));
            }
            Ok(serde_json::to_string_pretty(&results).unwrap())
        }
        "hover" => {
            let (source, _t, mut analysis) = parse_file(file);
            // Pack languages present the CandidateSet's hover projection
            // (matches the LSP server — the same construction goto-def uses);
            // Perl keeps its rich renderer.
            let reg = language_driver::LanguageRegistry::with_enabled();
            if let Some(lang) = reg.for_path_sniffed(std::path::Path::new(file), &source)
                .map(|d| d.id()).filter(|id| *id != "perl")
            {
                let routed = idx.lookup_for(lang);
                let abs = std::fs::canonicalize(file)
                    .unwrap_or_else(|_| std::path::PathBuf::from(file));
                let _staged = ScopedWorkspaceEntry::insert(ws, abs.clone(), analysis);
                let origin = ws.workspace_raw().get(&abs).map(|r| r.value().clone())
                    .expect("origin staged above");
                let cs = resolve::resolve(
                    ws, &origin, file_store::FileKey::Path(abs), point,
                    Some(routed.as_lookup()), resolve::OverrideScope::default(),
                )
                .with_source(&source);
                return symbols::pack_hover_markdown(&cs, lang)
                    .ok_or_else(|| format!("No hover info at {}:{}", req.line, req.col));
            }
            resolve_imports_blocking(idx, &analysis);
            analysis.enrich_imported_types_with_keys(Some(idx));
            analysis.hover_info(point, &source, Some(idx))
                .ok_or_else(|| format!("No hover info at {}:{}", req.line, req.col))
        }
        "type-at" => {
            let (_s, _t, analysis) = parse_file(file);
            resolve_imports_blocking(idx, &analysis);
            if let Some(r) = analysis.ref_at(point) {
                if let Some(ty) = analysis.inferred_type_via_bag(&r.target_name, point) {
                    return Ok(file_analysis::format_inferred_type(&ty));
                }
            }
            if let Some(sym) = analysis.symbol_at(point) {
                if let Some(ty) = analysis.inferred_type_via_bag(&sym.name, point) {
                    return Ok(file_analysis::format_inferred_type(&ty));
                }
            }
            Err(format!("No type info at {}:{}", req.line, req.col))
        }
        "completion" => {
            let doc = cli_open_document(file, idx);
            // Pack languages: member (sentinel) → in-scope, via the same
            // path the LSP server uses; Perl keeps cursor-context.
            let items = if doc.language != "perl" {
                tphase!("completion_items", backend::pack_completion(
                    ws, &doc.analysis, &doc.text, &doc.tree, point, doc.language,
                    doc.path.as_deref(), idx).0)
            } else {
                let file_path = std::path::Path::new(file).canonicalize()
                    .unwrap_or_else(|_| std::path::PathBuf::from(file));
                tphase!("completion_items", symbols::completion_items(
                    ws, &file_store::FileKey::Path(file_path),
                    &doc.analysis, &doc.tree, &doc.text, pos, idx,
                    Some(doc.stable_outline.package_lines())))
            };
            let mut out = String::new();
            for it in &items {
                match &it.detail {
                    Some(d) if !d.is_empty() => out.push_str(&format!("{}\t{}\n", it.label, d)),
                    _ => out.push_str(&format!("{}\n", it.label)),
                }
            }
            Ok(out.trim_end_matches('\n').to_string())
        }
        "signature-help" => {
            let doc = cli_open_document(file, idx);
            match symbols::signature_help(&doc.analysis, &doc.tree, &doc.text, pos, idx) {
                Some(sh) => {
                    let active = sh.active_signature.unwrap_or(0) as usize;
                    let mut out = String::new();
                    for (i, sig) in sh.signatures.iter().enumerate() {
                        let marker = if i == active { "* " } else { "  " };
                        out.push_str(&format!("{}{}\n", marker, sig.label));
                        if i == active {
                            if let Some(p) = sh.active_parameter {
                                let plabel = sig.parameters.as_ref()
                                    .and_then(|ps| ps.get(p as usize))
                                    .map(|pi| match &pi.label {
                                        tower_lsp::lsp_types::ParameterLabel::Simple(s) => s.clone(),
                                        tower_lsp::lsp_types::ParameterLabel::LabelOffsets([a, b]) =>
                                            sig.label.get(*a as usize..*b as usize).unwrap_or("").to_string(),
                                    })
                                    .unwrap_or_default();
                                out.push_str(&format!("    active param: {} ({})\n", p, plabel));
                            }
                        }
                    }
                    Ok(out.trim_end_matches('\n').to_string())
                }
                None => Err(format!("No signature help at {}:{}", req.line, req.col)),
            }
        }
        "document-highlight" => {
            let doc = cli_open_document(file, idx);
            // Same construction as the LSP handler (routed index, origin
            // key): highlights is the set's origin-narrowed projection.
            // Staging classifies the origin as workspace tier, matching how
            // the references verb attributes the queried file.
            let abs = std::fs::canonicalize(file)
                .unwrap_or_else(|_| std::path::PathBuf::from(file));
            let routed = idx.lookup_for(doc.language);
            let _staged = ScopedWorkspaceEntry::insert_arc(
                ws, abs.clone(), std::sync::Arc::clone(&doc.analysis));
            let cs = resolve::resolve(
                ws, &doc.analysis, file_store::FileKey::Path(abs), point,
                Some(routed.as_lookup()), override_scope_from_env(),
            );
            let highlights = symbols::document_highlights(&cs);
            let mut sources = SourceCache::new(fmt);
            let path = std::fs::canonicalize(file).map(|p| p.display().to_string())
                .unwrap_or_else(|_| file.to_string());
            let mut out = String::new();
            for h in &highlights {
                let (line, col) = sources.display(&path, h.range.start.line as usize, h.range.start.character as usize);
                let kind = match h.kind {
                    Some(tower_lsp::lsp_types::DocumentHighlightKind::WRITE) => "WRITE",
                    _ => "READ",
                };
                out.push_str(&format!("{}:{}:{}\t{}\n", path, line, col, kind));
            }
            Ok(out.trim_end_matches('\n').to_string())
        }
        "linked-editing" => {
            let doc = cli_open_document(file, idx);
            let path = std::fs::canonicalize(file).map(|p| p.display().to_string())
                .unwrap_or_else(|_| file.to_string());
            let abs = std::fs::canonicalize(file)
                .unwrap_or_else(|_| std::path::PathBuf::from(file));
            let routed = idx.lookup_for(doc.language);
            let _staged = ScopedWorkspaceEntry::insert_arc(
                ws, abs.clone(), std::sync::Arc::clone(&doc.analysis));
            let cs = resolve::resolve(
                ws, &doc.analysis, file_store::FileKey::Path(abs), point,
                Some(routed.as_lookup()), override_scope_from_env(),
            );
            match symbols::linked_editing_ranges(&cs) {
                Some(ranges) => {
                    let mut sources = SourceCache::new(fmt);
                    let mut out = String::new();
                    for r in &ranges {
                        let (line, col) = sources.display(&path, r.start.line as usize, r.start.character as usize);
                        out.push_str(&format!("{}:{}:{}\n", path, line, col));
                    }
                    Ok(out.trim_end_matches('\n').to_string())
                }
                None => Err(format!("No linked-editing ranges at {}:{} (need >= 2 occurrences)", req.line, req.col)),
            }
        }
        "semantic-tokens" => {
            let doc = cli_open_document(file, idx);
            let tokens = symbols::semantic_tokens(&doc.analysis);
            let legend = symbols::semantic_token_types();
            let (mut line, mut col): (u32, u32) = (0, 0);
            let mut out = String::new();
            for t in &tokens {
                line += t.delta_line;
                if t.delta_line == 0 { col += t.delta_start; } else { col = t.delta_start; }
                let name = legend.get(t.token_type as usize).map(|tt| tt.as_str()).unwrap_or("?");
                out.push_str(&format!("{}:{} len={} {}\n", line + 1, col, t.length, name));
            }
            Ok(out.trim_end_matches('\n').to_string())
        }
        "outline" => {
            let (_s, _t, analysis) = parse_file(file);
            resolve_imports_blocking(idx, &analysis);
            Ok(outline_json(&analysis))
        }
        "workspace-symbol" => {
            let q = req.query.clone().unwrap_or_default().to_lowercase();
            let mut results = Vec::new();
            // Same identity-tuple dedup as the LSP handler's
            // `dedup_workspace_symbols`: twin accessor synthesis (getter +
            // fluent-writer) mints two symbols at one span, and a path can be
            // seen by both the resident sweep and the rows pass.
            let mut seen: std::collections::HashSet<(String, String, String, usize, usize)> =
                std::collections::HashSet::new();
            let mut push = |name: &str, kind: &file_analysis::SymKind, file: String, span: file_analysis::Span| {
                if name.to_lowercase().contains(&q) {
                    let key = (
                        name.to_string(), format!("{:?}", kind), file.clone(),
                        span.start.row, span.start.column,
                    );
                    if !seen.insert(key) {
                        return;
                    }
                    results.push(serde_json::json!({
                        "name": name, "kind": format!("{:?}", kind),
                        "file": file,
                        "line": span.start.row, "col": span.start.column,
                    }));
                }
            };
            // Same resident + rows composition as the LSP handler: a
            // symbols-present copy answers residently; evicted copies are
            // rows-guaranteed and answer from the store.
            let mut covered: std::collections::HashSet<std::path::PathBuf> =
                std::collections::HashSet::new();
            for entry in ws.workspace_raw().iter() {
                let file = entry.key().display().to_string();
                if !entry.value().symbols_are_evicted() {
                    if let Ok(canon) = std::fs::canonicalize(entry.key()) {
                        covered.insert(canon);
                    }
                    covered.insert(entry.key().clone());
                }
                for sym in &entry.value().symbols {
                    if sym.hidden_in_outline() {
                        continue;
                    }
                    push(&sym.name, &sym.kind, file.clone(), sym.selection_span);
                }
            }
            // Pack-language (C/C++/…) symbols live in per-language sub-indexes,
            // not the Perl workspace map — sweep them so a C typedef/class/free
            // function surfaces in workspace search too.
            idx.for_each_pack_registered_file(&mut |path, analysis| {
                let file = path.display().to_string();
                if !analysis.symbols_are_evicted() {
                    covered.insert(path.to_path_buf());
                }
                for sym in &analysis.symbols {
                    if sym.hidden_in_outline() {
                        continue;
                    }
                    push(&sym.name, &sym.kind, file.clone(), sym.selection_span);
                }
            });
            for hit in symbols::sym_row_search(idx, &q) {
                let path = std::path::PathBuf::from(&hit.path);
                if covered.contains(&path) {
                    continue;
                }
                if hit.flags & file_analysis::SymRowSeed::FLAG_HIDDEN_IN_OUTLINE != 0 {
                    continue;
                }
                let Some(kind) = file_analysis::sym_kind_from_code(hit.kind) else { continue };
                let span = file_analysis::Span {
                    start: tree_sitter::Point::new(hit.start_row, hit.start_col),
                    end: tree_sitter::Point::new(hit.end_row, hit.end_col),
                };
                push(&hit.name, &kind, hit.path.clone(), span);
            }
            Ok(serde_json::to_string_pretty(&results).unwrap())
        }
        "rename" => {
            let new_name = req.newname.clone().unwrap_or_else(|| "RENAMED".to_string());
            // Rename edits are a machine-applied blob keyed on spans, not a
            // location round-tripped into `--at`, so the batch path emits them
            // in engine coordinates (0-based / byte) — the historical shape the
            // gold fixtures encode. The single-mode `--rename` wrapper calls
            // `run_rename` directly and passes the input dialect instead.
            run_rename(ws, idx, file, point, &new_name, CoordFmt::ZeroBasedByte)
        }
        "diagnostics" => Ok(batch_diagnostics(ws, idx)),
        other => Err(format!("unknown query: {}", other)),
    }
}

/// Document-symbol outline as the pretty-JSON array string (shared by
/// `cli_outline` and `run_one`).
fn outline_json(analysis: &file_analysis::FileAnalysis) -> String {
    let mut results = Vec::new();
    let mut seen: std::collections::HashSet<(String, String, usize, usize)> =
        std::collections::HashSet::new();
    for sym in &analysis.symbols {
        match sym.kind {
            file_analysis::SymKind::Sub | file_analysis::SymKind::Method
            | file_analysis::SymKind::Package | file_analysis::SymKind::Class
            | file_analysis::SymKind::Variable | file_analysis::SymKind::Field
            | file_analysis::SymKind::Enumerator | file_analysis::SymKind::Handler => {}
            _ => continue,
        }
        if sym.hidden_in_outline() { continue; }
        if matches!(sym.kind, file_analysis::SymKind::Variable | file_analysis::SymKind::Enumerator)
            && analysis.scope_within_sub_body(sym.scope)
        {
            continue;
        }
        let mut entry = serde_json::json!({
            "name": sym.name,
            "kind": format!("{:?}", sym.kind),
            "line": sym.selection_span.start.row,
            "col": sym.selection_span.start.column,
        });
        // Richer per-symbol detail (package, params, inferred return type,
        // method flag, display flavor, handler dispatchers) — the QA/debug
        // signal the outline carries beyond name/kind/line/col.
        if let Some(ref pkg) = sym.package {
            entry["package"] = serde_json::json!(pkg);
        }
        // Union members nest under their container in the LSP outline tree;
        // this flat view carries the container explicitly (`package` stays
        // the class — that's the completion/refs identity).
        if let Some(container) = analysis.union_container_of(sym) {
            entry["container"] = serde_json::json!(container.name);
        }
        if let file_analysis::SymbolDetail::Sub { ref params, is_method, .. } = sym.detail {
            if params.iter().any(|p| !p.is_invocant) {
                let param_names: Vec<&str> = params.iter()
                    .filter(|p| !p.is_invocant)
                    .map(|p| p.name.as_str())
                    .collect();
                entry["params"] = serde_json::json!(param_names);
            }
            if let Some(rt) = analysis.symbol_return_type_via_bag(sym.id, None) {
                entry["return_type"] = serde_json::json!(file_analysis::format_inferred_type(&rt));
            }
            if is_method {
                entry["is_method"] = serde_json::json!(true);
            }
            if let Some(d) = sym.presentation.display {
                entry["display"] = serde_json::json!(format!("{:?}", d));
            }
        }
        if let file_analysis::SymbolDetail::Handler { ref params, ref dispatchers, .. } = sym.detail {
            let param_names: Vec<&str> = params.iter()
                .filter(|p| !p.is_invocant)
                .map(|p| p.name.as_str())
                .collect();
            entry["params"] = serde_json::json!(param_names);
            entry["dispatchers"] = serde_json::json!(dispatchers);
            entry["display"] = serde_json::json!(format!("{:?}", sym.presentation.display.unwrap_or_default()));
        }
        use file_analysis::Namespace;
        let is_framework = matches!(sym.namespace, Namespace::Framework { .. });
        let is_dupeable = is_framework && matches!(sym.kind,
            file_analysis::SymKind::Sub | file_analysis::SymKind::Method);
        if is_dupeable {
            let key = (format!("{:?}", sym.kind), sym.name.clone(),
                sym.span.start.row, sym.span.start.column);
            if !seen.insert(key) { continue; }
        }
        results.push(entry);
    }
    serde_json::to_string_pretty(&results).unwrap()
}

/// The override-fan-out scope for CLI references/rename, from
/// `PERL_LSP_RENAME_SCOPE` (`hierarchy` | `dispatch`). Mirrors the LSP
/// `initializationOptions.rename.overrideScope`; absent = the `hierarchy`
/// default. Lets the gold harness exercise both modes per row.
pub(super) fn override_scope_from_env() -> resolve::OverrideScope {
    std::env::var("PERL_LSP_RENAME_SCOPE")
        .ok()
        .map(|s| resolve::OverrideScope::from_option(&s))
        .unwrap_or_default()
}

/// Cross-file rename edit-set as the pretty-JSON object string (shared by
/// `cli_rename` and `run_one`). `file` is the originating file; `point` the cursor.
fn run_rename(
    ws: &file_store::FileStore,
    idx: &module_index::ModuleIndex,
    file: &str,
    point: tree_sitter::Point,
    new_name: &str,
    fmt: CoordFmt,
) -> Result<String, String> {
    use std::collections::HashMap;
    if !resolve::is_valid_rename_name(new_name) {
        return Err("rename: the new name must not be empty or whitespace".to_string());
    }
    let file_path = std::path::Path::new(file).canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(file));
    let (s, _t, mut analysis) = parse_file(file);
    analysis.enrich_imported_types_with_keys(Some(idx));
    // Same shape as the LSP handler: stage the origin, construct the set
    // once, project the rename — the per-arm policy (cross-file vs group vs
    // single-file, rewritability, the pack full-or-refuse) lives on the set.
    let reg = language_driver::LanguageRegistry::with_enabled();
    let lang_id = reg.for_path_sniffed(std::path::Path::new(file), &s)
        .map(|d| d.id()).filter(|id| *id != "perl");
    let routed = idx.lookup_for(lang_id.unwrap_or("perl"));
    let _staged = ScopedWorkspaceEntry::insert(ws, file_path.clone(), analysis);
    let origin = ws.workspace_raw().get(&file_path).map(|r| r.value().clone())
        .expect("origin staged above");
    let cs = resolve::resolve(
        ws, &origin, file_store::FileKey::Path(file_path), point,
        Some(routed.as_lookup()), override_scope_from_env(),
    );
    if cs.resolution().is_none() {
        return Err(format!("Nothing renameable at {}:{}", point.row, point.column));
    }
    let mut sources = SourceCache::new(fmt);
    let mut all_edits: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    for (loc, text) in cs.rename_edits(new_name)? {
        let path = match &loc.key {
            file_store::FileKey::Path(p) => p.display().to_string(),
            file_store::FileKey::Url(u) => u.to_file_path()
                .map(|p| p.display().to_string()).unwrap_or_else(|_| u.to_string()),
        };
        let edit = span_to_json(&mut sources, &path, loc.span, text);
        all_edits.entry(path).or_default().push(edit);
    }
    Ok(serde_json::to_string_pretty(&serde_json::json!(all_edits)).unwrap())
}

/// Whole-tree diagnostics with enrichment parity: each workspace entry
/// goes through the hub's enrichment overlay (`enriched_snapshot` — a
/// derived, fingerprint-keyed copy; the same pass `publish_diagnostics`
/// runs on open docs), so cross-file-typed shapes hint here too. A file
/// whose snapshot fails degrades to its unenriched whole view rather
/// than vanishing.
fn enriched_tree_diagnostics(
    ws: &file_store::FileStore,
    idx: &module_index::ModuleIndex,
    options: symbols::DiagnosticOptions,
) -> Vec<(String, tower_lsp::lsp_types::Diagnostic)> {
    let mut all = Vec::new();
    for entry in ws.workspace_raw().iter() {
        let file = entry.key().display().to_string();
        let cached = std::sync::Arc::new(file_analysis::CachedModule::new(
            entry.key().clone(),
            std::sync::Arc::clone(entry.value()),
        ));
        let diags = match idx.enriched_snapshot(&cached) {
            Some(fa) => symbols::collect_diagnostics(&fa, idx, options),
            None => {
                // Index copies may be refs/bag-evicted; diagnostics read
                // refs AND the bag, so degrade to the whole-on-both-axes
                // view, not the resident copy.
                let whole =
                    file_analysis::CrossFileLookup::whole_present(idx, &cached);
                symbols::collect_diagnostics(&whole, idx, options)
            }
        };
        for d in diags {
            all.push((file.clone(), d));
        }
    }
    // Pack-language files (C++/…) live in the per-language sub-indexes, not the
    // Perl-only `FileStore` above. Mirror the backend's language dispatch: they
    // get `pack_diagnostics` (Mode B — member-op swap + peel), so `--batch
    // diagnostics` / `--check` / gold see the same Mode-B answers the LSP
    // publishes. No enrichment (pack files aren't cross-file-enriched).
    idx.for_each_pack_index(|_lang, pack| {
        pack.for_each_registered_file(&mut |cm| {
            let file = cm.path.display().to_string();
            // Same whole-view routing: pack index copies are evicted.
            let whole = file_analysis::CrossFileLookup::whole_present(pack.as_ref(), cm);
            for d in symbols::pack_diagnostics(&whole, options) {
                all.push((file.clone(), d));
            }
        });
    });
    all
}

/// Whole-tree diagnostics as the pretty-JSON array string (warning+; shared by
/// `--batch` diagnostics requests). Mirrors `cli_check`'s JSON path.
fn batch_diagnostics(ws: &file_store::FileStore, idx: &module_index::ModuleIndex) -> String {
    let options = symbols::DiagnosticOptions::default();
    let mut all = Vec::new();
    for (file, d) in enriched_tree_diagnostics(ws, idx, options) {
        let sev = match d.severity {
            Some(s) if s == tower_lsp::lsp_types::DiagnosticSeverity::ERROR => "error",
            Some(s) if s == tower_lsp::lsp_types::DiagnosticSeverity::WARNING => "warning",
            Some(s) if s == tower_lsp::lsp_types::DiagnosticSeverity::INFORMATION => "info",
            Some(s) if s == tower_lsp::lsp_types::DiagnosticSeverity::HINT => "hint",
            _ => "warning",
        };
        all.push(serde_json::json!({
            "file": file, "line": d.range.start.line, "col": d.range.start.character,
            "severity": sev,
            "code": d.code.map(|c| match c {
                tower_lsp::lsp_types::NumberOrString::String(s) => s,
                tower_lsp::lsp_types::NumberOrString::Number(n) => n.to_string(),
            }).unwrap_or_default(),
            "message": d.message,
        }));
    }
    serde_json::to_string_pretty(&all).unwrap()
}

/// --batch <root> — read JSONL queries on stdin, share one startup, print one
/// JSON response per line. The harness substrate: amortizes the workspace-index
/// + @INC cost across every query instead of paying it per process.
pub(crate) fn cli_batch(root: &str) {
    use std::io::{BufRead, Write};
    let (ws, idx) = cli_full_startup(root);
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut diag_memo: Option<String> = None;
    for line in stdin.lock().lines() {
        let line = match line { Ok(l) => l, Err(_) => break };
        if line.trim().is_empty() { continue; }
        let req: BatchReq = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let _ = writeln!(out, "{}", serde_json::json!({"ok": false, "err": format!("bad request: {}", e)}));
                continue;
            }
        };
        // Whole-tree diagnostics are request-independent within one
        // batch process (ScopedWorkspaceEntry restores any staging) —
        // compute the enriched pass once, replay it for every row.
        let result = if req.q == "diagnostics" {
            Ok(diag_memo
                .get_or_insert_with(|| batch_diagnostics(&ws, &idx))
                .clone())
        } else {
            // The batch protocol's input is engine-native (0-based/byte) and its
            // output dialect is fixed to what gold fixtures encode: the location
            // modes render 1-based/char (this fmt), while rename/workspace-symbol
            // stay engine-coordinated within `run_one` itself.
            run_one(&ws, &idx, &req, CoordFmt::EditorOneBasedChar)
        };
        let resp = match result {
            Ok(s)  => serde_json::json!({"id": req.id, "ok": true,  "out": s}),
            Err(e) => serde_json::json!({"id": req.id, "ok": false, "err": e}),
        };
        let _ = writeln!(out, "{}", serde_json::to_string(&resp).unwrap());
        let _ = out.flush();   // flush per line so a later abort still localizes
    }
}

/// --rename <root> <file> <line> <col> <new> (positional, 0-based/byte) or
/// --rename <root> --at <file>:<line>:<col> <new> (editor, 1-based/char).
/// `cursor` is the args between `<root>` and `<new>`. Output edit coordinates
/// match the input dialect.
pub(crate) fn cli_rename(root: &str, cursor: &[String], new_name: &str) {
    let target = parse_cursor_target(cursor, root).unwrap_or_else(|| {
        eprintln!(
            "perl-lsp --rename: expected `<root> <file> <line> <col> <new>` or `<root> --at <file>:<line>:<col> <new>`"
        );
        std::process::exit(2);
    });
    emit_pos_annotation(&target);
    // Full startup so workspace files are built with the same plugins, type
    // inference, and enrichment that the LSP backend would use.
    let (ws, idx) = cli_full_startup(root);
    match run_rename(&ws, &idx, &target.file, target.point, new_name, target.fmt) {
        Ok(s) => println!("{}", s),
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

/// --dump-package <root> <package> — Dump every sub in a package with
/// derived type info. Debugging aid for the witness/reducer pipeline:
/// prints the raw `return_type` baked into the Symbol, the witness-bag
/// projection at the default and a few common arities, the structural
/// tail-delegation, every param's inferred type at a point
/// just past the sub's signature, and a witness count so you can see at
/// a glance whether the bag has anything to say.
pub(crate) fn cli_dump_package(root: &str, package_name: &str) {
    use std::sync::Arc;
    use file_analysis::{SymKind, SymbolDetail};

    let (ws, module_index) = cli_full_startup(root);

    // Find a FileAnalysis whose package matches. Workspace first; fall
    // back to cached @INC modules. No bespoke discovery — only what
    // the normal startup populated.
    let mut found: Option<(String, Arc<file_analysis::CachedModule>)> = None;
    for entry in ws.workspace_raw().iter() {
        let cm = std::sync::Arc::new(file_analysis::CachedModule::new(
            entry.key().clone(),
            std::sync::Arc::clone(entry.value()),
        ));
        let analysis = file_analysis::CrossFileLookup::whole_present(&module_index, &cm);
        let has_package = analysis.symbols.iter().any(|s| {
            matches!(s.kind, SymKind::Package | SymKind::Class)
                && s.name == package_name
        });
        if has_package {
            found = Some((entry.key().display().to_string(), cm));
            break;
        }
    }
    if found.is_none() {
        if let Some(cached) = module_index.get_cached(package_name) {
            found = Some((cached.path.display().to_string(), cached));
        }
    }

    let Some((path, cached)) = found else {
        eprintln!("Package '{}' not found in workspace or module cache.", package_name);
        eprintln!("(Run the LSP against this workspace once to populate cached @INC modules.)");
        std::process::exit(1);
    };

    // The enrichment overlay (R4): the same derived, fingerprint-keyed
    // enriched copy the diagnostics sweep reads — imported return types
    // visible, shared Arc untouched. Degrade to the whole view when the
    // overlay declines (cycle guard).
    let analysis = module_index.enriched_snapshot(&cached).unwrap_or_else(|| {
        // Overlay declined (serde break / byte-cap giant / cycle taint):
        // dump unenriched, LOUDLY — silent degrade here looks exactly like
        // the inference bug the user is debugging.
        eprintln!(
            "warning: enrichment overlay declined for {path}; cross-file return \
             types will be missing from this dump"
        );
        file_analysis::CrossFileLookup::whole_present(&module_index, &cached)
    });

    // Collect subs/methods declared inside this package.
    let mut subs: Vec<&file_analysis::Symbol> = analysis
        .symbols
        .iter()
        .filter(|s| {
            matches!(s.kind, SymKind::Sub | SymKind::Method)
                && s.package.as_deref() == Some(package_name)
        })
        .collect();
    subs.sort_by_key(|s| (s.span.start.row, s.span.start.column));

    let framework = analysis.package_framework(package_name).map(|f| format!("{:?}", f));

    let mut sub_entries = Vec::with_capacity(subs.len());
    for sym in &subs {
        let SymbolDetail::Sub {
            ref params,
            is_method,
            opaque_return,
            ref doc,
            ..
        } = sym.detail
        else {
            continue;
        };

        // Pick a point inside the sub body so scope-resolved param
        // lookups land in the right scope. End of line N+1 is past
        // any signature parens for almost every shape.
        let probe = tree_sitter::Point::new(
            sym.span.start.row.saturating_add(1),
            0,
        );

        let bag_default = analysis
            .sub_return_type_at_arity(&sym.name, None)
            .as_ref()
            .map(file_analysis::format_inferred_type);
        let mut by_arity = serde_json::Map::new();
        for arity in 0u32..=2 {
            if let Some(t) = analysis.sub_return_type_at_arity(&sym.name, Some(arity)) {
                by_arity.insert(arity.to_string(), serde_json::json!(file_analysis::format_inferred_type(&t)));
            }
        }

        let raw_return = analysis
            .symbol_return_type_via_bag(sym.id, None)
            .as_ref()
            .map(file_analysis::format_inferred_type);

        let param_entries: Vec<_> = params
            .iter()
            .map(|p| {
                let inferred = analysis
                    .inferred_type_via_bag_ctx(&p.name, probe, Some(&module_index))
                    .as_ref()
                    .map(file_analysis::format_inferred_type);
                serde_json::json!({
                    "name": p.name,
                    "is_invocant": p.is_invocant,
                    "is_slurpy": p.is_slurpy,
                    "default": p.default,
                    "inferred_type": inferred,
                })
            })
            .collect();

        // Witness count on the sub's Symbol attachment — surfaces
        // arity / branch-arm observations the reducer is folding.
        let symbol_witness_count = analysis
            .witnesses
            .for_attachment(&witnesses::WitnessAttachment::Symbol(sym.id))
            .len();

        // Provenance: where did this return type come from? Default
        // (Inferred) is implicit; surfaced explicitly only when
        // something else (plugin override / reducer / delegation)
        // contributed. Critical debugging aid when inference grows
        // complex enough that "the LSP says X" needs to come with
        // "because Y" — without re-running the build.
        let provenance = match analysis.return_type_provenance(sym.id) {
            file_analysis::TypeProvenance::Inferred => None,
            file_analysis::TypeProvenance::PluginOverride { plugin_id, reason } => {
                Some(serde_json::json!({
                    "kind": "PluginOverride",
                    "plugin_id": plugin_id,
                    "reason": reason,
                }))
            }
            file_analysis::TypeProvenance::ReducerFold { reducer, evidence } => {
                Some(serde_json::json!({
                    "kind": "ReducerFold",
                    "reducer": reducer,
                    "evidence": evidence,
                }))
            }
            file_analysis::TypeProvenance::Delegation { kind, via } => {
                Some(serde_json::json!({
                    "kind": "Delegation",
                    "delegation_kind": kind,
                    "via": via,
                }))
            }
            file_analysis::TypeProvenance::FrameworkSynthesis { framework, reason } => {
                Some(serde_json::json!({
                    "kind": "FrameworkSynthesis",
                    "framework": framework,
                    "reason": reason,
                }))
            }
        };

        // Variables typed inside this sub's scope. Surfaces chain
        // assignments like `my $route = $self->_route(...)->...->to(...)`
        // — when `$route` shows up here with a class type, the chain
        // typer worked. When it doesn't, the chain died at some link.
        // The same dump that answers "why is `_generate_route`'s
        // return type None" answers "is `$route` typed at all".
        let sub_scope_id = analysis
            .scopes
            .iter()
            .find(|s| {
                matches!(
                    &s.kind,
                    file_analysis::ScopeKind::Sub { name } | file_analysis::ScopeKind::Method { name }
                        if name == &sym.name
                ) && s.span.start == sym.span.start
            })
            .map(|s| s.id);
        let mut vars_in_scope: Vec<serde_json::Value> = Vec::new();
        if let Some(sid) = sub_scope_id {
            use crate::model::witnesses::{WitnessAttachment, WitnessPayload};
            for w in analysis.witnesses.all() {
                let WitnessAttachment::Variable { name, scope } = &w.attachment else { continue };
                if *scope != sid { continue; }
                let WitnessPayload::InferredType(t) = &w.payload else { continue };
                vars_in_scope.push(serde_json::json!({
                    "var": name,
                    "type": file_analysis::format_inferred_type(t),
                    "line": w.span.start.row,
                }));
            }
        }

        let mut entry = serde_json::json!({
            "name": sym.name,
            "kind": format!("{:?}", sym.kind),
            "is_method": is_method,
            "line": sym.selection_span.start.row,
            "params": param_entries,
            "raw_return_type": raw_return,
            "bag_return_type": bag_default,
            "bag_return_type_at_arity": serde_json::Value::Object(by_arity),
            "symbol_witness_count": symbol_witness_count,
            "vars_in_scope": vars_in_scope,
        });
        if let Some(prov) = provenance {
            entry["return_type_provenance"] = prov;
        }
        if let Some(d) = sym.presentation.display {
            entry["display"] = serde_json::json!(format!("{:?}", d));
        }
        if sym.presentation.hide_in_outline {
            entry["hide_in_outline"] = serde_json::json!(true);
        }
        if opaque_return {
            entry["opaque_return"] = serde_json::json!(true);
        }
        if let Some(ref outline) = sym.presentation.label {
            entry["outline_label"] = serde_json::json!(outline);
        }
        if let Some(d) = doc.as_ref() {
            let first_line = d.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
            if !first_line.is_empty() {
                entry["doc_first_line"] = serde_json::json!(first_line);
            }
        }
        sub_entries.push(entry);
    }

    let parents = analysis.declared_parents(package_name).to_vec();

    let out = serde_json::json!({
        "package": package_name,
        "file": path,
        "framework": framework,
        "parents": parents,
        "total_witnesses_in_file": analysis.witnesses.len(),
        "subs": sub_entries,
    });

    eprintln!(
        "Dumped {} subs from {} (file: {})",
        subs.len(),
        package_name,
        out.get("file").and_then(|v| v.as_str()).unwrap_or("?")
    );
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

/// --workspace-symbol <root> <query> — Search symbols
pub(crate) fn cli_workspace_symbol(root: &str, query: &str) {
    // Full startup so workspace symbols reflect plugin-synthesized entities
    // (helpers, routes, accessors), built with `root`'s plugins not cwd's.
    let (ws, idx) = cli_full_startup(root);
    let req = BatchReq {
        id: String::new(), q: "workspace-symbol".into(),
        file: String::new(), line: 0, col: 0,
        query: Some(query.to_string()), newname: None,
    };
    // workspace-symbol emits engine-coordinated spans (0-based/byte) directly,
    // independent of the location seam; the dialect here is nominal.
    print_run_one(&ws, &idx, &req, CoordFmt::ZeroBasedByte);
}
