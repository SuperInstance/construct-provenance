//! # construct-provenance
//!
//! Full provenance tracking for GPU constructs.
//! Every compile, deploy, execute is recorded in an append-only log.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventType { Compiled, Deployed, Executed, Hotswapped, RolledBack }

#[derive(Debug, Clone)]
pub struct ProvenanceEntry {
    pub timestamp_us: u64,
    pub construct_name: String,
    pub version: String,
    pub git_hash: String,
    pub event: EventType,
    pub node: String,
    pub result_hash: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProvenanceLog {
    entries: Vec<ProvenanceEntry>,
    index_by_construct: HashMap<String, Vec<usize>>,
    index_by_result: HashMap<String, Vec<usize>>,
    time_us: u64,
}

impl ProvenanceLog {
    pub fn new() -> Self {
        Self { entries: Vec::new(), index_by_construct: HashMap::new(), index_by_result: HashMap::new(), time_us: 0 }
    }

    fn advance_time(&mut self, delta: u64) { self.time_us += delta; }

    pub fn record_compile(&mut self, name: &str, version: &str, git_hash: &str) {
        self.advance_time(100);
        let idx = self.entries.len();
        self.entries.push(ProvenanceEntry {
            timestamp_us: self.time_us, construct_name: name.into(), version: version.into(),
            git_hash: git_hash.into(), event: EventType::Compiled, node: "builder".into(), result_hash: None,
        });
        self.index_by_construct.entry(name.into()).or_default().push(idx);
    }

    pub fn record_deploy(&mut self, name: &str, version: &str, node: &str) {
        self.advance_time(50);
        let idx = self.entries.len();
        self.entries.push(ProvenanceEntry {
            timestamp_us: self.time_us, construct_name: name.into(), version: version.into(),
            git_hash: String::new(), event: EventType::Deployed, node: node.into(), result_hash: None,
        });
        self.index_by_construct.entry(name.into()).or_default().push(idx);
    }

    pub fn record_execute(&mut self, name: &str, version: &str, node: &str, result_hash: &str) {
        self.advance_time(200);
        let idx = self.entries.len();
        self.entries.push(ProvenanceEntry {
            timestamp_us: self.time_us, construct_name: name.into(), version: version.into(),
            git_hash: String::new(), event: EventType::Executed, node: node.into(), result_hash: Some(result_hash.into()),
        });
        self.index_by_construct.entry(name.into()).or_default().push(idx);
        self.index_by_result.entry(result_hash.into()).or_default().push(idx);
    }

    pub fn record_hotswap(&mut self, name: &str, old_ver: &str, new_ver: &str, node: &str) {
        self.advance_time(150);
        let idx = self.entries.len();
        self.entries.push(ProvenanceEntry {
            timestamp_us: self.time_us, construct_name: name.into(), version: format!("{}→{}", old_ver, new_ver),
            git_hash: String::new(), event: EventType::Hotswapped, node: node.into(), result_hash: None,
        });
        self.index_by_construct.entry(name.into()).or_default().push(idx);
    }

    /// Query: what version produced this result?
    pub fn find_producer(&self, result_hash: &str) -> Option<&ProvenanceEntry> {
        self.index_by_result.get(result_hash)
            .and_then(|indices| indices.last())
            .and_then(|&i| self.entries.get(i))
    }

    /// Get full history of a construct.
    pub fn construct_history(&self, name: &str) -> Vec<&ProvenanceEntry> {
        self.index_by_construct.get(name)
            .map(|indices| indices.iter().filter_map(|&i| self.entries.get(i)).collect())
            .unwrap_or_default()
    }

    /// Get all entries in time range.
    pub fn range(&self, start_us: u64, end_us: u64) -> Vec<&ProvenanceEntry> {
        self.entries.iter().filter(|e| e.timestamp_us >= start_us && e.timestamp_us <= end_us).collect()
    }

    pub fn entry_count(&self) -> usize { self.entries.len() }
    pub fn construct_count(&self) -> usize { self.index_by_construct.len() }
}

impl Default for ProvenanceLog {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_lifecycle() {
        let mut log = ProvenanceLog::new();
        log.record_compile("attention", "v1", "abc123");
        log.record_deploy("attention", "v1", "gpu-0");
        log.record_execute("attention", "v1", "gpu-0", "result_001");
        assert_eq!(log.entry_count(), 3);
    }

    #[test]
    fn test_find_producer() {
        let mut log = ProvenanceLog::new();
        log.record_compile("reduce", "v2", "def456");
        log.record_deploy("reduce", "v2", "gpu-1");
        log.record_execute("reduce", "v2", "gpu-1", "hash_xyz");
        let entry = log.find_producer("hash_xyz").unwrap();
        assert_eq!(entry.version, "v2");
        assert_eq!(entry.construct_name, "reduce");
    }

    #[test]
    fn test_construct_history() {
        let mut log = ProvenanceLog::new();
        log.record_compile("kernel", "v1", "aaa");
        log.record_deploy("kernel", "v1", "gpu-0");
        log.record_hotswap("kernel", "v1", "v2", "gpu-0");
        log.record_execute("kernel", "v2", "gpu-0", "res");
        let history = log.construct_history("kernel");
        assert_eq!(history.len(), 4);
    }

    #[test]
    fn test_time_range() {
        let mut log = ProvenanceLog::new();
        log.record_compile("a", "v1", "a1"); // t=100
        log.record_compile("b", "v1", "b1"); // t=200
        log.record_deploy("a", "v1", "g0");  // t=250
        let range = log.range(150, 300);
        assert_eq!(range.len(), 2); // b compile (200) and a deploy (250)
    }

    #[test]
    fn test_no_producer() {
        let log = ProvenanceLog::new();
        assert!(log.find_producer("nonexistent").is_none());
    }

    #[test]
    fn test_multiple_executions() {
        let mut log = ProvenanceLog::new();
        log.record_compile("filter", "v1", "fff");
        log.record_deploy("filter", "v1", "gpu-0");
        log.record_execute("filter", "v1", "gpu-0", "r1");
        log.record_execute("filter", "v1", "gpu-0", "r2");
        log.record_execute("filter", "v1", "gpu-0", "r3");
        assert_eq!(log.construct_history("filter").len(), 5);
        assert!(log.find_producer("r3").is_some());
    }
}
