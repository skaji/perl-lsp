//! The relational row store (`files`/`strings`/`refs`/`syms`):
//! shredding, invalidation, chunked deferred writes, and the retrieval
//! views (ref candidates, workspace/symbol rows, dead exports).

use super::*;

/// Replace one file's derived rows — refs AND symbols — in the relational
/// index. One function so both families are the same generation by
/// construction (`files` presence is the single "already shredded" marker;
/// a marker per family would let them drift). Runs inside the caller's
/// transaction when one is open (bulk drains wrap N files per `BEGIN`);
/// standalone callers get per-statement autocommit, which is fine for
/// single-file updates. Upserts the `files` row even for an empty file.
pub fn shred_derived_rows(
    conn: &Connection,
    path: &str,
    source: &str,
    seeds: &[crate::model::file_analysis::RefRowSeed],
    sym_seeds: &[crate::model::file_analysis::SymRowSeed],
) -> rusqlite::Result<()> {
    // Sticky workspace tier: project lib/ files are inside the walk AND on
    // @INC (add_project_lib_paths), so the resolver re-shreds them as
    // 'import'. The walk's verdict wins — downgrading would let the @INC
    // hard-clear take an editable file's generation out from under its
    // stripped resident copy.
    conn.execute(
        "INSERT INTO files (path, source) VALUES (?1, ?2)
         ON CONFLICT(path) DO UPDATE SET source =
           CASE WHEN files.source = 'workspace' THEN 'workspace' ELSE excluded.source END",
        params![path, source],
    )?;
    let file_id: i64 = conn.query_row(
        "SELECT file_id FROM files WHERE path = ?1",
        params![path],
        |row| row.get(0),
    )?;
    conn.execute("DELETE FROM refs WHERE file_id = ?1", params![file_id])?;
    conn.execute("DELETE FROM syms WHERE file_id = ?1", params![file_id])?;
    let mut intern = conn.prepare_cached("INSERT OR IGNORE INTO strings (s) VALUES (?1)")?;
    let mut lookup = conn.prepare_cached("SELECT str_id FROM strings WHERE s = ?1")?;
    let mut insert = conn.prepare_cached(
        "INSERT INTO refs (file_id, name_id, kind, start_row, start_col, end_row, end_col,
                           access, flags, qual_kind, qual_id, arg_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )?;
    // Per-call interning memo: files repeat the same handful of names heavily.
    let mut memo: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut intern_str = |s: &str,
                          memo: &mut std::collections::HashMap<String, i64>|
     -> rusqlite::Result<i64> {
        if let Some(id) = memo.get(s) {
            return Ok(*id);
        }
        intern.execute(params![s])?;
        let id: i64 = lookup.query_row(params![s], |row| row.get(0))?;
        memo.insert(s.to_string(), id);
        Ok(id)
    };
    for seed in seeds {
        let name_id = intern_str(&seed.key, &mut memo)?;
        let qual_id = match seed.qual.as_deref() {
            Some(q) => Some(intern_str(q, &mut memo)?),
            None => None,
        };
        insert.execute(params![
            file_id,
            name_id,
            seed.kind,
            seed.span.start.row as i64,
            seed.span.start.column as i64,
            seed.span.end.row as i64,
            seed.span.end.column as i64,
            seed.access,
            seed.flags,
            seed.qual_kind,
            qual_id,
            seed.arg_count,
        ])?;
    }
    let mut insert_sym = conn.prepare_cached(
        "INSERT INTO syms (file_id, name_id, kind, start_row, start_col, end_row, end_col,
                           container_id, flags)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    for seed in sym_seeds {
        let name_id = intern_str(&seed.name, &mut memo)?;
        let container_id = match seed.container.as_deref() {
            Some(c) => Some(intern_str(c, &mut memo)?),
            None => None,
        };
        insert_sym.execute(params![
            file_id,
            name_id,
            seed.kind,
            seed.span.start.row as i64,
            seed.span.start.column as i64,
            seed.span.end.row as i64,
            seed.span.end.column as i64,
            container_id,
            seed.flags,
        ])?;
    }
    Ok(())
}

/// Drop one file's whole persisted generation — blob row AND derived ref
/// rows, together (the eraser twin of the write invariant "blob + rows
/// describe one generation"). Every invalidation seam calls this; nobody
/// else spells modules-table SQL.
pub fn invalidate_generation(conn: &Connection, path: &str) {
    let _ = conn.execute("DELETE FROM modules WHERE path = ?1", params![path]);
    delete_stub(conn, path);
    delete_ref_rows(conn, path);
}

