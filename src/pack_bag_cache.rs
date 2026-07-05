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

use crate::file_analysis::FileAnalysis;

type Loader = Box<dyn Fn(&Path) -> Option<FileAnalysis> + Send + Sync>;

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
    /// Keyed single-file decode (opens the pack SQLite conn on demand).
    loader: Loader,
}

impl PackBagCache {
    pub fn new(
        cap_bytes: usize,
        loader: impl Fn(&Path) -> Option<FileAnalysis> + Send + Sync + 'static,
    ) -> Self {
        PackBagCache {
            entries: DashMap::new(),
            recency: DashMap::new(),
            clock: AtomicU64::new(0),
            bytes: AtomicUsize::new(0),
            cap_bytes,
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
        if let Some(hit) = self.entries.get(path) {
            let arc = hit.value().clone();
            drop(hit);
            self.recency.insert(path.to_path_buf(), self.tick());
            return Some(arc);
        }
        let fa = Arc::new((self.loader)(path)?);
        if self.cap_bytes == 0 {
            return Some(fa); // rehydrate-and-drop
        }
        let sz = fa.heap_estimate().total();
        self.entries.insert(path.to_path_buf(), fa.clone());
        self.recency.insert(path.to_path_buf(), self.tick());
        self.bytes.fetch_add(sz, Ordering::Relaxed);
        self.evict_to_cap(path);
        Some(fa)
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
        let mut parser = crate::builder::create_parser();
        let tree = parser.parse(src, None).unwrap();
        crate::builder::build(&tree, src.as_bytes())
    }

    #[test]
    fn cap_zero_never_retains() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let cache = PackBagCache::new(0, move |_p| {
            c.fetch_add(1, Ordering::Relaxed);
            Some(empty_fa())
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
            Some(empty_fa())
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
        let cache = PackBagCache::new(one, move |_p| Some(empty_fa()));
        let a = PathBuf::from("/x/a.h");
        let b = PathBuf::from("/x/b.h");
        cache.bag_for(&a);
        cache.bag_for(&b); // pushes a out (a is LRU, b is the keep)
        assert!(cache.entries.contains_key(&b));
        assert!(!cache.entries.contains_key(&a));
        assert!(cache.bytes.load(Ordering::Relaxed) <= one);
    }

    #[test]
    fn invalidate_drops_entry() {
        let cache = PackBagCache::new(128 * 1024 * 1024, move |_p| Some(empty_fa()));
        let p = PathBuf::from("/x/a.h");
        cache.bag_for(&p);
        assert!(cache.entries.contains_key(&p));
        cache.invalidate(&p);
        assert!(!cache.entries.contains_key(&p));
        assert_eq!(cache.bytes.load(Ordering::Relaxed), 0);
    }
}
