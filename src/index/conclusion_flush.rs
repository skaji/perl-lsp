//! The flush driver's worklist: propagate a change until the answers stop
//! moving, then publish one generation.
//!
//! `docs/prompt-enrichment-alternatives.md` §3c′/§3c″ owns the design. What
//! lives here is the loop and its two hard properties — the cutoff and
//! termination — factored so both are testable without a store, a thread, or a
//! corpus.
//!
//! **The diff artifact is the EVALUATED surface, never the persisted map.** A
//! map is index-free by construction, so when C changes B's map is
//! byte-identical while B's answers have moved; cutting on map equality stops
//! the wave at B and starves B's consumers. That is the whole soundness of the
//! cutoff, it passes every two-file fixture either way, and only a chain can
//! tell the two apart — see
//! `conclusions_tests::a_chain_needs_the_evaluated_surface_not_the_map`.

use crate::index::module_cache;
use crate::index::module_cache::Generation;
use crate::model::witnesses::{ConclusionMap, EvaluatedSurface};
use rusqlite::Connection;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Rounds a single flush may take before it is declared non-convergent.
///
/// The all-builds safety net, same role as `MAX_FOLD_ITERATIONS` one tier
/// down: convergence is a property of the lattice, not of this number, and a
/// flush that reaches the cap has found a bug rather than a deep chain. Real
/// dependency chains in a workspace are single digits.
pub const MAX_FLUSH_ROUNDS: usize = 32;

/// What one flush did. Returned rather than logged so a caller — and a test —
/// can assert on the shape of the propagation, not just its result.
#[derive(Debug, Default, PartialEq)]
pub struct FlushOutcome {
    /// Files whose evaluated surface moved, with the surface that will be
    /// published. A file evaluated and found unchanged is deliberately absent:
    /// the cutoff is the point.
    pub changed: Vec<(PathBuf, EvaluatedSurface)>,
    /// How many worklist rounds ran. `1` means the frontier cut immediately.
    pub rounds: usize,
    /// Files evaluated, including those that cut. The propagation's real cost.
    pub evaluated: usize,
    /// The round cap fired: the surfaces never stopped moving. The flush is
    /// abandoned rather than published — a half-propagated generation is worse
    /// than none, because a consult pinned to it would compose answers from a
    /// wave that never finished.
    pub non_convergent: bool,
}

/// Run one flush to quiescence.
///
/// `evaluate` re-bakes a file and evaluates its surface against the FROZEN
/// generation; `baseline` is that file's surface as of the same generation;
/// `consumers_of` is the freshness reverse-dep walk.
///
/// The cutoff compares against the surface recorded EARLIER IN THIS FLUSH when
/// there is one, and against the baseline otherwise. That distinction is what
/// makes a cycle terminate: on the second visit A is compared to A-as-just-
/// recorded, so an unchanged re-derivation cuts instead of re-enqueuing B
/// forever. Comparing against the baseline every time would make any cycle run
/// until the cap.
pub fn run_flush(
    dirty: impl IntoIterator<Item = PathBuf>,
    evaluate: &dyn Fn(&Path) -> Option<EvaluatedSurface>,
    baseline: &dyn Fn(&Path) -> Option<EvaluatedSurface>,
    consumers_of: &dyn Fn(&Path) -> Vec<PathBuf>,
) -> FlushOutcome {
    let mut recorded: HashMap<PathBuf, EvaluatedSurface> = HashMap::new();
    let mut frontier: Vec<PathBuf> = dirty.into_iter().collect();
    let mut out = FlushOutcome::default();

    while !frontier.is_empty() {
        out.rounds += 1;
        if out.rounds > MAX_FLUSH_ROUNDS {
            crate::util::ghost_stats::count("flush.non_convergent");
            log::error!(
                "conclusion flush did not converge in {MAX_FLUSH_ROUNDS} rounds; \
                 abandoning rather than publishing a half-propagated generation"
            );
            out.non_convergent = true;
            return out;
        }
        let round = std::mem::take(&mut frontier);
        // Deduped per round: a file reached by three consumers in one round is
        // one re-bake, not three. Without this a wide fan-in multiplies the
        // round's cost by its width for no additional information.
        let mut seen_this_round: HashSet<PathBuf> = HashSet::new();
        for path in round {
            if !seen_this_round.insert(path.clone()) {
                continue;
            }
            out.evaluated += 1;
            let Some(surface) = evaluate(&path) else {
                // Cannot evaluate — a file that vanished, or one whose map is
                // gone. Cutting here is right: we have nothing to say about it
                // and inventing a change would propagate noise.
                crate::util::ghost_stats::count("flush.unevaluable");
                continue;
            };
            let prior = recorded.get(&path).cloned().or_else(|| baseline(&path));
            if prior.as_ref() == Some(&surface) {
                crate::util::ghost_stats::count("flush.cut");
                continue;
            }
            crate::util::ghost_stats::count("flush.moved");
            recorded.insert(path.clone(), surface);
            frontier.extend(consumers_of(&path));
        }
    }

    out.changed = recorded.into_iter().collect();
    // Sorted for the same reason the surface itself is: the caller publishes
    // this and compares it, and a `HashMap` drain order would make two equal
    // flushes look different.
    out.changed.sort_by(|a, b| a.0.cmp(&b.0));
    out
}


