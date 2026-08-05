//! The FileAnalysis blob codec (bincode+zstd), the stamp currencies
//! (file / mtime-nanos / closure), keyed single-file rehydration with
//! WAL-race recovery, and the blob-row writers.

use super::*;

/// zstd compression level for the `analysis` blob. Lower numbers are faster;
/// 3 is zstd's default and gives a solid space/speed tradeoff.
pub(super) const ZSTD_LEVEL: i32 = 3;

/// Discriminated rehydration failure — the honest replacement for the
/// collapsed "loader returned None". Every arm names a distinct on-disk
/// reality so the strict-residency panic points at a mechanism, not a
/// shrug.
#[derive(Debug, Clone)]
pub enum RehydrateMiss {
    /// Couldn't even open the cache DB read-only (SQLite error text).
    OpenerFailed(String),
    /// The `modules` table has no row for any candidate path spelling — not
    /// even through a read-write open (so not the recoverable WAL race).
    NoRow,
    /// Row(s) exist but every candidate blob is NULL/empty.
    EmptyBlob,
    /// Blob present but zstd/bincode decode failed (shape/version skew).
    DecodeFailed,
}

impl std::fmt::Display for RehydrateMiss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RehydrateMiss::OpenerFailed(e) => write!(f, "opener failed: {e}"),
            RehydrateMiss::NoRow => write!(f, "no row for path (read-only and read-write both empty)"),
            RehydrateMiss::EmptyBlob => write!(f, "row present but blob empty/NULL"),
            RehydrateMiss::DecodeFailed => write!(f, "blob decode failed"),
        }
    }
}

/// The row validation stamp: (mtime hashed at NANOSECOND precision, size).
/// Whole seconds miss two same-length writes within one second (generated
/// files, rapid saves) — the M1 staleness window. The `mtime_secs` column
/// name is historical; the value is an opaque equality-checked stamp.
pub fn file_stamp(path: &std::path::Path) -> Option<(i64, i64)> {
    use std::hash::{Hash, Hasher};
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    let nanos = mtime
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    nanos.hash(&mut h);
    let size = meta.len() as i64;
    Some((h.finish() as i64, size))
}

/// Raw mtime in nanoseconds since the epoch — an ORDERED source-generation
/// currency, unlike `file_stamp`'s hashed-and-sized equality token. A later
/// save has a strictly greater value (editors write mtime = now, monotone
/// forward even across git operations), so the registration guard can reject
/// a re-analysis built from an EARLIER generation: the `PackInvalidator` swap
/// registers a result only when its event generation is ≥ the one already
/// registered for that path (H9-1 stale-winner race). `None` if unstattable.
pub fn file_mtime_nanos(path: &std::path::Path) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    let nanos = mtime
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Some(nanos as i64)
}

/// Stamp over every file in an analysis' include closure — the ANALYSIS-INPUT
/// half of a pack row's validation key. A consumer `.c` row bakes its headers'
/// macro splices and type witnesses; its own (stamp, size) can't see a header
/// edit, so the closure stamp must (M2). Perl analyses have an empty closure
/// → 0, so the Perl path pays nothing. `stat_memo` dedups stats across a warm
/// run (closures overlap heavily — op.c and sv.c share ~90% of perl5's tree).
pub(super) fn closure_stamp(
    closure: &crate::model::file_analysis::path_intern::ClosureList,
    stat_memo: &mut std::collections::HashMap<String, (i64, i64)>,
) -> i64 {
    use std::hash::{Hash, Hasher};
    if closure.is_empty() {
        return 0;
    }
    // Commutative fold: the id-list iterates in global mint order, which
    // varies run-to-run (Rayon interning races) — an order-sensitive hash
    // would invalidate every warm row every session, and sorting per file
    // per warm row is n·log n string compares on the path the stamp exists
    // to make cheap. Hash each member independently, fold order-free.
    let mut acc: u64 = 0;
    for p in closure.iter_strs() {
        let stamp = *stat_memo
            .entry(p.as_ref().to_owned())
            .or_insert_with(|| file_stamp(std::path::Path::new(p.as_ref())).unwrap_or((0, -1)));
        let mut h = std::collections::hash_map::DefaultHasher::new();
        p.as_ref().hash(&mut h);
        stamp.hash(&mut h);
        acc = acc.wrapping_add(h.finish());
    }
    acc as i64
}

