//! Parse-damage scoring and the validated preprocess: declarator-macro
//! stripping, salvage grouping, and structural/directive strip fallbacks.

use super::*;

/// ERROR + MISSING node count — the parser's own verdict on a parse.
pub fn parse_damage(node: tree_sitter::Node) -> usize {
    let mut n = 0;
    let mut cur = node.walk();
    let mut stack = vec![node];
    while let Some(x) = stack.pop() {
        if x.is_error() || x.is_missing() {
            n += 1;
        }
        for c in x.children(&mut cur) {
            stack.push(c);
        }
    }
    n
}

/// BODIED structure-container count (class/struct/union/enum/namespace) —
/// the damage count's blind spot. tree-sitter's recovery can trade many
/// small ERRORs for one giant ERROR that swallows a whole class: the damage
/// COUNT drops while the file's structure evaporates (abseil's
/// `raw_hash_set` did exactly this under a blanking round). A repair gate
/// that only compares damage adopts that trade; pairing it with "bodied
/// containers must not decrease" rejects it.
pub(super) fn structure_count(node: tree_sitter::Node) -> usize {
    let mut n = 0;
    let mut cur = node.walk();
    let mut stack = vec![node];
    while let Some(x) = stack.pop() {
        if matches!(
            x.kind(),
            "class_specifier" | "struct_specifier" | "union_specifier" | "enum_specifier"
                | "namespace_definition"
        ) && x.child_by_field_name("body").is_some()
        {
            n += 1;
        }
        for c in x.children(&mut cur) {
            stack.push(c);
        }
    }
    n
}

/// Length-preserving blanking of an UNRESOLVED declarator-position macro.
/// `class API_EXPORT Foo {` — an export macro from a GENERATED header (Qt's
/// `Q_CORE_EXPORT`, never in the source tree) the gather can't reach —
/// parses as a corrupt function and the class evaporates. A class/struct
/// head with TWO identifiers before its body has a macro in the first slot:
/// valid C++ names the type once (the exceptions — `class Name final`,
/// brace-init declarations, range-for bindings — are excluded below).
/// Blank the macro token with spaces (same length → every extracted span
/// stays put, no SpliceMap needed) so the class parses. Returns the
/// rewritten source plus the `(class_name, macro_token)` pairs it recovered
/// — the analyze path looks the token up in the attribute-macro manifest to
/// annotate the class with what the macro signals (`exported`/`deprecated`);
/// an unknown token still recovers the class, it just carries no signal.
///
/// The parse-damage gate can't police this repair: the misparse it fixes
/// (`class API_EXPORT Foo { … }` as a bogus function_definition) contains
/// ZERO error nodes, and so does the valid C++11 it must not touch
/// (`struct Point p {1, 2};`). Instead each candidate is gated on **type-
/// position context** from a parse of the untouched source: valid C++ spells
/// `struct ID1 ID2 ⟨head⟩` only when the head token opens a *value* or *loop*
/// construct (a brace initializer, a range-for binding) — a closed grammar
/// fact, so those (plus comment/string text, which is not code at all) are
/// skipped and everything else is the misparse this repair exists for.
fn strip_declarator_macros(
    parser: &mut tree_sitter::Parser,
    src: &str,
) -> (String, Vec<(String, String)>) {
    let bytes = src.as_bytes();
    // (macro span, name span, head-token byte) candidate sites, textually.
    let mut candidates: Vec<((usize, usize), (usize, usize), usize)> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let kwlen = if bytes[i..].starts_with(b"class") {
            5
        } else if bytes[i..].starts_with(b"struct") {
            6
        } else {
            i += 1;
            continue;
        };
        let word_boundary = (i == 0 || !is_ident_byte(bytes[i - 1]))
            && bytes.get(i + kwlen).is_some_and(|b| b.is_ascii_whitespace());
        if !word_boundary {
            i += kwlen;
            continue;
        }
        // IDENT1 (candidate macro), then IDENT2 (candidate name).
        let mut p = i + kwlen;
        let skip_ws = |p: &mut usize| while *p < bytes.len() && bytes[*p].is_ascii_whitespace() { *p += 1; };
        let read_id = |p: &mut usize| { let s = *p; while *p < bytes.len() && is_ident_byte(bytes[*p]) { *p += 1; } (s, *p) };
        skip_ws(&mut p);
        let (id1s, id1e) = read_id(&mut p);
        skip_ws(&mut p);
        let (id2s, id2e) = read_id(&mut p);
        skip_ws(&mut p);
        let head = p < bytes.len() && matches!(bytes[p], b'{' | b':' | b'<');
        if id1e > id1s && id2e > id2s && head {
            let id2 = &src[id2s..id2e];
            if id2 != "final" && id2 != "sealed" {
                candidates.push(((id1s, id1e), (id2s, id2e), p));
            }
        }
        i += kwlen;
    }
    if candidates.is_empty() {
        return (src.to_string(), Vec::new());
    }
    let tree = parser.parse(src, None);
    let valid_context = |head: usize| -> bool {
        let Some(t) = &tree else { return false };
        let Some(n) = t
            .root_node()
            .named_descendant_for_byte_range(head, head + 1)
        else {
            return false;
        };
        matches!(
            n.kind(),
            // `struct Point p {1, 2};` / `struct sockaddr_in addr {};`
            "initializer_list"
            // `for (struct Point p : v)`
            | "for_range_loop"
            // not code — blanking would mint phantom recovered pairs
            | "comment" | "string_literal" | "raw_string_literal"
            | "string_content" | "char_literal"
        )
    };
    let mut out = src.to_string();
    let mut recovered: Vec<(String, String)> = Vec::new();
    // SAFETY: only ASCII spaces are written, over ASCII identifier bytes —
    // length-preserving and UTF-8-valid.
    let ob = unsafe { out.as_bytes_mut() };
    for ((id1s, id1e), (id2s, id2e), head) in candidates {
        if valid_context(head) {
            continue;
        }
        recovered.push((src[id2s..id2e].to_string(), src[id1s..id1e].to_string()));
        for b in &mut ob[id1s..id1e] {
            *b = b' ';
        }
    }
    (out, recovered)
}

