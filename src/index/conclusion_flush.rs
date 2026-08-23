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

use crate::model::witnesses::EvaluatedSurface;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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

#[cfg(test)]
#[path = "conclusion_flush_tests.rs"]
mod conclusion_flush_tests;
