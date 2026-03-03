# MemCore CLI API Reference

> Binary: `memcore` v0.1.0 — Non-parametric memory core for AI agents

## Architecture

```
memcore <command>     CLI client mode (thin: parse args -> TCP to daemon -> print result)
memcore daemon        Daemon mode (holds graph + vector index + name index in memory)
```

The CLI auto-starts the daemon on first call. Daemon auto-exits after 30 min idle.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | User error (invalid args, node not found, validation failure) |
| 2 | System error (disk I/O, WAL failure, embedding model issue) |
| 3 | Connection error (daemon communication failure) |

All errors print to stderr. Normal output goes to stdout.

## Node naming rules

Regex: `^[a-zA-Z][a-zA-Z0-9 \-]{0,126}[a-zA-Z0-9]$`

- Must start with a letter (upper or lower case)
- Must end with a letter or digit
- May contain letters, digits, spaces, and hyphens
- Length: 2-128 characters
- Examples: `How to deploy the app`, `Project Alpha v2`, `rust-memory-tips`

## Node file format

Every node is a YAML-frontmatter + Markdown body file stored in `~/.memcore/memories/<name>.md`:

```yaml
---
created: '2026-02-27T10:30:00Z'
updated: '2026-02-27T10:30:00Z'
weight: 1.0
last_accessed: '2026-02-27T10:30:00Z'
access_count: 0
pinned: false
links:
  - other-node
abstract: Short description used for embedding
---

Body content in markdown.
```

**System-managed fields** (set automatically, preserved across updates):

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `created` | DateTime (UTC) | now | Immutable after creation |
| `updated` | DateTime (UTC) | now | Updated on content changes |
| `last_accessed` | DateTime (UTC) | now | Updated on get/search/recall |
| `access_count` | u32 | 0 | Incremented on access |

**User-controlled fields**:

| Field | Type | Default | Range |
|-------|------|---------|-------|
| `weight` | f32 | 1.0 | 0.0-1.0 (clamped) |
| `pinned` | bool | false | |
| `links` | Vec\<String\> | [] | Must reference existing nodes |
| `abstract` | String | "" | Used for embedding computation |

**Minimal input format** (everything else auto-filled):

```yaml
---
abstract: Brief description
---

Body content here.
```

---

## Commands

### init

Initialize memcore directory structure and default config.

```
memcore init [--dir <path>]
```

| Arg | Type | Default | Description |
|-----|------|---------|-------------|
| `--dir` | String | `~/.memcore/` | Custom base directory |

Creates: `memories/`, `index/`, `models/`, and `memcore.toml` (if not present).

Client-side only, does not contact daemon. Idempotent.

**Output:**
```
initialized memcore at /home/user/.memcore
```

---

### create

Create a new memory node.

```
memcore create <name> [-f <file>]
```

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `name` | String | yes | Node name (must pass name regex) |
| `-f` | String | no | Read content from file; omit to read from stdin |

**Logic:**
1. Validates name format
2. Rejects if node already exists
3. Parses YAML frontmatter + body
4. Validates links: rejects self-references, rejects links to nonexistent nodes
5. Sets system timestamps and defaults
6. Writes `memories/<name>.md` atomically (tempfile + rename)
7. Updates in-memory indices (name, graph, metadata)
8. Adds bidirectional edges; updates peer `.md` files
9. Computes embedding from abstract text (if `--features embedding`)
10. Logs to WAL

**Output:**
```
created: my-node [2 links]
```

**Errors (exit 1):** Invalid name, duplicate name, invalid YAML, self-link, link to nonexistent node.

---

### get

Read one or more nodes.

```
memcore get <name> [<name2> ...]
```

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `names` | String... | yes (1+) | One or more node names |

Returns complete file content (frontmatter + body). Increments `access_count` and updates `last_accessed` in memory.

**Output (single):** Raw file content to stdout.
**Output (batch):** Sections separated by `\n=== <name> ===\n` headers.

