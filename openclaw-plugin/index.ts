/**
 * OpenClaw memory plugin for MemCore.
 *
 * Replaces OpenClaw's built-in memory with MemCore's knowledge graph.
 * All memory operations delegate to the memcore CLI binary.
 */

import { execFile } from "node:child_process";

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

interface MemCoreConfig {
  binaryPath?: string;
  memcoreDir?: string;
  timeoutMs?: number;
}

function resolveConfig(pluginConfig: Record<string, unknown>): MemCoreConfig {
  return {
    binaryPath: (pluginConfig.binaryPath as string) || "memcore",
    memcoreDir: pluginConfig.memcoreDir as string | undefined,
    timeoutMs: (pluginConfig.timeoutMs as number) || 10_000,
  };
}

// ---------------------------------------------------------------------------
// CLI wrapper
// ---------------------------------------------------------------------------

export async function memcoreExec(
  args: string[],
  config: MemCoreConfig,
): Promise<{ stdout: string; stderr: string; code: number }> {
  const bin = config.binaryPath || "memcore";
  const env: Record<string, string> = { ...process.env } as Record<string, string>;
  if (config.memcoreDir) {
    env.MEMCORE_DIR = config.memcoreDir;
  }

  return new Promise((resolve) => {
    execFile(
      bin,
      args,
      {
        timeout: config.timeoutMs || 10_000,
        env,
        encoding: "utf8",
        maxBuffer: 1024 * 1024, // 1 MB
      },
      (err, stdout, stderr) => {
        if (err) {
          const e = err as { stdout?: string; stderr?: string; code?: number };
          resolve({
            stdout: e.stdout || stdout || "",
            stderr: e.stderr || stderr || String(err),
            code: e.code ?? 1,
          });
        } else {
          resolve({ stdout: stdout || "", stderr: stderr || "", code: 0 });
        }
      },
    );
  });
}

// ---------------------------------------------------------------------------
// Tool helpers
// ---------------------------------------------------------------------------

function textResult(text: string, details?: Record<string, unknown>) {
  return {
    content: [{ type: "text" as const, text }],
    ...(details ? { details } : {}),
  };
}

function errorResult(msg: string) {
  return textResult(`Error: ${msg}`);
}

// ---------------------------------------------------------------------------
// Plugin entry
// ---------------------------------------------------------------------------

