use super::*;
use rusqlite::Connection;

fn test_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();
    conn
}

fn parse_source_to_cached(source: &str, path: &std::path::Path) -> Arc<CachedModule> {
    use tree_sitter::Parser;
    let mut parser = Parser::new();
    parser
        .set_language(&ts_parser_perl::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let fa = crate::builder::build(&tree, source.as_bytes());
    Arc::new(CachedModule::new(path.to_path_buf(), Arc::new(fa)))
}

/// Slice-2: `load_one` decodes a single persisted analysis BY PATH with its
/// full witness bag present — the rehydration primitive. A resident copy may
/// have had its bag evicted, but the on-disk blob is whole, so `load_one`
/// resurrects it.
#[test]
fn load_one_rehydrates_full_bag() {
    let conn = test_db();
    let dir = std::env::temp_dir();
    let pm = dir.join("TestModule_load_one.pm");
    std::fs::write(&pm, "package L;\nsub f { my $s = shift; return 'x'; }\n1;\n").unwrap();
    let source = std::fs::read_to_string(&pm).unwrap();
    let cached = parse_source_to_cached(&source, &pm);
    // Sanity: the freshly built analysis has a populated bag.
    assert!(!cached.analysis.witnesses.is_empty());
    save_to_db(&conn, &pm.to_string_lossy(), &Some(cached.clone()), "workspace");

    let loaded = load_one(&conn, &pm.to_string_lossy()).expect("row should decode");
    assert!(!loaded.bag_is_evicted());
    assert!(
        !loaded.witnesses.is_empty(),
        "load_one must return the full bag, not an evicted one"
    );
    assert_eq!(loaded.witnesses.len(), cached.analysis.witnesses.len());
    // A path with no row yields None (miss → caller degrades to bag-less).
    assert!(load_one(&conn, "/no/such/path.pm").is_none());

    let _ = std::fs::remove_file(&pm);
}

#[test]
fn test_db_save_and_load_roundtrip() {
    let conn = test_db();
    let dir = std::env::temp_dir();
    let pm = dir.join("TestModule_roundtrip.pm");
    std::fs::write(&pm, "package TestModule;\nour @EXPORT = qw(foo bar);\nour @EXPORT_OK = qw(baz);\nsub foo { 1 }\nsub bar { 2 }\nsub baz { 3 }\n1;\n").unwrap();

    let source = std::fs::read_to_string(&pm).unwrap();
    let cached = Some(parse_source_to_cached(&source, &pm));
    save_to_db(&conn, "TestModule", &cached, "import");

    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let (n, stale) = warm_cache(&conn, &cache, false);
    assert_eq!(n, 1);
    assert!(stale.is_empty());

    let loaded = cache.get("TestModule").unwrap();
    let loaded = loaded.as_ref().unwrap();
    assert_eq!(loaded.analysis.export, vec!["foo", "bar"]);
    assert_eq!(loaded.analysis.export_ok, vec!["baz"]);

    let _ = std::fs::remove_file(&pm);
}

/// Pin-the-fix: `plugin_namespaces` survives the bincode +
/// zstd + SQLite round trip with entities, bridges, and
/// plugin_id intact. Without this test, schema drift on the
/// PluginNamespace struct would silently truncate cached
/// modules and we'd notice only when cross-file bridge lookups
/// mysteriously missed entries.
#[test]
fn test_db_plugin_namespaces_roundtrip() {
    let conn = test_db();
    let dir = std::env::temp_dir();
    let pm = dir.join("TestMojoApp_namespaces.pm");
    // A Mojolicious::Lite script — mojo-lite + mojo-routes +
    // mojo-helpers should all emit namespaces that round-trip.
    std::fs::write(
        &pm,
        "package TestMojoApp;\n\
             use Mojolicious::Lite;\n\
             app->helper(current_user => sub { my ($c) = @_; });\n\
             get '/users' => sub { my $c = shift; };\n\
             1;\n",
    )
    .unwrap();

    let source = std::fs::read_to_string(&pm).unwrap();
    let cached = Some(parse_source_to_cached(&source, &pm));
    let original_ns_count = cached.as_ref().unwrap().analysis.plugin_namespaces.len();
    assert!(
        original_ns_count > 0,
        "sanity: fixture must produce at least one PluginNamespace"
    );

    save_to_db(&conn, "TestMojoApp", &cached, "import");

    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let (n, stale) = warm_cache(&conn, &cache, false);
    assert_eq!(n, 1);
    assert!(stale.is_empty(), "fresh insert should not be stale");

    let loaded = cache.get("TestMojoApp").unwrap();
    let loaded = loaded.as_ref().unwrap();
    let loaded_ns = &loaded.analysis.plugin_namespaces;
    assert_eq!(
        loaded_ns.len(),
        original_ns_count,
        "PluginNamespace count must round-trip; got: {:?}",
        loaded_ns
    );

    // Every namespace must preserve its plugin_id, kind, and at
    // least one Bridge::Class — the three fields that `bridges_index`
    // and `for_each_entity_bridged_to` depend on.
    for ns in loaded_ns {
        assert!(!ns.plugin_id.is_empty(), "plugin_id preserved");
        assert!(!ns.kind.is_empty(), "kind preserved");
        assert!(!ns.bridges.is_empty(), "bridges preserved");
        assert!(
            ns.bridges
                .iter()
                .any(|b| matches!(b, crate::file_analysis::Bridge::Class(_))),
            "at least one Class bridge survives"
        );
    }

    let _ = std::fs::remove_file(&pm);
}

#[test]
fn test_db_negative_result_roundtrip() {
    let conn = test_db();
    save_to_db(&conn, "Nonexistent::Module", &None, "import");

    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, false);
    assert_eq!(n, 1);

    let entry = cache.get("Nonexistent::Module").unwrap();
    assert!(entry.is_none());
}