/// Tier-scoped eraser: drops the generation ONLY when its rows carry
/// `source` — the walk's dead-row GC must not take a dual-homed file's
/// import-tier generation (project-lib files leave the walk when
/// gitignored but stay valid @INC modules).
pub fn invalidate_generation_tier(conn: &Connection, path: &str, source: &str) {
    let _ = conn.execute(
        "DELETE FROM modules WHERE path = ?1 AND source = ?2",
        params![path, source],
    );
    // Stubs are workspace-tier only — an import-tier invalidation must not
    // orphan a dual-homed file's still-valid workspace stub.
    if source == "workspace" {
        delete_stub(conn, path);
    }
    let _ = conn.execute(
        "DELETE FROM refs WHERE file_id IN
           (SELECT file_id FROM files WHERE path = ?1 AND source = ?2)",
        params![path, source],
    );
    let _ = conn.execute(
        "DELETE FROM syms WHERE file_id IN
           (SELECT file_id FROM files WHERE path = ?1 AND source = ?2)",
        params![path, source],
    );
    let _ = conn.execute(
        "DELETE FROM files WHERE path = ?1 AND source = ?2",
        params![path, source],
    );
}

/// Remove a deleted file's rows (the removal half of `shred_derived_rows`).
pub fn delete_ref_rows(conn: &Connection, path: &str) {
    let _ = conn.execute(
        "DELETE FROM refs WHERE file_id IN (SELECT file_id FROM files WHERE path = ?1)",
        params![path],
    );
    let _ = conn.execute(
        "DELETE FROM syms WHERE file_id IN (SELECT file_id FROM files WHERE path = ?1)",
        params![path],
    );
    let _ = conn.execute("DELETE FROM files WHERE path = ?1", params![path]);
}

/// Has `path` been shredded into the relational index? (`files` presence is
/// the marker — `shred_derived_rows` upserts it even for empty files.)
#[cfg(test)]
pub fn has_ref_rows(conn: &Connection, path: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM files WHERE path = ?1",
        params![path],
        |_| Ok(()),
    )
    .is_ok()
}

/// The retrieval half: every indexed file containing at least one ref row
/// whose match key is one of `keys` — the candidate-file set `refs_to`'s
/// SQL arms rehydrate and run the (unchanged) matcher over.
pub fn ref_candidate_files(conn: &Connection, keys: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    // Usage candidacy comes from ref rows; DECLARATION candidacy from sym
    // rows — a file that declares `helper` but never mentions it again has
    // no matching ref row, and without the union the backward walk's
    // matcher (whose declaration half reads symbols) never rehydrates it.
    let Ok(mut stmt) = conn.prepare_cached(
        "SELECT DISTINCT f.path FROM refs r
           JOIN files f ON f.file_id = r.file_id
          WHERE r.name_id = (SELECT str_id FROM strings WHERE s = ?1)
         UNION
         SELECT DISTINCT f.path FROM syms y
           JOIN files f ON f.file_id = y.file_id
          WHERE y.name_id = (SELECT str_id FROM strings WHERE s = ?1)",
    ) else {
        return out;
    };
    let mut seen = std::collections::HashSet::new();
    for key in keys {
        let rows = stmt.query_map(params![key], |row| row.get::<_, String>(0));
        if let Ok(rows) = rows {
            for r in rows {
                match r {
                    Ok(p) => {
                        if seen.insert(p.clone()) {
                            out.push(p);
                        }
                    }
                    // A step-level error (corrupt page, IO) ends the scan —
                    // the candidate list is TRUNCATED, which reads as
                    // "fewer references" with no other witness. Say so.
                    Err(e) => {
                        log::warn!("ref candidate scan aborted mid-iteration: {}", e);
                        break;
                    }
                }
            }
        }
    }
    out
}

/// One workspace/symbol row hit: (path, name, kind code, selection span,
/// container, flags). The caller applies the adapter's kind/flag filters
/// and skips paths a fresher resident copy already answered.
pub struct SymRowHit {
    pub path: String,
    pub name: String,
    pub kind: u8,
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
    pub container: Option<String>,
    pub flags: u8,
}

