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
    let fa = crate::build::builder::build(&tree, source.as_bytes());
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
    let all_defs: DashMap<String, Vec<Arc<CachedModule>>> = DashMap::new();
    let (n, stale) = warm_cache(&conn, &cache, &all_defs, false);
    assert_eq!(n, 1);
    assert!(stale.is_empty());

    let loaded = cache.get("TestModule").unwrap();
    let loaded = loaded.as_ref().unwrap();
    assert_eq!(loaded.analysis.export, vec!["foo", "bar"]);
    assert_eq!(loaded.analysis.export_ok, vec!["baz"]);

    let _ = std::fs::remove_file(&pm);
}

/// The @INC pool is keyed by scheme, not by writer. Every name-keyed
/// producer (resolver thread, one-shot CLI) tags rows `NAME_KEYED_SOURCE`
/// and `warm_cache` reads exactly that tag; a writer-specific tag stranded
/// CLI-resolved rows unread, so each CLI verb re-resolved the whole tier.
/// Path-keyed `workspace` rows must stay out — they stream separately.
#[test]
fn warm_cache_shares_the_name_keyed_pool_and_excludes_path_keyed_rows() {
    let conn = test_db();
    let dir = std::env::temp_dir();

    let inc = dir.join("WarmPoolInc.pm");
    std::fs::write(&inc, "package WarmPoolInc;\nsub f { 1 }\n1;\n").unwrap();
    let inc_cached = Some(parse_source_to_cached(
        &std::fs::read_to_string(&inc).unwrap(),
        &inc,
    ));
    save_to_db(&conn, "WarmPoolInc", &inc_cached, NAME_KEYED_SOURCE);

    // Path-keyed: same table, different keying scheme.
    let ws = dir.join("WarmPoolWorkspace.pm");
    std::fs::write(&ws, "package WarmPoolWorkspace;\nsub g { 2 }\n1;\n").unwrap();
    let ws_cached = Some(parse_source_to_cached(
        &std::fs::read_to_string(&ws).unwrap(),
        &ws,
    ));
    save_to_db(&conn, &ws.to_string_lossy(), &ws_cached, "workspace");

    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let all_defs: DashMap<String, Vec<Arc<CachedModule>>> = DashMap::new();
    let (n, _stale) = warm_cache(&conn, &cache, &all_defs, false);

    assert_eq!(n, 1, "exactly the name-keyed row warms");
    assert!(
        cache.contains_key("WarmPoolInc"),
        "a name-keyed row must warm back regardless of which writer resolved it"
    );
    assert!(
        !cache.contains_key(&*ws.to_string_lossy()),
        "path-keyed rows must not pollute the name-keyed cache"
    );

    let _ = std::fs::remove_file(&inc);
    let _ = std::fs::remove_file(&ws);
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
    let original_ns_count = cached.as_ref().unwrap().analysis.plugin.namespaces.len();
    assert!(
        original_ns_count > 0,
        "sanity: fixture must produce at least one PluginNamespace"
    );

    save_to_db(&conn, "TestMojoApp", &cached, "import");

    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let all_defs: DashMap<String, Vec<Arc<CachedModule>>> = DashMap::new();
    let (n, stale) = warm_cache(&conn, &cache, &all_defs, false);
    assert_eq!(n, 1);
    assert!(stale.is_empty(), "fresh insert should not be stale");

    let loaded = cache.get("TestMojoApp").unwrap();
    let loaded = loaded.as_ref().unwrap();
    let loaded_ns = &loaded.analysis.plugin.namespaces;
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
                .any(|b| matches!(b, crate::model::file_analysis::Bridge::Class(_))),
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
    let all_defs: DashMap<String, Vec<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, &all_defs, false);
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
    let all_defs: DashMap<String, Vec<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, &all_defs, false);
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
    let all_defs: DashMap<String, Vec<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, &all_defs, false);
    assert_eq!(n, 1, "cache should survive identical fingerprint");

    // Plugin set changed → cache cleared.
    validate_plugin_fingerprint(&conn, "hash-B").unwrap();
    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let all_defs: DashMap<String, Vec<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, &all_defs, false);
    assert_eq!(n, 0, "cache should be empty after plugin set change");

    // Stamp persists — second run with hash-B doesn't re-clear.
    save_to_db(&conn, "Bar", &None, "import");
    validate_plugin_fingerprint(&conn, "hash-B").unwrap();
    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let all_defs: DashMap<String, Vec<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, &all_defs, false);
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
    let all_defs: DashMap<String, Vec<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, &all_defs, false);
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
    let all_defs: DashMap<String, Vec<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, &all_defs, false);
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
    let original_refs_count = cached.analysis.refs().len();
    let original_packages = cached.analysis.packages.clone();
    save_to_db(&conn, "Fidelity", &Some(Arc::clone(&cached)), "import");

    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let all_defs: DashMap<String, Vec<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, &all_defs, false);
    assert_eq!(n, 1);

    let loaded = cache.get("Fidelity").unwrap();
    let loaded = loaded.as_ref().unwrap();
    assert_eq!(
        loaded.analysis.refs().len(),
        original_refs_count,
        "refs survive roundtrip"
    );
    assert_eq!(
        loaded.analysis.packages, original_packages,
        "per-package facts survive"
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
    let all_defs: DashMap<String, Vec<Arc<CachedModule>>> = DashMap::new();
            let (n, _) = warm_cache(&conn, &cache, &all_defs, false);
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
    let mut fa = crate::build::builder::build(&tree, source.as_bytes());
    fa.pack.include_closure =
        crate::model::file_analysis::path_intern::ClosureList::from_iter(std::iter::once(hdr_canon.as_str()));
    let cached = Some(Arc::new(CachedModule::new(pm.clone(), Arc::new(fa))));
    // warm_cache serves the 'import' tier ('workspace' rows stream through
    // warm_cache_streaming); the deps_stamp semantics under test are
    // source-independent.
    save_to_db(&conn, "Consumer", &cached, "import");

    // Unchanged closure → row warms.
    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let all_defs: DashMap<String, Vec<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, &all_defs, false);
    assert_eq!(n, 1, "row valid while the closure is unchanged");

    // Header changes; the consumer file itself does not.
    std::fs::write(&hdr, "#define LIMIT 5\n#define LIMIT2 7\n").unwrap();
    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let all_defs: DashMap<String, Vec<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, &all_defs, false);
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
    let mut fa = crate::build::builder::build(&tree, source.as_bytes());
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
    let all_defs: DashMap<String, Vec<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, &all_defs, false);
    assert_eq!(n, 1, "same inputs: cache survives");

    validate_input_fingerprint(&conn, 0xB).unwrap();
    let cache: DashMap<String, Option<Arc<CachedModule>>> = DashMap::new();
    let all_defs: DashMap<String, Vec<Arc<CachedModule>>> = DashMap::new();
    let (n, _) = warm_cache(&conn, &cache, &all_defs, false);
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
    let seeds: Vec<_> = cached.analysis.ref_row_seeds();
    assert!(!seeds.is_empty(), "call sites must produce row seeds");
    shred_derived_rows(&conn, &path_str, "workspace", &seeds, &[]).unwrap();
    assert!(has_ref_rows(&conn, &path_str));

    // Retrieval by the match key finds the file; an unknown key finds nothing.
    let hits = ref_candidate_files(&conn, &["helper".to_string()]);
    assert_eq!(hits, vec![path_str.clone()]);
    assert!(ref_candidate_files(&conn, &["nonesuch".to_string()]).is_empty());
    // Two call sites in ONE file are ONE row: rows are (name, file) pairs,
    // and every reader projects onto exactly that.
    assert_eq!(
        ref_candidate_file_count(&conn, "helper"),
        1,
        "a file's repeated mentions of a name must collapse to one row",
    );

    // Re-shred replaces: same seeds again must not double the rows.
    shred_derived_rows(&conn, &path_str, "workspace", &seeds, &[]).unwrap();
    assert_eq!(ref_candidate_file_count(&conn, "helper"), 1);

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

    let seeds: Vec<_> = cached.analysis.ref_row_seeds();
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

