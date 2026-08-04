//! The layer DAG, enforced. CLAUDE.md's architecture rules #1/#2 say
//! data flows down only and the model never touches the tree; this
//! suite makes a violation a red `cargo test` instead of a review
//! catch. (The alternative — a crate-per-layer workspace — buys the
//! same guarantee from the compiler at the price of five published
//! crates; the executed-and-rejected split lives on branch `workspace-split`.)
//!
//! The tree IS the map: a module's layer is its top-level directory
//! (`src/model/**` = Model, `src/build/**` = Build, …), so placing a
//! file places it in the architecture. The only non-directory members
//! are `cst.rs` (the Cst layer is one module) and `main.rs` (the Lsp
//! entry point); any other `.rs` directly under `src/` is unassigned
//! and fails the walk.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Layer order — an import may only point at the same layer or lower.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Layer {
    Model = 0,
    Cst = 1,
    Build = 2,
    Index = 3,
    Lsp = 4,
}

/// Top-level path segment → layer. `crate::build::builder::…` resolves
/// through its first segment, so the directory name is the whole story.
fn layer_of_segment(seg: &str) -> Option<Layer> {
    Some(match seg {
        "model" => Layer::Model,
        "cst" => Layer::Cst,
        "build" => Layer::Build,
        "index" => Layer::Index,
        "lsp" => Layer::Lsp,
        _ => return None,
    })
}

/// Non-test source files with their layer and owning module, derived
/// from the tree. A file directly under a layer dir IS its module; a
/// file nested deeper (a split module's directory) reports the
/// directory's module name, so a submodule can't dodge the DAG or the
/// allowlists below. Test suites (`*_tests.rs` / `*_test.rs`) are
/// exempt — they deliberately drive lower layers through upper ones.
fn source_files() -> Vec<(PathBuf, Layer, String)> {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    for entry in fs::read_dir(&src).expect("read src/") {
        let path = entry.expect("dir entry").path();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
        if path.is_dir() {
            let layer = layer_of_segment(&stem)
                .unwrap_or_else(|| panic!("src/{stem}/ is not a layer directory"));
            collect_rs(&path, layer, None, &mut out);
            continue;
        }
        if path.extension().is_none_or(|e| e != "rs") || is_test_file(&stem) {
            continue;
        }
        let layer = match stem.as_str() {
            "main" => Layer::Lsp,
            "cst" => Layer::Cst,
            _ => panic!(
                "unassigned module src/{stem}.rs — place it in a layer directory \
                 (model/ cst build/ index/ lsp/)"
            ),
        };
        out.push((path, layer, stem));
    }
    out
}

fn is_test_file(stem: &str) -> bool {
    stem.ends_with("_tests") || stem.ends_with("_test") || stem == "layering_tests"
}

fn collect_rs(
    dir: &PathBuf,
    layer: Layer,
    module: Option<&str>,
    out: &mut Vec<(PathBuf, Layer, String)>,
) {
    for entry in fs::read_dir(dir).unwrap_or_else(|_| panic!("read {}", dir.display())) {
        let path = entry.expect("dir entry").path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string)
        else {
            continue;
        };
        if path.is_dir() {
            // First directory under the layer names the module; deeper
            // nesting stays attributed to it.
            collect_rs(&path, layer, Some(module.unwrap_or(&stem)), out);
            continue;
        }
        if path.extension().is_none_or(|e| e != "rs") || is_test_file(&stem) {
            continue;
        }
        out.push((path.clone(), layer, module.unwrap_or(&stem).to_string()));
    }
}

/// `crate::xxx` references in non-test code, with `use` lines and
/// inline paths both counted. Lines inside `#[cfg(test)]` regions are
/// NOT excluded — test modules live in `_tests.rs` files here, which
/// the walker already skips.
fn crate_refs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let needle = b"crate::";
    let mut i = 0;
    while let Some(j) = text[i..].find("crate::").map(|j| i + j) {
        i = j + needle.len();
        // skip `::crate::` false positives and doc-comment mentions in
        // strings is overkill; module names are what we extract.
        let rest = &bytes[i..];
        let end = rest
            .iter()
            .position(|c| !(c.is_ascii_alphanumeric() || *c == b'_'))
            .unwrap_or(rest.len());
        if end > 0 {
            out.push(text[i..i + end].to_string());
        }
    }
    out
}

