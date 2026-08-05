//! Backend construction, the `PackHealCtx` single-flight gather heal,
//! debounced pack rebuilds, pack invalidation, and diagnostics publishing.

use super::*;

impl PackHealCtx {
    /// Single-flight gather request. If a gather loop is already running for
    /// `uri`, coalesces into it (no new task); otherwise registers the URI and
    /// spawns the loop. Never awaits a gather — the change path stays
    /// cached-only + fire-and-forget.
    pub(super) fn request_gather(&self, uri: Url) {
        if !self.gather_reg.request(&uri) {
            return; // a loop already owns this URI; the request coalesced in
        }
        let ctx = self.clone();
        tokio::spawn(async move {
            ctx.run_gather_loop(uri).await;
        });
    }

    /// One gather owner per URI: gather → (maybe) re-run once if the buffer
    /// moved mid-gather → retire. When the loop retires it clears the degraded
    /// window and ends the provisional-diagnostics progress — i.e. progress
    /// ends exactly when full-quality diagnostics have published.
    async fn run_gather_loop(self, uri: Url) {
        loop {
            self.run_gather_once(&uri).await;
            if !self.gather_reg.finish(&uri) {
                break;
            }
        }
        self.clear_degraded(&uri).await;
    }

    /// Announce the degraded window: begin a work-done progress that says the
    /// gather is warming and diagnostics are provisional. Idempotent per
    /// window — the token is reserved once and reused across keystrokes (no
    /// spam), and released by `clear_degraded`/close. Capability-gated: a no-op
    /// when the client never advertised `window/workDoneProgress`.
    pub(super) async fn begin_progress(&self, uri: &Url, language: &str) {
        if !self.work_done.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        static DEGRADED_TOKEN: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(0);
        let token = NumberOrString::String(format!(
            "perl-lsp/degraded-{}",
            DEGRADED_TOKEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        // Reserve the slot atomically so two concurrent begins for the same
        // URI mint exactly one token.
        if !reserve_degraded_token(&self.degraded_progress, uri, token.clone()) {
            return; // this window already announced itself; reuse the token
        }
        let title = format!("{language} index warming — diagnostics are provisional");
        progress_create_and_begin(&self.client, &token, &title).await;
    }

    /// End the degraded window's progress if one is live (removes the token —
    /// bounded, one End per Begin).
    async fn end_progress(&self, uri: &Url) {
        if let Some((_, token)) = self.degraded_progress.remove(uri) {
            progress_end(&self.client, token).await;
        }
    }

    /// Clear the degraded-open mark, wake `await_open_full` waiters, and end
    /// the provisional-diagnostics progress. The window is over.
    async fn clear_degraded(&self, uri: &Url) {
        if let Some((_, n)) = self.degraded_open.remove(uri) {
            n.notify_waiters();
        }
        self.end_progress(uri).await;
    }

    /// One cross-file gather + full-quality re-analyze + re-publish for an open
    /// pack document. Cold gather allowed (this task has cached-only OFF).
    /// Does NOT clear the degraded window or spawn a successor — the enclosing
    /// `run_gather_loop` owns retirement. A stale-text result is dropped
    /// (no clobber); the loop's `finish` decides whether to re-run.
    async fn run_gather_once(&self, uri: &Url) {
        let Some((text, path, language)) = self
            .files
            .get_open(uri)
            .filter(|d| d.language != "perl")
            .map(|d| (d.text.clone(), d.path.clone(), d.language))
        else {
            return;
        };
        let snapshot = text.clone();
        // Full analyze on a blocking thread so the ~1.5 s gather never stalls
        // the executor.
        let analysis = tokio::task::spawn_blocking(move || {
            crate::build::language_driver::LanguageRegistry::with_enabled()
                .for_id(language)
                .map(|d| d.analyze_with_path(&text, path.as_deref()))
        })
        .await
        .ok()
        .flatten();
        let Some(analysis) = analysis else {
            return;
        };
        // A keystroke may have landed while we gathered; the debounced rebuild
        // owns the newer text, so don't clobber it with this stale build (the
        // loop re-runs against the latest text — the gather cache stays warm
        // for unchanged included files, so the re-run is cheap).
        if self
            .files
            .get_open(uri)
            .map(|d| d.text != snapshot)
            .unwrap_or(true)
        {
            return;
        }
        for imp in &analysis.imports {
            self.module_index.request_resolve(&imp.module_name);
        }
        for parents in analysis.package_parents.values() {
            for parent in parents {
                self.module_index.request_resolve(parent);
            }
        }
        if let Some(mut doc) = self.files.get_open_mut(uri) {
            doc.apply_rebuilt(analysis);
        }
        let diags = self
            .files
            .get_open(uri)
            .map(|doc| symbols::pack_diagnostics(&doc.analysis, self.options));
        if let Some(diags) = diags {
            self.client
                .publish_diagnostics(uri.clone(), diags, None)
                .await;
        }
    }
}

impl Backend {
    /// Build the shared context a background pack-gather heal runs with.
    pub(super) fn pack_heal_ctx(&self) -> PackHealCtx {
        PackHealCtx {
            files: Arc::clone(&self.files),
            module_index: Arc::clone(&self.module_index),
            client: self.client.clone(),
            options: self.diagnostic_options(),
            degraded_open: Arc::clone(&self.degraded_open),
            degraded_progress: Arc::clone(&self.degraded_progress),
            gather_reg: Arc::clone(&self.gather_reg),
            work_done: Arc::clone(&self.work_done_progress),
        }
    }

    pub fn new(client: Client) -> Self {
        let files: Arc<FileStore> = Arc::new(FileStore::new());

        // We need Arc<ModuleIndex> so the refresh callback can access it.
        // Two-phase init: create ModuleIndex whose refresh callback references
        // a later-set Arc<ModuleIndex>, then wire up the Arc.
        let diag_options = Arc::new(std::sync::Mutex::new(symbols::DiagnosticOptions::default()));

        let refresh_client = client.clone();
        let refresh_files = Arc::clone(&files);
        let refresh_diag_options = Arc::clone(&diag_options);

        let module_index_holder: Arc<std::sync::OnceLock<Arc<ModuleIndex>>> =
            Arc::new(std::sync::OnceLock::new());
        let holder_clone = Arc::clone(&module_index_holder);

        // Coalesce generation for the per-module refresh storm: each resolved
        // module fires `on_refresh` (~33 in ~400ms opening a Perl file with a
        // dozen `use`s), each otherwise a full all-open re-enrich + publish —
        // CPU + stdout pressure that WIDENS the cold-open degraded window. Every
        // fire bumps this generation and debounces; only the latest surviving
        // fire republishes, so the burst collapses to ~one refresh. Lives only
        // in the closure — nothing outside bumps it.
        let refresh_gen_cb = Arc::new(std::sync::atomic::AtomicU64::new(0));

        // Capture the tokio handle so the callback can spawn async work
        // from the resolver thread (which has no tokio context).
        let tokio_handle = tokio::runtime::Handle::current();
        let on_refresh = move || {
            use std::sync::atomic::Ordering;
            let client = refresh_client.clone();
            let files = Arc::clone(&refresh_files);
            let holder = Arc::clone(&holder_clone);
            let diag_options = Arc::clone(&refresh_diag_options);
            let refresh_gen = Arc::clone(&refresh_gen_cb);
            // Debounce: bump the generation, then only the LATEST fire that
            // survives the settle window does the work. A tight resolver burst
            // (~45 modules in ~400ms) thus republishes once, not 45×.
            let my_gen = refresh_gen.fetch_add(1, Ordering::Relaxed) + 1;
            log::debug!("diag-refresh fired (gen {})", my_gen);
            tokio_handle.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                if refresh_gen.load(Ordering::Relaxed) != my_gen {
                    return; // a newer fire superseded this one
                }
                let module_index = match holder.get() {
                    Some(idx) => idx,
                    None => return,
                };
                log::debug!("diag-refresh executing (gen {})", my_gen);
                // Derive (uri, diagnostics) first without holding the store lock
                // across the await — publishing is async and could deadlock.
                let options = *diag_options.lock().unwrap();
                let pending =
                    refresh_open_diagnostics(&files, module_index, options, OpenDocScope::All);
                for (uri, diags) in pending {
                    client.publish_diagnostics(uri, diags, None).await;
                }
            });
        };

        let module_index = Arc::new(ModuleIndex::new(client.clone(), on_refresh));
        let _ = module_index_holder.set(Arc::clone(&module_index));

        Backend {
            module_index,
            client,
            files,
            change_gen: Arc::new(dashmap::DashMap::new()),
            perl_indexed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            pack_indexed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            work_done_progress: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            pack_invalidator: Arc::new(crate::index::pack_invalidator::PackInvalidator::default()),
            diag_options,
            rename_options: Arc::new(std::sync::Mutex::new(crate::index::resolve::RenameOptions::default())),
            index_ready: Arc::new(IndexReady::default()),
            cold_wait_ms: Arc::new(std::sync::atomic::AtomicU64::new(DEFAULT_COLD_WAIT_MS)),
            max_cache_mb: Arc::new(std::sync::atomic::AtomicU64::new(max_cache_mb_default())),
            opening: Arc::new(dashmap::DashMap::new()),
            degraded_open: Arc::new(dashmap::DashMap::new()),
            degraded_progress: Arc::new(dashmap::DashMap::new()),
            gather_reg: Arc::new(GatherRegistry::default()),
        }
    }