/// `FLAG_EXPORTED` is minted from the real `@EXPORT`/`@EXPORT_OK` surface
/// (`exports_name`), so the rows agree with the source the Surface projects —
/// never a parallel notion of exportedness.
#[test]
fn flag_exported_minted_from_exports_source() {
    let dir = std::env::temp_dir();
    let pm = dir.join("UE_Flags.pm");
    let src = "package UE::Flags;\n\
        our @EXPORT = qw(alpha);\n\
        our @EXPORT_OK = qw(beta);\n\
        sub alpha { 1 }\n\
        sub beta { 2 }\n\
        sub gamma { 3 }\n1;\n";
    std::fs::write(&pm, src).unwrap();
    let cached = parse_source_to_cached(src, &pm);
    let seeds = cached.analysis.sym_row_seeds();
    let exported = |name: &str| {
        seeds
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.flags & crate::model::file_analysis::SymRowSeed::FLAG_EXPORTED != 0)
    };
    assert_eq!(exported("alpha"), Some(true), "@EXPORT member is flagged");
    assert_eq!(exported("beta"), Some(true), "@EXPORT_OK member is flagged");
    assert_eq!(exported("gamma"), Some(false), "non-exported sub is not flagged");
    // The flag must never diverge from the source it is baked from.
    assert!(cached.analysis.exports_name("alpha"));
    assert!(cached.analysis.exports_name("beta"));
    assert!(!cached.analysis.exports_name("gamma"));
    let _ = std::fs::remove_file(&pm);
}

