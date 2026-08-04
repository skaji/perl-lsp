//! Layer 0 — the data model. `FileAnalysis` is the single source of
//! truth; nothing here imports tree-sitter beyond `Point` or any upper
//! layer (enforced by `layering_tests`).

pub mod conventions;
pub mod file_analysis;
pub mod graph;
pub mod surface;
// Leaf instrumentation util (std-only, no crate imports): lives at the
// bottom so every layer — builder included — may import it downward.
pub mod timings;
pub mod witnesses;