#[test]
fn test_db_stale_entry_skipped() {
    let conn = test_db();

    let dir = std::env::temp_dir();
    let pm = dir.join("StaleModule_v9.pm");
    std::fs::write(
        &pm,
        "package StaleModule;\nour @EXPORT_OK = qw(old);\nsub old {}\n1;\n",
    )
    .unwrap();

    let source = std::fs::read_to_string(&pm).unwrap();
    let cached = Some(parse_source_to_cached(&source, &pm));
    save_to_db(&conn, "StaleModule", &cached, "import");

    std::thread::sleep(std::time::Duration::from_secs(1));
    std::fs::write(
        &pm,
        "package StaleModule;\nour @EXPORT_OK = qw(v2 with more content);\n1;\n",
    )
    .unwrap();

    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, false);
    assert_eq!(n, 0, "stale entry should not be loaded");
    assert!(!cache.contains_key("StaleModule"));

    let _ = std::fs::remove_file(&pm);
}

#[test]
fn test_db_plugin_fingerprint_invalidation() {
    let conn = test_db();

    // First run: claims plugin set fingerprint "hash-A".
    validate_plugin_fingerprint(&conn, "hash-A").unwrap();
    save_to_db(&conn, "Foo", &None, "import");

    // Same fingerprint → cache survives.
    validate_plugin_fingerprint(&conn, "hash-A").unwrap();
    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, false);
    assert_eq!(n, 1, "cache should survive identical fingerprint");

    // Plugin set changed → cache cleared.
    validate_plugin_fingerprint(&conn, "hash-B").unwrap();
    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, false);
    assert_eq!(n, 0, "cache should be empty after plugin set change");

    // Stamp persists — second run with hash-B doesn't re-clear.
    save_to_db(&conn, "Bar", &None, "import");
    validate_plugin_fingerprint(&conn, "hash-B").unwrap();
    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, false);
    assert_eq!(n, 1, "stamp should persist between same-fingerprint runs");
}

#[test]
fn test_db_inc_hash_invalidation() {
    let conn = test_db();
    let paths1 = vec![PathBuf::from("/usr/lib/perl5")];
    let paths2 = vec![
        PathBuf::from("/usr/lib/perl5"),
        PathBuf::from("/home/user/lib"),
    ];

    validate_inc_paths(&conn, &paths1).unwrap();
    save_to_db(&conn, "Foo", &None, "import");

    validate_inc_paths(&conn, &paths2).unwrap();
    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, false);
    assert_eq!(n, 0, "cache should be empty after @INC change");
}