/// Expand seeded with EXTERNAL macros from `#include`d headers, then **let
/// the parser validate**: keep the transform only if it does not increase
/// parse damage. An expansion that helps (declarator macros, simple
/// declaration macros) lands; one that hurts (nested macro CALLS like
/// X-macros, `##` token-paste — the tail this single pass does not model)
/// is salvaged per-splice: the good expansions land, only the bad ones are
/// dropped (or blank-degraded), so one bad macro never discards a whole
/// file's recoveries.
pub fn preprocess_validated_with(
    parser: &mut tree_sitter::Parser,
    src: &str,
    external: &PreExpandedExternal,
) -> (String, SpliceMap, Vec<(String, String)>) {
    // Blank unresolved declarator-position macros first (length-preserving,
    // parse-context-gated — recovers `class Q_CORE_EXPORT Foo` even when the
    // macro is unreachable). Spans stay in original coordinates. `recovered`
    // carries each (class_name, macro_token) so the analyze path can annotate
    // the class with the macro's signal — surviving regardless of which return
    // arm fires below, since `src` is the stripped text throughout.
    let (stripped, recovered) = strip_declarator_macros(parser, src);
    // Blank UNRESOLVED structural macros (macro-before-`namespace`, macro
    // before a constructor) — expansion can't repair a token it has no
    // definition for; known names are left for the expansion below.
    let stripped = strip_unresolved_structural_macros(parser, &stripped, external);
    // Repair a conditional directive in DECLARATION position (a ctor-init `#if`)
    // that misparses the enclosing class — blank the directive lines so the
    // declaration parses, gated by the parser's damage/structure verdict.
    let stripped = strip_declaration_position_directives(parser, &stripped);
    let src = stripped.as_str();
    let Some(tree) = parser.parse(src, None) else {
        return (src.to_string(), SpliceMap::default(), recovered);
    };
    let before = parse_damage(tree.root_node());
    let structure = structure_count(tree.root_node());
    // First attempt: narrow exclusion — conditional-region BODIES are
    // expandable, so a macro use inside `#ifdef`/`#if`/`#else` expands
    // (perl5's `pTHX_` context-param convention). See `EXCLUDE_QUERY`.
    let (rewritten, map) = preprocess_with(&tree, src, external);
    if rewritten == src {
        return (rewritten, map, recovered);
    }
    if parser
        .parse(&rewritten, None)
        .is_some_and(|t| parse_damage(t.root_node()) <= before)
    {
        return (rewritten, map, recovered);
    }
    // The widened expansion RAISED damage. Re-exclude conditional-region bodies
    // (the pre-widening WIDE scope) and retry: a huge macro-heavy file
    // (perl.h/op.c) keeps its prior fast expansion and never pays the salvage
    // cliff for the widened scope — small clean files already validated above
    // and kept the win. `docs/adr/config-superposition-declarations.md` slice 1.
    let (rewritten, map) = preprocess_with_mode(&tree, src, external, false, false);
    if rewritten == src {
        return (rewritten, map, recovered);
    }
    match parser.parse(&rewritten, None) {
        Some(after) if parse_damage(after.root_node()) <= before => (rewritten, map, recovered),
        _ => {
            // The full rewrite raised damage — one bad expansion (an
            // unexpanded `##` call inside a namespace-open macro's body)
            // must not discard the file's GOOD expansions. Bisect the
            // splice set against the parser's own damage verdict, keeping
            // every subset that stays at-or-below the baseline; a rejected
            // splice degrades to a length-preserving blank when THAT
            // validates (leaving the raw token glues the next declaration
            // into garbage — the reason it was spliced at all).
            let full = compute_splices(&tree, src, external, false, false);
            let mut budget: u32 = SALVAGE_PARSE_BUDGET;
            let mut good =
                salvage_splices(parser, src, &full, (before, structure), &mut budget);
            if std::env::var_os("PERL_LSP_SALVAGE_DEBUG").is_some() {
                eprintln!(
                    "salvage-debug: before_damage={} splices={} kept={} (blanked={}) budget_left={}",
                    before,
                    full.len(),
                    good.len(),
                    good.iter().filter(|s| s.replacement.bytes().all(|b| b == b' ')).count(),
                    budget
                );
                let mut names: Vec<&str> = full
                    .iter()
                    .map(|s| s.name.as_str())
                    .filter(|n| !good.iter().any(|g| g.name == *n))
                    .collect();
                names.sort_unstable();
                names.dedup();
                eprintln!("salvage-debug: dropped-names={names:?}");
                let mut blanked: Vec<&str> = good
                    .iter()
                    .filter(|s| s.replacement.bytes().all(|b| b == b' '))
                    .map(|s| s.name.as_str())
                    .collect();
                blanked.sort_unstable();
                blanked.dedup();
                eprintln!("salvage-debug: blanked-names={blanked:?}");
                if let Ok(p) = std::env::var("PERL_LSP_SALVAGE_DUMP") {
                    let mut g = good.clone();
                    let (rw, _) = apply(src, &mut g);
                    let _ = std::fs::write(p, rw);
                }
            }
            if !good.is_empty() {
                let (rw, map) = apply(src, &mut good);
                if let Some(t) = parser.parse(&rw, None) {
                    if parse_damage(t.root_node()) <= before
                        && structure_count(t.root_node()) >= structure
                    {
                        return (rw, map, recovered);
                    }
                }
            }
            // Nothing salvageable splice-wise. Keep only the provably-safe
            // IDENTIFIER-ALIAS expansions (`op_prune_chain_head →
            // Perl_op_prune_chain_head`) so macro-name indirection —
            // goto-def + references THROUGH the alias — survives even when
            // the rest is discarded.
            let (alias_rw, alias_map) = preprocess_with_mode(&tree, src, external, true, false);
            match (alias_rw != src).then(|| parser.parse(&alias_rw, None)).flatten() {
                Some(a) if parse_damage(a.root_node()) <= before => (alias_rw, alias_map, recovered),
                _ => (src.to_string(), SpliceMap::default(), recovered),
            }
        }
    }
}