/// Rule: every `crate::X` reference points at the same layer or lower.
#[test]
fn imports_flow_down_only() {
    let mut violations = Vec::new();
    for (f, my_layer, _module) in source_files() {
        let text = fs::read_to_string(&f).expect("read source");
        for target in crate_refs(&text) {
            let Some(target_layer) = layer_of_segment(&target) else {
                continue; // not a layer path (a type/fn at crate root, etc.)
            };
            if target_layer > my_layer {
                violations.push(format!(
                    "{} ({:?}) imports crate::{} ({:?}) — data flows down only",
                    f.display(),
                    my_layer,
                    target,
                    target_layer,
                ));
            }
        }
    }
    assert!(violations.is_empty(), "layer violations:\n{}", violations.join("\n"));
}

/// Rule #2's teeth: the model layer never touches the tree. The only
/// tree-sitter name it may utter is `Point` (plus the serde shim that
/// wraps it). `cst` may not appear at all — the typed view is for
/// sanctioned tree consumers, and the model is not one.
#[test]
fn model_layer_cannot_walk_trees() {
    let mut violations = Vec::new();
    for (f, layer, _module) in source_files() {
        if layer != Layer::Model {
            continue;
        }
        let text = fs::read_to_string(&f).expect("read source");
        for (ln, line) in text.lines().enumerate() {
            let mut i = 0;
            while let Some(j) = line[i..].find("tree_sitter::").map(|j| i + j) {
                i = j + "tree_sitter::".len();
                let rest = &line[i..];
                let end = rest
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(rest.len());
                let name = &rest[..end];
                if name != "Point" {
                    violations.push(format!(
                        "{}:{}: tree_sitter::{} — the model is Point-only",
                        f.display(),
                        ln + 1,
                        name,
                    ));
                }
            }
            for forbidden in ["TreeCursor", "child_by_field_name", "named_child("] {
                if line.contains(forbidden) {
                    violations.push(format!(
                        "{}:{}: `{}` — tree walking belongs in the builder",
                        f.display(),
                        ln + 1,
                        forbidden,
                    ));
                }
            }
        }
        if text.contains("crate::cst") {
            violations.push(format!(
                "{}: imports crate::cst — the typed view is for tree consumers",
                f.display(),
            ));
        }
    }
    assert!(violations.is_empty(), "rule #2 violations:\n{}", violations.join("\n"));
}

/// Only the builder layer (and `cst` itself) may speak the grammar:
/// `ts_parser_perl::` anywhere above `build` means a second parser
/// entry point is growing. The index layer gets a pass for parsing
/// (resolver/document call `builder::create_parser`), so the check is
/// on the grammar crate, not `tree_sitter` generally.
#[test]
fn grammar_stays_in_the_builder_layer() {
    let mut violations = Vec::new();
    for (f, layer, _module) in source_files() {
        if layer == Layer::Build || layer == Layer::Cst {
            continue;
        }
        // main.rs hosts --parse; backend/document parse via
        // builder::create_parser. Direct grammar naming outside
        // build/cst is the smell.
        let text = fs::read_to_string(&f).expect("read source");
        for (ln, line) in text.lines().enumerate() {
            if line.contains("ts_parser_perl::") {
                violations.push(format!(
                    "{}:{}: names the grammar directly — route through builder::create_parser",
                    f.display(),
                    ln + 1,
                ));
            }
        }
    }
    assert!(violations.is_empty(), "grammar violations:\n{}", violations.join("\n"));
}

