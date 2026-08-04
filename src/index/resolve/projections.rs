//! CandidateSet builders, accessors, and the read-side projections:
//! `references()`, `renameable()`, `rename_edits()`, `implementations()`.
use super::*;

impl<'a> CandidateSet<'a> {
    /// Constrain every projection to `mask`. The one knob demonstrating the
    /// symmetry invariant: narrowing visibility here narrows references AND
    /// rename AND group walks together — no per-feature re-application. The
    /// seam future construction axes (closure visibility, language
    /// boundaries) ride; exercised by the invariant test and by
    /// `--heatmap`'s `--include-deps` scope knob.
    pub fn with_visibility(mut self, mask: RoleMask) -> Self {
        self.visibility_override = Some(mask);
        self.visibility = std::sync::OnceLock::new();
        self
    }

    /// Supply the origin document's raw text — unlocks the raw-word
    /// candidate lanes (macro variants in `definitions()`).
    pub fn with_source(mut self, source: &'a str) -> Self {
        self.source = Some(source);
        self
    }

    /// Per-language name semantics on the set's identity keying: normalize
    /// a typed NEW NAME to the bare identity token edits write. Perl names
    /// carry sigils (`conventions.rs` owns the rule); pack languages
    /// canonicalize spellings at extraction (the LangPack `shape_name`
    /// hook — cpp's `canonical_template_spelling` is that seam's cpp
    /// instance), so their typed names pass through bare. New per-language
    /// spelling rules plug in HERE, never inline in a projection.
    pub(super) fn bare_new_name<'n>(&self, typed: &'n str) -> &'n str {
        if self.pack {
            typed
        } else {
            crate::model::conventions::strip_variable_sigils(typed)
        }
    }

    /// The origin-scoped index — every forward resolution (identity,
    /// goto-def, implementations) reads through the closure scope built at
    /// construction. Backward walks take `self.module_index` (the base):
    /// `collect_from_analysis` re-scopes per scanned file.
    pub(super) fn idx(&self) -> Option<&dyn CrossFileLookup> {
        self.scoped
            .as_ref()
            .map(|s| s as &dyn CrossFileLookup)
    }

    /// What the cursor resolved to. Exposed for callers that need
    /// target-level policy questions (e.g. diagnostics asking a target's
    /// kind); projections below cover the feature verbs.
    pub fn resolution(&self) -> Option<&ResolvedTarget> {
        self.resolution
            .get_or_init(|| {
                let mut r =
                    resolve_symbol_scoped(self.origin, self.point, self.idx(), self.scope);
                // Pack routing: a plain function (Sub) target's visibility
                // identity is closure-keyed like every other pack target —
                // its def_paths are minted HERE, on the routing fact the
                // caller declared, because the Sub cursor shape itself is
                // language-neutral (a Perl `sub` mints the same RenameKind)
                // and Perl visibility is package-keyed, never closure-gated.
                if self.pack {
                    if let Some(ResolvedTarget::Target(t)) = &mut r {
                        if matches!(t.kind, TargetKind::Sub { .. }) && t.def_paths.is_empty() {
                            let origin_defines = self.origin.symbols.iter().any(|s| {
                                s.name == t.name
                                    && matches!(s.kind, SymKind::Sub | SymKind::Method)
                            });
                            t.def_paths = pack_def_paths(&t.name, origin_defines, self.idx());
                        }
                    }
                }
                r
            })
            .as_ref()
    }

    /// The set-level visibility for a `Target` resolution: the override when
    /// present; VISIBLE for pack routing (pack workspace files ride the
    /// DEPENDENCY role); else `references_mask_for`'s editable-vs-visible
    /// verdict.
    pub(super) fn target_visibility(&self, target: &TargetRef) -> RoleMask {
        *self.visibility.get_or_init(|| {
            self.visibility_override.unwrap_or_else(|| {
                if self.pack {
                    RoleMask::VISIBLE
                } else {
                    references_mask_for(self.files, self.module_index, target)
                }
            })
        })
    }

    /// The backward image of the set: every reference (declarations + use
    /// sites) across the visible universe. Lexical/unowned cursors answer
    /// from the origin file's in-file union.
    pub fn references(&self) -> Vec<RefLocation> {
        match self.resolution() {
            Some(ResolvedTarget::Target(t)) => {
                let mask = self.target_visibility(t);
                refs_to(self.files, self.module_index, t, mask)
            }
            Some(ResolvedTarget::Group { local_spans, pinned_spans, members }) => group_refs(
                self.files,
                self.module_index,
                &self.origin_key,
                local_spans,
                pinned_spans,
                members,
                self.visibility_override,
            ),
            Some(ResolvedTarget::Local) | None => self
                .origin
                .find_references(self.point, self.idx())
                .into_iter()
                .map(|span| RefLocation {
                    key: self.origin_key.clone(),
                    span,
                    access: AccessKind::Read,
                    rewritable: true,
                    label: None
                })
                .collect(),
        }
    }

    /// Whether rename at this cursor would produce edits — the prepareRename
    /// gate. Mirrors `rename_edits`' arms so the box is offered exactly where
    /// edits exist. Pack targets probe the real edit set: a set rename would
    /// refuse (alias-spelled sites) or no-op on must not offer a box.
    pub fn renameable(&self) -> bool {
        match self.resolution() {
            Some(ResolvedTarget::Target(t)) if t.supports_cross_file_rename() => {
                if self.pack {
                    self.rename_edits("x").is_ok_and(|e| !e.is_empty())
                } else {
                    true
                }
            }
            Some(ResolvedTarget::Group { .. }) => true,
            Some(_) => self
                .origin
                .rename_at(self.point, "x")
                .is_some_and(|e| !e.is_empty()),
            None => false,
        }
    }

    /// Rename = the references image + rewritability policy, with each span
    /// paired to ITS replacement text (bare vs re-derived affixed accessor
    /// names for groups). Policy lives on the set/locations, not in handlers:
    /// non-rewritable sites (const-folded names) are references but never
    /// edits, and the walk stops at editable space (for pack routing,
    /// "editable" includes the per-language cache).
    /// `Ok(empty)` = nothing renameable here; `Err` = a rename that would
    /// SILENTLY BREAK code — a pack set containing an alias-spelled site (a
    /// use through a delegating `#define`, `rewritable: false`) refuses: the
    /// macro's body isn't a collected span, so renaming the target would
    /// leave the delegation chain pointing at the old name. Perl's
    /// non-rewritable sites (variable-folded dispatch) keep their
    /// long-standing skip.
    pub fn rename_edits(&self, new_name: &str) -> Result<Vec<(RefLocation, String)>, String> {
        let editable = if self.pack {
            RoleMask::VISIBLE
        } else {
            self.visibility_override
                .map(|m| m & RoleMask::EDITABLE)
                .unwrap_or(RoleMask::EDITABLE)
        };
        Ok(match self.resolution() {
            Some(ResolvedTarget::Target(t)) if t.supports_cross_file_rename() => {
                let locations = refs_to(self.files, self.module_index, t, editable);
                if self.pack && locations.iter().any(|l| !l.rewritable) {
                    return Err(format!(
                        "rename of `{}` would leave sites spelled through a delegating macro \
                         unchanged (the macro body is not rewritten) — refusing rather than \
                         emitting a partial edit",
                        t.name
                    ));
                }
                locations
                    .into_iter()
                    .filter(|loc| loc.rewritable)
                    .map(|loc| (loc, new_name.to_string()))
                    .collect()
            }
            Some(ResolvedTarget::Group { local_spans, pinned_spans, members }) => {
                // Group spellings are bare name tokens; a sigil on the typed
                // name applies only to variable-shaped members' own rules.
                let bare_new = self.bare_new_name(new_name);
                group_rename_edits(
                    self.files,
                    self.module_index,
                    &self.origin_key,
                    local_spans,
                    pinned_spans,
                    members,
                    bare_new,
                    editable,
                )
            }
            // Lexical variables, unowned hash keys, non-cross-file targets:
            // the origin file's rename machinery owns the edit set.
            Some(_) => self
                .origin
                .rename_at(self.point, new_name)
                .unwrap_or_default()
                .into_iter()
                .map(|(span, text)| {
                    (
                        RefLocation {
                            key: self.origin_key.clone(),
                            span,
                            access: AccessKind::Read,
                            rewritable: true,
                            label: None
                        },
                        text,
                    )
                })
                .collect(),
            None => Vec::new(),
        })
    }

    /// The family/descendants walk over the set: every override/composer
    /// definition of a Method target, the specialization family of a
    /// template primary (Package targets), and — from an enum TYPE's own
    /// def — the reverse domain bridge: the field-slot sites whose recovered
    /// domain is that enum. The bridge is an implementations-style
    /// projection of the domain edge, deliberately NOT part of plain
    /// references (from an enumerator it fanned ~56 real references out to
    /// the field's ~950 sites).
    pub fn implementations(&self) -> Vec<RefLocation> {
        // Domain slot sites come off the cursor's own Class def, before
        // identity minting — the enum def resolves to a Package target whose
        // family walk is a different edge set.
        let mut out: Vec<RefLocation> = Vec::new();
        if let Some(idx) = self.module_index {
            if let Some(sym) = self.origin.symbol_at(self.point) {
                // Enums are `SymKind::Class` in cpp (no distinct kind), so gate
                // the enum→field-slot bridge on the Class actually HAVING
                // enumerators — otherwise a plain class fires it, and any field
                // member whose owning class shares the class's name resolves as
                // a bogus "enumerator of this enum" (leveldb `Iterator` matched
                // SkipList::Iterator's `node_` field). An empty/real class has
                // no enumerators → no domain sites key to it anyway, so this
                // never suppresses a genuine enum result.
                if matches!(sym.kind, SymKind::Class)
                    && !self.origin.enum_members(&sym.name, Some(idx)).is_empty()
                {
                    let enum_name = sym.name.clone();
                    idx.for_each_cached_file(&mut |cached| {
                        // `resolve_enumerator_enum`'s local arm reads the
                        // copy's own symbols — take the whole view.
                        for span in idx
                            .whole_present(cached)
                            .field_sites_for_enum(&enum_name, Some(idx))
                        {
                            out.push(RefLocation {
                                key: FileKey::Path(cached.path.clone()),
                                span,
                                access: AccessKind::Read,
                                rewritable: false,
                                label: None
                            });
                        }
                    });
                }
            }
        }
        if let Some(ResolvedTarget::Target(t)) = self.resolution() {
            out.extend(implementations_of(self.origin, self.idx(), t));
        }
        // Domain sites first (the bridge is the headline answer on an enum
        // def), then the family walk; first occurrence wins the dedup.
        let mut seen = std::collections::HashSet::new();
        out.retain(|l| seen.insert((key_for_sort(&l.key), l.span)));
        out
    }

    /// Hover projection: the top-ranked candidate of the forward walk — the
    /// SAME identity, visibility, and ranking `definitions()` computes, so
    /// hover and goto-def answer one resolution and can't disagree on what
    /// the cursor means (no hover dark where gd works, no
    /// bare-name hijack where gd is right). Presentation — markdown, kind
    /// labels, member drill-downs — is the adapter's
    /// (`symbols::pack_hover_markdown`); this returns WHAT to present.
    pub fn hover_candidate(&self) -> Option<RefLocation> {
        self.definitions().into_iter().next()
    }

    /// Read access for adapters presenting a projection (the hover renderer
    /// works from the same origin/point/scoped-index the set resolved with,
    /// so presentation lookups can't drift from resolution).
    pub fn origin_analysis(&self) -> &'a FileAnalysis {
        self.origin
    }
    pub fn origin_file_key(&self) -> &FileKey {
        &self.origin_key
    }
    pub fn cursor(&self) -> tree_sitter::Point {
        self.point
    }
    pub fn origin_source(&self) -> Option<&'a str> {
        self.source
    }
    /// The origin-scoped index — the closure-scoped view every forward
    /// resolution reads (`idx`), exposed so adapters query member types /
    /// config-variant leaves through the same visibility the set used.
    pub fn scoped_index(&self) -> Option<&dyn CrossFileLookup> {
        self.idx()
    }
}
