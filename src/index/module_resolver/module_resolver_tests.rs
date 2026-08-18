use super::*;
use crate::index::module_index::strip_import_copy;

#[test]
fn test_resolve_module_list_util() {
    let inc_paths = discover_inc_paths();
    if inc_paths.is_empty() {
        return;
    }
    let path = resolve_module_path(&inc_paths, "List::Util");
    assert!(path.is_some(), "List::Util should be resolvable");
    let p = path.unwrap();
    assert!(p.to_str().unwrap().contains("List/Util.pm"));
}

#[test]
fn test_extract_exports_qw() {
    let source = r#"
package Foo;
use Exporter 'import';
our @EXPORT_OK = qw(alpha beta gamma);
our @EXPORT = qw(delta);
1;
"#;
    let mut parser = create_parser();
    let tree = parser.parse(source, None).unwrap();
    let analysis = crate::build::builder::build(&tree, source.as_bytes());
    assert_eq!(analysis.export, vec!["delta"]);
    assert_eq!(analysis.export_ok, vec!["alpha", "beta", "gamma"]);
}

#[test]
fn test_extract_exports_parenthesized() {
    let source = r#"
package Bar;
our @EXPORT_OK = ('foo', 'bar', 'baz');
1;
"#;
    let mut parser = create_parser();
    let tree = parser.parse(source, None).unwrap();
    let analysis = crate::build::builder::build(&tree, source.as_bytes());
    assert_eq!(analysis.export_ok, vec!["foo", "bar", "baz"]);
}

#[test]
fn test_discover_inc_paths() {
    let paths = discover_inc_paths();
    if !paths.is_empty() {
        assert!(paths.iter().all(|p| p.is_dir()));
    }
}

#[test]
fn insert_resolved_none_does_not_clobber_indexed_module() {
    // A workspace-indexed module (built with plugins, carries a Handler
    // symbol) must survive a later on-demand @INC miss for the same name.
    // The miss happens for project modules under a relative `use lib` the
    // resolver's @INC doesn't cover; clobbering with `None` while leaving
    // the reverse index pointing at it orphaned cross-file Handler /
    // dispatch lookup (mojo-events goto-def + sig help).
    let source = r#"
package Demo::Has::Event;
use parent 'Mojo::EventEmitter';
sub new {
    my $self = bless {}, shift;
    $self->on('ready', sub { my ($s, $ts) = @_; });
    $self;
}
1;
"#;
    let mut parser = create_parser();
    let tree = parser.parse(source, None).unwrap();
    let analysis = std::sync::Arc::new(crate::build::builder::build(&tree, source.as_bytes()));
    assert!(
        analysis.symbols().iter().any(|s| matches!(s.kind, crate::model::file_analysis::SymKind::Handler)),
        "fixture should synthesize a Handler symbol via the mojo-events plugin",
    );

    let core = IndexCore::new();
    let cached = Arc::new(CachedModule::new(PathBuf::from("/x/Demo/Has/Event.pm"), analysis));

    // Workspace-index style insert: a resolved module.
    core.insert_resolved("Demo::Has::Event", Some(cached), false, false);
    assert!(core.cache.get("Demo::Has::Event").as_deref().unwrap().is_some());

    // On-demand resolver miss: `None`. Must NOT clobber the indexed copy.
    core.insert_resolved("Demo::Has::Event", None, false, false);
    assert!(
        core.cache.get("Demo::Has::Event").as_deref().unwrap().is_some(),
        "a None on-demand miss clobbered an already-indexed module",
    );

    // A genuine resolved entry still updates (sanity: the guard only
    // protects against None-over-Some).
    let tree2 = parser.parse(source, None).unwrap();
    let analysis2 = std::sync::Arc::new(crate::build::builder::build(&tree2, source.as_bytes()));
    let cached2 = Arc::new(CachedModule::new(PathBuf::from("/y/Demo/Has/Event.pm"), analysis2));
    core.insert_resolved("Demo::Has::Event", Some(cached2), false, false);
    assert_eq!(
        core.cache.get("Demo::Has::Event").as_deref().unwrap().as_ref().unwrap().path,
        PathBuf::from("/y/Demo/Has/Event.pm"),
    );
}