#[test]
fn test_db_schema_version_migration() {
    let conn = test_db();

    conn.execute(
        "UPDATE meta SET value = '0' WHERE key = 'schema_version'",
        [],
    )
    .unwrap();
    save_to_db(&conn, "OldModule", &None, "import");

    init_schema(&conn).unwrap();
    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, false);
    assert_eq!(n, 0, "old data should be gone after migration");
}

#[test]
fn test_db_source_column() {
    let conn = test_db();
    let dir = std::env::temp_dir();
    let pm = dir.join("SourceTest_v9.pm");
    std::fs::write(
        &pm,
        "package SourceTest;\nour @EXPORT_OK = qw(foo);\nsub foo {}\n1;\n",
    )
    .unwrap();

    let source = std::fs::read_to_string(&pm).unwrap();
    let cached = Some(parse_source_to_cached(&source, &pm));
    save_to_db(&conn, "SourceTest", &cached, "cpanfile");

    let source_val: String = conn
        .query_row(
            "SELECT source FROM modules WHERE module_name = 'SourceTest'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(source_val, "cpanfile");

    let _ = std::fs::remove_file(&pm);
}

#[test]
fn test_workspace_cache_dir_uniqueness() {
    let d1 = cache_dir_for_workspace(Some("file:///home/user/project-a"));
    let d2 = cache_dir_for_workspace(Some("file:///home/user/project-b"));
    let d_none = cache_dir_for_workspace(None);
    assert_ne!(d1, d2, "Different roots should produce different paths");
    assert_ne!(d1, d_none, "Root vs no-root should differ");
    assert_eq!(
        d1,
        cache_dir_for_workspace(Some("file:///home/user/project-a")),
        "Same root should produce same path"
    );
}

#[test]
fn test_full_file_analysis_survives_roundtrip() {
    // Verify that FileAnalysis fields lost in the old ModuleExports representation
    // (refs, type_constraints, call_bindings, full package_parents) now survive.
    let conn = test_db();
    let dir = std::env::temp_dir();
    let pm = dir.join("Fidelity_v9.pm");
    std::fs::write(
            &pm,
            "package Fidelity;\nuse parent 'Base';\nour @EXPORT_OK = qw(make);\nsub make { return { host => 1, port => 2 } }\n1;\n",
        )
        .unwrap();

    let source = std::fs::read_to_string(&pm).unwrap();
    let cached = parse_source_to_cached(&source, &pm);
    let original_refs_count = cached.analysis.refs.len();
    let original_package_parents = cached.analysis.package_parents.clone();
    save_to_db(&conn, "Fidelity", &Some(Arc::clone(&cached)), "import");

    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, false);
    assert_eq!(n, 1);

    let loaded = cache.get("Fidelity").unwrap();
    let loaded = loaded.as_ref().unwrap();
    assert_eq!(
        loaded.analysis.refs.len(),
        original_refs_count,
        "refs survive roundtrip"
    );
    assert_eq!(
        loaded.analysis.package_parents, original_package_parents,
        "package_parents survive"
    );

    let _ = std::fs::remove_file(&pm);
}

