//! The metaprogram-projection ENGINE — ONE fixpoint worklist for every
//! "declared generator projected over witnesses" domain. Two producers
//! ride it today with different substitution domains:
//!
//!   * Perl plugin symbol generators (`perl_generators.rs`) — args are
//!     literal STRINGS, outputs are eagerly-minted symbols (one call
//!     site = one finite group);
//!   * C++ template monomorphization (`cpp_templates.rs`) — args are
//!     TYPES (`TypeArg`), outputs are resolved instantiations for a
//!     whole-project consumer (per-QUERY typing stays lazy on the
//!     `ReturnExpr::ParamOf` reducer seam and never runs this engine —
//!     one template × every spelling is combinatorial, so nothing
//!     pre-materializes per-instantiation member copies).
//!
//! The engine owns only the SPINE — the worklist, the seen-set keyed
//! `(name, args)`, and root-chained provenance (a transitively-generated
//! witness carries the ROOT's `P`, so goto-def/rename land on the call
//! that started the cascade). Everything domain-shaped — what a
//! definition is, how args substitute, what an output looks like, and
//! the EMISSION POLICY — lives in the caller's `step` closure.
//!
//! Seen-set GRANULARITY is a policy the call boundary expresses, not a
//! knob: one `project_fixpoint` call = one seen-set. A per-call-site
//! policy (PR #100's `generators::project` — two identical call sites
//! still mint two groups, each with its own provenance) runs the engine
//! once per root; a whole-program policy (template monomorphization —
//! one `Box<int>` no matter how many spellings witness it) passes every
//! seed in one call. PR #100 re-extracts onto this seam by making its
//! `project(defs, root)` a one-root `project_fixpoint` whose step
//! interpolates `${param}` templates — the engine bodies are already
//! line-for-line the same discipline.

use std::collections::HashSet;
use std::hash::Hash;

/// A witnessed generator/template use: `name` names the definition,
/// `args` are the concrete substitution arguments (the domain decides
/// their type), `prov` is the provenance payload chained onto every
/// transitively-queued witness.
#[derive(Debug, Clone)]
pub struct ProjWitness<A, P> {
    pub name: String,
    pub args: A,
    pub prov: P,
}

/// Project witnesses to a fixpoint. For each not-yet-seen `(name, args)`
/// the `step` closure runs once: it may emit outputs and enqueue nested
/// witnesses (name + args only — the engine attaches the CURRENT
/// witness's provenance, chaining every descendant to the root). The
/// seen-set bounds recursive generators/templates: we never execute the
/// metaprogram, so we never diverge.
pub fn project_fixpoint<A, P, O>(
    roots: Vec<ProjWitness<A, P>>,
    mut step: impl FnMut(&ProjWitness<A, P>, &mut Vec<O>, &mut Vec<(String, A)>),
) -> Vec<O>
where
    A: Clone + Eq + Hash,
    P: Clone,
{
    let mut seen: HashSet<(String, A)> = HashSet::new();
    let mut queue = roots;
    let mut out = Vec::new();
    let mut nested: Vec<(String, A)> = Vec::new();
    while let Some(w) = queue.pop() {
        if !seen.insert((w.name.clone(), w.args.clone())) {
            continue;
        }
        step(&w, &mut out, &mut nested);
        for (name, args) in nested.drain(..) {
            // provenance chains to the root call site
            queue.push(ProjWitness { name, args, prov: w.prov.clone() });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provenance_chains_to_root_and_seen_set_bounds_recursion() {
        // A self-requeueing generator: emits one output per distinct arg,
        // queues itself with the same arg (bounded) and once with a new
        // arg (chained provenance).
        let roots = vec![ProjWitness { name: "g".to_string(), args: "a".to_string(), prov: 7u32 }];
        let out: Vec<(String, u32)> = project_fixpoint(roots, |w, out, nested| {
            out.push((w.args.clone(), w.prov));
            nested.push(("g".to_string(), w.args.clone())); // recursion — dropped by seen
            if w.args == "a" {
                nested.push(("g".to_string(), "b".to_string()));
            }
        });
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(out.contains(&("a".to_string(), 7)));
        assert!(
            out.contains(&("b".to_string(), 7)),
            "transitive witness carries the ROOT provenance: {out:?}"
        );
    }
}