#[test]
fn test_uri_to_path() {
    assert_eq!(
        uri_to_path("file:///Users/foo/project"),
        Some(PathBuf::from("/Users/foo/project"))
    );
    assert_eq!(uri_to_path("http://example.com"), None);
}

#[test]
fn entrypoint_scan_finds_shebang_scripts_in_conventional_dirs() {
    let dir = std::env::temp_dir().join(format!("qx-entry-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    std::fs::create_dir_all(dir.join("script")).unwrap();
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    // root-level Perl entrypoint (no extension) — found
    std::fs::write(dir.join("jobs"), "#!/usr/bin/env perl\nuse Mojolicious::Lite;\n").unwrap();
    // bin/ + script/ entrypoints — found
    std::fs::write(dir.join("bin/login"), "#! /usr/bin/perl\n").unwrap();
    std::fs::write(dir.join("script/cron"), "#!/usr/bin/env perl\n").unwrap();
    // non-Perl shebang — not found
    std::fs::write(dir.join("deploy"), "#!/bin/bash\n").unwrap();
    // extensionless script buried in lib/ — NOT scanned by default
    std::fs::write(dir.join("lib/buried"), "#!/usr/bin/env perl\n").unwrap();

    let mut found: Vec<String> = scan_entrypoint_scripts(&dir, &[])
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    found.sort();
    assert_eq!(found, vec!["cron", "jobs", "login"]);

    // the config seam: an `extra` dir brings its entrypoints in.
    std::fs::create_dir_all(dir.join("daemons")).unwrap();
    std::fs::write(dir.join("daemons/worker"), "#!/usr/bin/env perl\n").unwrap();
    let mut with_extra: Vec<String> = scan_entrypoint_scripts(&dir, &["daemons".into()])
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    with_extra.sort();
    assert_eq!(with_extra, vec!["cron", "jobs", "login", "worker"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn workspace_index_progress_is_throttled_monotone_and_completes() {
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::Mutex;

    // A real-ish tree: enough files that per-file emission would be a storm,
    // so the throttle's effect is observable.
    let dir = std::env::temp_dir().join(format!("qx-progress-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let n_files = 240usize;
    for i in 0..n_files {
        std::fs::write(
            dir.join(format!("Mod{i}.pm")),
            format!("package Mod{i};\nsub run {{ my ($self) = @_; return {i}; }}\n1;\n"),
        )
        .unwrap();
    }

    // Mirror the backend's throttle: emit only on a >=2% advance or the final
    // tick. `emitted` is what a client would see as Report notifications.
    let last_pct = AtomicU8::new(0);
    let emitted: Mutex<Vec<(u8, usize, usize)>> = Mutex::new(Vec::new());
    let raw_ticks = std::sync::atomic::AtomicUsize::new(0);
    let cb = |done: usize, total: usize| {
        raw_ticks.fetch_add(1, Ordering::Relaxed);
        let pct = if total == 0 {
            100u8
        } else {
            ((done * 100 / total).min(100)) as u8
        };
        let prev = last_pct.fetch_max(pct, Ordering::Relaxed);
        if pct >= prev.saturating_add(2) || done >= total {
            emitted.lock().unwrap().push((pct, done, total));
        }
    };

    let files = crate::index::file_store::FileStore::new();
    let indexed =
        index_workspace_with_index(&dir, &files, None, Some(&cb as &(dyn Fn(usize, usize) + Sync)), None);
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(indexed, n_files, "all files indexed");
    // The callback fires once per file (no matter success/skip).
    assert_eq!(raw_ticks.load(Ordering::Relaxed), n_files);

    let emitted = emitted.into_inner().unwrap();
    // Bounded: a >=2% throttle caps Reports well under the file count. With 240
    // files this is ~50 max, never hundreds.
    assert!(
        emitted.len() <= 60,
        "throttled emission count should be bounded, got {}",
        emitted.len()
    );
    assert!(!emitted.is_empty(), "at least one Report");

    // Percentages are monotone non-decreasing (the client bar never rewinds).
    for w in emitted.windows(2) {
        assert!(w[1].0 >= w[0].0, "percent must not decrease: {:?}", emitted);
    }
    // The stream ends at 100% with done == total (the final Report before End).
    let (last_pct, last_done, last_total) = *emitted.last().unwrap();
    assert_eq!(last_pct, 100);
    assert_eq!(last_done, n_files);
    assert_eq!(last_total, n_files);
}

/// The @INC registration-owned strip: a persisted, non-degraded module's
/// resident copy drops its bag (rehydratable via the hub LRU); unpersisted
/// or eviction-off copies stay whole (the bag would be unrecoverable).
#[test]
fn import_tier_strip_gates_on_persistence() {
    let source = "package Strip;\nsub go { my $s = shift; return bless {}, 'X' }\n1;\n";
    let mut parser = create_parser();
    let tree = parser.parse(source, None).unwrap();
    let fa = crate::build::builder::build(&tree, source.as_bytes());
    assert!(!fa.witnesses.is_empty());
    let cm = Some(Arc::new(CachedModule::new(
        PathBuf::from("/inc/Strip.pm"),
        Arc::new(fa),
    )));

    let stripped = strip_import_copy(&cm, true, true).unwrap();
    assert!(stripped.analysis.bag_is_evicted(), "persisted + eviction → bag drops");
    assert!(!stripped.analysis.symbols_are_evicted(), "symbols stay resident this slice");
    assert!(!stripped.analysis.refs_are_evicted(), "refs stay resident this slice");

    let whole = strip_import_copy(&cm, false, true).unwrap();
    assert!(!whole.analysis.bag_is_evicted(), "unpersisted → bag unrecoverable → keep");
    let whole2 = strip_import_copy(&cm, true, false).unwrap();
    assert!(!whole2.analysis.bag_is_evicted(), "NO_EVICT → keep");
    assert!(strip_import_copy(&None, true, true).is_none());
}


/// The priority lane is guarded by its OWN mutex while the drain waits on the
/// `pending` one, so a `request_resolve` for a stale module can land after the
/// drain's priority check and before it parks — the notify reaches nobody and
/// the wait loop, which only re-checked `pending`, never looks at priority
/// again. In an all-stale workload (an `EXTRACT_VERSION` bump: every
/// `request_resolve` takes the priority branch) nothing ever pushes `pending`,
/// so the resolver sleeps for the rest of the session and cross-file
/// resolution silently never completes.
#[test]
fn priority_push_wakes_a_parked_drain() {
    use std::sync::mpsc;
    use std::sync::{Condvar, Mutex};

    let queue = Arc::new(ResolveQueue {
        priority: Mutex::new(Vec::new()),
        pending: Mutex::new(Vec::new()),
        condvar: Condvar::new(),
    });
    let (tx, rx) = mpsc::channel();
    let q = Arc::clone(&queue);
    std::thread::spawn(move || {
        let _ = tx.send(drain_next_batch(&q));
    });

    // Let the drain get past its priority check and park in `wait(pending)`.
    // Generous: the drain reaches the park in microseconds, so an early push
    // (which the first priority check would catch, hiding the bug) is not a
    // realistic outcome here.
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Exactly what `ModuleIndex::request_resolve` does for a stale module.
    {
        let mut p = queue.priority.lock().unwrap();
        p.push("Stale::Module".to_string());
    }
    queue.condvar.notify_one();

    let batch = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("a priority push must wake the parked drain");
    assert_eq!(batch, vec!["Stale::Module".to_string()]);
}
