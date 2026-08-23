//! The flush over a WORLD: the overlay, the seed rule, and the atomic publish.
//!
//! `conclusion_flush_tests` covers the loop's own properties (cutoff,
//! termination, fan-in). What is tested here is everything the loop needs a
//! world for — which maps a round evaluates against, and what a converged
//! round writes.

use super::*;
use crate::model::file_analysis::InferredType;
use crate::model::witnesses::{Conclusion, ConclusionKey, ReceiverRule};
use std::collections::BTreeMap;

fn key(class: &str, name: &str) -> ConclusionKey {
    ConclusionKey::MethodOnClass { class: class.into(), name: name.into() }
}

/// A map holding one key, with `class` enumerated so absence elsewhere in it
/// reads as `NotLocal` rather than a proven `None`.
fn map_of(class: &str, name: &str, c: Conclusion) -> ConclusionMap {
    let mut m = HashMap::new();
    m.insert(key(class, name), c);
    let mut enumerated = HashSet::new();
    enumerated.insert(class.to_string());
    ConclusionMap(m, HashSet::new(), enumerated, HashMap::new())
}

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

/// The hop that only an overlay can make.
///
/// B's map holds a `Link` into A's class, so B's bytes say the same thing
/// before and after A changes — that is what makes the map useless as a change
/// signal. B's ANSWER moves only if the resolver B is evaluated against sees
/// A's fresh bake. Frozen-only resolution reproduces B's old surface exactly,
/// cuts, and starves C.
///
/// Base-verify by flipping `resolve`'s `overlay` argument to `false` in
/// `evaluate`: B then cuts, `evaluated` drops to 2, and C is never reached —
/// with every other assertion in the suite still passing.
#[test]
fn a_consumer_is_evaluated_against_the_seeds_fresh_bake_not_the_frozen_one() {
    let a_old = map_of("A", "m", Conclusion::Value(InferredType::ClassName("Old".into())));
    let a_new = map_of("A", "m", Conclusion::Value(InferredType::ClassName("New".into())));
    // B answers by walking to A. Byte-identical in both worlds, deliberately.
    let b = map_of(
        "B",
        "m",
        Conclusion::Link {
            targets: vec![key("A", "m")],
            arity: None,
            receiver: ReceiverRule::Thread,
        },
    );
    let c = map_of("C", "m", Conclusion::Value(InferredType::String));

    let frozen_src = |path: &Path| match path.to_string_lossy().as_ref() {
        "/A.pm" => Some(a_old.clone()),
        "/B.pm" => Some(b.clone()),
        "/C.pm" => Some(c.clone()),
        _ => None,
    };
    let re_bake = |path: &Path| match path.to_string_lossy().as_ref() {
        "/A.pm" => Some(a_new.clone()),
        "/B.pm" => Some(b.clone()),
        "/C.pm" => Some(c.clone()),
        _ => None,
    };
    let candidates_of = |class: &str| match class {
        "A" => vec![p("/A.pm")],
        "B" => vec![p("/B.pm")],
        "C" => vec![p("/C.pm")],
        _ => Vec::new(),
    };
    let consumers_of = |path: &Path| match path.to_string_lossy().as_ref() {
        "/A.pm" => vec![p("/B.pm")],
        "/B.pm" => vec![p("/C.pm")],
        _ => Vec::new(),
    };

    // Precondition, asserted rather than assumed: B's map really is unchanged.
    // Without this the test could pass for the wrong reason — B moving because
    // B's own bytes moved proves nothing about the overlay.
    assert_eq!(
        re_bake(Path::new("/B.pm")),
        frozen_src(Path::new("/B.pm")),
        "precondition: B's map is byte-identical across the change"
    );

    let world = FlushWorld::new(&frozen_src, &re_bake, &candidates_of);
    let (outcome, writes) = flush_over_world(&world, vec![p("/A.pm")], &consumers_of);

    assert!(!outcome.non_convergent);
    let moved: Vec<&PathBuf> = outcome.changed.iter().map(|(q, _)| q).collect();
    assert_eq!(
        moved,
        vec![&p("/A.pm"), &p("/B.pm")],
        "A moved, and B moved THROUGH it — B's own bytes never changed"
    );
    assert_eq!(outcome.evaluated, 3, "the wave reached C, which then cut");

    let written: Vec<&PathBuf> = writes.iter().map(|(q, _)| q).collect();
    assert_eq!(written, vec![&p("/A.pm"), &p("/B.pm")]);
}