/// M1: two same-length writes within the same whole second must still
/// invalidate the row — the stamp is nanosecond-mtime + size, not whole
/// seconds. Retries until both writes land in one second so the assertion
/// exercises exactly the old failure window.
#[test]
fn same_second_same_size_rewrite_invalidates_row() {
    let conn = test_db();
    let dir = std::env::temp_dir();
    let pm = dir.join("SubSecond_m1.pm");
    let secs = |t: std::time::SystemTime| {
        t.duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap().as_secs()
    };
    for _ in 0..20 {
        std::fs::write(&pm, "package SubSecond;\nsub a { 1 }\n1;\n").unwrap();
        let s1 = std::fs::metadata(&pm).unwrap().modified().unwrap();
        let source = std::fs::read_to_string(&pm).unwrap();
        let cached = Some(parse_source_to_cached(&source, &pm));
        save_to_db(&conn, "SubSecond", &cached, "import");
        // Same byte length, different content.
        std::fs::write(&pm, "package SubSecond;\nsub b { 2 }\n1;\n").unwrap();
        let s2 = std::fs::metadata(&pm).unwrap().modified().unwrap();
        // Require DIFFERENT nanos within the SAME second: that's the window
        // the nanosecond stamp fixed. Two writes inside one clock tick get
        // identical mtimes — the stamp's residual one-tick blind spot, not
        // the regression under test — so retry those instead of asserting.
        if secs(s1) == secs(s2) && s1 != s2 {
            let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
            let (n, _) = warm_cache(&conn, &cache, false);
            assert_eq!(n, 0, "same-second same-size rewrite must invalidate the row");
            let _ = std::fs::remove_file(&pm);
            return;
        }
        conn.execute("DELETE FROM modules", []).unwrap();
    }
    panic!("could not land both writes in one second");
}

/// M2: a consumer row is valid only while its whole include closure is
/// unchanged — its OWN (stamp, size) can't see a header edit, the
/// deps_stamp must.
#[test]
fn header_change_invalidates_consumer_row_via_deps_stamp() {
    let conn = test_db();
    let dir = std::env::temp_dir();
    let hdr = dir.join("dep_hdr_m2.h");
    std::fs::write(&hdr, "#define LIMIT 5\n").unwrap();
    let hdr_canon = hdr.canonicalize().unwrap().to_string_lossy().into_owned();
    let pm = dir.join("dep_consumer_m2.pm");
    std::fs::write(&pm, "package Consumer;\n1;\n").unwrap();

    let source = std::fs::read_to_string(&pm).unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&ts_parser_perl::LANGUAGE.into()).unwrap();
    let tree = parser.parse(&source, None).unwrap();
    let mut fa = crate::builder::build(&tree, source.as_bytes());
    fa.include_closure =
        crate::file_analysis::path_intern::ClosureList::from_iter(std::iter::once(hdr_canon.as_str()));
    let cached = Some(Arc::new(CachedModule::new(pm.clone(), Arc::new(fa))));
    // warm_cache serves the 'import' tier ('workspace' rows stream through
    // warm_cache_streaming); the deps_stamp semantics under test are
    // source-independent.
    save_to_db(&conn, "Consumer", &cached, "import");

    // Unchanged closure → row warms.
    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, false);
    assert_eq!(n, 1, "row valid while the closure is unchanged");

    // Header changes; the consumer file itself does not.
    std::fs::write(&hdr, "#define LIMIT 5\n#define LIMIT2 7\n").unwrap();
    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, false);
    assert_eq!(n, 0, "header edit must invalidate the consumer's row");

    let _ = std::fs::remove_file(&pm);
    let _ = std::fs::remove_file(&hdr);
}

/// H8: a degraded analysis (parse/extract failure, skipped gather) must
/// never be persisted — the row would validate on the source stamp alone
/// and re-serve the degraded blob every future session.
#[test]
fn degraded_analysis_is_not_persisted() {
    let conn = test_db();
    let dir = std::env::temp_dir();
    let pm = dir.join("Degraded_h8.pm");
    std::fs::write(&pm, "package Degraded;\n1;\n").unwrap();

    let source = std::fs::read_to_string(&pm).unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&ts_parser_perl::LANGUAGE.into()).unwrap();
    let tree = parser.parse(&source, None).unwrap();
    let mut fa = crate::builder::build(&tree, source.as_bytes());
    fa.degraded = true;
    let cached = Some(Arc::new(CachedModule::new(pm.clone(), Arc::new(fa))));
    save_to_db(&conn, "Degraded", &cached, "workspace");

    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM modules", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0, "degraded analyses must not reach the persist tier");
    let _ = std::fs::remove_file(&pm);
}