**Errors (exit 1):** Node not found.

---

### update

Full replacement update of an existing node.

```
memcore update <name> [-f <file>]
```

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `name` | String | yes | Existing node name |
| `-f` | String | no | Read content from file; omit to read from stdin |

**Logic:**
1. Preserves original `created` timestamp and `access_count`
2. Sets `updated` to now
3. Computes link diff (added/removed edges)
4. Updates peer `.md` files for link changes
5. Re-computes embedding only if abstract text changed (xxHash64 comparison)
6. Logs to WAL

**Output:**
```
updated: my-node [links: +1 -2]
```

**Errors (exit 1):** Node not found, invalid YAML, invalid links.

---

### patch

Local modification of a node's body.

```
memcore patch <name> --replace <old> <new>
memcore patch <name> --append <text>
memcore patch <name> --prepend <text>
```

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `name` | String | yes | Existing node name |
| `--replace` | 2 Strings | one of | Replace first occurrence of `<old>` with `<new>` |
| `--append` | String | one of | Append text to end of body |
| `--prepend` | String | one of | Prepend text to beginning of body |

Exactly one of `--replace`, `--append`, `--prepend` is required.

Modifies body only (not frontmatter). Updates `updated` timestamp. Does not re-compute embedding (abstract unchanged). No WAL (lightweight single-file op).

**Output:**
```
patched: my-node [append]
```

**Errors (exit 1):** Node not found, no patch op specified.

---

### delete

Delete a memory node. Irreversible.

```
memcore delete <name>
```

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `name` | String | yes | Node to delete |

**Logic:**
1. Removes from graph (collects neighbor list)
2. Updates all neighbors' in-memory link lists
3. Removes from name index, metadata, vector index
4. Updates all neighbors' `.md` files (removes this node from their links)
5. Deletes `memories/<name>.md`
6. Logs to WAL

**Output:**
```
deleted: my-node [3 edges removed]
```

**Errors (exit 1):** Node not found.

---

### rename

Rename a node, preserving all metadata, content, and links.

```
memcore rename <old> <new>
```

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `old` | String | yes | Current node name |
| `new` | String | yes | New name (must pass name regex) |

**Logic:**
1. Validates new name
2. Rejects if new name already exists
3. Renames in graph, name index, metadata, vector index
4. Renames file on disk
5. Updates all neighbor `.md` files (old name -> new name in their link arrays)
6. Logs to WAL

**Output:**
```
renamed: old-name -> new-name [3 neighbor files updated]
```

**Errors (exit 1):** Old not found, new name invalid, new name already exists.

---

### ls

List all nodes.

```
memcore ls [--sort <field>]
```

| Arg | Type | Default | Values |
|-----|------|---------|--------|
| `--sort` | String | `name` | `name`, `weight`, `date` |

Sort order:
- `name` — alphabetical ascending
- `weight` — descending (highest first)
- `date` — by `last_accessed` string (ascending)

**Output:**
```
NAME                WEIGHT  EDGES  PIN  ACCESSED
project-alpha         1.00      3    *  5m ago
helper-utils          0.85      1       2h ago
old-notes             0.30      0       14d ago
```

**Errors (exit 1):** Unknown sort field.

---

### link

Create a bidirectional edge between two nodes.

```
memcore link <a> <b>
```

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `a` | String | yes | First node |
| `b` | String | yes | Second node |

Idempotent: linking already-linked nodes is a no-op (no duplicate entries). Updates `updated` timestamp on both nodes in memory. Updates both `.md` files (link arrays only; `updated` timestamp is not persisted to disk). Logs to WAL.

**Output:**
```
linked: node-a <-> node-b
```

**Errors (exit 1):** Node not found, self-link (a == b).

---

### unlink

Remove a bidirectional edge between two nodes.

```
memcore unlink <a> <b>
```

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `a` | String | yes | First node |
| `b` | String | yes | Second node |