/// Serialize FileAnalysis via bincode then compress with zstd.
pub fn encode_analysis(fa: &FileAnalysis) -> Option<Vec<u8>> {
    let bin = bincode::serialize(fa).ok()?;
    zstd::encode_all(bin.as_slice(), ZSTD_LEVEL).ok()
}

/// Decompress + deserialize an analysis blob.
/// Public for the bulk writers' failure recovery: a failed chunk commit
/// un-strips its resident copies by decoding the blobs it still holds.
pub fn decode_analysis(blob: &[u8]) -> Option<FileAnalysis> {
    let bin = zstd::decode_all(blob).ok()?;
    let mut fa: FileAnalysis = bincode::deserialize(&bin).ok()?;
    fa.after_deserialize();
    Some(fa)
}

/// Keyed single-file decode — the Slice-2 rehydration primitive
/// (`docs/adr/memory-slice-2-lru.md`). Loads ONE file's persisted analysis
/// (full witness bag present) by path, without warming the whole table. The
/// resident pack-index copy has its bag evicted after indexing; a type query
/// that reaches into an evicted file rehydrates the exact bag through here.
/// No mtime/closure validation: the caller (`PackBagCache`) invalidates its
/// entry on file change, and the row's shape is EXTRACT_VERSION-pinned.
pub fn load_one(conn: &Connection, path: &str) -> Option<FileAnalysis> {
    load_one_diag(conn, path).ok()
}