/// The unused-exports view: exported syms with zero CROSS-FILE reference rows.
/// Same-file refs are excluded (an export used only internally is dead to
/// consumers); a cross-file consumer keeps an export live; a non-exported sym
/// is never listed.
#[test]
fn unused_exports_view() {
    let conn = test_db();
    let dir = std::env::temp_dir();

    // Producer exports three subs: `lonely` (used nowhere), `used` (a consumer
    // calls it), `internal_only` (referenced only in its own file).
    let prod = dir.join("UE_Producer.pm");
    let prod_src = "package UE::Producer;\n\
        our @EXPORT_OK = qw(lonely used internal_only);\n\
        sub lonely { 1 }\n\
        sub used { 2 }\n\
        sub internal_only { 3 }\n\
        sub caller_here { internal_only(); }\n1;\n";
    std::fs::write(&prod, prod_src).unwrap();
    let prod_cached = parse_source_to_cached(prod_src, &prod);
    let prod_path = prod.to_string_lossy().to_string();
    let prod_refs: Vec<_> = prod_cached.analysis.ref_row_seeds();
    let prod_syms = prod_cached.analysis.sym_row_seeds();
    shred_derived_rows(&conn, &prod_path, "workspace", &prod_refs, &prod_syms).unwrap();

    // Consumer in ANOTHER file references `used`.
    let cons = dir.join("UE_Consumer.pm");
    let cons_src = "package UE::Consumer;\n\
        use UE::Producer qw(used);\n\
        sub go { used(); }\n1;\n";
    std::fs::write(&cons, cons_src).unwrap();
    let cons_cached = parse_source_to_cached(cons_src, &cons);
    let cons_path = cons.to_string_lossy().to_string();
    let cons_refs: Vec<_> = cons_cached.analysis.ref_row_seeds();
    let cons_syms = cons_cached.analysis.sym_row_seeds();
    shred_derived_rows(&conn, &cons_path, "workspace", &cons_refs, &cons_syms).unwrap();

    let dead: std::collections::HashSet<String> =
        unused_exported_syms(&conn).into_iter().map(|d| d.name).collect();

    assert!(dead.contains("lonely"), "exported, unreferenced → dead: {dead:?}");
    assert!(
        dead.contains("internal_only"),
        "same-file use does not make an export live: {dead:?}"
    );
    assert!(
        !dead.contains("used"),
        "a cross-file consumer keeps the export live: {dead:?}"
    );
    assert!(
        !dead.contains("caller_here"),
        "a non-exported sub is never a dead export: {dead:?}"
    );

    let _ = std::fs::remove_file(&prod);
    let _ = std::fs::remove_file(&cons);
}

