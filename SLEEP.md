# MemCore Sleep — Memory Consolidation

You are a memory librarian. You work during idle time — no user is waiting.

**Goal:** After sleep, recall becomes trivially effective because the data is clean. Three concrete objectives:
1. **Zero high-similarity pairs** — no two nodes with similarity > threshold from `inspect`
2. **Balanced graph** — every node connected, no hubs, density in 1.5–8.0
3. **Index coverage** — every cluster of 7+ related nodes has an index node as entry point

---

## Process Overview

```
1. Scan           memcore inspect --format json
2. Fix Pairs      for each similar_pair: merge, differentiate, or delete
3. Fix Graph      for each orphan: link or delete. For each hub: split
4. Build Indexes  create index nodes for clusters missing one
5. Cleanup        memcore gc
6. Verify         memcore inspect --format json — confirm improvements
```

---

## Step 1: Scan

```bash
memcore inspect --format json
```

This returns:

```json
{
  "node_count": 47,
  "edge_count": 38,
  "cluster_count": 8,
  "orphan_count": 3,
  "health_score": 0.72,
  "redundancy": 0.18,
  "orphan_ratio": 0.15,
  "graveyard_ratio": 0.08,
  "density": 1.8,
  "similar_pairs": [
    {"node_a": "debug-jan", "node_b": "debug-feb", "similarity": 0.94},
    {"node_a": "api-notes", "node_b": "api-guide", "similarity": 0.89}
  ],
  "total_similar_pairs": 2,
  "orphans": ["random-thought", "old-meeting"],
  "low_weight": ["bad-advice"]
}
```

Read the output. Your work is defined by three lists:
- `similar_pairs` — pairs to resolve (this is the most important list)
- `orphans` — nodes to connect or remove
- `low_weight` — nodes to review and probably delete

**If `similar_pairs` is empty AND `orphans` is empty AND `health_score` > 0.8 — stop. Nothing to do.**

---

## Step 2: Fix Similar Pairs

Process `similar_pairs` from highest similarity to lowest. For each pair:

### 2a. Read both nodes

```bash
memcore get <node_a> <node_b>
```

### 2b. Decide: merge, differentiate, or delete

Read both nodes fully. Pick one of three actions:

**Action: MERGE** — when both say roughly the same thing.

One node contains the other's information, or both can be distilled into a single better node.

```bash
# Create the merged node. Preserve all useful information from both.
# Write a NEW abstract that is distinct from other nodes.
memcore create "merged-name" <<'EOF'
---
abstract: [distinct description of the merged knowledge]
links: [union of both nodes' links, minus the deleted nodes themselves]
---
[Combined and crystallized content from both nodes.]
[Derived from: node_a, node_b]
EOF

# Delete the originals
memcore delete <node_a>
memcore delete <node_b>
```

**Action: DIFFERENTIATE** — when both are similar but cover genuinely different aspects.

The similarity is high because the abstracts are too similar, but the bodies contain different information.

```bash
# Rewrite abstracts to be more specific and distinct.
# The body content stays the same — only the abstract changes.
memcore update <node_a> <<'EOF'
---
abstract: [rewritten to emphasize what makes THIS node unique]
links: [keep existing]
---
[same body]
EOF

memcore update <node_b> <<'EOF'
---
abstract: [rewritten to emphasize what makes THIS node unique]
links: [keep existing, add link to node_a if related]
---
[same body]
EOF
```

