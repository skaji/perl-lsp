//! The span-free cross-file Surface (`docs/prompt-storage-engine.md`).
//!
//! A position-independent projection of one file's cross-file-VISIBLE facts.
//! Equality of two Surfaces means "no cross-file-visible change": a body
//! edit, a reformat, a comment, a private-local rename must yield an EQUAL
//! Surface — that equality is the early-cutoff firewall the freshness engine
//! gates on (rebuild → Surface equal? → stop; else re-enrich exactly the
//! dirty consumers). One smuggled span collapses the firewall silently, so:
//!
//! - **No spans, no `Point`s, no byte offsets, no `ScopeId`/`SymbolId`/
//!   `RefIdx`, anywhere.** Every one of those shifts on unrelated edits.
//!   The equality tests are the regression net; a field addition without an
//!   equality test is a review reject (the prompt's R1).
//! - **Typed fields, not display strings** — `Option<InferredType>`, never
//!   `"returns Foo"` (rule #10's lossy-string form). File-internal
//!   attachment identities inside a type (a `CodeRef` body edge) are
//!   sanitized by `despan` below.
//! - **Canonical ordering.** Everything is sorted so builder iteration
//!   order can never masquerade as a semantic change.
//!
//! The Surface is NOT the outline: `documentSymbol` is span-bearing and
//! type-blind — riding it would both under-invalidate (return-type/body
//! `@ISA` edits keep the symbol list identical) and over-invalidate (every
//! sub moves on reformat). The Surface is the lower, position-independent
//! layer; the outline stays a span-bearing sibling.

use serde::{Deserialize, Serialize};

use crate::file_analysis::{
    FileAnalysis, HashKeyOwner, InferredType, ParametricType, SymKind,
};

/// One package/class/namespace's cross-file-visible facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PackageSurface {
    pub name: String,
    /// Resolved isa/roles/loaded components, post-fold (`package_parents`).
    pub parents: Vec<String>,
    /// Is this package a role (plugin-declared role-maker verdict)?
    pub is_role: bool,
    /// Cross-file-callable members, sorted by (name, kind).
    pub methods: Vec<MethodSurface>,
}

/// One callable's cross-file-visible contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MethodSurface {
    pub name: String,
    /// `SymKind` discriminant via `sym_kind_code` — a method vs sub vs
    /// handler distinction IS cross-file-visible (dispatch differs).
    pub kind: u8,
    /// Declared arity (total, required, variadic) when the language mints
    /// it — the overload-ranking axis.
    pub arity: Option<(usize, usize, bool)>,
    /// The bag-resolved return type, `despan`ned. Local conclusion only
    /// (no module index at projection time): a cross-file-dependent return
    /// that resolves to `None` here is honest — the consumer's enrichment
    /// re-asks with an index.
    pub ret: Option<InferredType>,
    /// Hash keys owned by this sub (`HashKeyOwner::Sub`) — the
    /// imported-hash-key completion surface.
    pub hash_keys: Vec<String>,
}

/// The whole file's span-free cross-file surface. `Default` is the empty
/// surface (a file exporting nothing).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Surface {
    pub packages: Vec<PackageSurface>,
    /// Modules this file loads (`use`/`require`/plugin loads) — the
    /// DEPENDENCY half of the freshness edge: this file's enrichment
    /// depends on the Surface of each import ∪ parent ∪ bridge.
    pub imports: Vec<String>,
    pub exports: Vec<String>,
    pub exports_ok: Vec<String>,
    pub reexports: Vec<String>,
    /// Classes plugin namespaces in THIS file bridge content onto.
    pub plugin_bridges: Vec<String>,
    /// Manifest-declared app-surface consumer classes.
    pub app_surface_consumers: Vec<String>,
}