/// Reparse budget for the per-splice salvage: each `validates` probe is one
/// full parse of the file, so the bisection is bounded. Exhaustion degrades
/// to dropping the unprocessed subset — never to keeping an unvalidated one.
pub(super) const SALVAGE_PARSE_BUDGET: u32 = 48;

/// Bisect `splices` down to a subset whose application keeps parse damage
/// at or below `base`. Returns a VALIDATED subset or an empty vec — never an
/// unvalidated one (the damage-never-rises invariant holds by construction).
///
/// Bisection runs over per-MACRO-NAME groups, not individual splices: a
/// broken body (`##` token paste the single pass doesn't model) breaks
/// EVERY use of that macro, so the group is the natural validation unit —
/// and it keeps the reparse count O(names), not O(uses) (json.hpp: ~500
/// splices, a few dozen names). A rejected group is retried as
/// length-preserving BLANKS of its use tokens: for a statement-position
/// macro (the namespace-open idiom) the blank recovers the region even when
/// the expansion is broken; a blank that breaks an expression fails its own
/// validation and is dropped — the parser's verdict decides, never the
/// macro's shape. Paired open/close macros couple through the whole-file
/// validation: an END whose `}}` lands without its BEGIN raises damage,
/// fails, and degrades to blanks alongside it.
pub(super) fn salvage_splices(
    parser: &mut tree_sitter::Parser,
    src: &str,
    splices: &[Splice],
    base: (usize, usize),
    budget: &mut u32,
) -> Vec<Splice> {
    // Context-free-safe splices (empty-body byte-deletions — see
    // `is_context_free_safe`) are KEPT without a probe: their expansion can't
    // raise damage in any position, so the budget must not be spent bisecting
    // them (`docs/prompt-macro-salvage-scaling.md`, fix #1 — `pTHX_`/`aTHX_` used
    // across the whole file no longer cost anything). They double as the
    // always-applied BASELINE the ambiguous bisection validates against: since a
    // deletion only lowers damage, keeping them out can never raise a surviving
    // subset's damage, so the remaining groups keep at least as much as before.
    let (safe, ambiguous): (Vec<Splice>, Vec<Splice>) = splices
        .iter()
        .cloned()
        .partition(|s| s.replacement.chars().all(char::is_whitespace));
    let mut by_name: BTreeMap<&str, Vec<Splice>> = BTreeMap::new();
    for s in &ambiguous {
        by_name.entry(&s.name).or_default().push(s.clone());
    }
    let groups: Vec<Vec<Splice>> = by_name.into_values().collect();
    let mut kept = salvage_groups(parser, src, &groups, &safe, base, budget);
    kept.extend(safe);
    kept.sort_by_key(|s| s.start);
    kept
}