    /// After a debounce, rebuild the pack analysis for `uri` OFF the document
    /// lock (snapshot text → `spawn_blocking` build → write back) + publish
    /// diagnostics — but only while `generation` is still the latest edit, so
    /// a burst of keystrokes collapses to ONE rebuild after typing settles.
    pub(super) fn spawn_debounced_rebuild(&self, uri: Url, generation: u64) {
        let files = Arc::clone(&self.files);
        let module_index = Arc::clone(&self.module_index);
        let client = self.client.clone();
        let change_gen = Arc::clone(&self.change_gen);
        let options = self.diagnostic_options();
        let degraded_open = Arc::clone(&self.degraded_open);
        let heal_ctx = self.pack_heal_ctx();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            let is_latest = || change_gen.get(&uri).map(|v| *v) == Some(generation);
            if !is_latest() {
                return;
            }
            // Snapshot the latest text off the lock; build on a blocking
            // thread so the ~0.7s analysis never stalls completion/hover.
            let Some((text, path, language)) = files
                .get_open(&uri)
                .map(|d| (d.text.clone(), d.path.clone(), d.language))
            else {
                return;
            };
            // A pack file's cross-file GATHER is cold on the first change after
            // a cold open (did_open's gather bails once the text changes, so it
            // can't warm us). Paying the ~24 s cold gather HERE would make the
            // first keystroke's diagnostics land 24 s late. Run CACHED-ONLY for
            // fast, degraded diagnostics — same as did_open — then heal via a
            // background gather refresh below. The flag is a thread-local no-op
            // for perl. See docs/open-forks.md.
            let analysis = tokio::task::spawn_blocking(move || {
                crate::build::cpp_reparse::set_gather_cached_only(true);
                let a = crate::build::language_driver::LanguageRegistry::with_enabled()
                    .for_id(language)
                    .map(|d| d.analyze_with_path(&text, path.as_deref()));
                crate::build::cpp_reparse::set_gather_cached_only(false);
                a
            })
            .await
            .ok()
            .flatten();
            let Some(analysis) = analysis else {
                return;
            };
            if !is_latest() {
                return; // a newer keystroke superseded this build
            }
            for imp in &analysis.imports {
                module_index.request_resolve(&imp.module_name);
            }
            for parents in analysis.package_parents.values() {
                for parent in parents {
                    module_index.request_resolve(parent);
                }
            }
            if let Some(mut doc) = files.get_open_mut(&uri) {
                doc.apply_rebuilt(analysis);
            }
            let diags = files
                .get_open(&uri)
                .map(|doc| symbols::pack_diagnostics(&doc.analysis, options));
            if let Some(diags) = diags {
                client.publish_diagnostics(uri.clone(), diags, None).await;
            }
            // Heal: warm the cross-file gather off this task and re-publish
            // full-quality diagnostics when it lands. The cached-only rebuild
            // just re-opened the degraded window for cross-file verbs; mark it
            // (so `await_open_full` holds Complete verbs until the heal lands),
            // announce it via progress (Part 1), then route the heal through
            // the single-flight registry (Part 2) so a typing burst coalesces
            // into ONE gather instead of abandoning one per keystroke. Perl has
            // no gather and is skipped.
            if language != "perl" {
                degraded_open
                    .entry(uri.clone())
                    .or_insert_with(|| Arc::new(tokio::sync::Notify::new()));
                heal_ctx.begin_progress(&uri, language).await;
                heal_ctx.request_gather(uri);
            }
        });
    }

    /// A pack file's bytes changed on disk (save or watcher event) — forward
    /// the fact to the invalidation owner off the message loop, then publish
    /// its outcome: every returned open URI re-gathers through the
    /// single-flight registry (Part 2), so a consumer already mid-gather
    /// coalesces (re-runs once against the freshly evicted caches) instead
    /// of double-gathering the same cone. Which analyses are stale, the
    /// serialization, and the H9 disciplines are all `PackInvalidator`'s.
    pub(super) fn schedule_pack_invalidate(&self, path: PathBuf, deleted: bool) {
        let files = Arc::clone(&self.files);
        let module_index = Arc::clone(&self.module_index);
        let invalidator = Arc::clone(&self.pack_invalidator);
        let root = self.module_index.workspace_root();
        let heal_ctx = self.pack_heal_ctx();
        tokio::spawn(async move {
            let outcome = tokio::task::spawn_blocking(move || {
                invalidator.file_changed(root.as_deref(), &module_index, &files, &path, deleted)
            })
            .await;
            let Ok(outcome) = outcome else { return };
            if outcome.deferred {
                // Reconciled at end-of-index; `heal_open_docs` re-publishes
                // the open docs then.
                return;
            }
            for uri in outcome.refresh_open {
                heal_ctx.request_gather(uri);
            }
        });
    }

    /// A bare identifier at the cursor that names a CROSS-FILE top-level
    /// symbol — a macro (`OP_NULL`, `BASEOP`), enum constant, global, or
    /// type. Resolves off the RAW word, so it works even when the macro
    /// expanded AWAY in the analysis (the token isn't a captured ref).
    /// Returns (target uri, def span, the def's source line for hover).
    pub(super) fn pack_xfile_word_at(
        &self,
        text: &str,
        doc_analysis: &crate::model::file_analysis::FileAnalysis,
        pos: Position,
        idx: &dyn crate::model::file_analysis::CrossFileLookup,
    ) -> Option<(Option<Url>, crate::model::file_analysis::Span, String)> {
        let word = symbols::word_at_point(text, symbols::position_to_point(pos))?;
        // Pick the best DEFINITION among same-named symbols (a `#define X` plus
        // its raw usages): prefer the `#define` line, else the earliest.
        let pick = |analysis: &crate::model::file_analysis::FileAnalysis, src: &str| {
            let lines: Vec<&str> = src.lines().collect();
            let line_of =
                |s: &crate::model::file_analysis::Symbol| lines.get(s.selection_span.start.row).copied();
            let cands: Vec<&crate::model::file_analysis::Symbol> =
                analysis.symbols.iter().filter(|s| s.name == word).collect();
            let sym = cands
                .iter()
                .find(|s| line_of(s).is_some_and(|l| l.trim_start().starts_with("#define")))
                .or_else(|| cands.iter().min_by_key(|s| s.selection_span.start.row))
                .copied()?;
            Some((sym.selection_span, line_of(sym).map(|l| l.trim().to_string()).unwrap_or_default()))
        };
        // A macro defined in THIS file (`BASEOP` in op.h) — the usage isn't a
        // captured ref, so find_definition missed it, but the def symbol is
        // local. Fall back to the cross-file index for usages from elsewhere.
        if let Some((span, line)) = pick(doc_analysis, text) {
            return Some((None, span, line));
        }
        let cached = idx.get_cached(word)?;
        let text = std::fs::read_to_string(&cached.path).ok()?;
        let (span, line) = pick(&idx.whole_present(&cached), &text)?;
        Some((Url::from_file_path(&cached.path).ok(), span, line))
    }

    /// The freshness engine's consumption half for OPEN docs: after an
    /// edit to `uri` rebuilt its analysis, record the new surface. An
    /// `Unchanged` verdict is the early-cutoff — a body edit refreshes
    /// nobody. `Changed` re-enriches + republishes exactly the OPEN docs
    /// in the transitive dirty closure (closed workspace consumers stay
    /// correct through the query-time walks; their always-enriched
    /// materialization is the next phase).
    /// Records `Document::baseline_surface` — the build-time, pre-enrichment
    /// projection — through `record_and_dirty_value`, the shared
    /// record→verdict→dirty seam. Enrichment state can't reach the record by
    /// construction, so this may run before or after any publish. The caller
    /// acts on the returned set (republish).
    pub(super) fn record_open_doc_surface(&self, uri: &Url) -> Option<crate::index::module_index::SurfaceDirty> {
        let path = uri.to_file_path().ok()?;
        let canon = std::fs::canonicalize(&path).unwrap_or(path);
        let surface = self.files.get_open(uri)?.baseline_surface.clone()?;
        Some(self.module_index.record_and_dirty_value(
            &canon,
            surface,
            crate::index::module_index::SurfaceWrite::OpenDoc,
        ))
    }

    /// Re-enrich + republish every OPEN doc in a dirty closure — the one
    /// speller of the membership rule (canonical-path match), shared by
    /// the in-editor verdict path and the watcher's aggregated closure.
    pub(super) async fn republish_open_docs_in(
        &self,
        dirty: &std::collections::HashSet<std::path::PathBuf>,
    ) {
        if dirty.is_empty() {
            return;
        }
        let mut to_refresh: Vec<Url> = Vec::new();
        self.files.for_each_open(|u, _doc| {
            if let Ok(p) = u.to_file_path() {
                let c = std::fs::canonicalize(&p).unwrap_or(p);
                if dirty.contains(&c) {
                    to_refresh.push(u.clone());
                }
            }
        });
        for u in to_refresh {
            self.publish_diagnostics(&u).await;
        }
    }

    /// Publish `uri`'s diagnostics — a pure read over the derived enriched
    /// analysis. The one enrichment writer is `FileStore::enrich_open`
    /// (clone-and-enrich off the store lock, ptr-guarded swap); this path
    /// reads the artifact it returns, never mutates a stored analysis.
    pub(super) async fn publish_diagnostics(&self, uri: &Url) {
        let options = self.diagnostic_options();
        let language = self.files.get_open(uri).map(|d| d.language);
        let diagnostics = match language {
            Some("perl") => match self.files.enrich_open(uri, &*self.module_index) {
                Some(analysis) => {
                    symbols::collect_diagnostics(&analysis, &self.module_index, options)
                }
                None => vec![],
            },
            // Pack languages stay honest-silent EXCEPT the always-on
            // member-access operator mismatch and the opt-in use-after-move
            // (gated by `DiagnosticOptions.use_after_move`).
            Some(_) => self
                .files
                .get_open(uri)
                .map(|doc| symbols::pack_diagnostics(&doc.analysis, options))
                .unwrap_or_default(),
            None => vec![],
        };
        self.client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;
    }
}

