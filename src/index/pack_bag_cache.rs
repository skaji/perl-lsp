//! Byte-capped LRU of rehydrated pack `FileAnalysis`es — the Slice-2
//! rehydration store (`docs/adr/memory-slice-2-lru.md`).
//!
//! After a pack workspace is indexed, every resident pack-index copy of a
//! `FileAnalysis` has its witness bag evicted (the fold already baked its
//! conclusions into pinned fields). The FULL bag stays on disk in the per-lang
//! SQLite blob. When a TYPE query reaches into an evicted file's bag, the pack
//! `ModuleIndex` asks this cache for a bag-present copy: hit → the retained
//! Arc; miss → a keyed single-file decode from SQLite into the LRU.
//!
//! The cap is BYTE-based (`maxCacheMb`), not count-based: cpp bags average
//! ~700 KB/file (10–100× a Perl module), so a count cap would either thrash or
//! blow the footprint. `cap_bytes == 0` disables retention (rehydrate-and-drop)
//! for the most aggressive footprint.
//!
//! Concurrency: a `DashMap` + atomic recency clock. `bag_for` never holds a
//! shard guard across the SQLite decode (it reads/inserts under short per-shard
//! locks) and never across an `.await` (it is fully synchronous), so it adds no
//! guard-across-await hazard (`filestore-guard-discipline`). Two threads racing
//! to rehydrate the same path produce equal Arcs from the same blob; last
//! insert wins, no correctness impact.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use dashmap::DashMap;

use crate::model::file_analysis::FileAnalysis;
use crate::index::module_cache::RehydrateMiss;

type Loader = Box<dyn Fn(&Path) -> Result<FileAnalysis, RehydrateMiss> + Send + Sync>;

pub struct PackBagCache {
    /// Rehydrated, bag-present analyses keyed by canonical path.
    entries: DashMap<PathBuf, Arc<FileAnalysis>>,
    /// Last-touch stamp per retained path — the LRU recency source.
    recency: DashMap<PathBuf, u64>,
    /// Monotone recency clock; every touch bumps it.
    clock: AtomicU64,
    /// Sum of retained analyses' estimated resident payload.
    bytes: AtomicUsize,
    /// `maxCacheMb * 1 MiB`. `0` ⇒ never retain (rehydrate-and-drop).
    cap_bytes: usize,
    /// Per-path invalidation generation: `invalidate` bumps, a loading
    /// `bag_for` records the value BEFORE its decode and only retains when
    /// it is unchanged after — otherwise a decode racing a writer's
    /// commit+invalidate would insert the PREVIOUS generation just after
    /// the invalidate fired, pinning stale spans until the next edit.
    generation: DashMap<PathBuf, u64>,
    /// Keyed single-file decode (opens the pack SQLite conn on demand).
    loader: Loader,
}

impl PackBagCache {
    pub fn new(
        cap_bytes: usize,
        loader: impl Fn(&Path) -> Result<FileAnalysis, RehydrateMiss> + Send + Sync + 'static,
    ) -> Self {
        PackBagCache {
            entries: DashMap::new(),
            recency: DashMap::new(),
            clock: AtomicU64::new(0),
            bytes: AtomicUsize::new(0),
            cap_bytes,
            generation: DashMap::new(),
            loader: Box::new(loader),
        }
    }

