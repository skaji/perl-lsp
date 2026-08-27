//! Machine-readable sinks for the instrumentation tiers.
//!
//! The human reports (`ghost_stats::emit_all`, `timings::report`) are formatted
//! for a person reading a terminal: rounded, sorted, top-N. A measurement
//! harness needs the opposite — every key, unrounded, and no parsing of prose
//! that a later format tweak silently breaks.
//!
//! Std-only like the rest of `util`, so the JSON is hand-written. That is a
//! deliberate cost: the alternative is `util` importing serde, which the
//! layering test forbids precisely so this tier stays a neutral leaf.
//!
//! **Writes happen after the measured region, never during it.** Emitting
//! per-file lines to a stream inside the region under test once cost a run
//! 3.2M lines and 43 minutes — the instrument became the dominant term. These
//! sinks accumulate in memory and serialize once, at exit.

use std::fmt::Write as _;

/// Escape a string for a JSON scalar. Keys here are counter tags and file
/// paths — both can carry anything a filesystem allows.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// Write `body` to the path in `var`, if that variable is set.
///
/// Returns whether it wrote. A failure is reported to stderr and swallowed:
/// an instrument that aborts the run it is measuring is worse than a missing
/// file, and the harness treats an absent sink as a failed run anyway.
pub fn write_if_requested(var: &str, body: &str) -> bool {
    let Some(path) = std::env::var_os(var) else {
        return false;
    };
    match std::fs::write(&path, body) {
        Ok(()) => true,
        Err(e) => {
            eprintln!(
                "[{}] could not write {}: {e}",
                var,
                std::path::Path::new(&path).display()
            );
            false
        }
    }
}
