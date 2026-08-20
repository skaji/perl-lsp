//! The ADR's projection contracts, held against EVERY cursor of a real
//! corpus instead of hand-picked ones (`docs/adr/resolution-candidate-set.md`).
//!
//! Four of this round's bugs were two sibling code paths disagreeing about
//! one question, each defensible alone — the defect only exists in the
//! comparison. The set's projections are the one place such pairs are a
//! CONTRACT rather than a hunch: the ADR promises relations between them,
//! so comparing projections against each other needs no oracle. A cursor
//! where the relations break is a bug by the ADR's own words, found on a
//! schedule instead of by accident.
//!
//! Checked per cursor:
//!   I1  highlights() == references() ∩ origin file (span sets — the two
//!       sides ride DIFFERENT walk entries, `refs_to_in_file` vs `refs_to`,
//!       exactly the sibling-pair shape)
//!   I2  linked_editing_spans() ⊆ highlights() spans (the co-edit subset)
//!   I3  rename_edits() ⊆ references() image (edits outside it are
//!       "unrepresentable" — hold the word to it), when renameable()
//!   I4  references() names a Declaration ⇒ definitions() is non-empty
//!       (the decl-axis backstop: gd cannot come up empty where gr names
//!       a declaration)

use super::*;
use refs_tests::parse;

fn corpus_files() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_files");
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                // Perl corpus only — the pack lanes get their own instance
                // once this net proves cheap enough.
                if p.file_name().is_some_and(|n| n == "cpp" || n == "python") {
                    continue;
                }
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "pl" || x == "pm") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Per-file cursor budget. A cap is a coverage decision, so it is LOUD:
/// the test names how many cursors each capped file dropped in its
/// pass-line, never silently.
const CURSORS_PER_FILE: usize = 250;