/// The rows-backed workspace/symbol scan: every WORKSPACE-tier symbol row
/// whose name contains `query`, case-insensitively — the same containment
/// test the resident sweep applies. The `files.source` filter keeps the
/// import tier (@INC deps) out: the resident sweeps never enumerated it,
/// and folding it in would flood project searches with CPAN internals.
pub fn sym_rows_matching(conn: &Connection, query: &str) -> Vec<SymRowHit> {
    let mut out = Vec::new();
    // SQLite LIKE is case-insensitive for ASCII only; the resident sweep
    // lowercases with full Unicode semantics. ASCII queries (the hot path)
    // stay an indexed-ish LIKE; a non-ASCII query walks the name strings
    // with the SAME Rust containment test so an evicted file's `sub Übung`
    // matches exactly like a resident one.
    if !query.is_ascii() {
        let Ok(mut stmt) = conn.prepare_cached(
            "SELECT f.path, n.s, y.kind, y.start_row, y.start_col, y.end_row, y.end_col,
                    c.s, y.flags
               FROM syms y
               JOIN files f ON f.file_id = y.file_id
               JOIN strings n ON n.str_id = y.name_id
               LEFT JOIN strings c ON c.str_id = y.container_id
              WHERE f.source = 'workspace'",
        ) else {
            return out;
        };
        let q = query.to_lowercase();
        let rows = stmt.query_map([], |row| {
            Ok(SymRowHit {
                path: row.get(0)?,
                name: row.get(1)?,
                kind: row.get::<_, i64>(2)? as u8,
                start_row: row.get::<_, i64>(3)? as usize,
                start_col: row.get::<_, i64>(4)? as usize,
                end_row: row.get::<_, i64>(5)? as usize,
                end_col: row.get::<_, i64>(6)? as usize,
                container: row.get(7)?,
                flags: row.get::<_, i64>(8)? as u8,
            })
        });
        if let Ok(rows) = rows {
            for r in rows {
                match r {
                    Ok(hit) => {
                        if hit.name.to_lowercase().contains(&q) {
                            out.push(hit);
                        }
                    }
                    Err(e) => {
                        log::warn!("sym row scan aborted mid-iteration: {}", e);
                        break;
                    }
                }
            }
        }
        return out;
    }
    let Ok(mut stmt) = conn.prepare_cached(
        "SELECT f.path, n.s, y.kind, y.start_row, y.start_col, y.end_row, y.end_col,
                c.s, y.flags
           FROM syms y
           JOIN files f ON f.file_id = y.file_id
           JOIN strings n ON n.str_id = y.name_id
           LEFT JOIN strings c ON c.str_id = y.container_id
          WHERE f.source = 'workspace'
            AND n.s LIKE '%' || ?1 || '%' ESCAPE '\\'",
    ) else {
        return out;
    };
    // LIKE wildcards in the user's query are literals, not patterns.
    let escaped = query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    let rows = stmt.query_map(params![escaped], |row| {
        Ok(SymRowHit {
            path: row.get(0)?,
            name: row.get(1)?,
            kind: row.get::<_, i64>(2)? as u8,
            start_row: row.get::<_, i64>(3)? as usize,
            start_col: row.get::<_, i64>(4)? as usize,
            end_row: row.get::<_, i64>(5)? as usize,
            end_col: row.get::<_, i64>(6)? as usize,
            container: row.get(7)?,
            flags: row.get::<_, i64>(8)? as u8,
        })
    });
    if let Ok(rows) = rows {
        for r in rows {
            match r {
                Ok(hit) => out.push(hit),
                Err(e) => {
                    log::warn!("sym row scan aborted mid-iteration: {}", e);
                    break;
                }
            }
        }
    }
    out
}

/// Row count for one match key — the count-first surface for hot-name
/// capping (`docs/adr/relational-ref-index.md`).
#[cfg(test)]
pub fn ref_count_named(conn: &Connection, key: &str) -> u64 {
    conn.query_row(
        "SELECT COUNT(*) FROM refs
          WHERE name_id = (SELECT str_id FROM strings WHERE s = ?1)",
        params![key],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n as u64)
    .unwrap_or(0)
}

