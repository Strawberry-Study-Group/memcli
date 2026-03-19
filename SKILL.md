# MemCore — Persistent Memory for AI Agents

The binary path is printed before this document as `memcore: <path>`. Use that path for all commands below.

Do NOT use built-in memory files (`MEMORY.md`, `~/.claude/projects/*/memory/`).

## How to use memory

1. **Before acting**: `memcore recall "task"` — check if you've seen this before
2. **While acting**: learned something? `memcore create` or `memcore patch --append` immediately
3. **Before creating**: `memcore recall "topic"` first — if a similar node exists, patch or update it instead of creating a duplicate
4. **After acting**: `memcore boost` what helped, `memcore penalize` what misled
5. **Before web search**: `memcore recall` first — skip the web if it's already in memory
6. **After web search**: store findings with `memcore create` so you never search again
7. **Before answering from memory**: `memcore get <name>` to read the full node body — recall only returns names/abstracts, not full content

## Recall strategy — escalate on miss

Recall may not return relevant results on the first try. Use this escalation:

```
Level 1:  memcore recall "your query"                          # default top-k
Level 2:  memcore recall "your query" --top-k 10               # cast wider net
Level 3:  memcore multi-recall "query v1" "rephrased query"    # try different phrasings
Level 4:  memcore multi-recall "keyword" "synonym" --top-k 10  # max coverage
```

- **Always start at Level 1.** Only escalate if results look irrelevant or empty.
- **Rephrase with different vocabulary** — if "auth migration" returns nothing, try "OAuth2 upgrade" or "session store replacement"
- **Use multi-recall** to combine a specific query with a broader one (e.g., `"Alice OAuth2 meeting"` + `"auth system changes"`)
- After recall, **always `memcore get` the nodes** before using their content — the recall summary is for relevance ranking, not for answering questions

## Node format

```yaml
---
abstract: Dense summary with key dates, names, and 3–4 core facts (this gets embedded for search)
links: [related-node]    # optional, bidirectional
pinned: true              # optional, always in working memory
---
Body in markdown — full details, context, reasoning.
```

**Abstract rules — this is critical for recall quality:**
- Include **dates** (when it happened), **names** (people, projects, tools), and **key facts** (decisions, outcomes, numbers)
- The abstract is the ONLY thing the vector index sees. If a fact isn't in the abstract, recall won't find it
- Good: `"2026-03-15 meeting with Alice — decided to migrate auth to OAuth2, deadline April 1, blocked by legacy session store"`
- Bad: `"Notes from a meeting about auth"`
- Think of it as: if someone searches for any key detail, will this abstract match?

Names: letters, digits, spaces, hyphens; 2-128 chars; start with a letter, end with letter/digit. Use descriptive names — they're prefix-searchable.

**Naming rules:**
- Each name must be **unique and descriptive** — capture what makes this memory distinct
- Do NOT use uniform patterns like `session-1`, `note-2024-03-18`, `topic-a` — these are meaningless and unsearchable
- Good: `rust-lifetime-elision`, `user-prefers-terse-output`, `crawler-rate-limit-fix`
- Bad: `memory-001`, `note-3`, `task-20240318`, `info-1`
- The name is the first thing you see in recall results — make it tell you what's inside without reading the node

## Index nodes

When 3+ nodes share a topic, create an index node as their entry point. Name starts with `"index - "`.

```bash
memcore create "index - rust error handling" <<'EOF'
---
abstract: Index of all Rust error handling knowledge
links: [rust-result-patterns, rust-anyhow-tips, rust-custom-errors]
---
## Rust Error Handling

- **rust-result-patterns** — Result/Option combinators and best practices
- **rust-anyhow-tips** — using anyhow for application error handling
- **rust-custom-errors** — defining domain-specific error types with thiserror
EOF
```

Keep indexes updated when you create or delete nodes in their group.

## Commands

### CRUD
```bash
memcore create <name> [-f FILE] <<'EOF'   # create node from stdin or file
memcore get <name> [<name2>...]           # read one or more nodes
memcore update <name> [-f FILE] <<'EOF'   # full replace (preserves created/access_count)
memcore patch <name> --append "text"      # body-only edit (--prepend | --replace "old" "new")
memcore delete <name> [<name2>...]        # irreversible, supports batch
memcore rename <old> <new>                # preserves metadata, updates peer links
```

### Search & recall
```bash
memcore recall "query" [--top-k 5] [--depth 1]   # vector + graph + weight scoring
memcore recall "query" --full                     # same but returns scores + abstracts
memcore multi-recall "q1" "q2" [--top-k 5]       # multi-term recall (wider net)
memcore recall --name "prefix"                    # name prefix search
memcore recall                                    # working memory: pinned + high-weight
memcore search "query" --top-k 5                  # pure vector search (no graph)
memcore multi-search "q1" "q2" [--top-k 5]       # multi-term vector search (wider net)
memcore ls [--sort name|weight|date]
```

Scoring: `score = similarity × weight × vitality` — multiplicative, no coefficients. New nodes start hot (vitality=1.0), decay with age, frequent access slows decay. Floor configurable via `vitality_floor` in memcore.toml.

### Graph & feedback
```bash
memcore link <a> <b>                # bidirectional edge
memcore unlink <a> <b>              # remove edge
memcore neighbors <name> [--depth 1] [--limit 50] [--offset 0]
memcore boost <name>                # increase weight (+0.1, additive)
memcore penalize <name>             # decrease weight (×0.8, multiplicative)
memcore pin/unpin <name>            # toggle working memory inclusion
```

### Maintenance
```bash
memcore inspect [<name>] [-t 0.85] [--format human|json] [--cap 50]
memcore status
memcore gc                          # clean dangling references
memcore reindex                     # rebuild vector index (after model swap)
memcore baseline                    # compute similarity percentile distribution
memcore init [--dir PATH]           # initialize memcore directory
memcore stop                        # stop daemon
```

## Key behaviors

- **Bidirectional links**: `link a b` updates both nodes
- **Strict references**: links only point to existing nodes
- **Weight**: 0.0–1.0, default 1.0, shaped by boost/penalize
- **Auto daemon**: first call starts daemon; exits after 30 min idle
- **Directory resolution**: `MEMCORE_DIR` env var → `<DIRNAME>_DIR` env var (derived from binary's parent dir name, e.g. `work_memcore/` → `WORK_MEMCORE_DIR`) → binary's own parent dir → error
- **Exit codes**: 0 success, 1 user error, 2 system error, 3 connection error