    fn tick(&self) -> u64 {
        self.clock.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// A bag-present analysis for `path`: retained Arc if resident, else decode
    /// the exact persisted blob from SQLite into the LRU and return it. Returns
    /// `None` only when the loader can't produce the file (no row / decode
    /// failure) — the caller then degrades to the bag-less resident copy.
    pub fn bag_for(&self, path: &Path) -> Option<Arc<FileAnalysis>> {
        self.bag_for_diag(path).ok()
    }

    /// `bag_for` that surfaces the loader's discriminated failure so the
    /// strict-residency tripwire can name WHY rehydration missed instead of
    /// collapsing every cause into "loader returned None".
    pub fn bag_for_diag(&self, path: &Path) -> Result<Arc<FileAnalysis>, RehydrateMiss> {
        if let Some(hit) = self.entries.get(path) {
            let arc = hit.value().clone();
            drop(hit);
            self.recency.insert(path.to_path_buf(), self.tick());
            return Ok(arc);
        }
        let gen_before = self.generation.get(path).map(|g| *g).unwrap_or(0);
        let fa = Arc::new((self.loader)(path)?);
        if self.cap_bytes == 0 {
            return Ok(fa); // rehydrate-and-drop
        }
        // Retain only if no invalidation landed during the decode — the
        // decoded copy may be the generation the invalidate was erasing.
        // The caller still gets it (best answer available right now); it
        // just must not be pinned.
        let gen_after = self.generation.get(path).map(|g| *g).unwrap_or(0);
        if gen_after != gen_before {
            return Ok(fa);
        }
        let sz = fa.heap_estimate().total();
        self.entries.insert(path.to_path_buf(), fa.clone());
        self.recency.insert(path.to_path_buf(), self.tick());
        self.bytes.fetch_add(sz, Ordering::Relaxed);
        self.evict_to_cap(path);
        Ok(fa)
    }

    /// Drop LRU-tail entries until the retained byte total is within cap. Never
    /// evicts `keep` (the just-inserted path) so a single oversized bag over
    /// the whole cap still resolves the query it was loaded for.
    fn evict_to_cap(&self, keep: &Path) {
        while self.bytes.load(Ordering::Relaxed) > self.cap_bytes {
            // Lowest recency stamp = least recently used.
            let victim = self
                .recency
                .iter()
                .filter(|e| e.key().as_path() != keep)
                .min_by_key(|e| *e.value())
                .map(|e| e.key().clone());
            let Some(victim) = victim else { break };
            self.recency.remove(&victim);
            if let Some((_, fa)) = self.entries.remove(&victim) {
                let sz = fa.heap_estimate().total();
                self.bytes.fetch_sub(sz, Ordering::Relaxed);
            }
        }
    }

    /// Drop `path`'s retained bag (a changed/saved file's bag is stale) so the
    /// next type query rehydrates the fresh blob.
    pub fn invalidate(&self, path: &Path) {
        *self.generation.entry(path.to_path_buf()).or_insert(0) += 1;
        self.recency.remove(path);
        if let Some((_, fa)) = self.entries.remove(path) {
            let sz = fa.heap_estimate().total();
            self.bytes.fetch_sub(sz, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn empty_fa() -> FileAnalysis {
        // A minimal real analysis — its struct shell alone gives a nonzero
        // heap estimate, enough to exercise the byte cap.
        let src = "package M; sub f { 1 }";
        let mut parser = crate::build::builder::create_parser();
        let tree = parser.parse(src, None).unwrap();
        crate::build::builder::build(&tree, src.as_bytes())
    }

    #[test]
    fn cap_zero_never_retains() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let cache = PackBagCache::new(0, move |_p| {
            c.fetch_add(1, Ordering::Relaxed);
            Ok(empty_fa())
        });
        let p = PathBuf::from("/x/a.h");
        assert!(cache.bag_for(&p).is_some());
        assert!(cache.bag_for(&p).is_some());
        // Every access re-decodes; nothing is retained.
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        assert_eq!(cache.entries.len(), 0);
    }

    #[test]
    fn hit_avoids_reload() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let cache = PackBagCache::new(128 * 1024 * 1024, move |_p| {
            c.fetch_add(1, Ordering::Relaxed);
            Ok(empty_fa())
        });
        let p = PathBuf::from("/x/a.h");
        assert!(cache.bag_for(&p).is_some());
        assert!(cache.bag_for(&p).is_some());
        assert_eq!(calls.load(Ordering::Relaxed), 1); // second was a hit
    }

    #[test]
    fn evicts_lru_tail_over_cap() {
        // Each empty FA has a nonzero shell estimate; set a cap that holds
        // only one so inserting a second evicts the first.
        let one = empty_fa().heap_estimate().total();
        assert!(one > 0);
        let cache = PackBagCache::new(one, move |_p| Ok(empty_fa()));
        let a = PathBuf::from("/x/a.h");
        let b = PathBuf::from("/x/b.h");
        cache.bag_for(&a);
        cache.bag_for(&b); // pushes a out (a is LRU, b is the keep)
        assert!(cache.entries.contains_key(&b));
        assert!(!cache.entries.contains_key(&a));
        assert!(cache.bytes.load(Ordering::Relaxed) <= one);
    }

    #[test]
    fn invalidate_during_load_is_not_pinned() {
        // An invalidate landing between the loader's decode and the insert
        // must prevent retention (the decode may be the erased generation).
        // Simulate by invalidating from INSIDE the loader.
        let cache = Arc::new(std::sync::OnceLock::<PackBagCache>::new());
        let c2 = cache.clone();
        let p = PathBuf::from("/x/racy.h");
        let p2 = p.clone();
        let built = PackBagCache::new(128 * 1024 * 1024, move |_p| {
            if let Some(c) = c2.get() {
                c.invalidate(&p2);
            }
            Ok(empty_fa())
        });
        let _ = cache.set(built);
        let c = cache.get().unwrap();
        assert!(c.bag_for(&p).is_some(), "caller still gets the decode");
        assert!(
            !c.entries.contains_key(&p),
            "a decode overlapped by an invalidate must not be retained"
        );
    }

    #[test]
    fn invalidate_drops_entry() {
        let cache = PackBagCache::new(128 * 1024 * 1024, move |_p| Ok(empty_fa()));
        let p = PathBuf::from("/x/a.h");
        cache.bag_for(&p);
        assert!(cache.entries.contains_key(&p));
        cache.invalidate(&p);
        assert!(!cache.entries.contains_key(&p));
        assert_eq!(cache.bytes.load(Ordering::Relaxed), 0);
    }
}