/// H8: the analysis-input fingerprint (toolchain identity, including its
/// probe FAILURE) hard-clears the table on change — a generation built
/// under degraded/different inputs is never warmed under the current ones.
#[test]
fn input_fingerprint_change_clears_table() {
    let conn = test_db();
    validate_input_fingerprint(&conn, 0xA).unwrap();
    save_to_db(&conn, "Foo", &None, "import");

    validate_input_fingerprint(&conn, 0xA).unwrap();
    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, false);
    assert_eq!(n, 1, "same inputs: cache survives");

    validate_input_fingerprint(&conn, 0xB).unwrap();
    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, false);
    assert_eq!(n, 0, "changed inputs: table cleared");
}

/// Relational ref index: shred → candidate-file retrieval round-trip, the
/// re-shred replaces (never accumulates), and per-file deletion.
#[test]
fn shred_ref_rows_roundtrip() {
    let conn = test_db();
    let source = "package S;\nsub helper { 1 }\nsub caller_a { helper(); helper(); }\n1;\n";
    let dir = std::env::temp_dir();
    let pm = dir.join("TestModule_shred.pm");
    std::fs::write(&pm, source).unwrap();
    let cached = parse_source_to_cached(source, &pm);
    let path_str = pm.to_string_lossy().to_string();

    assert!(!has_ref_rows(&conn, &path_str));
    let seeds: Vec<_> = cached.analysis.refs.iter().map(|r| r.row_seed()).collect();
    assert!(!seeds.is_empty(), "call sites must produce row seeds");
    shred_derived_rows(&conn, &path_str, "workspace", &seeds, &[]).unwrap();
    assert!(has_ref_rows(&conn, &path_str));

    // Retrieval by the match key finds the file; an unknown key finds nothing.
    let hits = ref_candidate_files(&conn, &["helper".to_string()]);
    assert_eq!(hits, vec![path_str.clone()]);
    assert!(ref_candidate_files(&conn, &["nonesuch".to_string()]).is_empty());
    let n = ref_count_named(&conn, "helper");
    assert!(n >= 2, "two call sites expected, got {n}");

    // Re-shred replaces: same seeds again must not double the rows.
    shred_derived_rows(&conn, &path_str, "workspace", &seeds, &[]).unwrap();
    assert_eq!(ref_count_named(&conn, "helper"), n);

    // A zero-ref shred still marks the file as shredded (the backfill marker).
    let other = dir.join("TestModule_shred_empty.pm");
    shred_derived_rows(&conn, &other.to_string_lossy(), "workspace", &[], &[]).unwrap();
    assert!(has_ref_rows(&conn, &other.to_string_lossy()));

    delete_ref_rows(&conn, &path_str);
    assert!(!has_ref_rows(&conn, &path_str));
    assert!(ref_candidate_files(&conn, &["helper".to_string()]).is_empty());

    let _ = std::fs::remove_file(&pm);
}

