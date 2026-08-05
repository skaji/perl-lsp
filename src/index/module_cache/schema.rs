//! Schema and generation gates: table DDL, the version stamps
//! (schema / extract / ref-rows), and the meta-keyed hard-clear
//! validators (@INC, plugin set, analysis inputs) plus builtins
//! hydration (same meta-row pattern).

use super::*;

const SCHEMA_VERSION: &str = "9";

/// Bumped when the builder's analysis output changes shape in a way that
/// invalidates cached blobs. Unlike `SCHEMA_VERSION`, this does not drop
/// the table — stale entries are re-resolved lazily with priority.
pub const EXTRACT_VERSION: i64 = 178;

/// Bumped when the ROW format of the relational ref index changes shape.
/// Unlike `EXTRACT_VERSION` (which governs the blobs), a mismatch only wipes
/// the derived `refs`/`files`/`strings` tables — the blobs stay valid and the
/// next warm re-shreds rows from the already-decoded analyses for free.
pub(super) const REF_ROWS_VERSION: &str = "5";

pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS modules (
            module_name      TEXT PRIMARY KEY,
            path             TEXT NOT NULL,
            mtime_secs       INTEGER NOT NULL,
            file_size        INTEGER NOT NULL,
            source           TEXT NOT NULL DEFAULT 'import',
            analysis         BLOB,
            extract_version  INTEGER NOT NULL DEFAULT 0,
            deps_stamp       INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS builtins (
            name TEXT PRIMARY KEY,
            doc  TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS files (
            file_id INTEGER PRIMARY KEY,
            path    TEXT NOT NULL UNIQUE,
            source  TEXT NOT NULL DEFAULT 'import'
        );
        CREATE TABLE IF NOT EXISTS strings (
            str_id INTEGER PRIMARY KEY,
            s      TEXT NOT NULL UNIQUE
        );
        CREATE TABLE IF NOT EXISTS refs (
            file_id   INTEGER NOT NULL,
            name_id   INTEGER NOT NULL,
            kind      INTEGER NOT NULL,
            start_row INTEGER NOT NULL,
            start_col INTEGER NOT NULL,
            end_row   INTEGER NOT NULL,
            end_col   INTEGER NOT NULL,
            access    INTEGER NOT NULL,
            flags     INTEGER NOT NULL,
            qual_kind INTEGER NOT NULL,
            qual_id   INTEGER,
            arg_count INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_refs_name ON refs(name_id);
        CREATE INDEX IF NOT EXISTS idx_refs_file ON refs(file_id);
        CREATE TABLE IF NOT EXISTS syms (
            file_id      INTEGER NOT NULL,
            name_id      INTEGER NOT NULL,
            kind         INTEGER NOT NULL,
            start_row    INTEGER NOT NULL,
            start_col    INTEGER NOT NULL,
            end_row      INTEGER NOT NULL,
            end_col      INTEGER NOT NULL,
            container_id INTEGER,
            flags        INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_syms_name ON syms(name_id);
        CREATE INDEX IF NOT EXISTS idx_syms_file ON syms(file_id);
        CREATE TABLE IF NOT EXISTS stubs (
            path TEXT PRIMARY KEY,
            stub BLOB NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_modules_path ON modules(path);",
    )?;
    // Row-format generation for the derived tables (see REF_ROWS_VERSION).
    let rows_version: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'ref_rows_version'",
            [],
            |row| row.get(0),
        )
        .ok();
    // The stamp alone is trusted too far: a DB stamped current by a build
    // whose migration didn't actually reshape the tables would never
    // re-migrate, leaving every shred failing on a missing column while
    // composition quietly masks it (refs stay resident, retrieval dead,
    // diagnostics typeless). Probe the shape the current format requires so
    // a lying stamp still triggers the rebuild.
    let shape_ok = conn
        .prepare("SELECT source FROM files LIMIT 1")
        .map(|_| ())
        .and_then(|_| conn.prepare("SELECT qual_kind FROM refs LIMIT 1").map(|_| ()))
        .and_then(|_| conn.prepare("SELECT flags FROM syms LIMIT 1").map(|_| ()))
        .is_ok();
    if rows_version.as_deref() != Some(REF_ROWS_VERSION) || !shape_ok {
        // DROP, not DELETE: a format change may alter the table SHAPE, and
        // `CREATE TABLE IF NOT EXISTS` above no-ops on the old shape — a
        // row-only wipe would leave every future shred failing on a missing
        // column while composition quietly masks it (refs stay resident,
        // retrieval dead). Recreate from scratch.
        conn.execute_batch(
            "DROP TABLE IF EXISTS refs;
             DROP TABLE IF EXISTS syms;
             DROP TABLE IF EXISTS files;
             DROP TABLE IF EXISTS strings;
             CREATE TABLE files (
                file_id INTEGER PRIMARY KEY,
                path    TEXT NOT NULL UNIQUE,
                source  TEXT NOT NULL DEFAULT 'import'
             );
             CREATE TABLE strings (
                str_id INTEGER PRIMARY KEY,
                s      TEXT NOT NULL UNIQUE
             );
             CREATE TABLE refs (
                file_id   INTEGER NOT NULL,
                name_id   INTEGER NOT NULL,
                kind      INTEGER NOT NULL,
                start_row INTEGER NOT NULL,
                start_col INTEGER NOT NULL,
                end_row   INTEGER NOT NULL,
                end_col   INTEGER NOT NULL,
                access    INTEGER NOT NULL,
                flags     INTEGER NOT NULL,
                qual_kind INTEGER NOT NULL,
                qual_id   INTEGER,
                arg_count INTEGER
             );
             CREATE INDEX idx_refs_name ON refs(name_id);
             CREATE INDEX idx_refs_file ON refs(file_id);
             CREATE TABLE syms (
                file_id      INTEGER NOT NULL,
                name_id      INTEGER NOT NULL,
                kind         INTEGER NOT NULL,
                start_row    INTEGER NOT NULL,
                start_col    INTEGER NOT NULL,
                end_row      INTEGER NOT NULL,
                end_col      INTEGER NOT NULL,
                container_id INTEGER,
                flags        INTEGER NOT NULL
             );
             CREATE INDEX idx_syms_name ON syms(name_id);
             CREATE INDEX idx_syms_file ON syms(file_id);",
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('ref_rows_version', ?1)",
            params![REF_ROWS_VERSION],
        )?;
    }
    // Pre-existing tables (same schema version) predate `deps_stamp`; add it
    // in place rather than bumping SCHEMA_VERSION (a bump drops every row —
    // old rows carry 0, which validates only for empty-closure analyses, so
    // stale pack rows re-analyze while Perl caches survive the upgrade).
    let _ = conn.execute_batch(
        "ALTER TABLE modules ADD COLUMN deps_stamp INTEGER NOT NULL DEFAULT 0;",
    );
    // Stub generation gate — stamped here so every fresh DB is writable by
    // the persist writers (their per-chunk `stub_version_current` check
    // would otherwise fail-closed until the first warm scan stamped it).
    validate_stub_version(conn);

    let version: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .ok();

    match version.as_deref() {
        Some(SCHEMA_VERSION) => Ok(()),
        Some(_) => {
            conn.execute_batch("DROP TABLE IF EXISTS modules;")?;
            clear_derived_rows(conn)?;
            conn.execute_batch(
                "CREATE TABLE modules (
                    module_name      TEXT PRIMARY KEY,
                    path             TEXT NOT NULL,
                    mtime_secs       INTEGER NOT NULL,
                    file_size        INTEGER NOT NULL,
                    source           TEXT NOT NULL DEFAULT 'import',
                    analysis         BLOB,
                    extract_version  INTEGER NOT NULL DEFAULT 0,
                    deps_stamp       INTEGER NOT NULL DEFAULT 0
                );",
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)",
                params![SCHEMA_VERSION],
            )?;
            Ok(())
        }
        None => {
            conn.execute(
                "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)",
                params![SCHEMA_VERSION],
            )?;
            Ok(())
        }
    }
}

