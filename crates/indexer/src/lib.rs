//! Keeps the index in sync with the libraries: scan queue, walk, watcher, writer, rescan scheduling. Library
//! operations (add, remove, rescan) live here too, since they check reachability and queue scans.

//!
//! The web build (spec 019) has no threads, files or helper programs: it uses only the library operations and the
//! status types, and scans with its own code.

#[cfg(not(target_arch = "wasm32"))]
mod indexer;
pub mod libraries;
#[cfg(not(target_arch = "wasm32"))]
pub mod reachability;
#[cfg(not(target_arch = "wasm32"))]
pub mod retry;
#[cfg(not(target_arch = "wasm32"))]
pub mod scan;
#[cfg(not(target_arch = "wasm32"))]
pub mod schedule;
mod status;
#[cfg(not(target_arch = "wasm32"))]
pub mod thumb_cache;
#[cfg(not(target_arch = "wasm32"))]
pub mod thumbnails;
#[cfg(not(target_arch = "wasm32"))]
pub mod walk;
#[cfg(not(target_arch = "wasm32"))]
pub mod watcher;

#[cfg(not(target_arch = "wasm32"))]
pub use indexer::{Indexer, IndexerDeps, merge_scopes};
#[cfg(not(target_arch = "wasm32"))]
pub use scan::ScanScope;
pub use status::{CurrentScan, IndexerEvent, IndexerStatus, ScanResult};

/// How thoroughly a scan looks at a library (glossary: full scan, quick scan, changes scan). Ordered from least to
/// most thorough.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScanKind {
    Changes,
    Quick,
    Full,
}

/// Where library operations send their scan requests: the scan queue, or nothing.
pub trait ScanRequests: Send + Sync {
    fn request_scan(&self, library: i64, kind: ScanKind);
    /// A changes scan of these files (relative, `/`-separated), e.g. after the app wrote them (spec 021). Without a
    /// scan queue that knows paths: unknown changes.
    fn request_paths(&self, library: i64, paths: Vec<String>) {
        let _ = paths;
        self.request_scan(library, ScanKind::Changes);
    }
    /// Drops the library's queued scan, cancels its running scan (and waits until it has stopped) and stops its
    /// watcher.
    fn remove_library(&self, library: i64);
}

/// Ignores scan requests (no indexer running).
pub struct NoScans;

impl ScanRequests for NoScans {
    fn request_scan(&self, library: i64, kind: ScanKind) {
        tracing::info!(library, ?kind, "scan requested, but no indexer is running");
    }

    fn remove_library(&self, _library: i64) {}
}