/// Symbol rows ride the same shred generation as refs: written together,
/// replaced together (never accumulated), erased together.
#[test]
fn shred_sym_rows_same_generation() {
    let conn = test_db();
    let source = "package S;\nsub helper { 1 }\nsub caller_a { helper(); }\n1;\n";
    let dir = std::env::temp_dir();
    let pm = dir.join("TestModule_symshred.pm");
    std::fs::write(&pm, source).unwrap();
    let cached = parse_source_to_cached(source, &pm);
    let path_str = pm.to_string_lossy().to_string();

    let seeds: Vec<_> = cached.analysis.refs.iter().map(|r| r.row_seed()).collect();
    let sym_seeds = cached.analysis.sym_row_seeds();
    assert!(
        sym_seeds.iter().any(|s| s.name == "helper"),
        "sub symbols must project into row seeds; got {:?}",
        sym_seeds.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
    shred_derived_rows(&conn, &path_str, "workspace", &seeds, &sym_seeds).unwrap();
    let count = |conn: &Connection| -> i64 {
        conn.query_row("SELECT COUNT(*) FROM syms", [], |r| r.get(0)).unwrap()
    };
    let n = count(&conn);
    assert!(n >= 2, "expected sym rows for S's subs, got {n}");

    // Re-shred replaces.
    shred_derived_rows(&conn, &path_str, "workspace", &seeds, &sym_seeds).unwrap();
    assert_eq!(count(&conn), n);

    // Deletion takes both families.
    delete_ref_rows(&conn, &path_str);
    assert_eq!(count(&conn), 0);

    let _ = std::fs::remove_file(&pm);
}

/// The row seeds must key by the same spelling retrieval probes: qualified
/// calls key by their bare tail, sigil variables keep the sigil.
#[test]
fn ref_row_seed_match_keys() {
    let source = "package K;\nour $x = 1;\nFoo::Bar::baz();\nprint $Foo::Bar::x;\n1;\n";
    let dir = std::env::temp_dir();
    let pm = dir.join("TestModule_keys.pm");
    std::fs::write(&pm, source).unwrap();
    let cached = parse_source_to_cached(source, &pm);
    let keys: Vec<String> = cached.analysis.refs.iter().map(|r| r.match_key()).collect();
    assert!(
        keys.iter().any(|k| k == "baz"),
        "qualified call keys by bare tail; got {keys:?}"
    );
    assert!(
        keys.iter().any(|k| k == "$x"),
        "qualified sigil var keys by sigil+base; got {keys:?}"
    );
    assert!(
        !keys.iter().any(|k| k.contains("::")),
        "no qualified spellings in match keys; got {keys:?}"
    );
    let _ = std::fs::remove_file(&pm);
}

/// Hard-clears (inc hash / plugin fingerprint / input fingerprint) must wipe
/// the derived row tables together with the blobs they derive from.
#[test]
fn hard_clear_wipes_derived_rows() {
    let conn = test_db();
    let seeds = vec![crate::file_analysis::RefRowSeed {
        key: "k".into(),
        kind: 1,
        span: crate::file_analysis::Span {
            start: tree_sitter::Point { row: 0, column: 0 },
            end: tree_sitter::Point { row: 0, column: 1 },
        },
        access: 0,
        flags: 0,
        qual_kind: 0,
        qual: None,
        arg_count: None,
    }];
    shred_derived_rows(&conn, "/some/file.pm", "workspace", &seeds, &[]).unwrap();
    assert!(has_ref_rows(&conn, "/some/file.pm"));
    validate_plugin_fingerprint(&conn, "fingerprint-a").unwrap();
    validate_plugin_fingerprint(&conn, "fingerprint-b").unwrap();
    assert!(
        !has_ref_rows(&conn, "/some/file.pm"),
        "fingerprint change must clear derived rows"
    );
}

/// A row-format bump must recreate the derived tables, not just clear rows:
/// `CREATE TABLE IF NOT EXISTS` no-ops on the old SHAPE, and a shape change
/// (v1 `files` had no `source` column) would otherwise fail every future
/// shred while composition masks it (refs stay resident, retrieval dead).
#[test]
fn ref_rows_version_bump_recreates_old_shape_tables() {
    let conn = Connection::open_in_memory().unwrap();
    // Simulate a v1-era DB: old files shape + stale version stamp.
    conn.execute_batch(
        "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         INSERT INTO meta VALUES ('ref_rows_version', '1');
         CREATE TABLE files (file_id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE);
         CREATE TABLE strings (str_id INTEGER PRIMARY KEY, s TEXT NOT NULL UNIQUE);
         CREATE TABLE refs (file_id INTEGER, name_id INTEGER);",
    )
    .unwrap();
    init_schema(&conn).unwrap();
    // The v2 shape must accept a tier-tagged shred.
    shred_derived_rows(&conn, "/migrated.pm", "workspace", &[], &[]).unwrap();
    assert!(has_ref_rows(&conn, "/migrated.pm"));
}

/// The version stamp can lie: a DB stamped CURRENT whose tables still carry
/// an older shape (stamped by a build whose migration didn't reshape) would
/// never re-migrate on the stamp check alone — every shred fails on the
/// missing column while composition masks it (refs stay resident, retrieval
/// dead, diagnostics typeless). The shape probe must trigger the rebuild.
#[test]
fn ref_rows_current_stamp_with_stale_shape_recreates_tables() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(&format!(
        "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         INSERT INTO meta VALUES ('ref_rows_version', '{REF_ROWS_VERSION}');
         CREATE TABLE files (file_id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE);
         CREATE TABLE strings (str_id INTEGER PRIMARY KEY, s TEXT NOT NULL UNIQUE);
         CREATE TABLE refs (file_id INTEGER, name_id INTEGER);",
    ))
    .unwrap();
    init_schema(&conn).unwrap();
    shred_derived_rows(&conn, "/migrated.pm", "workspace", &[], &[]).unwrap();
    assert!(has_ref_rows(&conn, "/migrated.pm"));
}