/// Wipe the derived relational tables (`refs`/`files`/`strings`). Runs
/// alongside every `DELETE FROM modules` hard-clear: the rows are shredded
/// from the blobs, so a generation that invalidates the blobs invalidates
/// the rows with it. Cheap to rebuild — the next warm re-shreds from the
/// decoded analyses it is loading anyway.
pub fn clear_derived_rows(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "DELETE FROM refs; DELETE FROM syms; DELETE FROM files; DELETE FROM strings; \
         DELETE FROM stubs;",
    )
}

pub fn compute_inc_hash(inc_paths: &[PathBuf]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    for p in inc_paths {
        p.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

pub fn validate_inc_paths(conn: &Connection, inc_paths: &[PathBuf]) -> rusqlite::Result<()> {
    let current_hash = compute_inc_hash(inc_paths);
    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'inc_hash'",
            [],
            |row| row.get(0),
        )
        .ok();

    if stored.as_deref() != Some(&current_hash) {
        log::info!(
            "@INC changed (was {:?}, now {}), clearing module cache",
            stored,
            current_hash
        );
        // Import-tier only: workspace blobs bake plugin emissions and their
        // own source, not @INC paths — and the workspace indexer may have
        // written its rows BEFORE this validation runs (two writers, one
        // DB), so anything broader would delete rows mid-write and leave
        // already-evicted resident copies with no retrieval source.
        conn.execute("DELETE FROM modules WHERE source = 'import'", [])?;
        conn.execute(
            "DELETE FROM refs WHERE file_id IN (SELECT file_id FROM files WHERE source = 'import')",
            [],
        )?;
        conn.execute(
            "DELETE FROM syms WHERE file_id IN (SELECT file_id FROM files WHERE source = 'import')",
            [],
        )?;
        conn.execute("DELETE FROM files WHERE source = 'import'", [])?;
        conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('inc_hash', ?1)",
            params![current_hash],
        )?;
    }
    Ok(())
}