fn salvage_validates(
    parser: &mut tree_sitter::Parser,
    src: &str,
    keep_always: &[Splice],
    set: &[Splice],
    base: (usize, usize),
    budget: &mut u32,
) -> bool {
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    // `keep_always` (the context-free-safe deletions) is applied on every probe
    // so the ambiguous groups are judged in the same context they'll ship in.
    let mut v: Vec<Splice> = keep_always.iter().chain(set).cloned().collect();
    let (rw, _) = apply(src, &mut v);
    parser.parse(&rw, None).is_some_and(|t| {
        parse_damage(t.root_node()) <= base.0 && structure_count(t.root_node()) >= base.1
    })
}

fn salvage_groups(
    parser: &mut tree_sitter::Parser,
    src: &str,
    groups: &[Vec<Splice>],
    keep_always: &[Splice],
    base: (usize, usize),
    budget: &mut u32,
) -> Vec<Splice> {
    if groups.is_empty() {
        return Vec::new();
    }
    let all: Vec<Splice> = groups.iter().flatten().cloned().collect();
    if salvage_validates(parser, src, keep_always, &all, base, budget) {
        return all;
    }
    if groups.len() == 1 {
        // The group's expansions hurt — degrade to blanking its use tokens.
        let blanks: Vec<Splice> = all
            .iter()
            .map(|s| Splice {
                start: s.start,
                end: s.end,
                replacement: " ".repeat(s.end - s.start),
                name: s.name.clone(),
            })
            .collect();
        if salvage_validates(parser, src, keep_always, &blanks, base, budget) {
            return blanks;
        }
        return Vec::new();
    }
    let (l, r) = groups.split_at(groups.len() / 2);
    let lk = salvage_groups(parser, src, l, keep_always, base, budget);
    let rk = salvage_groups(parser, src, r, keep_always, base, budget);
    if lk.is_empty() {
        return rk;
    }
    if rk.is_empty() {
        return lk;
    }
    let mut keep = lk.clone();
    keep.extend(rk.iter().cloned());
    if salvage_validates(parser, src, keep_always, &keep, base, budget) {
        return keep;
    }
    // The halves validated separately but interact when combined — keep the
    // larger half, which validated on its own.
    if lk.len() >= rk.len() {
        lk
    } else {
        rk
    }
}