/// A seed's own row is refreshed even when its surface did not move.
///
/// The surface is evaluated with no binders, so a change visible only under a
/// receiver or an arity is invisible to it. Cutting the seed on surface
/// equality would leave the store serving a map the file no longer has — the
/// cutoff governs propagation, not whether the file that changed is rewritten.
#[test]
fn a_seed_is_written_even_when_its_surface_did_not_move() {
    let same = map_of("A", "m", Conclusion::Value(InferredType::String));
    let frozen_src = |_: &Path| Some(same.clone());
    let re_bake = |_: &Path| Some(same.clone());
    let candidates_of = |_: &str| vec![p("/A.pm")];
    let reached = std::cell::RefCell::new(Vec::<PathBuf>::new());
    let consumers_of = |path: &Path| {
        reached.borrow_mut().push(path.to_path_buf());
        vec![p("/B.pm")]
    };

    let world = FlushWorld::new(&frozen_src, &re_bake, &candidates_of);
    let (outcome, writes) = flush_over_world(&world, vec![p("/A.pm")], &consumers_of);

    assert!(outcome.changed.is_empty(), "the surface did not move");
    assert_eq!(outcome.evaluated, 1);
    assert!(
        reached.borrow().is_empty(),
        "a cut file's consumers are never even enumerated — the reverse-dep \
         walk is part of what the cutoff saves"
    );
    assert_eq!(
        writes.iter().map(|(q, _)| q.clone()).collect::<Vec<_>>(),
        vec![p("/A.pm")],
        "the seed is written regardless — its blob changed"
    );
}

/// A seed whose blob cannot be decoded writes nothing and propagates nothing.
///
/// Publishing an empty map for it would be worse than leaving the old row:
/// absence in a map is read as a proven `None`, so an "empty because we could
/// not look" map answers `None` to every key the file used to conclude.
#[test]
fn an_unbakeable_seed_writes_nothing() {
    let frozen_src = |_: &Path| None;
    let re_bake = |_: &Path| None;
    let candidates_of = |_: &str| Vec::new();
    let consumers_of = |_: &Path| vec![p("/B.pm")];

    let world = FlushWorld::new(&frozen_src, &re_bake, &candidates_of);
    let (outcome, writes) = flush_over_world(&world, vec![p("/A.pm")], &consumers_of);
    assert_eq!(outcome.evaluated, 1);
    assert!(outcome.changed.is_empty());
    assert!(writes.is_empty(), "nothing decodable, nothing to publish");
}

/// A file is re-EVALUATED per round but re-BAKED once per flush.
///
/// The bake is a pure function of the file's own blob; only the evaluation
/// depends on the round. Z is reached on two DIFFERENT rounds here — not the
/// same-round fan-in `a_fan_in_evaluates_each_file_once_per_round` covers,
/// which the round's dedup handles and which would pass with no memo at all.
#[test]
fn a_file_reached_on_two_rounds_is_baked_once() {
    let bakes = std::cell::RefCell::new(BTreeMap::<String, usize>::new());
    let seed = map_of("A", "m", Conclusion::Value(InferredType::ClassName("New".into())));
    let old = map_of("A", "m", Conclusion::Value(InferredType::ClassName("Old".into())));
    let sink = map_of("Z", "m", Conclusion::Value(InferredType::String));

    // Only A has a frozen map. Everything downstream is new to the store, so
    // its first visit moves and its second cuts — which is what puts Z on two
    // rounds without contriving a value that oscillates.
    let frozen_src = |path: &Path| match path.to_string_lossy().as_ref() {
        "/A.pm" => Some(old.clone()),
        _ => None,
    };
    let re_bake = |path: &Path| {
        *bakes
            .borrow_mut()
            .entry(path.to_string_lossy().into_owned())
            .or_default() += 1;
        match path.to_string_lossy().as_ref() {
            "/A.pm" => Some(seed.clone()),
            _ => Some(sink.clone()),
        }
    };
    let candidates_of = |_: &str| Vec::new();
    // A→B→Z is two hops; A→C→D→Z is three. Z therefore arrives on round 3 and
    // again on round 4.
    let consumers_of = |path: &Path| match path.to_string_lossy().as_ref() {
        "/A.pm" => vec![p("/B.pm"), p("/C.pm")],
        "/B.pm" => vec![p("/Z.pm")],
        "/C.pm" => vec![p("/D.pm")],
        "/D.pm" => vec![p("/Z.pm")],
        _ => Vec::new(),
    };

    let world = FlushWorld::new(&frozen_src, &re_bake, &candidates_of);
    let (outcome, _) = flush_over_world(&world, vec![p("/A.pm")], &consumers_of);
    assert!(!outcome.non_convergent);
    assert_eq!(outcome.rounds, 4);
    assert_eq!(
        outcome.evaluated, 6,
        "A, B, C, Z, D, Z — Z is evaluated on both rounds it is reached on"
    );
    assert_eq!(
        bakes.borrow().get("/Z.pm").copied(),
        Some(1),
        "two evaluations, one bake"
    );
}