/// The @INC hard-clear is tier-scoped: a PERL5LIB change must take the
/// import tier (blobs AND derived rows) while workspace rows — possibly
/// committed by the concurrent indexer moments earlier — survive.
#[test]
fn inc_clear_is_import_tier_scoped() {
    let conn = test_db();
    validate_inc_paths(&conn, &[PathBuf::from("/lib/a")]).unwrap();
    shred_derived_rows(&conn, "/ws/File.pm", "workspace", &[], &[]).unwrap();
    shred_derived_rows(&conn, "/inc/Dep.pm", "import", &[], &[]).unwrap();

    validate_inc_paths(&conn, &[PathBuf::from("/lib/CHANGED")]).unwrap();
    assert!(
        has_ref_rows(&conn, "/ws/File.pm"),
        "workspace rows must survive an @INC change"
    );
    assert!(
        !has_ref_rows(&conn, "/inc/Dep.pm"),
        "import rows must clear on an @INC change"
    );
}

/// The register-from-Surface warm stub: encode/decode round-trip, and the
/// warm stream's lane selection — a valid stub serves registration without
/// touching the full blob; a declined stub (rows missing) falls back to the
/// full decode; a stale file stamp serves neither.
#[test]
fn warm_stub_roundtrip_and_lane_selection() {
    let conn = test_db();
    let dir = std::env::temp_dir();
    let pm = dir.join("TestModule_warmstub.pm");
    std::fs::write(&pm, "package Stubbed;\nsub go { my $x = shift; return $x + 1 }\n1;\n")
        .unwrap();
    let source = std::fs::read_to_string(&pm).unwrap();
    let cached = parse_source_to_cached(&source, &pm);
    let path_str = pm.to_string_lossy().to_string();

    // Build the stub halves the way the fresh worker does: feed + surface
    // from the WHOLE analysis, skeleton stripped.
    let whole = (*cached.analysis).clone();
    let feed = vec![("go".to_string(), false)];
    let specs: Vec<(String, String)> = Vec::new();
    let surface = crate::surface::Surface::project(&whole);
    let mut skeleton = whole;
    skeleton.evict_witness_bag();
    skeleton.evict_refs();
    skeleton.evict_symbols();

    let blob = encode_stub(&feed, &specs, &surface, &skeleton).expect("encodes");
    let stub = decode_stub(&blob).expect("decodes");
    assert_eq!(stub.feed, feed);
    assert_eq!(stub.surface, surface);
    assert!(stub.skeleton.symbols_are_evicted() && stub.skeleton.refs_are_evicted());

    // Persist the modules row (deletes any stub for the path), then the stub.
    save_to_db(&conn, &path_str, &Some(cached.clone()), "workspace");
    validate_stub_version(&conn);
    save_stub(&conn, &path_str, &blob);

    let run = |conn: &Connection, accept: bool| -> (usize, usize) {
        let (mut stubs, mut fulls) = (0usize, 0usize);
        warm_pack_stream_with_stubs(
            conn,
            true,
            &mut |_p| true,
            &mut |_p, payload| match payload {
                WarmPayload::Stub(_) => {
                    stubs += 1;
                    if accept { WarmDirective::Handled } else { WarmDirective::NeedFull }
                }
                WarmPayload::Full(..) => {
                    fulls += 1;
                    WarmDirective::Handled
                }
            },
        );
        (stubs, fulls)
    };
    // Stub lane accepted: full blob untouched.
    assert_eq!(run(&conn, true), (1, 0));
    // Stub declined (e.g. derived rows missing): falls back to full decode.
    assert_eq!(run(&conn, false), (1, 1));
    // use_stubs=false (NO_EVICT): straight to the full lane.
    let (mut stubs, mut fulls) = (0usize, 0usize);
    warm_pack_stream_with_stubs(&conn, false, &mut |_p| true, &mut |_p, payload| {
        match payload {
            WarmPayload::Stub(_) => stubs += 1,
            WarmPayload::Full(..) => fulls += 1,
        }
        WarmDirective::Handled
    });
    assert_eq!((stubs, fulls), (0, 1));

    // A rewritten modules row must orphan the stub (stale-skeleton guard).
    save_to_db(&conn, &path_str, &Some(cached), "workspace");
    assert_eq!(run(&conn, true), (0, 1));

    // Stale file stamp: neither lane serves.
    std::fs::write(&pm, "package Stubbed;\nsub go { 2 }\nsub extra { 3 }\n1;\n").unwrap();
    assert_eq!(run(&conn, true), (0, 0));

    let _ = std::fs::remove_file(&pm);
}

