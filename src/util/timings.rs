//! Opt-in per-module build-timing instrumentation.
//!
//! Off by default: the gate is a single relaxed atomic load, so the hot
//! indexing/resolve paths pay nothing when timings aren't requested. When
//! enabled (via `--timings` on `--check` / `cli_full_startup`, or
//! `PERL_LSP_TIMINGS=1`), each module's parse + build wall time is recorded
//! into a global thread-safe collector and dumped slowest-first to stderr
//! after the index/check completes — so a cold-start outlier (the
//! `SQL::Abstract` blowup) is visible in one command.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;

static ENABLED: AtomicBool = AtomicBool::new(false);

/// One module's timing breakdown. `cached` marks a module served from the
/// SQLite blob (no parse/build) vs freshly built.
struct Entry {
    module: String,
    parse: Duration,
    build: Duration,
    cached: bool,
}

fn collector() -> &'static Mutex<Vec<Entry>> {
    static COLLECTOR: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
    COLLECTOR.get_or_init(|| Mutex::new(Vec::new()))
}

/// Turn instrumentation on. Honors an explicit flag OR `PERL_LSP_TIMINGS`.
/// Idempotent; safe to call from multiple CLI entry points.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

/// Enable from environment if `PERL_LSP_TIMINGS` is set (any value).
pub fn enable_from_env() {
    if std::env::var_os("PERL_LSP_TIMINGS").is_some() {
        enable();
    }
}

#[inline]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Record a freshly-built module's parse + build durations. Cheap no-op
/// when disabled (callers already gate the `Instant` capture on
/// `is_enabled()`, but this guards the lock too).
pub fn record_built(module: impl Into<String>, parse: Duration, build: Duration) {
    if !is_enabled() {
        return;
    }
    record(Entry {
        module: module.into(),
        parse,
        build,
        cached: false,
    });
}

/// Record a module served from the SQLite cache (no parse/build cost).
pub fn record_cached(module: impl Into<String>) {
    if !is_enabled() {
        return;
    }
    record(Entry {
        module: module.into(),
        parse: Duration::ZERO,
        build: Duration::ZERO,
        cached: true,
    });
}

fn record(e: Entry) {
    if let Ok(mut v) = collector().lock() {
        v.push(e);
    }
}

/// How many of the slowest entries to print in full. The rest are summarized.
const TOP_N: usize = 50;

/// Print the slowest-first breakdown to stderr. No-op when disabled or empty.
pub fn report() {
    if !is_enabled() {
        return;
    }
    let mut entries = match collector().lock() {
        Ok(mut v) => std::mem::take(&mut *v),
        Err(_) => return,
    };
    if entries.is_empty() {
        return;
    }

    entries.sort_by(|a, b| {
        let ta = a.parse + a.build;
        let tb = b.parse + b.build;
        tb.cmp(&ta)
    });

    let total: Duration = entries.iter().map(|e| e.parse + e.build).sum();
    let built = entries.iter().filter(|e| !e.cached).count();
    let cached = entries.len() - built;

    eprintln!();
    eprintln!(
        "=== per-module build timings ({} modules: {} built, {} cache-hit) ===",
        entries.len(),
        built,
        cached
    );
    eprintln!(
        "{:>10}  {:>10}  {:>10}  {:>6}  {}",
        "total_ms", "parse_ms", "build_ms", "source", "module"
    );

    for e in entries.iter().take(TOP_N) {
        let src = if e.cached { "cache" } else { "built" };
        eprintln!(
            "{:>10.3}  {:>10.3}  {:>10.3}  {:>6}  {}",
            (e.parse + e.build).as_secs_f64() * 1000.0,
            e.parse.as_secs_f64() * 1000.0,
            e.build.as_secs_f64() * 1000.0,
            src,
            e.module
        );
    }

    if entries.len() > TOP_N {
        eprintln!(
            "... {} more modules omitted (showing slowest {})",
            entries.len() - TOP_N,
            TOP_N
        );
    }
    eprintln!(
        "=== total build time across {} freshly-built modules: {:.3} ms ===",
        built,
        total.as_secs_f64() * 1000.0
    );
}

// ── Plugin pattern-match telemetry ─────────────────────────────────────
//
// `PERL_LSP_PLUGIN_STATS` turns on per-(plugin, pattern) match/dispatch
// counters, aggregated across every file the build touches and dumped
// once via `report_pattern_stats()` (called alongside `report()`). This
// is the query medium's observability answer: one workspace index run
// tells you which of your patterns never matched (a wrong pattern
// matches NOTHING, silently) and which matched but never dispatched
// (trigger gate never true).