/// Blank (length-preserving) UNRESOLVED macro tokens in the two structural
/// positions expansion cannot repair because no definition exists:
///
///   * **before `namespace`** — `NS_BEGIN\nnamespace d {…}`: the macro token
///     absorbs the keyword (`function_definition` with an `identifier`
///     declarator spelled "namespace") and the whole block's symbols orphan.
///     The grammar's own verdict is the gate: a `namespace` KEYWORD can never
///     parse as an `identifier` node in valid C++ (`using namespace` parses
///     as a using_declaration), so an identifier node spelled "namespace"
///     proves the token before it is a macro.
///   * **before a constructor** — `ATTR_NOINLINE Widget(Widget&& w)…` inside
///     `class Widget`: a member function whose name equals its class can
///     never carry a return type, so the token in the type slot is a macro.
///     (With a ctor-initializer the misparse cascades — the init list becomes
///     a `bitfield_clause` and the rest of the class reparents wrong.)
///
/// KNOWN names (file-local or gathered `#define`s) are skipped — expansion
/// owns those, and blanking a namespace-OPEN macro whose END expands to `}}`
/// would break brace balance. Iterates to a small fixpoint (blanking one
/// macro can expose the next misconsumed `namespace`); each round's blanking
/// must not raise parse damage or it is reverted.
fn strip_unresolved_structural_macros(
    parser: &mut tree_sitter::Parser,
    src: &str,
    external: &PreExpandedExternal,
) -> String {
    let mut cur = src.to_string();
    for _ in 0..4 {
        let Some(tree) = parser.parse(&cur, None) else { return cur };
        let damage = parse_damage(tree.root_node());
        let structure = structure_count(tree.root_node());
        let local = collect_macros(&tree, cur.as_bytes());
        let known = |name: &str| local.contains_key(name) || external.raw.contains_key(name);
        let bytes = cur.as_bytes();
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        let mut stack = vec![tree.root_node()];
        let mut walk = tree.root_node().walk();
        while let Some(n) = stack.pop() {
            // An ERROR whose entire content is one bare identifier sitting
            // right AFTER a function_declarator — the post-declarator
            // attribute-macro position (`T m(...) ATTR { ... }`); no valid
            // C++ token can stand there, the parser's own verdict. The
            // sibling gate keeps this away from other single-identifier
            // ERRORs (a namespace/class NAME stranded inside a macro-glued
            // misparse must never be blanked).
            if n.is_error()
                && n.named_child_count() == 1
                && n.prev_named_sibling().is_some_and(|p| p.kind() == "function_declarator")
            {
                if let Some(c) = n.named_child(0) {
                    let txt = c.utf8_text(bytes).unwrap_or("");
                    if c.kind() == "identifier"
                        && n.utf8_text(bytes).map(str::trim) == Ok(txt)
                        && !is_reserved_keyword(txt)
                        && !known(txt)
                    {
                        ranges.push((c.start_byte(), c.end_byte()));
                    }
                }
            }
            match n.kind() {
                "identifier" if n.utf8_text(bytes) == Ok("namespace") => {
                    // The token before the misconsumed keyword: skip
                    // whitespace backward, read the identifier.
                    let mut e = n.start_byte();
                    while e > 0 && bytes[e - 1].is_ascii_whitespace() {
                        e -= 1;
                    }
                    let mut s = e;
                    while s > 0 && is_ident_byte(bytes[s - 1]) {
                        s -= 1;
                    }
                    if s < e && !known(&cur[s..e]) {
                        ranges.push((s, e));
                    }
                }
                "field_declaration" => {
                    let mac = n
                        .child_by_field_name("type")
                        .filter(|t| t.kind() == "type_identifier");
                    let leaf = n
                        .child_by_field_name("declarator")
                        .filter(|d| d.kind() == "function_declarator")
                        .and_then(|d| descend_declarator_name(d, bytes));
                    if let (Some(t), Some(leaf)) = (mac, leaf) {
                        let class = enclosing_aggregate_name(
                            tree.root_node(),
                            &cur,
                            n.start_byte(),
                        );
                        let tt = t.utf8_text(bytes).unwrap_or("");
                        if class.as_deref() == leaf.utf8_text(bytes).ok()
                            && class.as_deref() != Some(tt)
                            && !known(tt)
                        {
                            ranges.push((t.start_byte(), t.end_byte()));
                        }
                    }
                }
                _ => {}
            }
            for c in n.children(&mut walk) {
                stack.push(c);
            }
        }
        if ranges.is_empty() {
            return cur;
        }
        // Per-candidate adopt/revert: each blank must individually keep
        // damage from rising AND keep every bodied container (a blank that
        // trades three small ERRORs for one class-swallowing ERROR lowers
        // the damage COUNT while erasing the structure — reject it).
        let mut adopted = false;
        for (s, e) in ranges {
            let tentative = blank_ranges(&cur, std::iter::once((s, e)));
            let Some(t) = parser.parse(&tentative, None) else { continue };
            if parse_damage(t.root_node()) <= damage
                && structure_count(t.root_node()) >= structure
            {
                if std::env::var_os("PERL_LSP_SALVAGE_DEBUG").is_some() {
                    eprintln!("strip-debug: blanking {:?}", &cur[s..e]);
                }
                cur = tentative;
                adopted = true;
            }
        }
        if !adopted {
            return cur;
        }
    }
    cur
}

