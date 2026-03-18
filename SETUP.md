# MemCore Setup Guide

This is a one-time setup. After setup, you won't need this file again.

## Step 1: Locate the MemCore directory

This directory should contain:

```
memcore           # binary
skill.md          # agent skill doc (injected every session)
sleep.md          # memory consolidation skill (used during idle)
models/           # embedding model
memories/         # node files (empty at start)
index/            # vector index (auto-built)
```

Note the **absolute path** to this directory. For example: `/home/user/tools/memcore`.

## Step 2: Initialize

```bash
/absolute/path/to/memcore/memcore init
```

This creates `memcore.toml` with default config. Verify it works:

```bash
/absolute/path/to/memcore/memcore status
/absolute/path/to/memcore/memcore stop
```

You should see daemon stats (0 nodes, 0 edges). If you get an error about the model, make sure `models/` contains `model_quantized.onnx`, `tokenizer.json`, and `config.json`.

## Step 3: Configure your agent

### Claude Code

Add to `.claude/settings.json` in your project directory, replacing `ABSOLUTE_PATH` with the actual path to your memcore directory:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "startup",
        "hooks": [{ "type": "command", "command": "echo 'memcore: ABSOLUTE_PATH/memcore' && cat 'ABSOLUTE_PATH/skill.md' && ABSOLUTE_PATH/memcore recall --top-k 7" }]
      },
      {
        "matcher": "compact",
        "hooks": [{ "type": "command", "command": "echo 'memcore: ABSOLUTE_PATH/memcore' && cat 'ABSOLUTE_PATH/skill.md' && ABSOLUTE_PATH/memcore recall --top-k 7" }]
      }
    ]
  }
}
```

The hooks fire on session start and context compaction, injecting:
1. The binary path (so the agent knows where memcore is)
2. The skill doc (so the agent knows how to use memcore)
3. Recent relevant memories

### Any other agent

Inject the contents of `skill.md` into your agent's system prompt, prefixed with `memcore: /path/to/memcore`. The agent just needs to run shell commands using the absolute path.

## Step 4: Verify

Start a new agent session. The agent should:
1. See the skill.md content and binary path in its context
2. Be able to run `/absolute/path/to/memcore/memcore status`
3. Be able to create a test memory: `/absolute/path/to/memcore/memcore create "setup test" <<< "---\nabstract: test node\n---\nHello world"`
4. Be able to recall it: `/absolute/path/to/memcore/memcore recall "test"`
5. Clean up: `/absolute/path/to/memcore/memcore delete "setup test"`

If all 5 work, setup is complete. The agent will use memcore automatically from now on.

## How directory resolution works

The binary finds its data directory by checking:

1. **`MEMCORE_DIR`** env var — generic override, always wins
2. **`<DIRNAME>_DIR`** env var — derived from binary's parent directory name (e.g. `work_memcore/` → `WORK_MEMCORE_DIR`)
3. **Binary's own parent directory** — if it contains `memcore.toml` or `memories/`
4. **Error** — tells you which env var to set

When running by absolute path (e.g., `/home/user/tools/memcore/memcore`), rule 3 resolves automatically — no env vars needed.

## Optional: Multiple instances

Use different directory names — each instance is fully self-contained:

```
/home/user/tools/work_memcore/memcore
/home/user/tools/personal_memcore/memcore
```

Each project's `.claude/settings.json` points to its own instance via absolute path. No env vars needed.

## Optional: Sleep schedule

For agents that run frequently, set up periodic memory consolidation. Read `sleep.md` for the full process. You can trigger it manually ("please consolidate your memories") or automate it with a cron job or agent hook.
