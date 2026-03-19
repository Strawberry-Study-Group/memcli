# MemCore Issues Tracker

Issues discovered during design review. Status updated as issues are resolved.

## Open Issues

### #1: Name validation regex compiled on every call
- **Severity:** Low (performance)
- **Location:** `src/node.rs:validate_name()`
- **Description:** `Regex::new()` is called every time `validate_name()` runs. Should use `LazyLock` or `once_cell` to compile once.

### #2: Graph.idx lazy write timing
- **Severity:** Low
- **Description:** `graph.idx` is written after every mutation. Could batch writes or debounce for high-throughput workloads.

### #4: Daemon error logging swallows context
- **Severity:** Low
- **Description:** Some daemon error paths log a generic message without the underlying error chain.

### #5: WAL file grows unbounded
- **Severity:** Medium
- **Description:** The WAL file is only cleared on startup recovery. Long-running daemons accumulate committed entries that are never cleaned up.

### #6: No rate limiting on TCP connections
- **Severity:** Low
- **Description:** The daemon accepts unlimited connections with no rate limiting. A misbehaving client could exhaust resources.

### #8: Rename does not update vector index key
- **Severity:** Medium
- **Location:** `src/handler.rs:handle_rename()`
- **Description:** When a node is renamed, the vector index entry still uses the old name. A `reindex` is needed after rename.

### #9: `update` does not validate name hasn't changed
- **Severity:** Low
- **Description:** The update handler trusts the name argument matches the content. No check prevents mismatches.

### #10: No pagination for `ls` command
- **Severity:** Low
- **Description:** `ls` returns all nodes at once. For large stores (1000+), this could be slow.

### #11: Concurrent multi-search deduplication
- **Severity:** Low
- **Description:** `multi-search` merges results from multiple queries but doesn't weight duplicates that appear in multiple query results.

### #12: Config hot-reload not supported
- **Severity:** Low
- **Description:** Changes to `memcore.toml` require daemon restart. No SIGHUP or watch mechanism.

### #13: No backup/export mechanism
- **Severity:** Low
- **Description:** No built-in command to export/import the full memory store.

### #14: Vector index not saved after single node operations
- **Severity:** Medium
- **Location:** `src/handler.rs:handle_create()`, `handle_update()`
- **Description:** After computing an embedding for a new/updated node, the vector index is updated in memory but not persisted to disk. Only `reindex` saves the full index.

### #15: Access count overflow
- **Severity:** Low
- **Description:** `access_count` is a u32 that could theoretically overflow for extremely active nodes.

### #16: Embedding model path not configurable
- **Severity:** Low
- **Description:** The model directory is hardcoded relative to the memcore dir. Users can't point to a shared model location.

## Resolved Issues

### #3: Baseline algorithm includes self-similarity ~~(OPEN)~~ RESOLVED
- **Severity:** Medium
- **Resolution:** Both `handle_baseline()` and `handle_inspect()` use `j in (i+1)..` which correctly excludes self-pairs. Verified in current code.

### #7: Daemon startup race condition ~~(OPEN)~~ RESOLVED
- **Severity:** High
- **Resolution:** Added `fs2::try_lock_exclusive()` on `.daemon.lock` file in `daemon.rs`. Prevents concurrent daemon starts. Lock released automatically on process exit.
