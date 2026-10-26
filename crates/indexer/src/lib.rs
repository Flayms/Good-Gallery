//! Keeps the index in sync with the libraries: scan queue, walk, watcher, writer, rescan scheduling. Library
//! operations (add, remove, rescan) live here too, since they check reachability and queue scans.

pub mod libraries;
pub mod reachability;

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
    /// Drops the library's queued scan, cancels its running scan and stops its watcher.
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
