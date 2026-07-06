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

#[cfg(test)]
#[path = "surface_tests.rs"]
mod tests;