/// The world one flush evaluates against: a frozen generation underneath, the
/// maps this flush has already re-baked on top.
///
/// The overlay is the whole reason a wave moves past its first hop. B's map is
/// index-free, so re-baking B after A changed yields BYTE-IDENTICAL bytes; the
/// only thing that moved is what B's `Link`s chase THROUGH. Evaluating B
/// against the frozen store would therefore reproduce B's frozen surface
/// exactly, cut, and starve B's consumers — the map-equality failure
/// `EvaluatedSurface` exists to avoid, arriving through the resolver instead
/// of through the diff.
///
/// Its two map sources are closures rather than a `Connection` for the same
/// reason `follow_link_with` takes a resolver: the overlay is delicate, and a
/// world that can only be built from a store is a world only exercised by
/// whatever a corpus happens to contain.
struct FlushWorld<'a> {
    frozen_src: &'a dyn Fn(&Path) -> Option<ConclusionMap>,
    re_bake: &'a dyn Fn(&Path) -> Option<ConclusionMap>,
    candidates_of: &'a dyn Fn(&str) -> Vec<PathBuf>,
    frozen: RefCell<HashMap<PathBuf, Option<Arc<ConclusionMap>>>>,
    fresh: RefCell<HashMap<PathBuf, Option<Arc<ConclusionMap>>>>,
}

impl<'a> FlushWorld<'a> {
    fn new(
        frozen_src: &'a dyn Fn(&Path) -> Option<ConclusionMap>,
        re_bake: &'a dyn Fn(&Path) -> Option<ConclusionMap>,
        candidates_of: &'a dyn Fn(&str) -> Vec<PathBuf>,
    ) -> Self {
        FlushWorld {
            frozen_src,
            re_bake,
            candidates_of,
            frozen: RefCell::new(HashMap::new()),
            fresh: RefCell::new(HashMap::new()),
        }
    }

    fn frozen_map(&self, path: &Path) -> Option<Arc<ConclusionMap>> {
        if let Some(hit) = self.frozen.borrow().get(path) {
            return hit.clone();
        }
        let loaded = (self.frozen_src)(path).map(Arc::new);
        self.frozen.borrow_mut().insert(path.to_path_buf(), loaded.clone());
        loaded
    }

    /// This flush's bake of a file, memoized.
    ///
    /// Memoized because the bake is a pure function of the file's own blob —
    /// nothing about it depends on the round, so a file revisited by a cycle
    /// or a fan-in re-EVALUATES (which is the point) but never re-BAKES.
    fn fresh_map(&self, path: &Path) -> Option<Arc<ConclusionMap>> {
        if let Some(hit) = self.fresh.borrow().get(path) {
            return hit.clone();
        }
        let baked = (self.re_bake)(path).map(Arc::new);
        self.fresh.borrow_mut().insert(path.to_path_buf(), baked.clone());
        baked
    }

    fn resolve(&self, class: &str, overlay: bool) -> Vec<(String, Option<Arc<ConclusionMap>>)> {
        (self.candidates_of)(class)
            .into_iter()
            .map(|p| {
                // Overlay reads only what this flush ALREADY baked — it never
                // bakes on demand. Baking a candidate here would pull the
                // whole reachable graph into a flush seeded by one file, and
                // the pull would be invisible: every candidate of every class
                // any evaluated key mentions, transitively.
                let map = if overlay {
                    self.fresh.borrow().get(p.as_path()).cloned().flatten()
                } else {
                    None
                };
                let map = map.or_else(|| self.frozen_map(&p));
                (p.to_string_lossy().into_owned(), map)
            })
            .collect()
    }

    fn evaluate(&self, path: &Path) -> Option<EvaluatedSurface> {
        let map = self.fresh_map(path)?;
        Some(map.evaluated_surface(&|class| self.resolve(class, true)))
    }

    /// The file's surface as of the frozen generation — the thing "moved" is
    /// measured against. Evaluated WITHOUT the overlay on purpose: it is the
    /// answer the world gave before this flush started.
    fn baseline(&self, path: &Path) -> Option<EvaluatedSurface> {
        let map = self.frozen_map(path)?;
        Some(map.evaluated_surface(&|class| self.resolve(class, false)))
    }
}

