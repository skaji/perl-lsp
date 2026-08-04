//! Layer 4 — the LSP adapter. `FileAnalysis` types → LSP protocol
//! types; no analysis, no tree walks, no Perl semantics decisions.

pub mod backend;
pub mod cursor_context;
// one Slot vocabulary over cursor_context (Perl) + cursor_sentinel
// (pack); consumers switch on Slot, never on language
pub mod cursor_slot;
// process-survival service wrapper: catches handler panics at the
// request/notification boundary (no crate:: imports, DAG-neutral)
pub mod panic_guard;
pub mod plugin_cli;
pub mod symbols;
