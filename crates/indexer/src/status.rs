//! What the indexer reports: the running scan, the queue and how scans ended. Shared by the native indexer and the
//! web build's.

use crate::ScanKind;

/// How a scan ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanResult {
    Completed,
    Offline,
    Cancelled,
    /// An index error or a panic; the scan counts as finished (REQ-002-13).
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentScan {
    pub library: i64,
    pub kind: ScanKind,
    pub scanned: u64,
    pub indexed: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexerStatus {
    pub current: Option<CurrentScan>,
    /// Libraries waiting to be scanned, in order.
    pub queue: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexerEvent {
    Status(IndexerStatus),
    /// Files of the library were written to the index.
    Written(i64),
    Done(i64, ScanResult),
}