/// Whole-copy registration is BUDGETED, not free: every call site of an API
/// that pins an unstripped `FileAnalysis` resident must appear here with a
/// reason its residency is bounded. The stripped alternatives
/// (`register_symbols_stripping` / `register_workspace_stripping` /
/// `prepare_pack_parts` / `prepare_workspace_parts` + the deferred writer
/// halves) are the DEFAULT for anything bulk — a new call site of the APIs
/// below compiles and passes every functional test while silently
/// re-pinning the gigabytes the eviction axes strip (the chromium 20 GB
/// wall), so this test is the tripwire: to add one, add the (file, count)
/// here WITH a bounded-residency justification in the code.
#[test]
fn whole_copy_registration_sites_are_allowlisted() {
    // fn name → (file stem, expected call-site count, why it's bounded)
    let allow: Vec<(&str, Vec<(&str, usize, &str)>)> = vec![
        (
            "register_symbols",
            vec![
                // 1 shared writer fallback (commit-fail + panic, via
                // run_persist_writer — bounded by failure, tripwire-
                // counted), 1 degraded/unpersisted worker arm (tripwire-
                // counted), 1 pack_file_changed unpersisted fallback.
                ("module_resolver", 3, "failure fallbacks + NO_EVICT arm, tripwire-counted"),
            ],
        ),
        (
            "register_symbols_inner",
            vec![
                ("module_index", 2, "the two registration front doors"),
                (
                    "module_resolver",
                    3,
                    "stub/full warm lanes + deferred writer — all take prepare_pack_parts output",
                ),
            ],
        ),
        // register_workspace_module: TEST-ONLY today (fixtures build whole
        // copies directly). Its first production caller lands here.
        ("register_workspace_module", vec![]),
        (
            "register_workspace_resident",
            vec![
                ("backend", 1, "watcher re-register — bounded by external change volume"),
                ("module_index", 1, "register_workspace_module's residency half"),
                ("module_resolver", 1, "shared writer failure fallback (run_persist_writer)"),
            ],
        ),
        (
            "register_workspace_residency",
            vec![
                ("module_index", 1, "register_workspace_stripping's residency half"),
                ("module_resolver", 2, "deferred writer halves — stripped arcs only"),
            ],
        ),
        (
            "register_materialized_whole",
            vec![(
                "module_index",
                1,
                "gated-emission CLI/batch materialization — plugin-triggered \
                 files only (sparse by construction), one-shot startup, whole \
                 copy deliberate so whole_present sees the emissions",
            )],
        ),
    ];
    let mut violations: Vec<String> = Vec::new();
    for (name, files) in &allow {
        let mut seen: HashMap<String, usize> = HashMap::new();
        for (path, _layer, stem) in source_files() {
            let text = fs::read_to_string(&path).unwrap();
            let needle = format!("{name}(");
            for line in text.lines() {
                let t = line.trim_start();
                if t.starts_with("//") {
                    continue;
                }
                let mut rest = t;
                while let Some(pos) = rest.find(&needle) {
                    // A call site, not the definition, and not a
                    // longer-named sibling (`register_symbols_inner(`
                    // must not count as `register_symbols(`).
                    let before = &rest[..pos];
                    let defn = before.trim_end().ends_with("fn");
                    let word_start = pos == 0
                        || !rest[..pos]
                            .chars()
                            .next_back()
                            .is_some_and(|c| c.is_alphanumeric() || c == '_');
                    if !defn && word_start {
                        *seen.entry(stem.clone()).or_default() += 1;
                    }
                    rest = &rest[pos + needle.len()..];
                }
            }
        }
        let expected: HashMap<String, usize> =
            files.iter().map(|(f, n, _)| (f.to_string(), *n)).collect();
        for (file, n) in &seen {
            match expected.get(file) {
                Some(exp) if exp == n => {}
                Some(exp) => violations.push(format!(
                    "{name}() call-site count changed in {file}: {n} (allowlisted {exp}) — \
                     if the new site registers WHOLE copies, justify its residency bound here; \
                     bulk paths use the stripping/parts APIs"
                )),
                None => violations.push(format!(
                    "{name}() called from {file} ({n} site(s)) — not allowlisted. Bulk \
                     registration must go through the stripping/parts APIs; a deliberate \
                     whole-copy site needs an entry here with its residency bound"
                )),
            }
        }
        for (file, exp) in &expected {
            if !seen.contains_key(file) {
                violations.push(format!(
                    "{name}() allowlisted in {file} ({exp}) but no call site found — \
                     update the allowlist"
                ));
            }
        }
    }
    assert!(violations.is_empty(), "whole-copy registration drift:\n{}", violations.join("\n"));
}