Idempotent: unlinking already-unlinked nodes is a no-op. Updates `updated` timestamp on both nodes in memory. Updates both `.md` files (link arrays only; `updated` timestamp is not persisted to disk). Logs to WAL.

**Output:**
```
unlinked: node-a <-> node-b
```

**Errors (exit 1):** Node not found.

---

### boost

Positive feedback — memory helped correct judgment.

```
memcore boost <name>
```

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `name` | String | yes | Node to boost |

**Formula:** `weight = min(weight + boost_amount, 1.0)`

Default `boost_amount` = 0.1 (configurable in `memcore.toml`).

Persists weight change to `.md` file. No WAL (lightweight single-file op).

**Output:**
```
boosted: my-node (0.80 -> 0.90)
```

**Errors (exit 1):** Node not found.

---

### penalize

Negative feedback — memory led to incorrect judgment.

```
memcore penalize <name>
```

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `name` | String | yes | Node to penalize |

**Formula:** `weight = weight * penalty_factor`

Default `penalty_factor` = 0.8 (configurable). Geometric decay: after N penalties, `weight = initial * 0.8^N`.

Persists weight change to `.md` file.

**Output:**
```
penalized: my-node (1.00 -> 0.80)
```

**Errors (exit 1):** Node not found.

---

### pin

Mark node as core long-term memory.

```
memcore pin <name>
```

Pinned nodes are always included in working memory recall (no-query mode). Persists to `.md` file.

**Output:** `pinned: my-node`

**Errors (exit 1):** Node not found.

---

### unpin

Remove core memory marker.

```
memcore unpin <name>
```

**Output:** `unpinned: my-node`

**Errors (exit 1):** Node not found.

---

### neighbors

Show neighbors of a node via graph traversal.

```
memcore neighbors <name> [--depth <d>] [--limit <n>] [--offset <m>]
```

| Arg | Type | Default | Description |
|-----|------|---------|-------------|
| `name` | String | (required) | Root node |
| `--depth` | usize | 1 | BFS traversal depth |
| `--limit` | usize | 50 | Max results returned |
| `--offset` | usize | 0 | Skip first N results (pagination) |

Uses breadth-first search. Returns `(name, depth, weight)` tuples.

**Output:**
```
depth=2, showing 5/47
  d=1  neighbor-a                    w=0.85
  d=1  neighbor-b                    w=0.92
  d=2  far-node                      w=0.70
```

**Errors (exit 1):** Node not found.

---

### search

Pure vector similarity search.

```
memcore search <query> [--top-k <n>]
```

| Arg | Type | Default | Description |
|-----|------|---------|-------------|
| `query` | String | (required) | Semantic query text |
| `--top-k` | usize | 5 | Number of results |

Requires `--features embedding`. Computes query embedding, searches HNSW index, returns raw similarity scores.

**Output:**
```
0.8234  project-alpha                 w=0.85  Memory about project alpha...
0.7956  related-topic                 w=0.92  Another relevant memory...
```

Columns: score (4 dec), name (30 chars), weight (2 dec), abstract (60 chars).

**Errors (exit 2):** Embedding model not loaded / feature not compiled.

---

### recall

Comprehensive memory retrieval (vector + graph + weight scoring).

```
memcore recall [<query>] [--top-k <n>] [--depth <d>] [--name <prefix>]
```

| Arg | Type | Default | Description |
|-----|------|---------|-------------|
| `query` | String | (optional) | Semantic query text |
| `--top-k` | usize | 5 | Max results |
| `--depth` | usize | 1 | Graph expansion depth from seed nodes |
| `--name` | String | (optional) | Name prefix search mode |

**Three operating modes:**

#### Mode 1: Name prefix (`--name <prefix>`)

Searches sorted name index for prefix matches. Returns up to `top_k` matches by name order. No embedding needed.

```
memcore recall --name project
```

#### Mode 2: Semantic query (`<query>`)

Requires `--features embedding`.