export default {
  id: "memory-memcore",
  name: "MemCore Memory",
  description: "Persistent knowledge graph memory with semantic search",
  kind: "memory" as const,

  register(api: any) {
    const config = resolveConfig(api.pluginConfig || {});

    // -----------------------------------------------------------------------
    // memory_recall — semantic + graph recall
    // -----------------------------------------------------------------------
    api.registerTool(
      {
        name: "memory_recall",
        label: "Memory Recall",
        description:
          "Search through long-term memories using semantic similarity, knowledge graph proximity, and importance weighting. " +
          "Returns ranked results combining vector search, graph traversal, and feedback-adjusted weights. " +
          "Use this before starting any task to check if relevant knowledge already exists.",
        parameters: {
          type: "object",
          properties: {
            query: {
              type: "string",
              description: "Natural language search query",
            },
            top_k: {
              type: "number",
              description: "Maximum number of results to return (default: 5)",
            },
            depth: {
              type: "number",
              description: "Graph traversal depth from seed nodes (default: 1)",
            },
            full: {
              type: "boolean",
              description: "Include scores and abstracts in results (default: false)",
            },
          },
          required: [],
        },
        async execute(_toolCallId: string, params: Record<string, unknown>) {
          const args = ["recall"];
          if (params.query) args.push(String(params.query));
          if (params.top_k) args.push("--top-k", String(params.top_k));
          if (params.depth) args.push("--depth", String(params.depth));
          if (params.full) args.push("--full");

          const result = await memcoreExec(args, config);
          if (result.code !== 0) return errorResult(result.stderr);
          return textResult(
            result.stdout || "No memories found.",
            { query: params.query, top_k: params.top_k },
          );
        },
      },
      { name: "memory_recall" },
    );

    // -----------------------------------------------------------------------
    // memory_get — read specific nodes
    // -----------------------------------------------------------------------
    api.registerTool(
      {
        name: "memory_get",
        label: "Memory Get",
        description:
          "Read one or more specific memory nodes by exact name. " +
          "Returns the full content including frontmatter and body.",
        parameters: {
          type: "object",
          properties: {
            names: {
              type: "array",
              items: { type: "string" },
              description: "Names of memory nodes to retrieve",
            },
          },
          required: ["names"],
        },
        async execute(_toolCallId: string, params: Record<string, unknown>) {
          const names = params.names as string[];
          if (!names?.length) return errorResult("No names provided");

          const result = await memcoreExec(["get", ...names], config);
          if (result.code !== 0) return errorResult(result.stderr);
          return textResult(result.stdout);
        },
      },
      { name: "memory_get" },
    );

    // -----------------------------------------------------------------------
    // memory_store — create or update a memory node
    // -----------------------------------------------------------------------
    api.registerTool(
      {
        name: "memory_store",
        label: "Memory Store",
        description:
          "Create a new memory node or update an existing one. " +
          "Content should be markdown with YAML frontmatter containing at minimum an 'abstract' field. " +
          "Use descriptive names (e.g., 'rust-lifetime-elision', 'user-prefers-terse-output'). " +
          "If a node with the same name exists, it will be updated.",
        parameters: {
          type: "object",
          properties: {
            name: {
              type: "string",
              description: "Descriptive node name (letters, digits, spaces, hyphens; 2-128 chars)",
            },
            content: {
              type: "string",
              description:
                "Full markdown content with YAML frontmatter. Must include '---' delimiters and at least an 'abstract' field.",
            },
          },
          required: ["name", "content"],
        },
        async execute(_toolCallId: string, params: Record<string, unknown>) {
          const name = String(params.name);
          const content = String(params.content);

          // Try create first; if it already exists, use update
          const createResult = await memcoreExec(["create", name], config, content);
          if (createResult.code !== 0) {
            if (createResult.stderr.includes("already exists")) {
              const updateResult = await memcoreExec(["update", name], config, content);
              if (updateResult.code !== 0) return errorResult(updateResult.stderr);
              return textResult(`Updated memory: ${name}`);
            }
            return errorResult(createResult.stderr);
          }
          return textResult(`Created memory: ${name}`);
        },
      },
      { name: "memory_store" },
    );

    // -----------------------------------------------------------------------
    // memory_forget — delete a memory node
    // -----------------------------------------------------------------------
    api.registerTool(
      {
        name: "memory_forget",
        label: "Memory Forget",
        description:
          "Permanently delete one or more memory nodes. This is irreversible. " +
          "Use memory_recall first to verify you're deleting the right node.",
        parameters: {
          type: "object",
          properties: {
            names: {
              type: "array",
              items: { type: "string" },
              description: "Names of memory nodes to delete",
            },
          },
          required: ["names"],
        },
        async execute(_toolCallId: string, params: Record<string, unknown>) {
          const names = params.names as string[];
          if (!names?.length) return errorResult("No names provided");

          const result = await memcoreExec(["delete", ...names], config);
          if (result.code !== 0) return errorResult(result.stderr);
          return textResult(`Deleted: ${names.join(", ")}`);
        },
      },
      { name: "memory_forget" },
    );

    // -----------------------------------------------------------------------
    // memory_boost — positive feedback
    // -----------------------------------------------------------------------
    api.registerTool(
      {
        name: "memory_boost",
        label: "Memory Boost",
        description:
          "Increase a memory's importance weight. Use this when a memory was helpful. " +
          "Weight increases by +0.1 (additive, capped at 1.0).",
        parameters: {
          type: "object",
          properties: {
            name: {
              type: "string",
              description: "Name of the memory node to boost",
            },
          },
          required: ["name"],
        },
        async execute(_toolCallId: string, params: Record<string, unknown>) {
          const result = await memcoreExec(["boost", String(params.name)], config);
          if (result.code !== 0) return errorResult(result.stderr);
          return textResult(result.stdout || `Boosted: ${params.name}`);
        },
      },
      { name: "memory_boost" },
    );

    // -----------------------------------------------------------------------
    // memory_penalize — negative feedback
    // -----------------------------------------------------------------------
    api.registerTool(
      {
        name: "memory_penalize",
        label: "Memory Penalize",
        description:
          "Decrease a memory's importance weight. Use this when a memory was misleading or unhelpful. " +
          "Weight decreases by ×0.8 (multiplicative decay).",
        parameters: {
          type: "object",
          properties: {
            name: {
              type: "string",
              description: "Name of the memory node to penalize",
            },
          },
          required: ["name"],
        },
        async execute(_toolCallId: string, params: Record<string, unknown>) {
          const result = await memcoreExec(["penalize", String(params.name)], config);
          if (result.code !== 0) return errorResult(result.stderr);
          return textResult(result.stdout || `Penalized: ${params.name}`);
        },
      },
      { name: "memory_penalize" },
    );

    // -----------------------------------------------------------------------
    // memory_link — create graph edges
    // -----------------------------------------------------------------------
    api.registerTool(
      {
        name: "memory_link",
        label: "Memory Link",
        description:
          "Create a bidirectional link between two memory nodes. " +
          "Linked nodes boost each other's recall scores via graph proximity.",
        parameters: {
          type: "object",
          properties: {
            a: { type: "string", description: "First node name" },
            b: { type: "string", description: "Second node name" },
          },
          required: ["a", "b"],
        },
        async execute(_toolCallId: string, params: Record<string, unknown>) {
          const result = await memcoreExec(
            ["link", String(params.a), String(params.b)],
            config,
          );
          if (result.code !== 0) return errorResult(result.stderr);
          return textResult(`Linked: ${params.a} <-> ${params.b}`);
        },
      },
      { name: "memory_link" },
    );

    // -----------------------------------------------------------------------
    // memory_inspect — health check
    // -----------------------------------------------------------------------
    api.registerTool(
      {
        name: "memory_inspect",
        label: "Memory Inspect",
        description:
          "Check the health of the memory graph. Shows node/edge counts, " +
          "near-duplicate pairs, orphan nodes, and low-weight nodes. " +
          "Use periodically to keep the knowledge graph clean.",
        parameters: {
          type: "object",
          properties: {
            format: {
              type: "string",
              enum: ["human", "json"],
              description: "Output format (default: human)",
            },
          },
          required: [],
        },
        async execute(_toolCallId: string, params: Record<string, unknown>) {
          const args = ["inspect"];
          if (params.format) args.push("--format", String(params.format));

          const result = await memcoreExec(args, config);
          if (result.code !== 0) return errorResult(result.stderr);
          return textResult(result.stdout);
        },
      },
      { name: "memory_inspect" },
    );
  },
};