// ---- against a real store ----

fn store_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    crate::index::module_cache::init_schema(&conn).unwrap();
    conn
}

/// A converged flush advances the generation by exactly one, and a reader
/// still pinned to the old one keeps reading the old world.
///
/// The retention half is the one that fails silently: rows are keyed
/// `(path, generation)` precisely so publishing N+1 does not REPLACE the gen-N
/// row. Under path-keying a pinned reader finds nothing, and absence in this
/// layer means a proven `None` — the pin would have become a way to get wrong
/// answers instead of a way to avoid them.
#[test]
fn a_published_flush_advances_one_generation_and_leaves_the_old_one_readable() {
    use crate::index::module_cache::{load_conclusions, publish_generation, Generation};
    let conn = store_db();
    let path = "/store/A.pm";

    // A deliberate gen-1 world, written directly rather than through a
    // persist: the persist path bakes its own conclusions, which would make
    // the baseline whatever the bake happens to produce instead of a value
    // this test controls.
    let before = map_of("A", "m", Conclusion::Value(InferredType::ClassName("Old".into())));
    let after = map_of("A", "m", Conclusion::Value(InferredType::ClassName("New".into())));
    publish_generation(&conn, Generation(1), &[(path.to_string(), before.clone())])
        .expect("baseline");

    let re_bake = |_: &Path| Some(after.clone());
    let frozen_src =
        |q: &Path| load_conclusions(&conn, &q.to_string_lossy(), Generation(1));
    let candidates_of = |_: &str| vec![p(path)];
    let consumers_of = |_: &Path| Vec::new();

    let world = FlushWorld::new(&frozen_src, &re_bake, &candidates_of);
    let (_, writes) = flush_over_world(&world, vec![p(path)], &consumers_of);
    let entries: Vec<(String, ConclusionMap)> = writes
        .iter()
        .map(|(q, m)| (q.to_string_lossy().into_owned(), (**m).clone()))
        .collect();
    publish_generation(&conn, Generation(2), &entries).expect("publish");

    assert_eq!(
        crate::index::module_cache::current_generation(&conn),
        Generation(2)
    );
    assert_eq!(
        load_conclusions(&conn, path, Generation(2)),
        Some(after),
        "a fresh reader gets the flush's map"
    );
    assert_eq!(
        load_conclusions(&conn, path, Generation(1)),
        Some(before),
        "a reader pinned to gen 1 still reads gen 1 — the pin is the whole \
         point of freezing a generation for the duration of a consult"
    );
}

/// `flush_to_store` end to end: a real blob in, a real conclusion row out at
/// the next generation.
#[test]
fn flush_to_store_republishes_a_real_blob() {
    use crate::index::module_cache::{current_generation, load_conclusions, save_to_db};
    let conn = store_db();
    let dir = std::env::temp_dir();
    let pm = dir.join("perl_lsp_flush_store_A.pm");
    std::fs::write(
        &pm,
        "package FlushStoreA;\nsub val { return 'x' }\n1;\n",
    )
    .unwrap();
    let source = std::fs::read_to_string(&pm).unwrap();

    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&ts_parser_perl::LANGUAGE.into()).unwrap();
    let tree = parser.parse(&source, None).unwrap();
    let fa = crate::build::builder::build(&tree, source.as_bytes());
    let cached = std::sync::Arc::new(crate::index::module_index::CachedModule::new(
        pm.clone(),
        std::sync::Arc::new(fa),
    ));
    assert!(save_to_db(&conn, &pm.to_string_lossy(), &Some(cached), "workspace"));

    let before = current_generation(&conn);
    let report = flush_to_store(&conn, vec![pm.clone()], &|_| Vec::new(), &|_| Vec::new());
    assert_eq!(report.published, 1, "the seed is always written");
    assert_eq!(report.generation, Generation(before.0 + 1));
    assert_eq!(current_generation(&conn), report.generation);

    let published = load_conclusions(&conn, &pm.to_string_lossy(), report.generation)
        .expect("the flush wrote a map at the new generation");
    assert!(
        !published.is_empty(),
        "a re-bake WITH the bag concludes something; an empty map here would \
         mean the decode dropped the witnesses and the flush published the \
         emptiness over a good row"
    );

    let _ = std::fs::remove_file(&pm);
}