/// True when `line` opens with a conditional preprocessor directive
/// (`#if`/`#ifdef`/`#ifndef`/`#elif`/`#else`/`#endif` and the C23 `#elifdef`
/// spellings). Leading whitespace already stripped by the caller.
fn is_conditional_directive(line: &str) -> bool {
    let Some(rest) = line.strip_prefix('#') else { return false };
    let kw: String = rest.trim_start().chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    matches!(
        kw.as_str(),
        "if" | "ifdef" | "ifndef" | "elif" | "elifdef" | "elifndef" | "else" | "endif"
    )
}

/// The `(line_start, newline_exclusive_end)` range of every conditional
/// directive line whose START falls inside `[span_start, span_end)`. Ranges
/// stop before the `\n` so blanking them is newline-preserving (the arm bodies
/// keep their line structure).
fn conditional_directive_line_ranges(
    bytes: &[u8],
    span_start: usize,
    span_end: usize,
) -> Vec<(usize, usize)> {
    let n = bytes.len();
    let mut i = span_start.min(n);
    while i > 0 && bytes[i - 1] != b'\n' {
        i -= 1; // rewind to the start of span_start's physical line
    }
    let mut ranges = Vec::new();
    let end = span_end.min(n);
    while i < end {
        let ls = i;
        let mut le = i;
        while le < n && bytes[le] != b'\n' {
            le += 1;
        }
        let line = std::str::from_utf8(&bytes[ls..le]).unwrap_or("");
        if is_conditional_directive(line.trim_start()) {
            ranges.push((ls, le));
        }
        i = le + 1;
    }
    ranges
}

