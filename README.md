# MemCore

**Persistent memory CLI tool for any AI agent. ~40 MB. No cloud. No dependencies. Just run it.**

MemCore is a local CLI + daemon that gives AI agents persistent, searchable, graph-linked memory across sessions. Any agent that can run shell commands — Claude, GPT, Gemini, Cursor, your own scripts — gets a brain that remembers everything.


## Get started in 60 seconds
[**Download the release**](../../releases) — unzip,

place in you project folder.

then  just start your AI agent and tell you agent:
"can you read @default_memcore/skill.md can set up memcore for me?"

---

## Why MemCore?


| | |
|---|---|
| **~40 MB total** | Rust implemented, Binary + embedded model, statically linked. No Python, no Docker, no external services. |
| **Fully local** | Your agent's memories never leave your machine. No API keys, no usage limits, no latency. |
| **Any agent** | Works with Claude Code, Cursor, GPT wrappers, custom scripts — anything that can call a CLI. |
| **Semantic search** | Ask questions in natural language. Get the right memories back, not just keyword matches. |
| **Knowledge graph** | Link related memories. Recall pulls in neighbors automatically via graph traversal. |
| **Just markdown files** | Every memory is a `.md` file. Read, edit, back up, or git-commit your memories directly. |
| **Crash-safe** | Write-ahead log + atomic file writes. Power-loss safe. |

---


## How it works

```
memcore <command>     →  thin CLI client (parse args → TCP → daemon → print result)
memcore --daemon      →  background daemon (graph + vector index + name index in RAM)
```

The CLI auto-starts the daemon on first call. The daemon holds everything in memory for speed, persists to disk atomically, and auto-exits after 30 min idle. Every memory is a `.md` file on disk — the index is just a cache and can always be rebuilt.

---

## Recall: smarter than search

`memcore recall` is not just vector search. It combines three signals:

```
score = 0.6 × similarity + 0.2 × weight + 0.2 × graph_proximity
```

- **similarity** — how semantically close your query is to the memory's abstract (via multilingual embedding)
- **weight** — how useful this memory has been (shaped over time by `boost` / `penalize`)
- **graph_proximity** — how close this node is to your top matches via linked neighbors

Vector search finds seeds. BFS traversal through the knowledge graph pulls in related nodes. Weighted scoring surfaces what's actually important. The result is a ranked list of exactly what your agent needs.

---

## Integrating with your agent

### Claude Code (recommended)

Add hooks to auto-inject memories at session start and after context compaction:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "startup",
        "hooks": [{ "type": "command", "command": "cat \"$MEMCORE_DIR/skill.md\" && memcore recall --top-k 7" }]
      },
      {
        "matcher": "compact",
        "hooks": [{ "type": "command", "command": "cat \"$MEMCORE_DIR/skill.md\" && memcore recall --top-k 7" }]
      }
    ]
  }
}
```

Add to `~/.claude/settings.json` (all projects) or `.claude/settings.json` (one project).

- **startup** — loads the skill doc so the agent knows how to use memcore, then injects the top 7 relevant memories
- **compact** — re-injects both when the context window compresses, so the agent never loses memory or skill knowledge mid-session

### Any other agent

MemCore is a plain CLI — any agent that can run shell commands can use it:

```bash
# Before starting work: load relevant context
memcore recall "topic of current task" --top-k 5

# Learned something useful: store it immediately
memcore create "node name" -f memory.md

# That memory helped: reinforce it
memcore boost "node name"

# That memory misled you: decay it
memcore penalize "node name"

# At session end: check working memory
memcore recall
```

The `skill.md` file included in the release is a ready-made prompt document you can inject into any agent's system prompt to teach it how to use MemCore.

---

## The memory workflow

| When | What to do |
|------|-----------|
| Before starting a task | `recall "task topic"` — have you seen this before? |
| Before creating a node | `recall "topic"` — does a similar node already exist? Patch it instead. |
| While working | `create` or `patch --append` as soon as you learn something |
| When memory helped | `boost` the node immediately |
| When memory misled | `penalize` the node immediately |
| Before searching the web | `recall` first — skip the search if it's already in memory |
| After searching the web | `create` to store findings so you never search again |
| Periodically | `inspect` to find duplicates, orphans, and low-value nodes to prune |

---

## Commands

| Category | Commands |
|----------|----------|
| **Node CRUD** | `create` `get` `update` `patch` `delete` `rename` |
| **Search** | `search` `multi-search` `recall` `multi-recall` |
| **Browse** | `ls` `inspect` `neighbors` |
| **Graph** | `link` `unlink` |
| **Feedback** | `boost` `penalize` `pin` `unpin` |
| **Maintenance** | `init` `status` `reindex` `gc` `baseline` `stop` |

<details>
<summary>Full command reference</summary>

```bash
# Setup
memcore init [--dir /path]             # initialize directory + config