/// Cached `PERL_LSP_PLUGIN_STATS` gate, read once.
pub fn pattern_stats_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PERL_LSP_PLUGIN_STATS").is_some())
}

#[derive(Default, Clone, Copy)]
struct PatternStat {
    /// Query matches seen (pre-gating), summed across files.
    matched: u64,
    /// Matches that passed the trigger gate and dispatched `on_match`.
    dispatched: u64,
}

fn pattern_collector() -> &'static Mutex<std::collections::HashMap<(String, String), PatternStat>>
{
    static C: OnceLock<Mutex<std::collections::HashMap<(String, String), PatternStat>>> =
        OnceLock::new();
    C.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// Record one file's raw match count for a pattern (called even when
/// zero, so never-matching patterns appear in the report at 0).
pub fn record_pattern_matches(plugin: &str, pattern: &str, matched: usize) {
    if !pattern_stats_enabled() {
        return;
    }
    if let Ok(mut m) = pattern_collector().lock() {
        m.entry((plugin.to_string(), pattern.to_string()))
            .or_default()
            .matched += matched as u64;
    }
}

/// Record one dispatched (gate-passing) match for a pattern.
pub fn record_pattern_dispatch(plugin: &str, pattern: &str) {
    if !pattern_stats_enabled() {
        return;
    }
    if let Ok(mut m) = pattern_collector().lock() {
        m.entry((plugin.to_string(), pattern.to_string()))
            .or_default()
            .dispatched += 1;
    }
}

/// Print the per-pattern counters to stderr. No-op when the gate is off
/// or nothing was recorded.
pub fn report_pattern_stats() {
    if !pattern_stats_enabled() {
        return;
    }
    let entries: Vec<((String, String), PatternStat)> = match pattern_collector().lock() {
        Ok(mut m) => m.drain().collect(),
        Err(_) => return,
    };
    if entries.is_empty() {
        return;
    }
    let mut entries = entries;
    entries.sort_by(|a, b| b.1.matched.cmp(&a.1.matched).then(a.0.cmp(&b.0)));
    eprintln!();
    eprintln!("=== plugin pattern stats ({} patterns) ===", entries.len());
    eprintln!("{:>10}  {:>10}  {}", "matched", "dispatched", "plugin:pattern");
    for ((plugin, pattern), s) in &entries {
        let flag = if s.matched == 0 {
            "  <- never matched"
        } else if s.dispatched == 0 {
            "  <- never passed the trigger gate"
        } else {
            ""
        };
        eprintln!(
            "{:>10}  {:>10}  {}:{}{}",
            s.matched, s.dispatched, plugin, pattern, flag
        );
    }
}

// ── Fine-grained per-phase timing ──────────────────────────────────────
//
// `phase()` wraps a single build() pass or query step; `PERL_LSP_PHASE_TIMING`
// turns it on — a finer cut than the per-module report above. All phase timing
// routes through here (including the `bphase!` / `tphase!` call-site sugar) so
// the gate is read once and the output format stays uniform.

/// Cached `PERL_LSP_PHASE_TIMING` gate, read from the environment once so the
/// hot build path never re-hits `std::env`.
pub fn phases_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PERL_LSP_PHASE_TIMING").is_some())
}

/// Time `body`, returning its result; when phase timing is on, print
/// `[PHASE] <label>  <ms>` to stderr, else run it untouched.
#[inline]
pub fn phase<T>(label: &str, body: impl FnOnce() -> T) -> T {
    if !phases_enabled() {
        return body();
    }
    let started = std::time::Instant::now();
    let out = body();
    eprintln!(
        "[PHASE] {label:<32} {:>8.2} ms",
        started.elapsed().as_secs_f64() * 1000.0
    );
    out
}

/// A scoped `phase` for a region that cannot be wrapped in a closure —
/// a loop that borrows its surroundings mutably. Times from `start` to
/// drop and prints the same `[PHASE]` line, so a call site chooses the
/// shape that fits without a second output format.
pub struct PhaseGuard {
    label: &'static str,
    started: Option<std::time::Instant>,
}

impl PhaseGuard {
    pub fn start(label: &'static str) -> Self {
        PhaseGuard {
            label,
            started: phases_enabled().then(std::time::Instant::now),
        }
    }
}