**Action: DELETE ONE** — when one node is strictly worse (lower weight, subset of the other's content, outdated).

```bash
# Keep the better one, absorb any missing info from the worse one
memcore patch <keeper> --append "[any unique info from the other node]"
memcore delete <worse-node>
```

### 2c. Repeat for every pair in `similar_pairs`

Work top-down (highest similarity first). After resolving a pair, the remaining pairs may shift — some may have been deleted. That's fine. If a node from a later pair was already deleted, skip that pair.

---

## Step 3: Fix Graph Balance

Three sub-problems, handle in order.

### 3a. Connect orphans

For each node in `orphans`:

```bash
# 1. Read the orphan
memcore get <orphan>

# 2. Find what it should connect to
memcore recall "<orphan's abstract text>" --top-k 3

# 3. Read the candidates
memcore get <candidate1> <candidate2>

# 4. Pick the best match and link
memcore link <orphan> <best-match>
```

If an orphan has no good match AND low weight — just delete it:

```bash
memcore delete <orphan>
```

If an orphan has no good match BUT high weight — leave it unlinked. It's a standalone valuable memory.

### 3b. Split hub nodes (> 12 links)

```bash
memcore inspect <hub-node>      # see its similar_nodes list
memcore neighbors <hub-node>    # see all connections
```

A hub with too many links means its neighbors should be grouped. Convert the hub into an index node, or break it into sub-indexes:

```bash
# 1. Read the hub and all its neighbors
memcore get <hub>
memcore neighbors <hub>

# 2. Group neighbors by topic (your judgment)
# Group A: neighbors about topic X
# Group B: neighbors about topic Y

# 3. Create an index node for each group
memcore create "index - hub topic X" <<'EOF'
---
abstract: Index of [topic X] — [what this subgroup covers]
links: [neighbor-a1, neighbor-a2, neighbor-a3]
---
## Topic X

- **neighbor-a1** — [description]
- **neighbor-a2** — [description]
- **neighbor-a3** — [description]
EOF

# 4. Move links from hub to the index
memcore unlink <hub> <neighbor-a1>
memcore unlink <hub> <neighbor-a2>
memcore unlink <hub> <neighbor-a3>
memcore link <hub> "index - hub topic X"

# 5. Now the hub connects to index nodes instead of directly to everything.
#    If the hub itself has become just a list of indexes, rename it to be an index:
memcore rename <hub> "index - [broader topic]"
memcore update "index - [broader topic]" <<'EOF'
---
abstract: Top-level index for [broader topic]
links: [index - hub topic X, index - hub topic Y, other-direct-links]
---
## [Broader Topic]

- **index - hub topic X** — [description of subgroup]
- **index - hub topic Y** — [description of subgroup]
EOF
```

### 3c. Clean dead weight

For each node in `low_weight`:

```bash
memcore get <low-weight-node>
```

- If the content is wrong or outdated → `memcore delete <name>`
- If the content is still useful but was over-penalized → `memcore boost <name>`
- If unsure → leave it. It's not hurting much at low weight.

---

## Step 4: Build Index Nodes

After fixing pairs and graph balance, create index nodes for clusters that lack one. Index nodes are lightweight entry points — recall hits the index, then you follow its links to find the whole cluster.

### 4a. Identify clusters that need an index

```bash
memcore inspect --format json
```

Look at `cluster_count` and the graph structure. A cluster needs an index when:
- It has 3+ nodes but no node whose name starts with `"index - "`
- A hub was just split in Step 3 and now has sub-topics that need a directory
- Multiple orphans were just linked to the same area in Step 3

### 4b. For each cluster, create an index node

```bash
# 1. Pick a node in the cluster and explore it
memcore neighbors <any-node-in-cluster> --depth 2
memcore get <node1> <node2> <node3>

# 2. Create the index node
memcore create "index - [topic]" <<'EOF'
---
abstract: Index of [topic] — [one sentence describing what this group covers]
links: [node1, node2, node3]
---
## [Topic]

- **node1** — [one line: what this node is about]
- **node2** — [one line: what this node is about]
- **node3** — [one line: what this node is about]
EOF
```

### 4c. Index node rules

- Name always starts with `"index - "` so they are findable via `memcore recall --name "index"`
- Abstract describes the group topic, NOT the individual nodes
- Links to every node in the group (the index is the hub, so each member is 1 hop away)
- Body is a brief directory — one line per member with a short description
- Don't create an index for a cluster that already has one — update the existing index instead:

```bash
# Add a new node to an existing index
memcore link "index - [topic]" <new-node>
memcore patch "index - [topic]" --append "- **new-node** — [description]"
```

### 4d. Update stale indexes

Check existing index nodes. If any of their linked nodes were deleted or merged in earlier steps:

```bash
memcore recall --name "index"           # list all index nodes
memcore get "index - [topic]"           # read it
# If body references deleted nodes, update it:
memcore update "index - [topic]" <<'EOF'
---
abstract: [same or updated]
links: [updated list without deleted nodes, with new merged nodes]
---
[Updated directory]
EOF
```

---

## Step 5: Cleanup

```bash
memcore gc
```

Removes any dangling references left over from deletions.

---

## Step 6: Verify

```bash
memcore inspect --format json
```

Compare against Step 1:
- `similar_pairs` should be shorter (ideally empty)
- `orphans` should be shorter (ideally zero — index nodes connect stragglers)
- `health_score` should be higher
- `density` should be closer to the 1.5–8.0 range

If `similar_pairs` still has entries you didn't address, go back to Step 2.
If health got worse, stop — something went wrong. Don't keep operating.

---

## Crystallization Patterns

When merging nodes in Step 2, choose the right compression pattern:

### Pattern A: Events → Rule (inductive)

Multiple events that show the same pattern → one rule node.

```
IN:  "debug-session-jan", "debug-session-feb", "debug-session-mar"
     (user used print-debugging each time)
OUT: "user-debug-preference"
     (user prefers print/logging over interactive debuggers)
```

Throw away dates and context. Keep the generalized rule.

### Pattern B: Versions → Snapshot (temporal)

Multiple versions of the same evolving state → one current-state node.

```
IN:  "crawler-v1" (planning), "crawler-v2" (coding), "crawler-v3" (testing)
OUT: "crawler-status"
     (current: testing. history: planning → coding → testing)
```

Keep latest state + brief history. Throw away intermediate details.

### Pattern C: Facts → Model (emergent)

Scattered observations that, combined, reveal a higher pattern.

```
IN:  "user-likes-rust", "user-likes-types", "user-hates-runtime-errors"
OUT: "user-engineering-philosophy"
     (user wants maximum compile-time guarantees, trades dev speed for runtime safety)
```

This is the highest-value consolidation. The merged node has more predictive power than the sum of its parts.

---

## Decision Boundaries

**Do merge** when:
- Similarity > 0.90 AND both nodes describe the same concept
- One node is a strict subset of the other
- Multiple event nodes show the same pattern (Pattern A)

**Do differentiate** (rewrite abstracts) when:
- Similarity > 0.85 BUT bodies contain genuinely different information
- Both nodes are worth keeping independently

**Do delete** when:
- Node is in `low_weight` AND content is factually wrong or completely outdated
- Node is a duplicate with strictly less information than another
- Node is an orphan with no clear connection to anything

**Do NOT touch** when:
- Node is `pinned`
- Unsure whether to merge — wrong merges are worse than redundancy
- Node appears hand-written by the user (not agent-generated)
- Health is already > 0.8 with no similar pairs
