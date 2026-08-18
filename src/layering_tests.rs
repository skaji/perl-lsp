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
/// `Util` sits below everything: std-only instrumentation with no crate
/// imports at all (`util_tier_is_std_only` enforces the stronger rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Layer {
    Util = 0,
    Model = 1,
    Cst = 2,
    Build = 3,
    Index = 4,
    Lsp = 5,
}

/// Top-level path segment → layer. `crate::build::builder::…` resolves
/// through its first segment, so the directory name is the whole story.
fn layer_of_segment(seg: &str) -> Option<Layer> {
    Some(match seg {
        "util" => Layer::Util,
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
                 (util/ model/ cst build/ index/ lsp/)"
            ),
        };
        out.push((path, layer, stem));
    }
    out
}

fn is_test_file(stem: &str) -> bool {
    // `_test_corpus`: data-only fixture files consumed via `#[path]` from
    // test suites — never compiled into the production binary.
    stem.ends_with("_tests")
        || stem.ends_with("_test")
        || stem.ends_with("_test_corpus")
        || stem == "layering_tests"
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

/// The util tier's charter is stricter than down-only: std-only, no
/// `crate::` references at all. Without this, util would be a laundering
/// hole — a file could dodge the DAG by moving there while still
/// importing model/build internals.
#[test]
fn util_tier_is_std_only() {
    let mut violations = Vec::new();
    for (f, layer, _module) in source_files() {
        if layer != Layer::Util {
            continue;
        }
        let text = fs::read_to_string(&f).expect("read source");
        for (ln, line) in text.lines().enumerate() {
            if line.contains("crate::") {
                violations.push(format!(
                    "{}:{}: util is std-only — no crate:: references",
                    f.display(),
                    ln + 1,
                ));
            }
        }
    }
    assert!(violations.is_empty(), "util-tier violations:\n{}", violations.join("\n"));
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
                // counted).
                ("module_resolver", 2, "failure fallbacks, tripwire-counted"),
                // The invalidation swap's unpersisted fallback — bounded by
                // in-session edit volume, whole so the bag stays recoverable.
                ("pack_invalidator", 1, "unpersisted-edit fallback"),
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

/// Verb-routing store selection has ONE speller: `ModuleIndex::lookup_for`.
/// An LSP-layer call to `pack_index()` re-derives "which store serves this
/// origin" per handler — the C1 disease: a verb that picks the store itself
/// can pick it wrong (or forget), and the CandidateSet's construction-derived
/// pack policy silently pairs with the wrong store. Whole-sub-index SWEEPS
/// (`for_each_pack_index`) are a different question and stay allowed.
#[test]
fn pack_store_selection_stays_in_lookup_for() {
    let mut violations = Vec::new();
    for (path, layer, _stem) in source_files() {
        if layer != Layer::Lsp {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        for (i, line) in text.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            if t.contains(".pack_index(") {
                violations.push(format!("{}:{}: {}", path.display(), i + 1, t));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "LSP-layer store selection must route through ModuleIndex::lookup_for \
         (the one speller), never pick a pack sub-index per handler:\n{}",
        violations.join("\n")
    );
}

/// `FileAnalysis::inferred_type` is raw-seed-state introspection for tests
/// only. Its last production caller (the MCB early-out) is gone — the
/// MCB→bag bridge publishes `Edge(MethodOnClass)` witnesses and lets the
/// registry's fold precedence arbitrate, so a new production
/// `.inferred_type(` call re-opens a parallel type query beside
/// `inferred_type_via_bag`.
#[test]
fn inferred_type_has_no_production_caller() {
    let mut violations = Vec::new();
    for (path, _layer, _stem) in source_files() {
        let text = fs::read_to_string(&path).unwrap();
        for (i, line) in text.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            if t.contains(".inferred_type(") {
                violations.push(format!("{}:{}: {}", path.display(), i + 1, t));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "production code must query types via inferred_type_via_bag (the registry), \
         never the raw seed-state reader:\n{}",
        violations.join("\n")
    );
}

/// D4: the blocking decision rides the query API, not per-handler memory.
/// Handlers reach set construction, the relational row search, and the
/// rehydration readers only through `run_query`'s `QueryCx` (minted on the
/// blocking pool) — the raw spellings appearing in the handler file mean a
/// verb grew an inline I/O path on the reactor.
#[test]
fn query_verbs_route_through_run_query() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/lsp/backend/server.rs");
    let text = fs::read_to_string(&path).unwrap();
    let forbidden = [
        "index::resolve::resolve(", // set construction → QueryCx::set
        "sym_row_search",           // relational rows → QueryCx::sym_rows
        ".whole_present(",          // rehydration reader → QueryCx lanes
        ".lookup_for(",             // routing binds inside the hop → QueryCx::routed
    ];
    let mut violations = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with("//") {
            continue;
        }
        for f in forbidden {
            if t.contains(f) {
                violations.push(format!("{}:{}: {}", path.display(), i + 1, t));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "query verbs reach I/O-capable lookups only through Backend::run_query's \
         QueryCx (the blocking hop):\n{}",
        violations.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Grammar-kind tripwire
// ---------------------------------------------------------------------------

/// Kinds a `kind()` comparison may name even though the grammar does not have
/// them YET. Everything here is deliberate forward-compatibility, not debt.
///
/// `parenthesized_expression` is absent from ts-parser-perl on purpose and is
/// coming in the next release — it is a breaking change, and nearly every
/// tree-sitter grammar needs that wrapper or aliases and fields misbehave. The
/// ~27 Perl-side arms that name it are inert today and become correct the day
/// the parser lands. **Do not "clean them up"** (see CLAUDE.md's gotchas), and
/// do not let this tripwire be the reason someone does.
///
/// The pack side is unaffected: `parenthesized_expression` is already a real
/// tree-sitter-cpp kind, which is why this check is per-language rather than
/// against the union — the union would silently excuse the Perl arms.
///
/// Adding a name here is a claim that a future grammar release will define it.
/// Anything else belongs in the grammar or out of the code.
const DECLARED_FUTURE_PERL_KINDS: &[&str] = &["parenthesized_expression"];

/// Dead `kind()` arms that already existed when this tripwire landed.
///
/// These are BUGS, not exemptions. Each one names a kind the Perl grammar does
/// not have, so the arm never fires — it is inert code sitting next to a
/// working sibling, which is exactly what makes the class hard to see. They
/// are quarantined rather than fixed here because a dead arm can be masking a
/// real behavioural gap whose fix wants its own change and its own test; see
/// the audit on issue #120.
///
/// **This list may only shrink.** Adding to it means shipping a new dead arm.
/// Keyed by (path suffix, kind) rather than line so it survives edits.
const KNOWN_DEAD_KIND_ARMS: &[(&str, &str)] = &[
    // Real kind is `loopex_expression` for all three, so `last if $x;` and
    // friends are not recognized as control-flow exits and do not narrow the
    // rest of the block. The one finding here with visible behaviour behind it.
    ("build/builder/narrowing.rs", "last_expression"),
    ("build/builder/narrowing.rs", "next_expression"),
    ("build/builder/narrowing.rs", "redo_expression"),
    // A Perl qualified name (`Foo::Bar::baz`) is a `bareword`; there is no
    // `scoped_identifier` in this grammar. Both sites pair it with `bareword`,
    // which does match, so no behaviour is lost — it is the textbook shape of
    // the hazard: a dead half that reads as handled.
    ("build/builder/emit.rs", "scoped_identifier"),
    // `do { }` is `do_expression` wrapping a `block`; the `block` arm already
    // claims the body, so this half is inert.
    ("build/builder/visit_decl.rs", "do_block"),
    // `foreach` is `for_statement`, which is listed alongside it, so the
    // parent-kind exclusion still works.
    ("build/builder/visit_decl.rs", "foreach_statement"),
    // `if`/`unless` are `conditional_statement`, `while`/`until` are
    // `loop_statement`. The fold range these arms wanted is added by the
    // `block` arm anyway, so folding is unaffected.
    ("build/builder/visit_decl.rs", "if_statement"),
    ("build/builder/visit_decl.rs", "unless_statement"),
    ("build/builder/visit_decl.rs", "until_statement"),
    ("build/builder/visit_decl.rs", "while_statement"),
];

/// Kinds tree-sitter defines for every grammar, so they never appear in a
/// grammar's own node list.
const TREE_SITTER_BUILTIN_KINDS: &[&str] = &["ERROR", "MISSING"];

/// Every kind a grammar can produce, NAMED AND ANONYMOUS.
///
/// Anonymous keyword tokens count: `node.kind()` returns `"my"`, `"sub"`,
/// `"and"` and friends for them, and the codebase legitimately compares
/// against those. Filtering to named kinds only would flag correct code.
fn grammar_kinds(lang: &tree_sitter::Language) -> std::collections::HashSet<String> {
    (0..lang.node_kind_count())
        .filter_map(|i| lang.node_kind_for_id(i as u16))
        .filter(|k| lang.id_for_node_kind(k, true) != 0 || lang.id_for_node_kind(k, false) != 0)
        .map(str::to_string)
        .collect()
}

/// Index just past the `close` matching the `open` at `start`.
///
/// Skips strings, char literals and comments. A brace inside any of those is
/// not structure, and one desync makes a block swallow the rest of the file —
/// which is exactly how an unrelated `match export_var_basename(..) { "EXPORT" => .. }`
/// first looked like a kind comparison.
fn balanced_from(src: &str, start: usize, open: u8, close: u8) -> usize {
    let b = src.as_bytes();
    let (mut i, mut depth) = (start, 0usize);
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            // `'x'` / `'\n'` is a char literal; `'a` alone is a lifetime and
            // closes nothing, so only the quoted form is skipped.
            b'\'' => {
                let rest = &b[i..];
                let lit = match rest {
                    [_, b'\\', _, b'\'', ..] => Some(4),
                    [_, c, b'\'', ..] if *c != b'\\' => Some(3),
                    _ => None,
                };
                if let Some(n) = lit {
                    i += n - 1;
                }
            }
            b'/' if src[i..].starts_with("//") => {
                i = src[i..].find('\n').map_or(b.len(), |n| i + n);
            }
            b'/' if src[i..].starts_with("/*") => {
                i = src[i + 2..].find("*/").map_or(b.len(), |n| i + 2 + n + 1);
            }
            c if c == open => depth += 1,
            c if c == close => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    b.len()
}

/// String literals in `s`, in order.
fn string_literals(s: &str) -> Vec<String> {
    let (b, mut out, mut i) = (s.as_bytes(), Vec::new(), 0);
    while i < b.len() {
        if b[i] == b'"' {
            let start = i + 1;
            i += 1;
            while i < b.len() && b[i] != b'"' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            if i <= b.len() {
                out.push(s[start..i.min(b.len())].to_string());
            }
        }
        i += 1;
    }
    out
}

fn is_kindish(s: &str) -> bool {
    !s.is_empty()
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
}

/// Every string this source compares against a tree-sitter `kind()`, with the
/// line it sits on.
///
/// Deliberately high-precision rather than exhaustive: a false positive fails
/// the build on correct code, which is far worse than missing an arm. Four
/// shapes are recognized, covering how this codebase actually spells it —
/// `k.kind() == "x"`, `matches!(k.kind(), "a" | "b")`, `match k.kind() { .. }`,
/// and the same three through a local bound from `.kind()`.
fn kind_comparison_literals(src: &str) -> Vec<(usize, String)> {
    let line_of = |idx: usize| src[..idx].matches('\n').count() + 1;
    let mut out: Vec<(usize, String)> = Vec::new();

    // Locals bound straight from `.kind()`, e.g. `let parent_kind = p.kind();`
    let mut bound: Vec<&str> = Vec::new();
    for (i, _) in src.match_indices(".kind()") {
        let head = &src[..i];
        if let Some(l) = head.rfind("let ") {
            let decl = &head[l + 4..];
            if !decl.contains(';') && !decl.contains('{') {
                let name = decl.trim().trim_start_matches("mut ").trim();
                let name = name.split(['=', ':', ' ']).next().unwrap_or("").trim();
                if !name.is_empty() && is_kindish(name) && src[i..].starts_with(".kind();") {
                    bound.push(name);
                }
            }
        }
    }
    let is_kind_expr = |head: &str| {
        head.contains(".kind()") || bound.iter().any(|b| head.split_whitespace().any(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '_') == *b))
    };

    // (1) `<kind expr> == "x"` / `!= "x"` / `== Some("x")`
    for (i, _) in src.match_indices(".kind()") {
        let rest = src[i + 7..].trim_start();
        let rest = rest.strip_prefix(')').unwrap_or(rest).trim_start();
        if let Some(r) = rest.strip_prefix("==").or_else(|| rest.strip_prefix("!=")) {
            let r = r.trim_start();
            let r = r.strip_prefix("Some(").unwrap_or(r).trim_start();
            if r.starts_with('"') {
                if let Some(lit) = string_literals(r).into_iter().next() {
                    out.push((line_of(i), lit));
                }
            }
        }
    }
    for name in &bound {
        for (i, _) in src.match_indices(name.to_owned()) {
            let rest = src[i + name.len()..].trim_start();
            if let Some(r) = rest.strip_prefix("==").or_else(|| rest.strip_prefix("!=")) {
                let r = r.trim_start();
                if r.starts_with('"') {
                    if let Some(lit) = string_literals(r).into_iter().next() {
                        out.push((line_of(i), lit));
                    }
                }
            }
        }
    }

    // (2) `matches!(<kind expr>, "a" | "b" | ...)`
    for (i, _) in src.match_indices("matches!(") {
        let open = i + "matches!".len();
        let end = balanced_from(src, open, b'(', b')');
        let inner = &src[open + 1..end.saturating_sub(1)];
        let Some(comma) = inner.find(',') else { continue };
        if !is_kind_expr(&inner[..comma]) {
            continue;
        }
        for lit in string_literals(&inner[comma..]) {
            out.push((line_of(i), lit));
        }
    }

    // (3) `match <kind expr> { "a" => .., "b" | "c" => .. }` — pattern position
    //     only, so a string in an arm BODY is never mistaken for a kind.
    for (i, _) in src.match_indices("match ") {
        let after = &src[i + 6..];
        let Some(brace_rel) = after.find('{') else { continue };
        let head = &after[..brace_rel];
        if head.contains(';') || head.contains(')') && !head.contains(".kind()") {
            continue;
        }
        if !is_kind_expr(head) {
            continue;
        }
        let open = i + 6 + brace_rel;
        let end = balanced_from(src, open, b'{', b'}');
        for (n, line) in src[open..end].split('\n').enumerate() {
            let pat = match line.split_once("=>") {
                Some((before, _)) => before.to_string(),
                None => {
                    let t = line.trim();
                    // A continued pattern line is only literals, `|` and space.
                    let only_pat = !t.is_empty()
                        && t.chars().all(|c| c == '"' || c == '|' || c == '_' || c.is_whitespace() || c.is_alphanumeric());
                    if only_pat && t.contains('"') { t.to_string() } else { String::new() }
                }
            };
            for lit in string_literals(&pat) {
                out.push((line_of(open) + n, lit));
            }
        }
    }

    out.retain(|(_, s)| is_kindish(s));
    out.sort();
    out.dedup();
    out
}

/// A `kind()` compared against a string the grammar does not define is a
/// SILENT no-op: the arm never fires, and it reads like handled behaviour
/// sitting next to a working sibling. Two live bugs came from exactly that —
/// a skip-list naming `require_statement` (the real kind is
/// `require_expression`), and `"bareword" | "scoped_identifier"` arms where
/// only the first half can ever match.
///
/// Per-language on purpose: see `DECLARED_FUTURE_PERL_KINDS`.
#[test]
fn kind_comparisons_name_real_grammar_kinds() {
    let perl = grammar_kinds(&ts_parser_perl::LANGUAGE.into());
    let pod = grammar_kinds(&ts_parser_pod::LANGUAGE.into());
    let cpp = grammar_kinds(&tree_sitter_cpp::LANGUAGE.into());
    assert!(perl.len() > 100 && cpp.len() > 100, "grammars failed to enumerate");

    let builtin: std::collections::HashSet<&str> = TREE_SITTER_BUILTIN_KINDS.iter().copied().collect();
    let future: std::collections::HashSet<&str> = DECLARED_FUTURE_PERL_KINDS.iter().copied().collect();

    let mut dead: Vec<String> = Vec::new();
    for (path, _, _) in source_files() {
        let rel = path.strip_prefix(env!("CARGO_MANIFEST_DIR")).unwrap_or(&path).display().to_string();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or_default();
        // Which grammar do this file's trees come from? Path-derived, because
        // that is how the codebase separates them.
        let (known, lang): (std::collections::HashSet<&str>, &str) =
            if rel.ends_with("build/pod.rs") {
                (pod.iter().map(String::as_str).collect(), "pod")
            } else if rel.contains("query_extract") {
                // The generic extraction driver serves every pack language.
                (perl.iter().chain(pod.iter()).chain(cpp.iter()).map(String::as_str).collect(), "any")
            } else if name.starts_with("cpp_") || rel.contains("cpp_reparse") {
                (cpp.iter().map(String::as_str).collect(), "cpp")
            } else {
                (perl.iter().map(String::as_str).collect(), "perl")
            };

        let text = fs::read_to_string(&path).expect("read source");
        for (line, lit) in kind_comparison_literals(&text) {
            if known.contains(lit.as_str()) || builtin.contains(lit.as_str()) {
                continue;
            }
            if lang == "perl" && future.contains(lit.as_str()) {
                continue;
            }
            if KNOWN_DEAD_KIND_ARMS
                .iter()
                .any(|(f, k)| *k == lit && rel.replace('\\', "/").ends_with(f))
            {
                continue;
            }
            dead.push(format!("{rel}:{line}: `{lit}` is not a {lang} grammar kind"));
        }
    }

    assert!(
        dead.is_empty(),
        "these `kind()` comparisons can never match — the arm is dead code that \
         reads as handled behaviour.\nFix the spelling, or if the kind is coming \
         in a future grammar release, add it to DECLARED_FUTURE_PERL_KINDS with \
         a note saying so:\n{}",
        dead.join("\n")
    );
}