1. Compute query embedding
2. Vector search for `top_k` seed nodes
3. BFS expansion from seeds to `depth`
4. Score candidates: `score = alpha*similarity + beta*weight + gamma*proximity`
5. Deduplicate, sort descending, return `top_k`

**Scoring formula:**

```
score = 0.6 * similarity + 0.2 * weight + 0.2 * graph_proximity
```

Proximity for node at `hops` distance from a seed (default `edge_distance` metric):
- hops=0 (seed itself): proximity = 0.0 (uses similarity score instead)
- hops=N (N>0): proximity = 1/(1+N)

```
memcore recall "how to handle errors in Rust"
```

#### Mode 3: Working memory (no arguments)

Returns pinned nodes + highest-weight non-pinned nodes, up to `top_k`. No embedding needed.

```
memcore recall
```

**Output:** Same format as `search`.

**Errors (exit 2):** Query mode requires embedding feature.

---

### multi-search

Multi-term semantic vector search. Casts a wider net by searching for multiple query terms independently and merging results.

```
memcore multi-search <query1> [<query2> ...] [--top-k <n>]
```

| Arg | Type | Default | Description |
|-----|------|---------|-------------|
| `queries` | String... | (required, 1+) | One or more query texts |
| `--top-k` | usize | 5 | Max results **per query term** |

Requires `--features embedding`. For each query term, computes embedding and searches for `top_k` results. Merges all results using **max-similarity** per unique node. The final result has no global cap — it returns the full deduplicated union (up to `n_terms * top_k` minus duplicates).

**Example:**
```bash
memcore multi-search "rust error handling" "exception patterns" --top-k 5
# Returns up to 10 results (5 per term, deduplicated)
```

**Output:** Same format as `search`.

**Errors (exit 2):** Embedding model not loaded / feature not compiled.

---

### multi-recall

Multi-term comprehensive retrieval (vector + graph + weight scoring). Wider recall than single-query `recall` by using multiple query terms as seeds.

```
memcore multi-recall <query1> [<query2> ...] [--top-k <n>] [--depth <d>]
```

| Arg | Type | Default | Description |
|-----|------|---------|-------------|
| `queries` | String... | (required, 1+) | One or more query texts |
| `--top-k` | usize | 5 | Max results **per query term** |
| `--depth` | usize | 1 | Graph expansion depth from seed nodes |

Requires `--features embedding`. For each query term:
1. Compute embedding, search for `top_k` seed nodes
2. BFS expand from seeds to `depth`

Merge across all queries:
- **Max similarity** per unique node
- **Min graph distance** per unique node
- Score: `alpha*max_sim + beta*weight + gamma*proximity(min_dist)`
- Sort descending, return all merged results (no global cap)

**Example:**
```bash
memcore multi-recall "deployment" "CI/CD pipeline" --top-k 5 --depth 2
# Returns union of seeds from both terms + graph neighbors
```

**Output:** Same format as `recall` (node names, one per line).

**Errors (exit 2):** Embedding model not loaded / feature not compiled.

---

### inspect

System diagnostics or per-node analysis.

#### Global inspection (no node name)

```
memcore inspect [--format <fmt>] [--threshold <t>] [-t <t>] [--cap <n>]
```

| Arg | Type | Default | Description |
|-----|------|---------|-------------|
| `--format` | String | `human` | `human` or `json` |
| `--threshold`, `-t` | f32 | 0.85 | Similarity threshold for finding similar pairs (0.0-1.0) |
| `--cap` | usize | 50 | Max number of similar pairs to display |

**Computed metrics:**

| Metric | Formula |
|--------|---------|
| `node_count` | Total nodes in system |
| `edge_count` | Total bidirectional edges |
| `cluster_count` | Connected components in graph |
| `orphan_count` | Components of size 1 |
| `orphan_ratio` | orphan_count / node_count |
| `graveyard_ratio` | count(weight < warn_threshold) / node_count |
| `density` | edge_count / node_count (0 if <= 1 node) |
| `health_score` | `1.0 - (0.20*orphan_ratio + 0.20*graveyard_ratio + 0.15*density_imbalance)` |
| `redundancy` | count(unique nodes in similar pairs) / node_count |