/// Deferred-write chunking: N items per `BEGIN IMMEDIATE`…`COMMIT`, the
/// SQLITE_BUSY_SNAPSHOT-safe shape every post-scan backfill shares (writing
/// inside a streaming SELECT's snapshot turns a concurrent commit into an
/// unretried BUSY_SNAPSHOT abort). A failed txn OPEN abandons the remaining
/// queue (the writer is likely gone); a failed COMMIT rolls back and keeps
/// going — later chunks may land.
pub fn write_in_chunks<T>(
    conn: &Connection,
    items: &[T],
    chunk_size: usize,
    label: &str,
    per_item: impl Fn(&Connection, &T),
) {
    for chunk in items.chunks(chunk_size) {
        if conn.execute_batch("BEGIN IMMEDIATE").is_err() {
            log::error!("{label}: txn open failed; remaining items defer to next warm");
            break;
        }
        for item in chunk {
            per_item(conn, item);
        }
        if let Err(e) = conn.execute_batch("COMMIT") {
            log::error!("{label}: commit failed: {}", e);
            let _ = conn.execute_batch("ROLLBACK");
        }
    }
}

/// Every path that currently has shredded derived rows — the bulk twin of
/// `has_ref_rows` for warm scans (one query instead of one per file).
pub fn paths_with_ref_rows(conn: &Connection) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let Ok(mut stmt) = conn.prepare("SELECT path FROM files") else {
        return out;
    };
    if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
        for p in rows.flatten() {
            out.insert(p);
        }
    }
    out
}

/// Every DISTINCT name key present in the `refs` table. The general
/// pre-prune set for `--heatmap`'s per-declaration references projection: a
/// declaration whose name key is ABSENT here has no reference row in any
/// indexed file, so — because rows over-approximate references — the
/// projection is provably empty and the walk can be skipped. Retrieval only;
/// the caller owns the coverage decision (trust "absent ⇒ zero references"
/// only when the store actually covers the files the walk would scan).
pub fn names_with_ref_rows(conn: &Connection) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let Ok(mut stmt) =
        conn.prepare("SELECT DISTINCT s.s FROM refs r JOIN strings s ON s.str_id = r.name_id")
    else {
        return out;
    };
    if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
        for n in rows.flatten() {
            out.insert(n);
        }
    }
    out
}

/// One unused-exported symbol row: an `@EXPORT`/`@EXPORT_OK` name with no
/// reference row in any OTHER file. Carries the identity the caller matches a
/// reported symbol against — path + name + the selection-span start.
pub struct DeadExportRow {
    pub path: String,
    pub name: String,
    pub start_row: usize,
    pub start_col: usize,
}

/// The unused-exports view (`docs/adr/relational-ref-index.md`): every
/// WORKSPACE-tier symbol row flagged exported (`SymRowSeed::FLAG_EXPORTED`)
/// whose name key has ZERO ref rows in any OTHER file. Same-file refs are
/// excluded on purpose — a module calling its own exported sub does not make
/// that export live for a *consumer*.
///
/// The result is SOUND IN EXACTLY ONE DIRECTION, and the asymmetry is the
/// point. Ref rows are name-match CANDIDATES — an over-approximation of real
/// references, since the per-`RefKind` matcher still runs per row — so:
///   * zero cross-file candidate rows ⇒ no cross-file reference can exist ⇒
///     the export is TRULY unused by any consumer (a sound "dead export").
///   * one or more candidate rows ⇒ UNKNOWN: a candidate may or may not
///     survive the matcher. Never read this as "used".
/// The right polarity for a dead-export review queue: it never fabricates a
/// dead export; at worst it MISSES one whose sole consumer's candidate row
/// would not have survived the matcher.
pub fn unused_exported_syms(conn: &Connection) -> Vec<DeadExportRow> {
    let mut out = Vec::new();
    let Ok(mut stmt) = conn.prepare(
        "SELECT f.path, n.s, y.start_row, y.start_col
           FROM syms y
           JOIN files f ON f.file_id = y.file_id
           JOIN strings n ON n.str_id = y.name_id
          WHERE f.source = 'workspace'
            AND (y.flags & ?1) != 0
            AND NOT EXISTS (
                  SELECT 1 FROM refs r
                   WHERE r.name_id = y.name_id
                     AND r.file_id != y.file_id
                )",
    ) else {
        return out;
    };
    let flag = crate::model::file_analysis::SymRowSeed::FLAG_EXPORTED as i64;
    let rows = stmt.query_map(params![flag], |row| {
        Ok(DeadExportRow {
            path: row.get(0)?,
            name: row.get(1)?,
            start_row: row.get::<_, i64>(2)? as usize,
            start_col: row.get::<_, i64>(3)? as usize,
        })
    });
    if let Ok(rows) = rows {
        for r in rows {
            match r {
                Ok(hit) => out.push(hit),
                Err(e) => {
                    log::warn!("unused-exports scan aborted mid-iteration: {}", e);
                    break;
                }
            }
        }
    }
    out
}