/// Propagate over a world and hand back the maps that must be written.
///
/// Two publication rules, and they answer different questions:
///
/// * A file the propagation found MOVED is written because its consumers'
///   answers depend on it.
/// * A SEED is written whether or not its surface moved, because its own blob
///   changed. The surface is evaluated with no binders, so a change visible
///   only under a receiver or an arity is invisible to it — cutting a seed on
///   surface equality would leave the store serving a map its file no longer
///   has. The cutoff governs PROPAGATION, never whether the file that changed
///   gets its own row refreshed.
fn flush_over_world(
    world: &FlushWorld<'_>,
    dirty: Vec<PathBuf>,
    consumers_of: &dyn Fn(&Path) -> Vec<PathBuf>,
) -> (FlushOutcome, Vec<(PathBuf, Arc<ConclusionMap>)>) {
    let seeds = dirty.clone();
    let outcome = run_flush(
        dirty,
        &|p| world.evaluate(p),
        &|p| world.baseline(p),
        consumers_of,
    );
    if outcome.non_convergent {
        return (outcome, Vec::new());
    }
    let mut paths: Vec<PathBuf> = outcome.changed.iter().map(|(p, _)| p.clone()).collect();
    paths.extend(seeds);
    paths.sort();
    paths.dedup();
    let writes = paths
        .into_iter()
        .filter_map(|p| world.fresh_map(&p).map(|m| (p, m)))
        .collect();
    (outcome, writes)
}

/// What one store-backed flush did.
#[derive(Debug)]
pub struct FlushReport {
    pub outcome: FlushOutcome,
    /// Files whose map landed in the new generation.
    pub published: usize,
    /// The generation a reader should pin AFTER this flush. Unchanged from
    /// the frozen one when nothing was published — including on a
    /// non-convergent flush, which publishes nothing by construction.
    pub generation: Generation,
}

/// Run one flush against the store and publish the result as one generation.
///
/// `dirty` is the frontier — the files whose blobs just changed.
/// `consumers_of` is the freshness reverse-dep walk (`dirty_consumers`);
/// `candidates_of` maps a class to the files that declare it, the same
/// relation the live `follow_link` resolves through.
///
/// The generation is read ONCE and frozen for the whole flush: a round that
/// re-read it could compose answers from two worlds, which is the failure the
/// pin exists to prevent.
pub fn flush_to_store(
    conn: &Connection,
    dirty: Vec<PathBuf>,
    consumers_of: &dyn Fn(&Path) -> Vec<PathBuf>,
    candidates_of: &dyn Fn(&str) -> Vec<PathBuf>,
) -> FlushReport {
    let at = module_cache::current_generation(conn);
    let frozen_src = |path: &Path| {
        module_cache::load_conclusions(conn, &path.to_string_lossy(), at)
    };
    let re_bake = |path: &Path| {
        // WITH the bag, for the reason the repair path takes it: a bagless
        // decode bakes a map that concludes nothing while looking like a
        // successful re-bake, and the flush would then publish that emptiness
        // over a good map.
        let fa = module_cache::load_one_diag(conn, &path.to_string_lossy(), true).ok()?;
        Some(module_cache::bake_conclusion_map(&fa, &fa.witnesses))
    };
    let world = FlushWorld::new(&frozen_src, &re_bake, candidates_of);
    let (outcome, writes) = flush_over_world(&world, dirty, consumers_of);
    if writes.is_empty() {
        // Nothing to say. Advancing the generation anyway would retire every
        // reader's pin for no new information and leave a generation whose
        // rows are all inherited from the one below it.
        return FlushReport { outcome, published: 0, generation: at };
    }

    let entries: Vec<(String, ConclusionMap)> = writes
        .iter()
        .map(|(p, m)| (p.to_string_lossy().into_owned(), (**m).clone()))
        .collect();
    let next = Generation(at.0 + 1);
    match module_cache::publish_generation(conn, next, &entries) {
        Ok(()) => {
            crate::util::ghost_stats::count_by("flush.published", entries.len() as u64);
            FlushReport { outcome, published: entries.len(), generation: next }
        }
        Err(e) => {
            // The publish is one transaction, so a failure left gen N intact.
            // Reporting the OLD generation is what keeps that true for the
            // caller: a reader pinned to it reads a complete world.
            log::warn!("conclusion flush: publish failed, generation {} stands: {e}", at.0);
            crate::util::ghost_stats::count("flush.publish_failed");
            FlushReport { outcome, published: 0, generation: at }
        }
    }
}

#[cfg(test)]
#[path = "conclusion_flush_tests.rs"]
mod conclusion_flush_tests;

#[cfg(test)]
#[path = "conclusion_flush_store_tests.rs"]
mod conclusion_flush_store_tests;
