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
- After recall, **always `memcore get` the nodes** before using their content — recall only returns names/abstracts, not full content. **Never answer a question using only the abstract** — the abstract is for ranking, the body has the full details.

### `recall` vs `search`

- **`recall`** = vector similarity + graph traversal + weight scoring. Best for general queries where following links to neighbors adds value.
- **`search`** = pure vector similarity, no graph. Best for very specific queries (a person's name, an exact date, an error code) where you want the closest embedding match without graph noise.

When a query is about a specific entity or date, try `search` first. When it's a broader topic, use `recall`.

### Multi-query decomposition

For complex questions that mention multiple entities or time periods, break them into sub-queries:

```
Question: "What did John do after losing his job in January?"
Sub-query 1: memcore recall "John lost job January"        # find the job loss event
Sub-query 2: memcore recall "John new career plans"        # find what came next
```

This gives better coverage than a single long query, because the embedding model compresses long queries into a single vector that may miss key terms.

## Node format

```yaml
---
abstract: Index of the body — key dates, names, facts that tell search what's inside (this gets embedded)
links: [related-node]    # optional, bidirectional
pinned: true              # optional, always in working memory
---
Body in markdown — full details, context, reasoning.
```

**Abstract rules — this is critical for recall quality:**
- The abstract is an **index of the body** — it tells vector search what facts are inside. If a fact isn't in the abstract, recall can't find it.
- Include **dates**, **names** (people, projects, tools), and **key facts** (decisions, outcomes, numbers)
- Think of it as: for every important fact in the body, can a search query find this node through the abstract?
- Good: `"2026-03-15 meeting with Alice — decided to migrate auth to OAuth2, deadline April 1, blocked by legacy session store"`
- Bad: `"Notes from a meeting about auth"` — tells you nothing about what's inside

**Detailed abstract rules:**
1. **Always use absolute dates** — never "last week", "recently", "a few years ago". Convert relative dates using the node's timestamp. Always `"March 15, 2023"` or `"mid-March 2023"`. Relative dates are unresolvable in embeddings.
2. **Keep abstracts under 60 tokens** — forces conciseness. The embedding model has a 512-token limit; long abstracts risk truncation of important facts at the end.
3. **Include concrete entities** — proper names (people, places, projects), specific numbers, dates, and decisions. These are the recall anchors that differentiate one node from another.
4. **Avoid semantic collision** — if many nodes share a theme (e.g., debugging, meetings, deployments), each abstract must lead with what makes *this* node different (the specific event, outcome, entity). Otherwise embeddings collide and recall retrieves the wrong node.

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

Scoring: `score = similarity × weight` — multiplicative, no coefficients. Trust (weight) is the only agent-controlled signal; no time-based decay.

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
