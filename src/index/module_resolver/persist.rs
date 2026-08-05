//! Residency policy (eviction switch, strict mode, the tripwire) and
//! persistence: per-module generation save, the stamp-guarded analyze
//! protocol, and the shared batched persist-writer harness.

use super::*;

/// Slice-2 eviction off-switch: `PERL_LSP_NO_EVICT` keeps every resident pack
/// bag in memory (the pre-Slice-2 footprint) — an emergency knob and the A/B
/// lever for isolating an eviction-caused regression.
pub(crate) fn eviction_enabled() -> bool {
    std::env::var_os("PERL_LSP_NO_EVICT").is_none()
}

/// `PERL_LSP_STRICT_RESIDENCY=1`: residency invariant breaks (an evicted
/// copy that can't rehydrate, a tripwire overrun) PANIC instead of
/// degrading. The gold harness sets it so a session serving
/// absence-as-answer dies as a CRASH row (hard fail) rather than scoring
/// wrong answers — the cold-flake net. Off by default: a live server
/// prefers degraded-but-useful.
pub(crate) fn strict_residency() -> bool {
    std::env::var_os("PERL_LSP_STRICT_RESIDENCY").is_some_and(|v| v != "0")
}

/// The post-bulk-index residency check, one speller for the pack tier and
/// the Perl workspace tier: fully-resident registered copies beyond the
/// deliberately-accounted ones (writer fallbacks, degraded/unpersisted
/// analyses) mean a registration path is silently pinning whole analyses —
/// the RAM regression no functional test can see. `debug_assert` catches it
/// in `cargo test`; strict mode makes it fatal in release (the gold net).
pub(super) fn residency_tripwire(tier: &str, whole: usize, expected: usize) {
    if whole <= expected {
        return;
    }
    log::error!(
        "residency tripwire ({tier}): {whole} fully-resident copies, only \
         {expected} accounted (writer fallbacks / degraded) — a registration \
         path is pinning whole analyses"
    );
    debug_assert!(
        false,
        "residency tripwire ({tier}): {whole} fully-resident > {expected} accounted"
    );
    if strict_residency() {
        panic!(
            "PERL_LSP_STRICT_RESIDENCY: residency tripwire ({tier}): {whole} \
             fully-resident copies, only {expected} accounted"
        );
    }
}

/// Persist one module's generation: blob + its relational ref rows, always
/// together (`docs/adr/relational-ref-index.md` — rows and blob describe the
/// same analysis or neither exists). `save_to_db` skips degraded analyses;
/// mirror that here so no rows exist for an unpersisted blob.
/// Returns whether the blob row landed (the strip-legality signal).
pub(super) fn save_module_generation(
    conn: &rusqlite::Connection,
    module_name: &str,
    result: &Option<Arc<CachedModule>>,
) -> bool {
    if let Some(m) = result {
        // A bag-evicted copy IS the already-persisted generation —
        // re-encoding it would overwrite the good blob with a bagless one.
        if m.analysis.bag_is_evicted() {
            return true;
        }
    }
    let persisted = module_cache::save_to_db(conn, module_name, result, "import");
    if !persisted {
        // Blob didn't land (busy/encode failure): shredding rows now would
        // pair a NEW generation's rows with an OLD (or absent) blob —
        // "blob + rows describe one generation" is the write invariant.
        return false;
    }
    if let Some(m) = result {
        if !m.analysis.degraded {
            let seeds: Vec<_> = m.analysis.refs.iter().map(|r| r.row_seed()).collect();
            let sym_seeds = m.analysis.sym_row_seeds();
            if let Err(e) = module_cache::shred_derived_rows(
                conn,
                &m.path.to_string_lossy(),
                "import",
                &seeds,
                &sym_seeds,
            ) {
                log::warn!("Failed to shred derived rows for '{}': {}", module_name, e);
            }
        }
    }
    persisted
}