impl Surface {
    /// Project `fa`'s surface. Runs right after `finalize_post_walk()` —
    /// the bag is present (return types resolve) and enrichment has NOT
    /// run (the surface is the file's OWN facts, never its imports').
    pub fn project(fa: &FileAnalysis) -> Surface {
        let mut by_pkg: std::collections::BTreeMap<String, PackageSurface> =
            std::collections::BTreeMap::new();
        // Every package with parents or role-ness exists on the surface
        // even if it declares no callable members.
        for (pkg, parents) in &fa.package_parents {
            let entry = by_pkg.entry(pkg.clone()).or_insert_with(|| PackageSurface {
                name: pkg.clone(),
                ..Default::default()
            });
            let mut parents = parents.clone();
            parents.sort_unstable();
            parents.dedup();
            entry.parents = parents;
        }
        for sym in &fa.symbols {
            match sym.kind {
                SymKind::Package | SymKind::Class | SymKind::Module => {
                    by_pkg.entry(sym.name.clone()).or_insert_with(|| PackageSurface {
                        name: sym.name.clone(),
                        ..Default::default()
                    });
                }
                SymKind::Sub | SymKind::Method | SymKind::Handler => {
                    let Some(pkg) = sym.package.clone() else { continue };
                    // Cross-file-visible only: lexical subs aren't
                    // addressable outside their block.
                    if matches!(
                        &sym.detail,
                        crate::file_analysis::SymbolDetail::Sub { lexical: true, .. }
                    ) {
                        continue;
                    }
                    let hash_keys: Vec<String> = {
                        let mut ks: Vec<String> = fa
                            .hash_key_defs_for_owner(&HashKeyOwner::Sub {
                                package: Some(pkg.clone()),
                                name: sym.name.clone(),
                            })
                            .iter()
                            .map(|s| s.name.clone())
                            .collect();
                        ks.sort_unstable();
                        ks.dedup();
                        ks
                    };
                    let entry =
                        by_pkg.entry(pkg.clone()).or_insert_with(|| PackageSurface {
                            name: pkg.clone(),
                            ..Default::default()
                        });
                    entry.methods.push(MethodSurface {
                        name: sym.name.clone(),
                        kind: crate::file_analysis::sym_kind_code(&sym.kind),
                        arity: sym
                            .param_arity()
                            .map(|a| (a.total, a.required, a.variadic)),
                        ret: fa
                            .symbol_return_type_via_bag(sym.id, None)
                            .map(|t| despan(&t)),
                        hash_keys,
                    });
                }
                _ => {}
            }
        }
        let mut packages: Vec<PackageSurface> = by_pkg.into_values().collect();
        for p in &mut packages {
            p.is_role = fa.is_role_package(&p.name);
            p.methods.sort_by(|a, b| (&a.name, a.kind).cmp(&(&b.name, b.kind)));
            // Duplicate symbols for one name (rw accessor pairs) surface
            // once — the FIRST after sort, matching sub_info_view's
            // primary-pick determinism closely enough for equality use.
            p.methods.dedup_by(|a, b| a.name == b.name && a.kind == b.kind);
        }
        let mut imports: Vec<String> = fa
            .imports
            .iter()
            .map(|i| i.module_name.clone())
            .chain(fa.plugin_loads.iter().map(|f| f.name.clone()))
            .collect();
        imports.sort_unstable();
        imports.dedup();
        let sorted = |v: &[String]| {
            let mut v = v.to_vec();
            v.sort_unstable();
            v.dedup();
            v
        };
        let mut plugin_bridges: Vec<String> = fa
            .plugin_namespaces
            .iter()
            .flat_map(|ns| {
                ns.bridges.iter().map(|b| {
                    let crate::file_analysis::Bridge::Class(c) = b;
                    c.clone()
                })
            })
            .collect();
        plugin_bridges.sort_unstable();
        plugin_bridges.dedup();
        Surface {
            packages,
            imports,
            exports: sorted(&fa.export),
            exports_ok: sorted(&fa.export_ok),
            reexports: sorted(&fa.reexport_modules),
            plugin_bridges,
            app_surface_consumers: sorted(&fa.app_surface_consumers),
        }
    }
}

/// Strip file-internal identities out of an `InferredType` so the surface
/// value is position-independent. The one offender is `CodeRef`'s
/// `return_edge`: an `Expr(span)` (or any other file-internal attachment)
/// shifts on unrelated edits AND is meaningless to another file — only the
/// `MethodOnClass` edge is both stable and cross-file-resolvable. Container
/// variants recurse.
fn despan(t: &InferredType) -> InferredType {
    use crate::witnesses::WitnessAttachment;
    match t {
        InferredType::CodeRef { return_edge } => InferredType::CodeRef {
            return_edge: match return_edge {
                Some(WitnessAttachment::MethodOnClass { .. }) => return_edge.clone(),
                _ => None,
            },
        },
        InferredType::Sequence(items) => {
            InferredType::Sequence(items.iter().map(despan).collect())
        }
        InferredType::TypeConstraintOf(inner) => {
            InferredType::TypeConstraintOf(Box::new(despan(inner)))
        }
        InferredType::Optional(inner) => InferredType::Optional(Box::new(despan(inner))),
        InferredType::HashWithKeys { keys, open } => InferredType::HashWithKeys {
            keys: keys
                .iter()
                .map(|(k, v)| (k.clone(), v.as_ref().map(|t| Box::new(despan(t)))))
                .collect(),
            open: *open,
        },
        InferredType::Parametric(p) => InferredType::Parametric(match p {
            ParametricType::ResultSet { .. } => p.clone(),
            ParametricType::Instance { base, args } => ParametricType::Instance {
                base: base.clone(),
                args: args.iter().map(despan).collect(),
            },
        }),
        other => other.clone(),
    }
}

/// The verdict `FreshnessIndex::record` hands back — what a rebuild of one
/// file means for everyone else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceVerdict {
    /// First sighting — no prior surface to compare (startup registration).
    FirstSeen,
    /// Surface equal: a body edit / reformat / comment. NOTHING cross-file
    /// changed — consumers stay fresh, the walk stops here.
    Unchanged,
    /// Cross-file-visible change — `dirty_consumers` names who must
    /// re-enrich.
    Changed,
}

