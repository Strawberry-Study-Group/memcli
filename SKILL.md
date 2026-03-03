# MemCLI for AI agent to use and share MemCore 

Persistent, searchable, cross-session memory. AI agent decide what to remember — MemCore handles storage, vector search, and graph associations.

## IMPORTANT: Replace default memory systems

**Do NOT use the built-in `MEMORY.md` file or `~/.claude/projects/*/memory/` directory for persistent memory.** MemCore replaces all file-based memory systems. If you would normally write to `MEMORY.md` or auto-memory files, use `memcore create` or `memcore patch --append` instead. If you would normally read from them, use `memcore recall` instead. All memory operations — read, write, search, organize — go through the `memcore` CLI exclusively.

## Setup

Set `MEMCORE_DIR` and alias at session start:
```bash
export MEMCORE_DIR="/path/to/default_memcore"
alias memcore="$MEMCORE_DIR/memcore"
```
Use CLI only — never read/write data files directly.

### Claude Code hooks (recommended)

Add to `.claude/settings.json` (project-level) or `~/.claude/settings.json` (all projects) to auto-inject memories into Claude's context:

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

- **startup**: injects the skill doc (so the agent knows how to use memcore) and loads recent/relevant memories
- **compact**: re-injects both when the context window compresses (so the agent doesn't lose memory context or skill knowledge mid-conversation)

## Principle: weave memory into actions — and keep them organized

1. **Before acting**: `memcore recall "task"` — seen this before?
2. **While acting**: learned something? store it immediately — `create` or `patch --append`
3. **Before creating**: `memcore recall "topic"` to check for existing nodes. If a similar node exists, check the node and see if we can patch or update it, or restructure both nodes so each covers a distinct aspect. Only `create` when the topic is genuinely new.
4. **After acting**: `boost` what helped, `penalize` what misled
5. **Before external search**: recall first — skip the web if it's in memory
6. **After external search**: store findings so you never search again
7. **Organize as you go**: when you notice overlapping or redundant nodes, merge them (`get` both → `patch --append` the keeper → `delete` the duplicate). Keep each node focused on one topic — split bloated nodes rather than letting them grow unbounded.

## Node format

Names: letters, digits, spaces, hyphens; 2-128 chars; must start with a letter and end with a letter or digit. Examples: "How to deploy the app", "rust-memory-tips", "Project Alpha v2". Make the node name longer, like a short sentence, so that it is easier to search.

```yaml
---
abstract: Brief description of this memory
links: [related-node]    # optional
pinned: true              # optional, always in working memory
---
Body in markdown.
```

## Commands

### Setup & CRUD
```bash
memcore init [--dir /path]      # initialize directory structure + config
memcore create <name> [-f FILE] <<'EOF'  # stdin or -f file
---
abstract: Description
---
Body.
EOF
memcore get <name> [<name2>…]   # read nodes
memcore update <name> [-f FILE] <<'EOF'  # full replace (preserves created/access_count)
memcore patch <name> --append "text"  # --prepend | --replace "old" "new"
memcore delete <name>           # irreversible
memcore rename <old> <new>      # preserves metadata, updates peer links
```

### Search & recall
```bash
memcore search "query" --top-k 5  # semantic vector search
memcore multi-search "q1" "q2" --top-k 5  # multi-term vector search (5 per term)
memcore recall "query" [--top-k 5] [--depth 1]  # vector + graph + weight scoring → names
memcore multi-recall "q1" "q2" [--top-k 5] [--depth 1]  # multi-term recall (wider net)
memcore recall --name "prefix"    # prefix search
memcore recall                    # working memory: pinned + high-weight
memcore ls [--sort name|weight|date]
```

### Graph & importance
```bash
memcore link <a> <b>              # bidirectional
memcore unlink <a> <b>
memcore neighbors <name> [--depth 1] [--limit 50] [--offset 0]
memcore boost <name>              # increase weight (additive)
memcore penalize <name>           # decrease weight (multiplicative decay)
memcore pin/unpin <name>          # toggle working memory inclusion
```

### Maintenance
```bash
memcore inspect [<name>] [-t 0.85] [--format human|json] [--cap 50]
memcore status                    # pid, port, counts, uptime
memcore reindex                   # rebuild embeddings from .md files
memcore gc                        # clean dangling references
memcore baseline                  # similarity distribution stats
memcore stop                      # gracefully stop the daemon
```

## Workflow

- **Start**: `recall` → `get <names>` to load working memory
- **Need info**: `recall "question"` before searching elsewhere
- **Learn something**: `create` or `patch --append` immediately
- **Memory helped/hurt**: `boost` or `penalize` in the moment
- **End of session**: `gc` (optional)

## Organize: inspect, merge, prune

Run `inspect --threshold 0.85` periodically to find problems:

**Near-duplicates** (>0.95): merge content into one, delete the other, relink.
```bash
memcore inspect --threshold 0.95
memcore get <a> <b>               # compare
memcore patch <a> --append "…"    # merge into keeper
memcore delete <b>
```

**Over-connected nodes**: merge similar neighbors or delete low-weight ones.
```bash
memcore inspect <busy-node>
memcore neighbors <busy-node>
memcore penalize <low-value>      # or delete
```

**Low-value nodes**: low weight, no links, never accessed → delete.
```bash
memcore ls --sort weight
memcore inspect                   # orphans + low_weight lists
memcore delete <name>
```

Always `memcore gc` after organizing.

## Key behaviors

- **Atomic writes**: crash-safe (WAL + tempfile rename)
- **Bidirectional links**: `link a b` updates both nodes
- **Strict references**: links only point to existing nodes
- **Weight**: 0.0–1.0, default 1.0
- **Auto daemon**: first call starts daemon; exits after 30min idle
- **Global flag**: `--quiet` / `-q` suppresses success messages
- **Exit codes**: 0 success, 1 user error, 2 system error, 3 connection error