**Density imbalance** (used in health):
- density < 1.5: `(1.5 - density) / 1.5`
- density > 8.0: `(density - 8.0) / density`
- otherwise: `0.0`

**Similar pairs:**

Upper-triangle brute-force cosine similarity across all indexed nodes. Reports pairs where `similarity >= threshold`. Sorted descending by similarity, capped at `--cap` pairs (default **50**). Uses a bounded min-heap internally so memory usage is O(N), not O(N^2), regardless of threshold. The response includes `total_similar_pairs` counting all pairs above threshold (not just the displayed ones).

Redundancy counts ALL nodes involved in above-threshold pairs (not just the displayed ones).

**Output (human):**
```
=== System Health ===
health:     87%
nodes:      42
edges:      128
clusters:   3
orphans:    2 (5%)
graveyard:  3%
density:    3.05
redundancy: 0.15

Similar pairs (3 found):
  node-a  <->  node-b  0.95
  node-c  <->  node-d  0.88
  node-e  <->  node-f  0.86

Orphans: lonely-node, island-node
Low weight: decaying-node
```

**Output (json):**
```json
{
  "node_count": 42,
  "edge_count": 128,
  "cluster_count": 3,
  "orphan_count": 2,
  "health_score": 0.87,
  "redundancy": 0.15,
  "orphan_ratio": 0.048,
  "graveyard_ratio": 0.024,
  "density": 3.05,
  "similar_pairs": [
    { "node_a": "node-a", "node_b": "node-b", "similarity": 0.95 }
  ],
  "orphans": ["lonely-node"],
  "low_weight": ["decaying-node"]
}
```

**Performance at scale:**

| Nodes | Pairs checked | Approx. time (384 dims) | RAM overhead |
|-------|--------------|------------------------|--------------|
| 1K | 500K | <0.1s | <1MB |
| 10K | 50M | ~1-2s | <1MB |
| 50K | 1.25B | ~10-30s | <1MB |
| 100K | 5B | ~1-2 min | <1MB |

RAM stays bounded regardless of threshold thanks to the min-heap (top-`cap` pairs only, O(cap) output memory; default 50).

#### Per-node inspection

```
memcore inspect <name> [--format <fmt>]
```

| Arg | Type | Default | Description |
|-----|------|---------|-------------|
| `name` | String | (required) | Node to inspect |
| `--format` | String | `human` | `human` or `json` |

Returns: edge count, top 10 most similar nodes (via vector index), and warnings.

**Output (human):**
```
inspect: my-node (5 edges)

  similar nodes:
    close-match     0.89
    related-topic   0.87
    tangential      0.72

warning: node 'my-node' has no embedding
```

**Errors (exit 1):** Node not found. Invalid format.

---

### status

Show daemon status.

```
memcore status
```

**Output:**
```
pid:        12345
port:       9876
nodes:      42
edges:      128
indexed:    38
uptime:     2h 15m
```

`indexed` = nodes with embeddings in vector index.

---

### reindex

Rebuild all embeddings from `.md` files.

```
memcore reindex
```

Requires `--features embedding`. Iterates all nodes, recomputes embeddings from abstract text, saves index to disk. Best-effort: continues on individual failures.

**Output:**
```
reindex: 42 nodes indexed, 0 errors
```

**Errors (exit 2):** Embedding feature not compiled / model not loaded.

---

### baseline

Compute similarity distribution statistics.

```
memcore baseline
```

Requires `--features embedding` and >= 2 indexed nodes. Computes pairwise cosine similarity for all node pairs (upper triangle), then reports distribution statistics.

