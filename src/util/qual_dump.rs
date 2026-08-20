//! MEASUREMENT-ONLY: an append-under-lock text sink, gated by
//! `PERL_LSP_QUAL_DUMP=<path>`.
//!
//! Std-only, like the rest of this tier: it takes formatted text and knows
//! nothing about what is being measured. The caller owns the format, because
//! the shape being dumped lives in a layer this one is not allowed to see.
//!
//! Used to cost a possible CLASS AXIS on the ref rows: `RefRowSeed` already
//! carries the resolved invocant class, but `shred_derived_rows` writes only
//! `(name_id, file_id)`, so the axis is computed and discarded today.

use std::io::Write;
use std::sync::{Mutex, OnceLock};

fn sink() -> Option<&'static Mutex<std::fs::File>> {
    static S: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    S.get_or_init(|| {
        let path = std::env::var("PERL_LSP_QUAL_DUMP").ok()?;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(Mutex::new)
    })
    .as_ref()
}

/// Is the dump on? Lets a caller skip formatting entirely when it is not.
pub fn enabled() -> bool {
    sink().is_some()
}

/// Append `text` as ONE write. The walk is Rayon-parallel, so a caller must
/// batch a whole file's lines into a single call or they interleave.
pub fn append(text: &str) {
    let Some(f) = sink() else { return };
    if let Ok(mut g) = f.lock() {
        let _ = g.write_all(text.as_bytes());
    }
}