/// `load_one` that discriminates the failure (see `RehydrateMiss`) instead
/// of collapsing to `None`, so the rehydration tripwire can name the cause.
pub fn load_one_diag(conn: &Connection, path: &str) -> Result<FileAnalysis, RehydrateMiss> {
    // A dual-homed project-lib file has TWO rows for one path (name-keyed
    // import + path-keyed workspace). Prefer a row whose stamp matches the
    // disk (one tier's persist may have failed or lagged, leaving a stale
    // generation); workspace-first is only the tiebreak. Single-row paths
    // deliberately skip stamp validation — the registered generation may
    // legitimately predate an unsaved edit, and the caller invalidates the
    // LRU on file change.
    let mut stmt = conn
        .prepare(
            "SELECT analysis, mtime_secs, file_size FROM modules WHERE path = ?1 \
             ORDER BY CASE source WHEN 'workspace' THEN 0 ELSE 1 END",
        )
        .map_err(|_| RehydrateMiss::NoRow)?;
    let rows: Vec<(Option<Vec<u8>>, i64, i64)> = stmt
        .query_map(params![path], |row| {
            Ok((
                row.get::<_, Option<Vec<u8>>>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|_| RehydrateMiss::NoRow)?
        .flatten()
        .collect();
    if rows.is_empty() {
        return Err(RehydrateMiss::NoRow);
    }
    let pick = |require_stamp: bool| -> Option<&Vec<u8>> {
        rows.iter().find_map(|(blob, m, sz)| {
            let blob = blob.as_ref().filter(|b| !b.is_empty())?;
            if require_stamp && file_stamp(std::path::Path::new(path)) != Some((*m, *sz)) {
                return None;
            }
            Some(blob)
        })
    };
    let blob = pick(rows.len() > 1)
        .or_else(|| pick(false))
        .ok_or(RehydrateMiss::EmptyBlob)?;
    decode_analysis(blob).ok_or(RehydrateMiss::DecodeFailed)
}

/// The bag-cache rehydration loader body, shared by every per-lang loader
/// closure (Perl hub + pack sub-indexes). Tries each candidate path spelling
/// (canonical vs raw walk path — the blob is written canonical but a
/// resident copy may be keyed raw) and survives the readonly-open CANTOPEN
/// race via `load_with_wal_fallback`'s read-write recovery. Every failure is
/// discriminated for the strict-residency tripwire.
#[cfg(not(test))]
pub fn open_and_load_diag(
    cache_key: Option<&str>,
    lang: &str,
    paths: &[String],
) -> Result<FileAnalysis, RehydrateMiss> {
    let dir = cache_dir_for_workspace(cache_key)
        .ok_or_else(|| RehydrateMiss::OpenerFailed("no cache dir for workspace".into()))?;
    load_with_wal_fallback(&db_path_for(&dir, lang), paths)
}

#[cfg(test)]
pub fn open_and_load_diag(
    _cache_key: Option<&str>,
    _lang: &str,
    _paths: &[String],
) -> Result<FileAnalysis, RehydrateMiss> {
    Err(RehydrateMiss::NoRow)
}

/// Rehydrate one file from an explicit cache DB, discriminating every
/// failure and surviving the readonly/WAL-checkpoint race. Path-taking so the
/// whole policy is unit-testable.
///
/// The captured cause: a fresh open of the WAL-mode cache DB transiently
/// returns `SQLITE_CANTOPEN` for BOTH read-only and read-write modes while a
/// sibling writer is mid-`wal_checkpoint`/WAL-reset — SQLite can't set up the
/// `-wal`/`-shm` auxiliaries in that window, and `busy_timeout` doesn't cover
/// the open. The blob is on disk the whole time. `open_reader_retrying` waits
/// the window out with bounded backoff; a recovering read-write read then
/// `wal_checkpoint`s so the next open faces a folded WAL. The strict-residency
/// tripwire still fires only when the window never clears or even a read-write
/// open can't produce the row — a genuinely unreadable/absent blob, a real
/// invariant break.
pub fn load_with_wal_fallback(
    db_path: &std::path::Path,
    paths: &[String],
) -> Result<FileAnalysis, RehydrateMiss> {
    // `open_reader_retrying` waits out the transient CANTOPEN window; the
    // rw_open closure below then handles the (rarer) opened-but-row-invisible
    // case. Both are the WAL-checkpoint recovery.
    rehydrate_from_opens(
        open_reader_retrying(db_path),
        || open_rw_shared_at(db_path),
        paths,
    )
}

/// The fallback POLICY, split from the openers so the read-only-open-failure
/// branch is deterministically testable (the real `SQLITE_CANTOPEN` race
/// can't be forced from static file state). `ro` is the read-only open
/// result (`Err` = the open itself failed — the captured CANTOPEN cause);
/// `rw_open` lazily opens the read-write recovery connection.
pub(super) fn rehydrate_from_opens(
    ro: Result<Connection, String>,
    rw_open: impl FnOnce() -> Option<Connection>,
    paths: &[String],
) -> Result<FileAnalysis, RehydrateMiss> {
    let ro_err = ro.as_ref().err().cloned();
    let mut last = RehydrateMiss::NoRow;
    if let Ok(conn) = &ro {
        for p in paths {
            match load_one_diag(conn, p) {
                Ok(fa) => return Ok(fa),
                Err(RehydrateMiss::NoRow) => {}
                Err(other) => last = other,
            }
        }
    }
    // RW fallback covers BOTH a failed readonly open (CANTOPEN race) and a
    // readonly conn that opened but couldn't see the row. Skip it only when
    // readonly gave a definitive non-NoRow verdict (empty/undecodable blob).
    if ro_err.is_some() || matches!(last, RehydrateMiss::NoRow) {
        if let Some(rw) = rw_open() {
            for p in paths {
                if let Ok(fa) = load_one_diag(&rw, p) {
                    let _ = rw.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
                    return Ok(fa);
                }
            }
        } else if let Some(e) = ro_err {
            // Neither open worked: surface the readonly error text so the
            // tripwire names it (a truly unreadable DB is a real break).
            return Err(RehydrateMiss::OpenerFailed(e));
        }
    }
    Err(last)
}

/// `save_blob_to_db` with a caller-captured `file_stamp` — the bulk drains
/// persist analyses parsed earlier, and stamping at WRITE time would blesses
/// a stale parse with a fresh stamp when the file changed in between (the
/// next warm would then serve the pre-edit analysis as valid). Capture the
/// stamp at parse time; a mid-index edit makes the row invalid by
/// construction.
pub fn save_blob_to_db_stamped(
    conn: &Connection,
    module_name: &str,
    path: &std::path::Path,
    include_closure: &crate::model::file_analysis::path_intern::ClosureList,
    blob: &[u8],
    source: &str,
    stamp: (i64, i64),
) {
    let (mtime, size) = stamp;
    let deps = closure_stamp(include_closure, &mut std::collections::HashMap::new());
    let r = conn.execute(
        "INSERT OR REPLACE INTO modules (module_name, path, mtime_secs, file_size, source, analysis, extract_version, deps_stamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            module_name,
            path.to_string_lossy(),
            mtime,
            size,
            source,
            Some(blob),
            EXTRACT_VERSION,
            deps
        ],
    );
    if let Err(e) = r {
        log::warn!("Failed to save module blob for '{}': {}", module_name, e);
    }
    // A rewritten modules row orphans any prior stub for the path — a stale
    // skeleton paired with a fresh stamp would be served as valid on the
    // next warm. Writers that have a fresh stub re-insert it right after.
    delete_stub(conn, &path.to_string_lossy());
}

/// Recompute a persisted row's `deps_stamp` from CURRENT disk state without
/// touching its blob/rows/stub. For consumers of an Unchanged-surface edit:
/// their content is still valid, but a closure member's mtime moved, so the
/// stored stamp would fail the next warm scan and re-trigger the very cold
/// storm the gate prevents in-session. The file's own mtime/size stamp is
/// left alone — a consumer that itself changed on disk stays invalid.
pub fn refresh_deps_stamp(
    conn: &Connection,
    path: &str,
    include_closure: &crate::model::file_analysis::path_intern::ClosureList,
    stat_memo: &mut std::collections::HashMap<String, (i64, i64)>,
) {
    let deps = closure_stamp(include_closure, stat_memo);
    let _ = conn.execute(
        "UPDATE modules SET deps_stamp = ?1 WHERE path = ?2",
        params![deps, path],
    );
}

/// Returns whether the modules row landed — stripping a resident copy is
/// only legal when its blob is actually recoverable.
pub fn save_to_db(
    conn: &Connection,
    module_name: &str,
    result: &Option<Arc<CachedModule>>,
    source: &str,
) -> bool {
    let (path_str, mtime, size, analysis_blob, deps_stamp) = match result {
        Some(cached) => {
            // Degraded analyses (parse/extract failure, skipped gather) must
            // not be persisted: the row would validate on the source file's
            // stamp alone and re-serve the degraded blob every session (H8).
            if cached.analysis.degraded {
                return false;
            }
            let (mtime, size) = file_stamp(&cached.path).unwrap_or((0, 0));
            let blob = encode_analysis(&cached.analysis);
            if blob.is_none() {
                // Encode failure: leave the PREVIOUS row intact — replacing
                // it with a NULL blob would destroy a good generation and
                // warm as a terminal negative sentinel across sessions.
                log::warn!(
                    "Failed to encode analysis for '{}'; keeping prior row",
                    module_name
                );
                return false;
            }
            let deps = closure_stamp(
                &cached.analysis.include_closure,
                &mut std::collections::HashMap::new(),
            );
            (cached.path.to_string_lossy().to_string(), mtime, size, blob, deps)
        }
        None => (String::new(), 0i64, 0i64, None, 0i64),
    };

    let r = conn.execute(
        "INSERT OR REPLACE INTO modules (module_name, path, mtime_secs, file_size, source, analysis, extract_version, deps_stamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![module_name, path_str, mtime, size, source, analysis_blob, EXTRACT_VERSION, deps_stamp],
    );
    let ok = match r {
        // A row whose blob failed to ENCODE landed as NULL — not a
        // recoverable generation; stripping against it would lose the bag.
        // (Negative sentinels have no blob by design and nothing to strip.)
        Ok(_) => result.is_none() || analysis_blob.is_some(),
        Err(e) => {
            log::warn!("Failed to save module cache for '{}': {}", module_name, e);
            false
        }
    };
    if !path_str.is_empty() {
        // Same stale-stub guard as `save_blob_to_db_stamped`.
        delete_stub(conn, &path_str);
    }
    ok
}