impl Drop for PhaseGuard {
    fn drop(&mut self) {
        if let Some(started) = self.started {
            eprintln!(
                "[PHASE] {:<32} {:>8.2} ms",
                self.label,
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Per-file breadcrumbs. At corpus scale (100k+ files) any failure that doesn't
// name its input costs a debugging session — a stack overflow in a rayon
// worker is uncatchable, so the only way to locate the culprit after the
// abort is the last breadcrumb printed before it.

/// Cached `PERL_LSP_TRACE_FILE` gate: when set, the bulk indexers announce
/// each file on stderr immediately BEFORE analyzing it. Completely inert
/// when unset.
pub fn trace_files_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PERL_LSP_TRACE_FILE").is_some())
}

/// Breadcrumb: print `path` before its analysis starts (gated, flushed —
/// stderr is unbuffered). If the analysis aborts the process, this is the
/// line that names the culprit.
pub fn trace_file(path: &std::path::Path) {
    if trace_files_enabled() {
        eprintln!("[trace-file] {}", path.display());
    }
}

thread_local! {
    static CURRENT_FILE: std::cell::RefCell<Option<std::path::PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Record the file this thread is currently analyzing, so failure messages
/// emitted deep inside the build (fold bail, panics caught per-file) can name
/// their input. Ungated: one PathBuf clone per file is noise next to a parse.
///
/// Also registers the unit with the stall watchdog, so a file that never
/// finishes names itself WHILE it is stuck rather than after — a run that
/// hangs never reaches an after-the-fact report, which is exactly the case
/// worth reporting.
pub fn set_current_file(path: Option<&std::path::Path>) {
    CURRENT_FILE.with(|c| *c.borrow_mut() = path.map(|p| p.to_path_buf()));
    match path {
        Some(p) => stall_watch_begin(&p.display().to_string()),
        None => stall_watch_end(),
    }
}

// ---------------------------------------------------------------------------
// Stall watchdog
//
// UNGATED BY DESIGN. Every other probe here hides behind an env var because it
// costs something when on; this one reports a pathology nobody would choose not
// to hear about, and its steady-state cost is two mutex touches per file
// against a parse. A 30-minute file is worth a line on stderr whether or not
// somebody remembered to set a variable beforehand.
//
// Re-warns back off exponentially (30s, 60s, 120s, …) so a genuinely stuck unit
// produces a handful of lines over an hour rather than hundreds. Printing per
// occurrence is how a previous probe emitted 3.2M lines and changed the run it
// was measuring.

struct InFlight {
    label: String,
    since: std::time::Instant,
    next_warn: Duration,
}

fn in_flight() -> &'static Mutex<std::collections::HashMap<std::thread::ThreadId, InFlight>> {
    static M: OnceLock<Mutex<std::collections::HashMap<std::thread::ThreadId, InFlight>>> =
        OnceLock::new();
    M.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// First warning after this long on one unit; doubles thereafter.
fn stall_threshold() -> Duration {
    static T: OnceLock<Duration> = OnceLock::new();
    *T.get_or_init(|| {
        std::env::var("PERL_LSP_STALL_WARN_SECONDS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(30))
    })
}

fn stall_watch_begin(label: &str) {
    ensure_watchdog();
    if let Ok(mut m) = in_flight().lock() {
        m.insert(
            std::thread::current().id(),
            InFlight {
                label: label.to_string(),
                since: std::time::Instant::now(),
                next_warn: stall_threshold(),
            },
        );
    }
}

fn stall_watch_end() {
    if let Ok(mut m) = in_flight().lock() {
        m.remove(&std::thread::current().id());
    }
}

fn ensure_watchdog() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        std::thread::Builder::new()
            .name("stall-watch".into())
            .spawn(|| loop {
                std::thread::sleep(Duration::from_secs(5));
                let now = std::time::Instant::now();
                if let Ok(mut m) = in_flight().lock() {
                    for u in m.values_mut() {
                        let held = now.duration_since(u.since);
                        if held >= u.next_warn {
                            eprintln!(
                                "[stall] {:.0}s on one unit and still going: {}",
                                held.as_secs_f64(),
                                u.label
                            );
                            u.next_warn = held * 2;
                        }
                    }
                }
            })
            .ok();
    });
}

/// The file this thread is analyzing, if the current work unit declared one.
pub fn current_file() -> Option<String> {
    CURRENT_FILE.with(|c| c.borrow().as_ref().map(|p| p.display().to_string()))
}