/// Which open docs a bulk diagnostics refresh covers.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum OpenDocScope {
    /// Every open doc (the resolver refresh storm — perl docs re-enrich,
    /// pack docs re-read).
    All,
    /// Perl docs only (the perl-family cold-open heal).
    PerlFamily,
}

/// Re-derive diagnostics for open docs: perl docs re-enrich through
/// `FileStore::enrich_open` (the one enrichment writer) and are read from
/// the returned artifact; pack docs are read as-is. URIs are collected
/// under the read iterator first, then each doc is derived with no store
/// guard held — safe to run from any task, publish after.
pub(super) fn refresh_open_diagnostics(
    files: &FileStore,
    module_index: &ModuleIndex,
    options: symbols::DiagnosticOptions,
    scope: OpenDocScope,
) -> Vec<(Url, Vec<Diagnostic>)> {
    let mut docs: Vec<(Url, &'static str)> = Vec::new();
    files.for_each_open(|uri, doc| {
        if scope == OpenDocScope::All || doc.language == "perl" {
            docs.push((uri.clone(), doc.language));
        }
    });
    let mut pending: Vec<(Url, Vec<Diagnostic>)> = Vec::new();
    for (uri, language) in docs {
        let diagnostics = if language == "perl" {
            match files.enrich_open(&uri, module_index) {
                Some(analysis) => symbols::collect_diagnostics(&analysis, module_index, options),
                None => continue, // closed mid-iteration
            }
        } else {
            match files.get_open(&uri) {
                Some(doc) => symbols::pack_diagnostics(&doc.analysis, options),
                None => continue,
            }
        };
        pending.push((uri, diagnostics));
    }
    pending
}

/// `(RefLocation, text)` pairs → one `WorkspaceEdit` (per-member texts).
pub(super) fn edit_pairs_to_workspace_edit(
    edits: Vec<(crate::index::resolve::RefLocation, String)>,
) -> Option<WorkspaceEdit> {
    if edits.is_empty() {
        return None;
    }
    let mut all_changes: std::collections::HashMap<Url, Vec<TextEdit>> =
        std::collections::HashMap::new();
    for (loc, text) in edits {
        if let Some(uri) = loc.to_url() {
            all_changes.entry(uri).or_default().push(TextEdit {
                range: symbols::span_to_range(loc.span),
                new_text: text,
            });
        }
    }
    if all_changes.is_empty() {
        None
    } else {
        Some(WorkspaceEdit { changes: Some(all_changes), ..Default::default() })
    }
}


pub(super) fn refs_to_locations(results: Vec<crate::index::resolve::RefLocation>) -> Option<Vec<Location>> {
    let mut locations: Vec<Location> = results
        .into_iter()
        .filter_map(|r| {
            let uri = r.to_url()?;
            Some(Location {
                uri,
                range: symbols::span_to_range(r.span),
            })
        })
        .collect();
    if locations.is_empty() {
        return None;
    }
    locations.sort_by(|a, b| {
        a.uri.as_str().cmp(b.uri.as_str())
            .then_with(|| a.range.start.line.cmp(&b.range.start.line))
            .then_with(|| a.range.start.character.cmp(&b.range.start.character))
    });
    locations.dedup_by(|a, b| a.uri == b.uri && a.range == b.range);
    Some(locations)
}

/// How often the parent-liveness monitor polls the client `processId`. ~10s is
/// the cadence vscode-languageserver-node / lsp4j / jdt.ls use — cheap enough to
/// run unconditionally, tight enough that a leaked server dies within a poll.
const PARENT_LIVENESS_POLL: std::time::Duration = std::time::Duration::from_secs(10);

/// Spawn a detached timer that self-exits when the LSP client (parent) process
/// dies. This is INDEPENDENT of the stdin read loop by design: the leak cases
/// are exactly when the read loop isn't running (server wedged mid-analysis, or
/// a hard SIGKILL of the editor that delivered no clean EOF). `None` disables
/// the check — per spec, a null `processId` means the client didn't fork us.
pub(super) fn spawn_parent_liveness_monitor(process_id: Option<u32>) {
    let Some(pid) = process_id else { return };
    if pid == 0 {
        return;
    }
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(PARENT_LIVENESS_POLL).await;
            if !parent_process_alive(pid) {
                // Client gone; nothing to flush after the connection drops.
                // Exit hard so background `spawn_blocking` indexing (which parks
                // on `send_request` once the client vanishes) can't keep the
                // runtime — and a multi-GB workspace index — alive.
                std::process::exit(0);
            }
        }
    });
}

/// Linux liveness probe: `/proc/<pid>` vanishes once the process is reaped. No
/// new dependency, no signal side effects (unlike `kill(pid, 0)`).
#[cfg(target_os = "linux")]
fn parent_process_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// Off Linux there's no cheap dependency-free probe, so assume alive — never
/// false-positive into an exit. The stdin-EOF path still covers clean shutdown.
#[cfg(not(target_os = "linux"))]
fn parent_process_alive(_pid: u32) -> bool {
    true
}