/// STUB_VERSION mismatch wipes the stubs table (never serves an old
/// generation's meaning under a new reader).
#[test]
fn stub_version_gate_wipes_on_mismatch() {
    let conn = test_db();
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('stub_version', 'ancient')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO stubs (path, stub) VALUES ('/x', x'00')",
        [],
    )
    .unwrap();
    validate_stub_version(&conn);
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM stubs", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0, "mismatched generation wiped");
    // Current version: idempotent, keeps rows.
    conn.execute("INSERT INTO stubs (path, stub) VALUES ('/y', x'00')", []).unwrap();
    validate_stub_version(&conn);
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM stubs", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
}

/// `refresh_deps_stamp` — the Unchanged gate's persistence half. A header
/// body edit moves every consumer row's closure stamp; refreshing it (and
/// nothing else) keeps the row warm-valid without re-persisting content.
#[test]
fn refresh_deps_stamp_revalidates_consumer_rows() {
    let conn = test_db();
    let dir = std::env::temp_dir().join(format!("deps-refresh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let header = dir.join("dep.h");
    let consumer = dir.join("use.c");
    std::fs::write(&header, "int helper(void);\n").unwrap();
    std::fs::write(&consumer, "#include \"dep.h\"\n").unwrap();

    // A consumer row whose closure contains the header.
    let cached = parse_source_to_cached("1;\n", &consumer);
    let mut fa = (*cached.analysis).clone();
    fa.include_closure = crate::file_analysis::path_intern::ClosureList::from_iter(
        [header.to_string_lossy()].iter().map(|s| s.as_ref()),
    );
    let blob = encode_analysis(&fa).unwrap();
    let consumer_str = consumer.to_string_lossy().into_owned();
    let stamp = file_stamp(&consumer).unwrap_or((0, 0));
    save_blob_to_db_stamped(&conn, &consumer_str, &consumer, &fa.include_closure, &blob, "workspace", stamp);
    let stored: i64 = conn
        .query_row("SELECT deps_stamp FROM modules WHERE path=?1", params![consumer_str], |r| r.get(0))
        .unwrap();

    // "Edit" the header (content + mtime move) — the stored stamp is stale.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(&header, "int helper(void); /* body-ish edit */\n").unwrap();
    let mut memo = std::collections::HashMap::new();
    refresh_deps_stamp(&conn, &consumer_str, &fa.include_closure, &mut memo);
    let refreshed: i64 = conn
        .query_row("SELECT deps_stamp FROM modules WHERE path=?1", params![consumer_str], |r| r.get(0))
        .unwrap();
    assert_ne!(stored, refreshed, "closure member moved: stamp must change");

    // And it now matches a fresh recompute (what the next warm scan checks).
    let mut memo2 = std::collections::HashMap::new();
    let expect = closure_stamp(&fa.include_closure, &mut memo2);
    assert_eq!(refreshed, expect);

    let _ = std::fs::remove_dir_all(&dir);
}