#[test]
fn projection_contracts_hold_at_every_corpus_cursor() {
    let files = corpus_files();
    assert!(files.len() >= 20, "corpus went missing: {} files", files.len());

    let store = FileStore::new();
    // Production wiring: the workspace bulk index feeds both the FileStore
    // AND the module index, so the forward lanes (method resolution,
    // package->file) see the same universe the backward walk does. A
    // store-only harness flags gd-empty asymmetries production never has.
    let idx = crate::index::module_index::ModuleIndex::new_for_test();
    let mut analyses: Vec<(PathBuf, std::sync::Arc<FileAnalysis>)> = Vec::new();
    for p in &files {
        let src = std::fs::read_to_string(p).unwrap();
        let fa = std::sync::Arc::new(parse(&src));
        store.insert_workspace_arc(p.clone(), fa.clone());
        idx.register_workspace_module(p.clone(), fa.clone());
        analyses.push((p.clone(), fa));
    }

    let mut violations: Vec<String> = Vec::new();
    let mut cursors = 0usize;
    let mut dropped = 0usize;

    for (path, fa) in &analyses {
        let refs = fa.refs();
        let stride = refs.len().div_ceil(CURSORS_PER_FILE).max(1);
        dropped += refs.len() - refs.len().div_ceil(stride);
        for r in refs.iter().step_by(stride) {
            cursors += 1;
            let key = FileKey::Path(path.clone());
            let cs = resolve(
                &store,
                fa,
                key.clone(),
                r.span.start,
                Some(&idx),
                OverrideScope::default(),
            );

            let refs_img = cs.references();
            let highlights = cs.highlights();

            // I1: highlights == references ∩ origin, as span SETS.
            let mut gr_origin: Vec<Span> = refs_img
                .iter()
                .filter(|l| file_key_eq(&l.key, &key))
                .map(|l| l.span)
                .collect();
            let mut hl: Vec<Span> = highlights.iter().map(|l| l.span).collect();
            gr_origin.sort_by_key(|s| (s.start.row, s.start.column, s.end.row, s.end.column));
            gr_origin.dedup();
            hl.sort_by_key(|s| (s.start.row, s.start.column, s.end.row, s.end.column));
            hl.dedup();
            if gr_origin != hl {
                violations.push(format!(
                    "I1 {}:{}:{} `{}` — highlights ({}) != references∩origin ({}): hl={:?} gr={:?}",
                    path.display(), r.span.start.row, r.span.start.column, r.target_name,
                    hl.len(), gr_origin.len(), hl, gr_origin,
                ));
            }

            // I2: linked editing spans ⊆ highlight spans.
            for s in cs.linked_editing_spans() {
                if !hl.contains(&s) {
                    violations.push(format!(
                        "I2 {}:{}:{} `{}` — linked-editing span {:?} not in highlights",
                        path.display(), r.span.start.row, r.span.start.column,
                        r.target_name, s,
                    ));
                }
            }

            // I3: rename edits ⊆ references image. CONTAINMENT, not span
            // equality: a variable rename edits the bare name inside the
            // sigil'd reference span, so the edit is a sub-span of its
            // reference site — outside ANY reference span is the violation.
            if cs.renameable() {
                if let Ok(edits) = cs.rename_edits("zzz_consistency_probe") {
                    for (loc, _text) in &edits {
                        let in_refs = refs_img.iter().any(|l| {
                            file_key_eq(&l.key, &loc.key)
                                && (l.span.start.row, l.span.start.column)
                                    <= (loc.span.start.row, loc.span.start.column)
                                && (loc.span.end.row, loc.span.end.column)
                                    <= (l.span.end.row, l.span.end.column)
                        });
                        if !in_refs {
                            violations.push(format!(
                                "I3 {}:{}:{} `{}` — rename edit at {:?} {:?} outside references",
                                path.display(), r.span.start.row, r.span.start.column,
                                r.target_name, loc.key, loc.span,
                            ));
                        }
                    }
                }
            }

            // I4: gr names a declaration ⇒ gd answers.
            if refs_img.iter().any(|l| l.access == AccessKind::Declaration)
                && cs.definitions().is_empty()
            {
                violations.push(format!(
                    "I4 {}:{}:{} `{}` — references name a Declaration but definitions() is empty",
                    path.display(), r.span.start.row, r.span.start.column, r.target_name,
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "{} projection-contract violations over {} cursors ({} capped out) — first 20:\n{}",
        violations.len(),
        cursors,
        dropped,
        violations.iter().take(20).cloned().collect::<Vec<_>>().join("\n"),
    );
    eprintln!(
        "projection contracts held at {cursors} cursors across {} files ({dropped} capped out)",
        files.len(),
    );
}

/// The pack instance of the same net — #141 and #142 (goto-def vs
/// references vs implementations disagreeing) both lived in this lane.
/// Corpus: the real cpp fixture trees, analyzed with their real paths so
/// include closures gather from disk; analyses register into a cpp
/// sub-index attached to a hub, the production topology (`lookup_for`
/// routes pack asks to the sub-index — pack symbols never live in the
/// Perl FileStore workspace map).
#[cfg(feature = "cpp")]
#[test]
fn projection_contracts_hold_at_every_pack_cursor() {
    use std::sync::Arc;
    let reg = crate::build::language_driver::LanguageRegistry::with_enabled();
    let driver = reg.for_id("cpp").expect("cpp driver");

    let mut files: Vec<PathBuf> = Vec::new();
    for root in ["gold-corpus/cpp-fixture", "test_files/cpp"] {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(root);
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| {
                x == "c" || x == "cpp" || x == "cc" || x == "h" || x == "hpp"
            }) {
                files.push(p);
            }
        }
    }
    files.sort();
    assert!(files.len() >= 40, "cpp corpus went missing: {} files", files.len());

    let store = FileStore::new();
    let hub = crate::index::module_index::ModuleIndex::new_for_test();
    let sub = Arc::new(crate::index::module_index::ModuleIndex::new_for_test());
    let mut analyses: Vec<(PathBuf, std::sync::Arc<FileAnalysis>)> = Vec::new();
    for p in &files {
        let Ok(src) = std::fs::read_to_string(p) else { continue };
        let fa = driver.analyze_with_path(&src, Some(p));
        // The bulk-index registration shape: parts carry the edge-index
        // FEED (extracted pre-strip) — `register_symbols` alone publishes
        // no name records, and the backward walk enumerates candidates
        // from them, so a feed-less harness sees empty references.
        let parts = crate::index::module_index::ModuleIndex::prepare_pack_parts(
            fa,
            crate::model::file_analysis::Residency::Whole,
        );
        let arc = std::sync::Arc::clone(parts.arc());
        sub.register_symbols_inner(p.clone(), parts);
        analyses.push((p.clone(), arc));
    }
    hub.attach_pack_index("cpp", Arc::clone(&sub));

    let mut violations: Vec<String> = Vec::new();
    let mut cursors = 0usize;
    let mut dropped = 0usize;

    for (path, fa) in &analyses {
        // Stage the origin the way every production query does
        // (`ScopedWorkspaceEntry` in the CLI, the open doc in the LSP):
        // the backward walk reads the queried file from the store.
        store.insert_workspace_arc(path.clone(), fa.clone());
        let refs = fa.refs();
        let stride = refs.len().div_ceil(CURSORS_PER_FILE).max(1);
        dropped += refs.len() - refs.len().div_ceil(stride);
        for r in refs.iter().step_by(stride) {
            cursors += 1;
            let key = FileKey::Path(path.clone());
            let cs = resolve(
                &store,
                fa,
                key.clone(),
                r.span.start,
                Some(&hub),
                OverrideScope::default(),
            );

            let refs_img = cs.references();
            let highlights = cs.highlights();

            let mut gr_origin: Vec<Span> = refs_img
                .iter()
                .filter(|l| file_key_eq(&l.key, &key))
                .map(|l| l.span)
                .collect();
            let mut hl: Vec<Span> = highlights.iter().map(|l| l.span).collect();
            gr_origin.sort_by_key(|s| (s.start.row, s.start.column, s.end.row, s.end.column));
            gr_origin.dedup();
            hl.sort_by_key(|s| (s.start.row, s.start.column, s.end.row, s.end.column));
            hl.dedup();
            if gr_origin != hl {
                violations.push(format!(
                    "I1 {}:{}:{} `{}` — highlights ({}) != references∩origin ({}): hl={:?} gr={:?}",
                    path.display(), r.span.start.row, r.span.start.column, r.target_name,
                    hl.len(), gr_origin.len(), hl, gr_origin,
                ));
            }

            for s in cs.linked_editing_spans() {
                if !hl.contains(&s) {
                    violations.push(format!(
                        "I2 {}:{}:{} `{}` — linked-editing span {:?} not in highlights",
                        path.display(), r.span.start.row, r.span.start.column,
                        r.target_name, s,
                    ));
                }
            }

            if cs.renameable() {
                if let Ok(edits) = cs.rename_edits("zzz_consistency_probe") {
                    for (loc, _text) in &edits {
                        let in_refs = refs_img.iter().any(|l| {
                            file_key_eq(&l.key, &loc.key)
                                && (l.span.start.row, l.span.start.column)
                                    <= (loc.span.start.row, loc.span.start.column)
                                && (loc.span.end.row, loc.span.end.column)
                                    <= (l.span.end.row, l.span.end.column)
                        });
                        if !in_refs {
                            violations.push(format!(
                                "I3 {}:{}:{} `{}` — rename edit at {:?} {:?} outside references",
                                path.display(), r.span.start.row, r.span.start.column,
                                r.target_name, loc.key, loc.span,
                            ));
                        }
                    }
                }
            }

            if refs_img.iter().any(|l| l.access == AccessKind::Declaration)
                && cs.definitions().is_empty()
            {
                violations.push(format!(
                    "I4 {}:{}:{} `{}` — references name a Declaration but definitions() is empty",
                    path.display(), r.span.start.row, r.span.start.column, r.target_name,
                ));
            }
        }
        store.remove_workspace(path);
    }

    assert!(
        violations.is_empty(),
        "{} pack projection-contract violations over {} cursors ({} capped out) — first 20:\n{}",
        violations.len(),
        cursors,
        dropped,
        violations.iter().take(20).cloned().collect::<Vec<_>>().join("\n"),
    );
    eprintln!(
        "pack projection contracts held at {cursors} cursors across {} files ({dropped} capped out)",
        analyses.len(),
    );
}