**Output:**
```
baseline: 861 pairs | mean=0.4232 min=0.0012 p25=0.3456 p50=0.5123 p75=0.7234 p95=0.8901 max=0.9876
saved to /home/user/.memcore/index/mem-base-info.json
```

Useful for calibrating the `--threshold` parameter in `inspect`.

**Errors (exit 1):** Less than 2 indexed nodes.
**Errors (exit 2):** Embedding feature not compiled.

---

### gc

Clean dangling references.

```
memcore gc
```

Scans all nodes for link targets that don't exist. Removes dangling links from both in-memory state and `.md` files. Rebuilds graph from cleaned state.

Idempotent: safe to run multiple times.

**Output:**
```
gc: cleaned 3 dangling references
```

---

### stop

Gracefully stop the daemon.

```
memcore stop
```

The next CLI command will auto-start a new daemon instance.

**Output:**
```
stopping daemon
```

---

## Configuration

Default config file: `~/.memcore/memcore.toml`

Override base directory via `MEMCORE_DIR` environment variable.

```toml
[weight]
boost_amount = 0.1          # Additive boost per positive feedback
penalty_factor = 0.8        # Multiplicative decay per negative feedback
warn_threshold = 0.1        # Nodes below this weight appear in inspect "low weight"

[recall]
alpha = 0.6                 # Weight for vector similarity in scoring
beta = 0.2                  # Weight for node importance (weight field)
gamma = 0.2                 # Weight for graph proximity
default_depth = 1           # Default BFS expansion depth
proximity_metric = "edge_distance"  # or "edge_distance_squared"

[index]
engine = "usearch"          # Vector search engine
metric = "cosine"           # Similarity metric
ef_construction = 128       # HNSW construction parameter
m = 16                      # HNSW max connections per layer

[inspect]
max_cluster_full_scan = 100
similarity_top_pairs = 50

[daemon]
idle_timeout_minutes = 30   # Auto-exit after idle period
bind_host = "127.0.0.1"    # Listen address (localhost only)
port = 0                    # 0 = auto-assign
```

### Proximity metrics

| Metric | Formula | Behavior |
|--------|---------|----------|
| `edge_distance` | `1 / (1 + hops)` | Gentle falloff |
| `edge_distance_squared` | `1 / (1 + hops)^2` | Sharp falloff, strongly favors direct neighbors |

Seed nodes (hops=0) always get proximity=0.0 since their contribution comes from the similarity term.

## Embedding model

Default: `intfloat/multilingual-e5-small` (int8 ONNX quantized)

| Property | Value |
|----------|-------|
| Dimensions | 384 |
| Max tokens | 512 |
| Languages | 100+ |
| ONNX size | ~118MB |

Model files in `~/.memcore/models/`:
- `model_quantized.onnx` — ONNX model
- `tokenizer.json` — tokenizer
- `config.json` — model metadata (`name`, `dimensions`, `max_tokens`)

Users can swap models by replacing these files.

## Wire protocol

TCP, length-prefixed JSON. Handshake before framing:

```
Client: MEMCORE_PING <nonce>\n
Daemon: MEMCORE_PONG <version>\n
```

Framing: `[4 bytes u32 LE length][N bytes JSON]`

Max response size: 10MB.

## File layout

```
~/.memcore/
  memories/          .md files (truth source, flat, no subdirectories)
  index/             vectors.map, vectors.dat
  graph.idx          binary edge cache (rebuildable)
  models/            model.onnx, tokenizer.json, config.json
  memcore.toml       user config
  wal.log            write-ahead log
  .daemon.pid        PID + port + nonce (runtime only)
```

## Invariants

1. **Bidirectional links**: A.links contains B if and only if B.links contains A
2. **Strict references**: links can only point to existing nodes
3. **No self-references**: a node cannot link to itself
4. **Atomic writes**: all `.md` file writes use tempfile + rename
5. **WAL transactions**: create, update, delete, link, unlink, rename are WAL-wrapped
6. **Pre-flight embedding**: embedding computed before touching disk (fail fast)