/// Hydrate the in-memory `builtins` mirror from SQLite, parsing
/// `perlfunc.pod` and writing rows on first use (or when the perl
/// version tag changes since the last run). Returns the populated
/// map. Keyed in `meta` under `builtins_perl_version`: mismatch wipes
/// the table and re-parses, same pattern as `validate_inc_paths` /
/// `validate_plugin_fingerprint`.
pub fn hydrate_builtins(conn: &Connection) -> rusqlite::Result<DashMap<String, String>> {
    let map: DashMap<String, String> = DashMap::new();

    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'builtins_perl_version'",
            [],
            |row| row.get(0),
        )
        .ok();

    let parsed = crate::index::builtins_pod::parse_perlfunc();

    let need_parse = match (&stored, &parsed) {
        (Some(s), Some(p)) => *s != p.perl_version,
        (None, Some(_)) => true,
        _ => false, // no parse + no cache rows we trust → leave map empty
    };

    if need_parse {
        if let Some(p) = parsed.as_ref() {
            conn.execute("DELETE FROM builtins", [])?;
            let tx = conn.unchecked_transaction()?;
            {
                let mut stmt = tx.prepare("INSERT INTO builtins (name, doc) VALUES (?1, ?2)")?;
                for (name, doc) in &p.entries {
                    stmt.execute(params![name, doc])?;
                }
            }
            tx.commit()?;
            conn.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('builtins_perl_version', ?1)",
                params![p.perl_version],
            )?;
            log::info!("Indexed {} Perl builtins from {}", p.entries.len(), p.perl_version);
        }
    }

    // Read whatever's in the table now (either freshly written, or
    // the same rows from a prior run) into the in-memory mirror.
    let mut stmt = conn.prepare("SELECT name, doc FROM builtins")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for r in rows {
        if let Ok((name, doc)) = r {
            map.insert(name, doc);
        }
    }
    Ok(map)
}

/// Drop the module cache when the plugin set has changed since the last
/// run. `fingerprint` is the value returned by
/// `plugin::rhai_host::plugin_fingerprint()` — a hash over bundled
/// plugin sources plus every `.rhai` in `$PERL_LSP_PLUGIN_DIR`.
///
/// Without this check, a plugin author who edits a `.rhai`, restarts
/// the LSP, and inspects a cross-file query will see the *old*
/// plugin's emissions in the cached `FileAnalysis` blobs — making
/// plugin QA impossible. Mirrors `validate_inc_paths`: same meta-row
/// pattern, same hard-clear on mismatch.
pub fn validate_plugin_fingerprint(conn: &Connection, fingerprint: &str) -> rusqlite::Result<()> {
    // IMMEDIATE: check-and-stamp must be atomic against the other writer
    // (resolver thread vs workspace indexer) — two validators both reading
    // a missing stamp would both hard-clear, the second deleting rows the
    // first writer committed in between.
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = validate_plugin_fingerprint_inner(conn, fingerprint);
    match &result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(_) => {
            let _ = conn.execute_batch("ROLLBACK");
        }
    }
    result
}

fn validate_plugin_fingerprint_inner(conn: &Connection, fingerprint: &str) -> rusqlite::Result<()> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'plugin_fingerprint'",
            [],
            |row| row.get(0),
        )
        .ok();

    if stored.as_deref() != Some(fingerprint) {
        log::info!(
            "Plugin set changed (was {:?}, now {}), clearing module cache",
            stored,
            fingerprint
        );
        conn.execute("DELETE FROM modules", [])?;
        clear_derived_rows(conn)?;
        conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('plugin_fingerprint', ?1)",
            params![fingerprint],
        )?;
    }
    Ok(())
}

/// Drop the modules table when the driver's external analysis inputs (the
/// C++ toolchain: system include roots, predefined macros — or its probe
/// FAILURE) changed since the rows were written. Same meta-row pattern as
/// `validate_inc_paths`: a generation built under degraded/different inputs
/// must not be served under the current ones (H8).
pub fn validate_input_fingerprint(conn: &Connection, fingerprint: u64) -> rusqlite::Result<()> {
    // Same atomic check-and-stamp rationale as `validate_plugin_fingerprint`.
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = validate_input_fingerprint_inner(conn, fingerprint);
    match &result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(_) => {
            let _ = conn.execute_batch("ROLLBACK");
        }
    }
    result
}

fn validate_input_fingerprint_inner(conn: &Connection, fingerprint: u64) -> rusqlite::Result<()> {
    let fingerprint = format!("{:016x}", fingerprint);
    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'input_fingerprint'",
            [],
            |row| row.get(0),
        )
        .ok();

    if stored.as_deref() != Some(&fingerprint) {
        log::info!(
            "Analysis inputs changed (was {:?}, now {}), clearing module cache",
            stored,
            fingerprint
        );
        conn.execute("DELETE FROM modules", [])?;
        clear_derived_rows(conn)?;
        conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('input_fingerprint', ?1)",
            params![fingerprint],
        )?;
    }
    Ok(())
}