/// The hand-rolled freshness engine (`docs/prompt-storage-engine.md`
/// phase 3, the eval's recommended first cut): per-file surface records +
/// a name-keyed reverse-dependency index. The dependency edge is DECLARED
/// by the consumer's own surface — file F depends on every name in its
/// imports ∪ parents ∪ bridges — and the dirty walk is provider-name →
/// consumers, transitive with a seen-set (C's change dirties B extends C,
/// which dirties A importing B, because A's enrichment reads through B).
#[derive(Default)]
pub struct FreshnessIndex {
    surfaces: dashmap::DashMap<std::path::PathBuf, Surface>,
    /// provider NAME (package/module) → consumer paths.
    consumers: dashmap::DashMap<String, std::collections::HashSet<std::path::PathBuf>>,
    /// consumer path → the provider names it last declared edges to
    /// (the removal half — edges must not accumulate across re-records).
    deps_of: dashmap::DashMap<std::path::PathBuf, Vec<String>>,
}

impl FreshnessIndex {
    /// Names `s` DEPENDS on: its imports, every package's parents, and the
    /// classes its plugins bridge onto.
    fn dep_names(s: &Surface) -> Vec<String> {
        let mut names: Vec<String> = s.imports.clone();
        for p in &s.packages {
            names.extend(p.parents.iter().cloned());
        }
        names.extend(s.plugin_bridges.iter().cloned());
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Names `s` PROVIDES: its declared packages (the keys consumers'
    /// edges point at — Perl imports/extends by package name).
    fn provided_names(s: &Surface) -> impl Iterator<Item = &str> {
        s.packages.iter().map(|p| p.name.as_str())
    }

    /// Record `path`'s freshly-built surface; maintain its outgoing edges;
    /// return what changed. Call with the WHOLE analysis's projection at
    /// registration/rebuild time.
    pub fn record(&self, path: &std::path::Path, surface: Surface) -> SurfaceVerdict {
        let verdict = match self.surfaces.get(path) {
            None => SurfaceVerdict::FirstSeen,
            Some(old) if *old == surface => SurfaceVerdict::Unchanged,
            Some(_) => SurfaceVerdict::Changed,
        };
        if verdict != SurfaceVerdict::Unchanged {
            let new_deps = Self::dep_names(&surface);
            let old_deps = self
                .deps_of
                .insert(path.to_path_buf(), new_deps.clone())
                .unwrap_or_default();
            for gone in old_deps.iter().filter(|d| !new_deps.contains(d)) {
                if let Some(mut set) = self.consumers.get_mut(gone) {
                    set.remove(path);
                }
            }
            for dep in &new_deps {
                self.consumers
                    .entry(dep.clone())
                    .or_default()
                    .insert(path.to_path_buf());
            }
            self.surfaces.insert(path.to_path_buf(), surface);
        }
        verdict
    }

    /// Drop a deleted file's record and edges.
    pub fn remove(&self, path: &std::path::Path) {
        self.surfaces.remove(path);
        if let Some((_, deps)) = self.deps_of.remove(path) {
            for d in deps {
                if let Some(mut set) = self.consumers.get_mut(&d) {
                    set.remove(path);
                }
            }
        }
    }

    /// The transitive dirty closure after `changed_path`'s surface changed:
    /// every file whose enrichment can observe it, walked provider-name →
    /// consumers with a seen-set (bounded, cycle-safe). The changed file
    /// itself is NOT in the set (its own rebuild triggered this).
    pub fn dirty_consumers(
        &self,
        changed_path: &std::path::Path,
    ) -> std::collections::HashSet<std::path::PathBuf> {
        let mut dirty: std::collections::HashSet<std::path::PathBuf> = Default::default();
        let mut frontier: Vec<String> = match self.surfaces.get(changed_path) {
            Some(s) => Self::provided_names(&s).map(str::to_owned).collect(),
            None => return dirty,
        };
        let mut seen_names: std::collections::HashSet<String> = frontier.iter().cloned().collect();
        while let Some(name) = frontier.pop() {
            let Some(consumers) = self.consumers.get(&name) else { continue };
            for c in consumers.iter() {
                if c == changed_path || !dirty.insert(c.clone()) {
                    continue;
                }
                // A dirty consumer's OWN providers propagate: its enriched
                // result feeds files that depend on IT.
                if let Some(s) = self.surfaces.get(c.as_path()) {
                    for p in Self::provided_names(&s) {
                        if seen_names.insert(p.to_owned()) {
                            frontier.push(p.to_owned());
                        }
                    }
                }
            }
        }
        dirty
    }
}

#[cfg(test)]
#[path = "surface_tests.rs"]
mod tests;