/// A candidate ref row in another file suppresses the dead-export flag even
/// when it is the ONLY reference — the view's nonzero side is "unknown, not
/// used", so any cross-file candidate is enough to withhold the verdict.
#[test]
fn unused_exports_view_cross_file_candidate_suppresses() {
    let conn = test_db();
    let dir = std::env::temp_dir();

    let prod = dir.join("UE2_Producer.pm");
    let prod_src = "package UE2::Producer;\n\
        our @EXPORT_OK = qw(widget);\n\
        sub widget { 1 }\n1;\n";
    std::fs::write(&prod, prod_src).unwrap();
    let prod_cached = parse_source_to_cached(prod_src, &prod);
    let prod_path = prod.to_string_lossy().to_string();
    let prod_syms = prod_cached.analysis.sym_row_seeds();
    // Producer has no ref rows of its own.
    shred_derived_rows(&conn, &prod_path, "workspace", &[], &prod_syms).unwrap();

    // Before any consumer: dead.
    let dead0: std::collections::HashSet<String> =
        unused_exported_syms(&conn).into_iter().map(|d| d.name).collect();
    assert!(dead0.contains("widget"), "no consumer yet → dead: {dead0:?}");

    // A consumer references it exactly once, cross-file.
    let cons = dir.join("UE2_Consumer.pm");
    let cons_src = "package UE2::Consumer;\nsub go { UE2::Producer::widget(); }\n1;\n";
    std::fs::write(&cons, cons_src).unwrap();
    let cons_cached = parse_source_to_cached(cons_src, &cons);
    let cons_path = cons.to_string_lossy().to_string();
    let cons_refs: Vec<_> = cons_cached.analysis.ref_row_seeds();
    assert!(
        cons_refs.iter().any(|s| s.key == "widget"),
        "consumer must produce a `widget` candidate row"
    );
    shred_derived_rows(&conn, &cons_path, "workspace", &cons_refs, &[]).unwrap();

    let dead1: std::collections::HashSet<String> =
        unused_exported_syms(&conn).into_iter().map(|d| d.name).collect();
    assert!(!dead1.contains("widget"), "one cross-file candidate → not listed: {dead1:?}");

    let _ = std::fs::remove_file(&prod);
    let _ = std::fs::remove_file(&cons);
}

