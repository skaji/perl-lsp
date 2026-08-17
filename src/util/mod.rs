//! Neutral leaf tier: std-only process instrumentation and primitives,
//! importable from every layer. `layering_tests::util_tier_is_std_only`
//! enforces the charter — a util file referencing any crate path fails the
//! walk, so this directory cannot become a laundering hole between layers.

pub mod ghost_stats;
pub mod text;
pub mod timings;