/// Stamp-before-read + re-stat-after-parse: capture the disk stamp, run the
/// read+analyze, and return None when the file changed underneath — a
/// write-time stamp would bless a stale parse as the current generation and
/// every future warm would serve it as valid. Both fresh workers route
/// their changed-under-us protocol through here.
pub(super) fn analyze_stamped<T>(
    path: &std::path::Path,
    f: impl FnOnce() -> Option<T>,
) -> Option<(T, (i64, i64))> {
    let stamp = module_cache::file_stamp(path).unwrap_or((0, 0));
    let out = f()?;
    if module_cache::file_stamp(path) != Some(stamp) {
        return None;
    }
    Some((out, stamp))
}

/// Byte budget for whole `FileAnalysis` copies the persist writer retains
/// when a chunk fails to commit (disk full) or panics. The strip is licensed
/// only by a landed blob, so a fallback keeps copies WHOLE — and a
/// persistently failing writer would otherwise pin the ENTIRE tree resident
/// (the docs/forks-resolved.md "writer fallback budget" entry). Past the cap
/// we DROP the resident copy rather than register a stripped one: the chunk
/// didn't commit, so a stripped copy's blob isn't on disk and could only
/// rehydrate to wrong-empty. Dropping is honest absence — the file reads as
/// "not indexed this session" and the next index/warm re-registers it; it
/// never serves wrong data and never leaves an evicted copy unrehydratable
/// (nothing is evicted — nothing is registered). Byte-accounted like the
/// enrichment overlay (`ENRICHED_BYTE_CAP`); 128 MiB per writer thread — a
/// transient failure degrades gracefully, a permanent one can't OOM.
pub(super) const FALLBACK_WHOLE_BYTE_CAP: usize = 128 * 1024 * 1024;

/// The persist-writer harness both bulk indexers share: batches entries off
/// the channel (≤128 per txn), owns BEGIN IMMEDIATE / COMMIT / ROLLBACK
/// (IMMEDIATE — a deferred txn that reads before writing can hit an
/// unretryable SQLITE_BUSY_SNAPSHOT against a concurrent writer), and hands
/// every entry to exactly one of `on_committed` (deferred registration) or
/// `on_fallback` (commit failure OR chunk panic — the whole-copy self-heal;
/// a panic must not kill the writer, workers keep stripping copies whose
/// sends would silently fail). With no Connection the channel drains
/// unregistered. Registration runs inside the panic guard, mirroring the
/// txn: entries a mid-batch registration panic leaves behind take the
/// fallback lane instead of vanishing.
pub(super) fn run_persist_writer<E>(
    rx: std::sync::mpsc::Receiver<E>,
    conn: Option<&rusqlite::Connection>,
    label: &str,
    write_batch: impl Fn(&rusqlite::Connection, &[E]),
    mut on_committed: impl FnMut(E),
    mut on_fallback: impl FnMut(E),
) {
    let Some(conn) = conn else {
        while rx.recv().is_ok() {}
        return;
    };
    let mut batch: Vec<E> = Vec::new();
    let mut process = |batch: &mut Vec<E>| {
        if batch.is_empty() {
            return;
        }
        let n = batch.len();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let txn_open = conn.execute_batch("BEGIN IMMEDIATE").is_ok();
            write_batch(conn, batch);
            let committed = txn_open
                && match conn.execute_batch("COMMIT") {
                    Ok(()) => true,
                    Err(err) => {
                        let _ = conn.execute_batch("ROLLBACK");
                        log::error!(
                            "{label}: commit failed ({n} files, registering whole copies): {err}"
                        );
                        false
                    }
                };
            if committed {
                for e in batch.drain(..) {
                    on_committed(e);
                }
            } else {
                for e in batch.drain(..) {
                    on_fallback(e);
                }
            }
        }));
        if r.is_err() {
            // A panic can leave the txn open; roll back defensively so the
            // NEXT chunk's BEGIN isn't poisoned.
            let _ = conn.execute_batch("ROLLBACK");
            log::error!("{label}: chunk panicked ({n} files) — registering whole copies");
            for e in batch.drain(..) {
                on_fallback(e);
            }
        }
    };
    while let Ok(entry) = rx.recv() {
        batch.push(entry);
        while batch.len() < 128 {
            match rx.try_recv() {
                Ok(e) => batch.push(e),
                Err(_) => break,
            }
        }
        process(&mut batch);
    }
    process(&mut batch);
}
