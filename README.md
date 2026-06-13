# construct-provenance

**Append-only provenance tracking for GPU compute constructs** — every compilation, deployment, execution, and hotswap is recorded in a tamper-evident log with O(1) indexed lookup. Query "what version produced this result?" and get the full chain of custody in constant time.

## Why It Matters

In distributed GPU fleets, the same kernel may be compiled from different git commits, deployed to different nodes, and hotswapped at runtime. When a model produces a bad result, you need to trace *which* version of *which* construct on *which* node generated it — and what happened before and after. This is the software-supply-chain problem applied to GPU artifacts.

Production systems like MLflow, Weights & Biases, and Kubernetes audit logs solve this for ML training runs and cluster events. `construct-provenance` brings the same discipline to GPU construct lifecycle management: a lightweight, in-memory append-only log with dual indexes (by construct name and by result hash) that answers provenance queries without scanning the entire history.

## How It Works

### Append-Only Log Model

The log is a `Vec<ProvenanceEntry>` — entries are only ever appended, never mutated or deleted. This gives:

- **Tamper evidence**: any modification breaks the time-ordered sequence
- **O(1) append**: push to the end, amortized constant time
- **O(n) full scan**: for range queries, linear in the number of entries

### Dual Index Structure

Two `HashMap` indexes provide fast lookup:

```
index_by_construct: HashMap<String, Vec<usize>>  // name → entry indices
index_by_result:    HashMap<String, Vec<usize>>  // result_hash → entry indices
```

| Operation | Time Complexity | Space Complexity |
|-----------|----------------|-----------------|
| `record_compile` | O(1) amortized | O(1) per entry |
| `record_deploy` | O(1) amortized | O(1) per entry |
| `record_execute` | O(1) amortized | O(1) per entry |
| `record_hotswap` | O(1) amortized | O(1) per entry |
| `find_producer(hash)` | O(1) expected | O(k) where k = executions with that hash |
| `construct_history(name)` | O(k) where k = entries for that construct | O(k) |
| `range(start, end)` | O(n) worst case | O(n) for result vec |

### Event Types

The state machine for a construct's lifecycle:

```
Compiled → Deployed → Executed → (Hotswapped → Executed)* → (RolledBack)?
```

Each transition is logged with a monotonically increasing timestamp (microsecond granularity), the construct name, version string, git hash (for compiles), node identifier, and optional result hash (for executions).

### Information-Theoretic Integrity

Each entry stores `git_hash` (SHA-1 of the source tree at compile time). The probability of a hash collision is:

$$P(\text{collision}) = \frac{n^2}{2 \times 2^{160}}$$

For n = 10⁶ constructs, this is ≈ 4.3 × 10⁻³⁵ — effectively zero.

## Quick Start

```rust
use construct_provenance::ProvenanceLog;

let mut log = ProvenanceLog::new();

// Record a construct's lifecycle
log.record_compile("attention", "v1", "abc123");
log.record_deploy("attention", "v1", "gpu-0");
log.record_execute("attention", "v1", "gpu-0", "result_001");

// Query: what version produced this result?
let producer = log.find_producer("result_001").unwrap();
assert_eq!(producer.version, "v1");
assert_eq!(producer.construct_name, "attention");

// Full history of a construct
let history = log.construct_history("attention");
assert_eq!(history.len(), 3); // compile + deploy + execute
```

## API

### `ProvenanceLog`
- `new() -> Self` — Create an empty log
- `record_compile(&mut self, name, version, git_hash)` — Log a compilation event
- `record_deploy(&mut self, name, version, node)` — Log a deployment to a node
- `record_execute(&mut self, name, version, node, result_hash)` — Log an execution with output hash
- `record_hotswap(&mut self, name, old_ver, new_ver, node)` — Log a runtime weight swap
- `find_producer(&self, result_hash) -> Option<&ProvenanceEntry>` — O(1) lookup of which construct produced a result
- `construct_history(&self, name) -> Vec<&ProvenanceEntry>` — Full lifecycle of a construct
- `range(&self, start_us, end_us) -> Vec<&ProvenanceEntry>` — Time-range query
- `entry_count(&self) -> usize` — Total entries in log
- `construct_count(&self) -> usize` — Distinct constructs tracked

### `ProvenanceEntry`
- `timestamp_us: u64` — Monotonic microsecond timestamp
- `construct_name: String` — Name of the GPU construct
- `version: String` — Semantic version string
- `git_hash: String` — Source tree hash at compile time
- `event: EventType` — One of: `Compiled`, `Deployed`, `Executed`, `Hotswapped`, `RolledBack`
- `node: String` — Node identifier where the event occurred
- `result_hash: Option<String>` — Hash of execution output (if applicable)

## Architecture Notes

This crate provides the provenance layer of the SuperInstance GPU orchestration stack. It links to:

- **construct-supply-chain** — feeds compilation/deployment events into the log
- **edge-conservation-rs** — verifies that provenance entries satisfy conservation invariants (γ_in + η_out = C)
- **drone-fleet-ternary** — records hotswap events during ternary weight updates

The conservation link is critical: the invariant γ + η = C (where γ is the set of active constructs and η is the set of retired constructs) ensures no construct is lost during hotswap. The provenance log provides the audit trail that makes this verifiable.

See the full architecture: [ARCHITECTURE.md](https://github.com/SuperInstance/SuperInstance/blob/main/ARCHITECTURE.md)

## References

1. Buneman, P., Khanna, S., & Tan, W.C. (2001). "Why and Where: A Characterization of Data Provenance." *ICDT 2001.*
2. Simmhan, Y.L., Plale, B., & Gannon, D. (2005). "A Survey of Data Provenance in e-Science." *ACM SIGMOD Record, 34(3).*
3. Herschel, U., et al. (2017). "A Classification of Provenance for Security Scenarios." *IEEE Transactions on Dependable and Secure Computing.*
4. MLflow — [mlflow.org](https://mlflow.org/) — Open-source model lifecycle management
5. OCI Image Spec — [github.com/opencontainers/image-spec](https://github.com/opencontainers/image-spec) — Digest-based artifact provenance

## License

MIT