# CRUD
memcore create <name> [-f file]        # create node from stdin or file
memcore get <name> [<name2>...]        # read one or more nodes
memcore update <name> [-f file]        # full replace (preserves created/access_count)
memcore patch <name> --append "text"   # body-only edit (--prepend | --replace old new)
memcore delete <name>                  # irreversible
memcore rename <old> <new>             # preserves all metadata, updates peer links

# Search & recall
memcore search "query" [--top-k 5]                     # pure vector search
memcore multi-search "q1" "q2" [--top-k 5]             # multi-term, results per query merged
memcore recall ["query"] [--top-k 5] [--depth 1]       # vector + graph + weight scoring
memcore multi-recall "q1" "q2" [--top-k 5] [--depth 1] # multi-term recall
memcore recall --name "prefix"                          # name prefix search
memcore recall                                          # working memory: pinned + high-weight
memcore ls [--sort name|weight|date]

# Graph
memcore link <a> <b>                   # create bidirectional edge
memcore unlink <a> <b>                 # remove edge
memcore neighbors <name> [--depth 1] [--limit 50]

# Feedback & importance
memcore boost <name>                   # weight += 0.1 (capped at 1.0)
memcore penalize <name>                # weight *= 0.8 (geometric decay)
memcore pin <name>                     # always included in working memory recall
memcore unpin <name>

# Maintenance
memcore inspect [<name>] [-t 0.85] [--format human|json] [--cap 50]
memcore status                         # pid, port, node/edge counts, uptime
memcore reindex                        # rebuild all embeddings from .md files
memcore gc                             # clean dangling link references
memcore baseline                       # compute similarity distribution stats
memcore stop                           # gracefully stop the daemon
```
</details>

---

## Node format

Every memory is a plain markdown file in `~/.memcore/memories/`. You can read and edit them directly.

**Minimal input** — the system fills in everything else:

```yaml
---
abstract: One-sentence description of this memory (this is what gets embedded for search)
links: [related-node]   # optional — creates bidirectional graph edges
pinned: true            # optional — always appears in working memory recall
---

Body content in full markdown.
Write as much as you need here.
```

**Node naming:** letters, digits, spaces, hyphens; 2–128 chars; must start with a letter. Use descriptive names — they're prefix-searchable. Examples: `How to deploy the app`, `rust error handling patterns`, `Project Alpha deployment notes`.

**System-managed fields** (auto-set, preserved across updates):

| Field | Notes |
|-------|-------|
| `created` | Immutable after creation |
| `updated` | Set on content changes |
| `last_accessed` | Updated on every get/recall/search |
| `access_count` | Incremented on each access |
| `weight` | 0.0–1.0, default 1.0 — shaped by boost/penalize over time |

---

## Health inspection

Run `memcore inspect` periodically to keep your knowledge graph clean:

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
  deployment-guide  <->  how to deploy     0.95
  rust-errors       <->  error-handling    0.88

Orphans: old-scratch-notes
Low weight: outdated-approach
```

Near-duplicates are worth merging. Orphans and low-weight nodes are candidates for deletion. `memcore gc` cleans up any dangling links automatically.

---

## Configuration

```toml
# ~/.memcore/memcore.toml

[weight]
boost_amount = 0.1        # additive boost per positive feedback
penalty_factor = 0.8      # multiplicative decay per negative feedback
warn_threshold = 0.1      # nodes below this flagged in inspect

[recall]
alpha = 0.6               # weight for vector similarity in scoring
beta = 0.2                # weight for node importance (weight field)
gamma = 0.2               # weight for graph proximity
default_depth = 1         # BFS expansion depth from seed nodes

[daemon]
idle_timeout_minutes = 30 # auto-exit after idle
port = 0                  # 0 = auto-assign
```

Override base directory via env var: `MEMCORE_DIR=/custom/path memcore <cmd>`

---

## Building from source

```bash
# Without embedding (graph + name search only, no model needed)
cargo build --release

# With semantic search (statically links the ONNX runtime)
cargo build --release --features embedding
```

The release binary at `target/release/memcore` is statically linked with no runtime dependencies.

---

## License

MIT