/// The general pre-prune set is exactly the DISTINCT ref-row name keys — the
/// witness `--heatmap` uses to skip the references projection for names that
/// have no reference row at all.
#[test]
fn names_with_ref_rows_is_the_distinct_key_set() {
    let conn = test_db();
    let dir = std::env::temp_dir();
    let pm = dir.join("UE_Names.pm");
    let src = "package UE::Names;\nsub helper { 1 }\nsub go { helper(); }\n1;\n";
    std::fs::write(&pm, src).unwrap();
    let cached = parse_source_to_cached(src, &pm);
    let refs: Vec<_> = cached.analysis.ref_row_seeds();
    shred_derived_rows(&conn, &pm.to_string_lossy(), "workspace", &refs, &[]).unwrap();

    let names = names_with_ref_rows(&conn);
    assert!(names.contains("helper"), "called name has a ref row: {names:?}");
    assert!(!names.contains("go"), "a name only DECLARED has no ref row: {names:?}");

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
    let keys: Vec<String> = cached.analysis.refs().iter().map(|r| r.match_key()).collect();
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
    let seeds = vec![crate::model::file_analysis::RefRowSeed {
        key: "k".into(),
        kind: 1,
        span: crate::model::file_analysis::Span {
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
    let surface = crate::model::surface::Surface::project(&whole);
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
    fa.pack.include_closure = crate::model::file_analysis::path_intern::ClosureList::from_iter(
        [header.to_string_lossy()].iter().map(|s| s.as_ref()),
    );
    let blob = encode_analysis(&fa).unwrap();
    let consumer_str = consumer.to_string_lossy().into_owned();
    let stamp = file_stamp(&consumer).unwrap_or((0, 0));
    save_blob_to_db_stamped(&conn, &consumer_str, &consumer, &fa.pack.include_closure, &blob, "workspace", stamp);
    let stored: i64 = conn
        .query_row("SELECT deps_stamp FROM modules WHERE path=?1", params![consumer_str], |r| r.get(0))
        .unwrap();

    // "Edit" the header (content + mtime move) — the stored stamp is stale.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(&header, "int helper(void); /* body-ish edit */\n").unwrap();
    let mut memo = std::collections::HashMap::new();
    refresh_deps_stamp(&conn, &consumer_str, &fa.pack.include_closure, &mut memo);
    let refreshed: i64 = conn
        .query_row("SELECT deps_stamp FROM modules WHERE path=?1", params![consumer_str], |r| r.get(0))
        .unwrap();
    assert_ne!(stored, refreshed, "closure member moved: stamp must change");

    // And it now matches a fresh recompute (what the next warm scan checks).
    let mut memo2 = std::collections::HashMap::new();
    let expect = closure_stamp(&fa.pack.include_closure, &mut memo2);
    assert_eq!(refreshed, expect);

    let _ = std::fs::remove_dir_all(&dir);
}

/// H7-16 regression: the bag-rehydration reader must survive the transient
/// `SQLITE_CANTOPEN` a fresh read-only open hits while a sibling writer is
/// mid-`wal_checkpoint` on the WAL-mode cache DB — a read-only connection
/// can't rebuild the wal-index in that window. The captured flake was a
/// strict-residency PANIC: the read-only open failed, the loader reported the
/// blob absent, and the tripwire aborted the run though the row was on disk
/// the whole time. `load_with_wal_fallback` recovers through a read-write open
/// (which waits the writer out via `busy_timeout`), so a live blob is never
/// mislabeled absent.
#[test]
fn readonly_open_failure_recovers_through_read_write() {
    // The captured H7-16 cause is a fresh read-only open transiently returning
    // SQLITE_CANTOPEN while a sibling writer is mid-checkpoint on the WAL DB —
    // an OS/SQLite timing race that can't be forced from static file state.
    // Inject it: a read-only open result of `Err` (open failed) with a working
    // read-write recovery connection. The fix loads the row through RW instead
    // of mislabeling the live blob absent; without the RW fallback this same
    // input yields OpenerFailed and the strict tripwire panics.
    let dir = std::env::temp_dir().join(format!("h716_inject_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("modules.db");
    let pm = dir.join("Seed.pm");
    std::fs::write(&pm, "package Seed;\nsub f { my $s = shift; return 'x'; }\n1;\n").unwrap();
    let cached = parse_source_to_cached(&std::fs::read_to_string(&pm).unwrap(), &pm);
    let pm_str = pm.to_string_lossy().into_owned();
    {
        let w = Connection::open(&db).unwrap();
        w.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        init_schema(&w).unwrap();
        save_to_db(&w, &pm_str, &Some(cached.clone()), "workspace");
    }

    // Read-only "open failed", RW open works → recovered, full bag present.
    let recovered = rehydrate_from_opens(
        Err("simulated SQLITE_CANTOPEN".to_string()),
        || open_rw_shared_at(&db),
        std::slice::from_ref(&pm_str),
    )
    .expect("RW fallback must recover the row a failed read-only open couldn't reach");
    assert!(!recovered.bag_is_evicted());
    assert_eq!(recovered.witnesses.len(), cached.analysis.witnesses.len());

    // Read-only "open failed" AND no RW recovery conn → honest OpenerFailed,
    // NOT a fabricated presence: the strict tripwire must still fire on a
    // genuinely unreadable DB.
    let miss = rehydrate_from_opens(
        Err("simulated SQLITE_CANTOPEN".to_string()),
        || None,
        std::slice::from_ref(&pm_str),
    )
    .unwrap_err();
    assert!(matches!(miss, RehydrateMiss::OpenerFailed(_)), "got {miss}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// The fix must never trade a false absence for a fabricated presence: a row
/// that is genuinely missing stays a discriminated miss so the strict
/// tripwire keeps firing on real invariant breaks.
#[test]
fn rehydrate_absent_row_is_honest_miss() {
    let dir = std::env::temp_dir().join(format!("h716_absent_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("modules.db");
    {
        let w = Connection::open(&db).unwrap();
        init_schema(&w).unwrap();
    }
    // Row truly absent (present DB, no matching row) → NoRow, via both opens.
    let miss = load_with_wal_fallback(&db, &["/no/such.pm".to_string()]).unwrap_err();
    assert!(matches!(miss, RehydrateMiss::NoRow), "got {miss}");
    // No DB file at all → OpenerFailed (neither read-only nor read-write open).
    let none = load_with_wal_fallback(&dir.join("nope.db"), &["/x.pm".to_string()]).unwrap_err();
    assert!(matches!(none, RehydrateMiss::OpenerFailed(_)), "got {none}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `load_one_diag` names each on-disk reality distinctly so the tripwire can
/// point at a mechanism instead of a collapsed "None".
#[test]
fn load_one_diag_discriminates_failures() {
    let conn = test_db();
    let dir = std::env::temp_dir();
    let pm = dir.join("h716_diag.pm");
    std::fs::write(&pm, "package D;\nsub f { 1 }\n1;\n").unwrap();
    let cached = parse_source_to_cached(&std::fs::read_to_string(&pm).unwrap(), &pm);
    let pm_str = pm.to_string_lossy().into_owned();
    save_to_db(&conn, &pm_str, &Some(cached), "workspace");
    assert!(load_one_diag(&conn, &pm_str).is_ok());
    assert!(matches!(
        load_one_diag(&conn, "/absent.pm").unwrap_err(),
        RehydrateMiss::NoRow
    ));
    conn.execute(
        "INSERT INTO modules (module_name, path, mtime_secs, file_size, source, \
         analysis, extract_version, deps_stamp) VALUES ('E','/empty.pm',0,0,'import',NULL,?1,0)",
        params![EXTRACT_VERSION],
    )
    .unwrap();
    assert!(matches!(
        load_one_diag(&conn, "/empty.pm").unwrap_err(),
        RehydrateMiss::EmptyBlob
    ));
    conn.execute(
        "INSERT INTO modules (module_name, path, mtime_secs, file_size, source, \
         analysis, extract_version, deps_stamp) VALUES ('G','/garbage.pm',0,0,'import',?1,?2,0)",
        params![Some(vec![9u8, 9, 9, 9]), EXTRACT_VERSION],
    )
    .unwrap();
    assert!(matches!(
        load_one_diag(&conn, "/garbage.pm").unwrap_err(),
        RehydrateMiss::DecodeFailed
    ));
    let _ = std::fs::remove_file(&pm);
}

// ---- The deduped ref-row model ----
//
// Rows are `(name_id, file_id)` pairs. Every reader is a set-valued
// projection onto exactly that, so the dedup is bit-identical rather than
// approximately safe — but the two ways it could go wrong are silent, so
// both are pinned here.

#[test]
fn ref_rows_dedup_per_file_and_not_across_files() {
    // Collapsing a file's repeated mentions is the win; collapsing ACROSS
    // files would drop candidates and silently shrink every backward walk.
    let conn = test_db();
    let dir = std::env::temp_dir();
    let mk = |name: &str, body: &str| {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        let cached = parse_source_to_cached(body, &p);
        (p.to_string_lossy().to_string(), cached.analysis.ref_row_seeds())
    };
    // `helper` mentioned many times in one file, and once in another.
    let (a_path, a_seeds) = mk(
        "dedup_a.pm",
        "package A;\nsub helper { 1 }\nsub go { helper(); helper(); helper(); helper(); }\n1;\n",
    );
    let (b_path, b_seeds) = mk("dedup_b.pm", "package B;\nsub go { helper(); }\n1;\n");

    shred_derived_rows(&conn, &a_path, "workspace", &a_seeds, &[]).unwrap();
    assert_eq!(
        ref_candidate_file_count(&conn, "helper"),
        1,
        "four call sites in one file must be one row",
    );

    shred_derived_rows(&conn, &b_path, "workspace", &b_seeds, &[]).unwrap();
    assert_eq!(
        ref_candidate_file_count(&conn, "helper"),
        2,
        "a second FILE is a second candidate — dedup is per file, never global",
    );

    let mut hits = ref_candidate_files(&conn, &["helper".to_string()]);
    hits.sort();
    let mut want = vec![a_path, b_path];
    want.sort();
    assert_eq!(hits, want, "both files must stay retrievable");
}

#[test]
fn wiping_the_strings_table_does_not_leave_dangling_name_ids() {
    // The shredder memoizes interned `str_id`s for the writer's lifetime.
    // `clear_derived_rows` empties `strings`, so without the
    // `strings_generation` guard the memo would keep handing out ids for
    // rows that no longer exist — refs would be written with a `name_id`
    // nothing joins to, and retrieval would answer EMPTY rather than fail.
    // That is the failure this test exists for, so it asserts on retrieval.
    let conn = test_db();
    let dir = std::env::temp_dir();
    let p = dir.join("dangling_probe.pm");
    let src = "package D;\nsub helper { 1 }\nsub go { helper(); }\n1;\n";
    std::fs::write(&p, src).unwrap();
    let path_str = p.to_string_lossy().to_string();
    let seeds = parse_source_to_cached(src, &p).analysis.ref_row_seeds();

    shred_derived_rows(&conn, &path_str, "workspace", &seeds, &[]).unwrap();
    assert_eq!(ref_candidate_files(&conn, &["helper".to_string()]), vec![path_str.clone()]);

    // The wipe every hard-clear performs, on the SAME connection and thread
    // that just populated the memo.
    clear_derived_rows(&conn).unwrap();
    assert!(ref_candidate_files(&conn, &["helper".to_string()]).is_empty());

    shred_derived_rows(&conn, &path_str, "workspace", &seeds, &[]).unwrap();
    assert_eq!(
        ref_candidate_files(&conn, &["helper".to_string()]),
        vec![path_str],
        "post-wipe rows carry a name_id that joins to nothing",
    );
}

#[test]
fn the_intern_memo_stays_bounded_and_correct_past_its_cap() {
    // The memo is per-thread and lives for the writer's lifetime, so an
    // unbounded one would accumulate a corpus's whole unique-name set per
    // Rayon worker. Crossing the cap must not change what gets written —
    // a cleared memo just re-interns, it never invents an id.
    let conn = test_db();
    let dir = std::env::temp_dir();
    // Far more distinct names than the cap, spread over several files so the
    // memo carries across shred calls the way it does in the writer.
    let mut all_names: Vec<String> = Vec::new();
    for f in 0..4 {
        let mut src = format!("package Cap{f};\n");
        for i in 0..12_000 {
            let n = format!("nm_{f}_{i}");
            src.push_str(&format!("sub {n} {{ 1 }}\n"));
            all_names.push(n);
        }
        src.push_str("1;\n");
        let p = dir.join(format!("cap_probe_{f}.pm"));
        std::fs::write(&p, &src).unwrap();
        let path_str = p.to_string_lossy().to_string();
        let cached = parse_source_to_cached(&src, &p);
        shred_derived_rows(
            &conn,
            &path_str,
            "workspace",
            &cached.analysis.ref_row_seeds(),
            &cached.analysis.sym_row_seeds(),
        )
        .unwrap();
    }
    // 48k distinct names went through a 32k-entry memo; every one must still
    // have interned to a real row that retrieval can join to.
    for probe in [&all_names[0], &all_names[all_names.len() / 2], all_names.last().unwrap()] {
        assert!(
            !ref_candidate_files(&conn, &[probe.to_string()]).is_empty(),
            "name {probe} lost its row after the memo wrapped",
        );
    }
}
