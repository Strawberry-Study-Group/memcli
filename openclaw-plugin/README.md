# MemCore Plugin for OpenClaw

OpenClaw memory plugin that replaces the built-in memory with MemCore's knowledge graph + semantic search.

## What you get

| OpenClaw built-in | MemCore replacement |
|---|---|
| `memory_search` (vector + BM25) | `memory_recall` (vector + graph + weight scoring) |
| `memory_get` (file read) | `memory_get` (node read) |
| — | `memory_store` (create/update with auto-dedup) |
| — | `memory_forget` (delete) |
| — | `memory_boost` / `memory_penalize` (feedback loop) |
| — | `memory_link` (knowledge graph edges) |
| — | `memory_inspect` (health check) |

## Setup

### 1. Install memcore

Download the [latest release](https://github.com/Strawberry-Study-Group/memcli/releases/latest), extract it, and place it somewhere permanent:

```bash
tar xzf memcore-v*-linux-x86_64.tar.gz -C ~/tools/
~/tools/memcore/memcore init
~/tools/memcore/memcore status   # verify it works
~/tools/memcore/memcore stop
```

### 2. Install the plugin

```bash
# From npm (once published)
openclaw plugins install @memcore/memory-memcore

# Or from local directory (for development)
openclaw plugins install -l ./path/to/openclaw-plugin
```

### 3. Configure OpenClaw

Add to your OpenClaw config:

```json5
{
  plugins: {
    entries: {
      "memory-memcore": {
        enabled: true,
        config: {
          binaryPath: "/home/user/tools/memcore/memcore",
          memcoreDir: "/home/user/tools/memcore"
        }
      }
    },
    slots: {
      // This replaces the built-in memory
      memory: "memory-memcore"
    }
  }
}
```

### 4. Verify

Start a new OpenClaw session and test:

```
You: recall what you know about me
Agent: [uses memory_recall tool — should see "No memories found." on first run]

You: remember that I prefer concise responses
Agent: [uses memory_store tool — creates a new node]

You: what do you know about me?
Agent: [uses memory_recall — finds the node just created]
```

## Configuration

| Option | Default | Description |
|--------|---------|-------------|
| `binaryPath` | `"memcore"` | Path to the memcore binary. If omitted, looks for `memcore` on PATH. |
| `memcoreDir` | *(binary's parent dir)* | Path to the memcore data directory. |
| `timeoutMs` | `10000` | Timeout for memcore CLI commands in milliseconds. |

## Testing

```bash
cd openclaw-plugin

# Install dev deps
npm install

# Run unit tests (mocked, no memcore binary needed)
npx vitest run

# Run live integration tests (requires real memcore)
MEMCORE_LIVE_TEST=1 \
MEMCORE_BIN=/path/to/memcore \
MEMCORE_TEST_DIR=/path/to/memcore/dir \
npx vitest run
```

## Releasing

The OpenClaw plugin has its own release cycle, separate from the memcore binary.

### Automated (CI)

Push a tag prefixed with `openclaw-v`:

```bash
git tag openclaw-v0.0.3
git push origin openclaw-v0.0.3
```

This triggers `.github/workflows/release-openclaw.yml` which runs tests, publishes to npm, and creates a GitHub release. Requires `NPM_TOKEN` secret.

### Manual

```bash
cd openclaw-plugin
npm version patch    # or minor/major
npm publish --access public
```

### memcore binary releases

The memcore binary (for Claude Code and standalone use) is released separately via `v*` tags:

```bash
git tag v0.0.3
git push origin v0.0.3
```

This triggers `.github/workflows/release.yml` which cross-compiles for Linux/macOS/Windows and uploads tarballs to GitHub Releases.

## How it works

The plugin registers 8 tools that wrap the memcore CLI binary via `child_process.execFile`. Each tool call spawns `memcore <command> [args]` with `MEMCORE_DIR` set, parses the output, and returns it to the agent.

The memcore daemon auto-starts on first call and auto-exits after 30 minutes idle. No separate daemon management needed.
