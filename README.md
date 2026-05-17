# MemClI for shareable MemCore

**Persistent memory CLI tool for any AI agent. ~40 MB. 100% local. No dependencies. Just run it and use copy paste to share.**

MemCore is a local CLI + daemon that gives AI agents persistent, searchable, graph-linked memory across sessions. Any agent that can run shell commands — Claude, GPT, Gemini, Cursor, your own scripts — gets a brain that remembers everything.


## Get started in 60 seconds

[**Download the latest release**](https://github.com/Strawberry-Study-Group/memcli/releases/latest), unzip it, place in your project folder.

Then just tell your AI agent:

> "Can you read `@<your-memcore-dir>/setup.md` and set up memcore for me?"

The agent reads the setup guide, configures hooks, and verifies everything works. After that, it uses memcore automatically every session.

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

`memcore recall` is not just vector search. It combines two signals via multiplicative scoring:

```
score = similarity × weight
```

- **similarity** — how semantically close your query is to the memory's abstract (via multilingual embedding)
- **weight** — trust signal shaped by agent feedback (`boost` / `penalize`)

Vector search finds seed nodes. BFS traversal through the knowledge graph pulls in related neighbors and computes their real similarity. Multiplicative scoring means a memory must be both relevant and trusted to rank high. No time-based decay — digital memory doesn't degrade. The agent controls trust explicitly via boost/penalize.

---

## Benchmarks

MemCore was evaluated on [**LoCoMo**](https://snap-research.github.io/locomo/) (Long-term Conversational Memory — Snap Research, ACL 2024): 1,540 QA pairs across 10 multi-session conversations.

> **90.7% LLM-judge accuracy** with a **98.4% evidence retrieval rate** — using retrieval-based access (vector + graph recall), not full context.

| System | Year | LoCoMo accuracy |
|---|---|---|
| EverMemOS | 2026 | 92.3% |
| MemMachine v0.2 | 2025 | 91.2% |
| **MemCore** | **2026** | **90.7%** |
| Backboard | 2025 | 90.0% |
| Zep | 2024 | 75.1% |
| Letta / MemGPT | 2025 | 74.0% |
| Mem0 Graph | 2024 | 68.5% |
| OpenAI Memory | 2024 | 52.9% |

LLM-as-a-Judge accuracy on categories 1–4, the standard LoCoMo evaluation protocol. MemCore ranks among the top memory systems on the leaderboard while running 100% locally on a ~40 MB binary. See [`EXPERIMENT_REPORT.md`](EXPERIMENT_REPORT.md) for the full methodology, per-category breakdown, and sleep-consolidation analysis.

---

## Roadmap

- [ ] Build and test on macOS and Windows
- [x] [OpenClaw plugin](openclaw-plugin/) — replaces built-in memory with MemCore's knowledge graph
- [ ] Memory forgetting skills— decay and intelligent pruning 
- [ ] Benchmarking suite and recall quality improvements
- [ ] Test Multi-agent support — shared memory cores across concurrent agents
- [ ] GUI for inspecting and editing your memory graph visually
- [ ] ...and a lot more!

Contributions welcome. Open an issue or PR.

---

## Skill files

The release includes three skill files. Each serves a different purpose:

| File | Purpose | When used |
|------|---------|-----------|
| `setup.md` | One-time setup guide — env var, hooks, verification | Agent reads this once during initial setup |
| `skill.md` | Runtime skill — how to use memcore while working | Injected automatically every session via hook |
| `sleep.md` | Memory consolidation — organizing and pruning | Invoked manually during idle time ("consolidate your memories") |

Only `skill.md` goes in the hook. It's kept lean so it doesn't waste tokens. Setup instructions and sleep procedures are separate files the agent reads only when needed.

### Claude Code

After running setup, hooks auto-inject `skill.md` + recent memories on every session start and context compaction. See `setup.md` for the exact hook config.

### OpenClaw

MemCore has an [OpenClaw plugin](openclaw-plugin/) that **replaces OpenClaw's built-in memory** with MemCore's knowledge graph. Install the plugin, set `plugins.slots.memory = "memory-memcore"` in your OpenClaw config, and you get graph-linked recall, feedback weighting, and health inspection instead of flat file search. See [`openclaw-plugin/README.md`](openclaw-plugin/README.md) for full setup.

### Any other agent

Inject the contents of `skill.md` into your agent's system prompt and set the appropriate env var (e.g. `WORK_MEMCORE_DIR` for a directory named `work_memcore`). Any agent that can run shell commands can use memcore. See `setup.md` for the naming convention.

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

Every memory is a plain markdown file in `memories/` inside your memcore directory. You can read and edit them directly.

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
# memcore.toml (in your memcore directory)

[weight]
boost_amount = 0.1        # additive boost per positive feedback
penalty_factor = 0.8      # multiplicative decay per negative feedback
warn_threshold = 0.1      # nodes below this flagged in inspect

[recall]
default_depth = 1         # BFS expansion depth from seed nodes

[daemon]
idle_timeout_minutes = 30 # auto-exit after idle
port = 0                  # 0 = auto-assign
```

Set the base directory via env var derived from your directory name (e.g. `export WORK_MEMCORE_DIR="/path/to/work_memcore"`). `MEMCORE_DIR` also works as a generic override.

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