/// Repair a conditional preprocessor directive sitting in DECLARATION position
/// — inside a class / struct / union body — that misparses. The ctor-
/// initializer case (`Widget(...) \n #if X : a(), b() #endif { ... }`,
/// nlohmann json.hpp `JSON_DIAGNOSTIC_POSITIONS`): tree-sitter recovers the
/// `#if`-guarded init list as ERROR-wrapped bogus field declarations, minting
/// PHANTOM members (`a`, `b`) and corrupting hover on the real ones. Blanking
/// only the `#if`/`#elif`/`#else`/`#endif` LINES (arm bodies kept, newlines
/// preserved) lets the declaration parse. Config-variant navigation is
/// untouched — `collect_macro_defs` reparses the ORIGINAL source, not this
/// transform.
///
/// Gated exactly like the sibling structural strips: a candidate region is
/// adopted only when blanking it does NOT raise parse damage AND keeps the
/// bodied-structure floor (`structure_count`), so a true `#if`/`#else` twin
/// whose arms don't concatenate cleanly is left alone (its blank raises damage
/// or drops a container → reverted). Candidates are narrowed to preproc regions
/// that (a) misparse and (b) sit under a `field_declaration_list`, so healthy
/// conditionals and file-scope config regions are never touched.
/// `docs/adr/config-superposition-declarations.md` slice 1 (declaration-
/// position repair).
fn strip_declaration_position_directives(parser: &mut tree_sitter::Parser, src: &str) -> String {
    let mut cur = src.to_string();
    for _ in 0..4 {
        let Some(tree) = parser.parse(&cur, None) else { return cur };
        let damage = parse_damage(tree.root_node());
        if damage == 0 {
            return cur;
        }
        let structure = structure_count(tree.root_node());
        let bytes = cur.as_bytes();
        // Candidate directive-line sets: one per misparsing preproc region in
        // declaration position.
        let mut regions: Vec<Vec<(usize, usize)>> = Vec::new();
        let mut walk = tree.root_node().walk();
        let mut stack = vec![tree.root_node()];
        while let Some(n) = stack.pop() {
            for c in n.children(&mut walk) {
                stack.push(c);
            }
            if matches!(n.kind(), "preproc_if" | "preproc_ifdef")
                && parse_damage(n) > 0
                && node_has_field_list_ancestor(n)
            {
                let lines = conditional_directive_line_ranges(bytes, n.start_byte(), n.end_byte());
                if !lines.is_empty() {
                    regions.push(lines);
                }
            }
        }
        if regions.is_empty() {
            return cur;
        }
        // Per-region adopt/revert against the parser's own verdict, so one bad
        // region never discards another's repair.
        let mut adopted = false;
        for lines in regions {
            let tentative = blank_ranges(&cur, lines.into_iter());
            let Some(t) = parser.parse(&tentative, None) else { continue };
            if parse_damage(t.root_node()) <= damage && structure_count(t.root_node()) >= structure {
                cur = tentative;
                adopted = true;
            }
        }
        if !adopted {
            return cur;
        }
    }
    cur
}

/// Whether `n` has a `field_declaration_list` (class/struct/union body)
/// ancestor — the "declaration position" gate for the directive repair.
fn node_has_field_list_ancestor(n: tree_sitter::Node) -> bool {
    let mut p = n.parent();
    while let Some(node) = p {
        if node.kind() == "field_declaration_list" {
            return true;
        }
        p = node.parent();
    }
    false
}
