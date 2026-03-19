/**
 * Tests for the MemCore OpenClaw plugin.
 *
 * Unit tests mock execFile to verify CLI argument construction.
 * Integration tests (gated by MEMCORE_LIVE_TEST=1) hit a real memcore binary.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { execFile } from "node:child_process";

// ---------------------------------------------------------------------------
// Mock setup — intercept child_process.execFile
// ---------------------------------------------------------------------------

vi.mock("node:child_process", () => ({
  execFile: vi.fn(),
}));

const mockedExecFile = vi.mocked(execFile) as any;

// Helper to make the mock resolve with given stdout/stderr
function mockExecSuccess(stdout: string, stderr = "") {
  mockedExecFile.mockImplementation(
    (_cmd: string, _args: string[], _opts: any, callback: Function) => {
      callback(null, stdout, stderr);
      return {} as any;
    },
  );
}

function mockExecFailure(stderr: string, code = 1) {
  mockedExecFile.mockImplementation(
    (_cmd: string, _args: string[], _opts: any, callback: Function) => {
      const err = Object.assign(new Error(stderr), {
        stdout: "",
        stderr,
        code,
      });
      callback(err, "", stderr);
      return {} as any;
    },
  );
}

// ---------------------------------------------------------------------------
// Import plugin after mocks are set up
// ---------------------------------------------------------------------------

import plugin, { memcoreExec } from "./index.js";

// ---------------------------------------------------------------------------
// Plugin registration tests
// ---------------------------------------------------------------------------

describe("plugin metadata", () => {
  it("has correct id and kind", () => {
    expect(plugin.id).toBe("memory-memcore");
    expect(plugin.kind).toBe("memory");
  });
});

describe("plugin registration", () => {
  let registeredTools: Array<{ tool: any; opts: any }>;
  let mockApi: any;

  beforeEach(() => {
    registeredTools = [];
    mockApi = {
      id: "memory-memcore",
      name: "MemCore Memory",
      source: "test",
      pluginConfig: {
        binaryPath: "/usr/local/bin/memcore",
        memcoreDir: "/tmp/test-memcore",
        timeoutMs: 5000,
      },
      runtime: {},
      logger: {
        info: vi.fn(),
        warn: vi.fn(),
        error: vi.fn(),
        debug: vi.fn(),
      },
      registerTool: (tool: any, opts: any) => {
        registeredTools.push({ tool, opts });
      },
      registerCli: vi.fn(),
      registerService: vi.fn(),
      on: vi.fn(),
      resolvePath: (p: string) => p,
    };
    plugin.register(mockApi);
  });

  it("registers all 8 tools", () => {
    expect(registeredTools).toHaveLength(8);
  });

  it("registers memory_recall", () => {
    const tool = registeredTools.find((t) => t.opts?.name === "memory_recall");
    expect(tool).toBeDefined();
    expect(tool!.tool.name).toBe("memory_recall");
  });

  it("registers memory_get", () => {
    const tool = registeredTools.find((t) => t.opts?.name === "memory_get");
    expect(tool).toBeDefined();
  });

  it("registers memory_store", () => {
    const tool = registeredTools.find((t) => t.opts?.name === "memory_store");
    expect(tool).toBeDefined();
  });

  it("registers memory_forget", () => {
    const tool = registeredTools.find((t) => t.opts?.name === "memory_forget");
    expect(tool).toBeDefined();
  });

  it("registers memory_boost", () => {
    const tool = registeredTools.find((t) => t.opts?.name === "memory_boost");
    expect(tool).toBeDefined();
  });

  it("registers memory_penalize", () => {
    const tool = registeredTools.find((t) => t.opts?.name === "memory_penalize");
    expect(tool).toBeDefined();
  });

  it("registers memory_link", () => {
    const tool = registeredTools.find((t) => t.opts?.name === "memory_link");
    expect(tool).toBeDefined();
  });

  it("registers memory_inspect", () => {
    const tool = registeredTools.find((t) => t.opts?.name === "memory_inspect");
    expect(tool).toBeDefined();
  });
});

// ---------------------------------------------------------------------------
// Tool execution tests (mocked CLI)
// ---------------------------------------------------------------------------

describe("memory_recall", () => {
  let recallTool: any;

  beforeEach(() => {
    const tools: any[] = [];
    const mockApi = {
      pluginConfig: { binaryPath: "/bin/memcore", memcoreDir: "/data" },
      registerTool: (tool: any, opts: any) => tools.push({ tool, opts }),
    };
    plugin.register(mockApi);
    recallTool = tools.find((t) => t.opts?.name === "memory_recall")!.tool;
  });

  it("calls recall with query", async () => {
    mockExecSuccess("rust-error-handling  0.92\nrust-anyhow-tips  0.85\n");

    const result = await recallTool.execute("test-id", { query: "rust errors" });
    expect(result.content[0].text).toContain("rust-error-handling");

    // Verify the binary was called with correct args
    expect(mockedExecFile).toHaveBeenCalledWith(
      "/bin/memcore",
      ["recall", "rust errors"],
      expect.objectContaining({
        env: expect.objectContaining({ MEMCORE_DIR: "/data" }),
      }),
      expect.any(Function),
    );
  });

  it("passes top_k and depth options", async () => {
    mockExecSuccess("result\n");

    await recallTool.execute("test-id", {
      query: "test",
      top_k: 10,
      depth: 2,
      full: true,
    });

    expect(mockedExecFile).toHaveBeenCalledWith(
      "/bin/memcore",
      ["recall", "test", "--top-k", "10", "--depth", "2", "--full"],
      expect.any(Object),
      expect.any(Function),
    );
  });

  it("returns working memory when no query", async () => {
    mockExecSuccess("pinned-node  1.00\n");

    const result = await recallTool.execute("test-id", {});
    expect(result.content[0].text).toContain("pinned-node");

    expect(mockedExecFile).toHaveBeenCalledWith(
      "/bin/memcore",
      ["recall"],
      expect.any(Object),
      expect.any(Function),
    );
  });

  it("returns error on failure", async () => {
    mockExecFailure("daemon not running");

    const result = await recallTool.execute("test-id", { query: "test" });
    expect(result.content[0].text).toContain("Error:");
  });
});

describe("memory_get", () => {
  let getTool: any;

  beforeEach(() => {
    const tools: any[] = [];
    const mockApi = {
      pluginConfig: { binaryPath: "/bin/memcore" },
      registerTool: (tool: any, opts: any) => tools.push({ tool, opts }),
    };
    plugin.register(mockApi);
    getTool = tools.find((t) => t.opts?.name === "memory_get")!.tool;
  });

  it("gets multiple nodes", async () => {
    mockExecSuccess("---\nabstract: test\n---\nbody");

    const result = await getTool.execute("test-id", {
      names: ["node-a", "node-b"],
    });

    expect(result.content[0].text).toEqual("---\nabstract: test\n---\nbody");
    expect(mockedExecFile).toHaveBeenCalledWith(
      "/bin/memcore",
      ["get", "node-a", "node-b"],
      expect.any(Object),
      expect.any(Function),
    );
  });

  it("errors on empty names", async () => {
    const result = await getTool.execute("test-id", { names: [] });
    expect(result.content[0].text).toContain("Error:");
  });
});

describe("memory_store", () => {
  let storeTool: any;

  beforeEach(() => {
    const tools: any[] = [];
    const mockApi = {
      pluginConfig: { binaryPath: "/bin/memcore" },
      registerTool: (tool: any, opts: any) => tools.push({ tool, opts }),
    };
    plugin.register(mockApi);
    storeTool = tools.find((t) => t.opts?.name === "memory_store")!.tool;
  });

  it("creates a new node", async () => {
    mockExecSuccess("created");

    const result = await storeTool.execute("test-id", {
      name: "test-node",
      content: "---\nabstract: test\n---\nbody",
    });

    expect(result.content[0].text).toContain("Created memory: test-node");
  });

  it("falls back to update if node exists", async () => {
    // First call (create) fails with "already exists"
    let callCount = 0;
    mockedExecFile.mockImplementation(
      (_cmd: any, args: any, _opts: any, callback?: any) => {
        callCount++;
        if (typeof callback === "function") {
          if (callCount === 1) {
            // create fails
            const err = Object.assign(new Error("already exists"), {
              stdout: "",
              stderr: "already exists",
              code: 1,
            });
            callback(err, "", "already exists");
          } else {
            // update succeeds
            callback(null, "updated", "");
          }
        }
        return {} as any;
      },
    );

    const result = await storeTool.execute("test-id", {
      name: "existing-node",
      content: "---\nabstract: updated\n---\nnew body",
    });

    expect(result.content[0].text).toContain("Updated memory: existing-node");
  });
});

describe("memory_forget", () => {
  let forgetTool: any;

  beforeEach(() => {
    const tools: any[] = [];
    const mockApi = {
      pluginConfig: { binaryPath: "/bin/memcore" },
      registerTool: (tool: any, opts: any) => tools.push({ tool, opts }),
    };
    plugin.register(mockApi);
    forgetTool = tools.find((t) => t.opts?.name === "memory_forget")!.tool;
  });

  it("deletes nodes", async () => {
    mockExecSuccess("deleted");

    const result = await forgetTool.execute("test-id", {
      names: ["old-note", "stale-info"],
    });

    expect(result.content[0].text).toContain("Deleted: old-note, stale-info");
    expect(mockedExecFile).toHaveBeenCalledWith(
      "/bin/memcore",
      ["delete", "old-note", "stale-info"],
      expect.any(Object),
      expect.any(Function),
    );
  });
});

describe("memory_boost", () => {
  let boostTool: any;

  beforeEach(() => {
    const tools: any[] = [];
    const mockApi = {
      pluginConfig: { binaryPath: "/bin/memcore" },
      registerTool: (tool: any, opts: any) => tools.push({ tool, opts }),
    };
    plugin.register(mockApi);
    boostTool = tools.find((t) => t.opts?.name === "memory_boost")!.tool;
  });

  it("boosts a node", async () => {
    mockExecSuccess("weight: 0.9 -> 1.0");

    const result = await boostTool.execute("test-id", { name: "helpful-tip" });
    expect(result.content[0].text).toBe("weight: 0.9 -> 1.0");
  });
});

describe("memory_penalize", () => {
  let penalizeTool: any;

  beforeEach(() => {
    const tools: any[] = [];
    const mockApi = {
      pluginConfig: { binaryPath: "/bin/memcore" },
      registerTool: (tool: any, opts: any) => tools.push({ tool, opts }),
    };
    plugin.register(mockApi);
    penalizeTool = tools.find((t) => t.opts?.name === "memory_penalize")!.tool;
  });

  it("penalizes a node", async () => {
    mockExecSuccess("weight: 1.0 -> 0.8");

    const result = await penalizeTool.execute("test-id", {
      name: "misleading-note",
    });
    expect(result.content[0].text).toBe("weight: 1.0 -> 0.8");
  });
});

describe("memory_link", () => {
  let linkTool: any;

  beforeEach(() => {
    const tools: any[] = [];
    const mockApi = {
      pluginConfig: { binaryPath: "/bin/memcore" },
      registerTool: (tool: any, opts: any) => tools.push({ tool, opts }),
    };
    plugin.register(mockApi);
    linkTool = tools.find((t) => t.opts?.name === "memory_link")!.tool;
  });

  it("links two nodes", async () => {
    mockExecSuccess("linked");

    const result = await linkTool.execute("test-id", {
      a: "node-a",
      b: "node-b",
    });

    expect(result.content[0].text).toContain("Linked: node-a <-> node-b");
    expect(mockedExecFile).toHaveBeenCalledWith(
      "/bin/memcore",
      ["link", "node-a", "node-b"],
      expect.any(Object),
      expect.any(Function),
    );
  });
});

describe("memory_inspect", () => {
  let inspectTool: any;

  beforeEach(() => {
    const tools: any[] = [];
    const mockApi = {
      pluginConfig: { binaryPath: "/bin/memcore" },
      registerTool: (tool: any, opts: any) => tools.push({ tool, opts }),
    };
    plugin.register(mockApi);
    inspectTool = tools.find((t) => t.opts?.name === "memory_inspect")!.tool;
  });

  it("runs inspect with json format", async () => {
    mockExecSuccess('{"health": 87, "nodes": 42}');

    const result = await inspectTool.execute("test-id", { format: "json" });

    expect(result.content[0].text).toBe('{"health": 87, "nodes": 42}');
    expect(mockedExecFile).toHaveBeenCalledWith(
      "/bin/memcore",
      ["inspect", "--format", "json"],
      expect.any(Object),
      expect.any(Function),
    );
  });
});

// ---------------------------------------------------------------------------
// memcoreExec unit tests
// ---------------------------------------------------------------------------

describe("memcoreExec", () => {
  it("pipes stdin when input is provided", async () => {
    // Track stdin writes
    const stdinWrites: string[] = [];
    let stdinEnded = false;
    mockedExecFile.mockImplementation(
      (_cmd: any, _args: any, _opts: any, callback?: any) => {
        if (typeof callback === "function") {
          callback(null, "created", "");
        }
        return {
          stdin: {
            write: (data: string) => stdinWrites.push(data),
            end: () => { stdinEnded = true; },
          },
        } as any;
      },
    );

    await memcoreExec(["create", "test-node"], {}, "---\nabstract: test\n---\nbody");

    expect(stdinWrites).toHaveLength(1);
    expect(stdinWrites[0]).toContain("abstract: test");
    expect(stdinEnded).toBe(true);
  });

  it("does not pipe stdin when input is undefined", async () => {
    let stdinAccessed = false;
    mockedExecFile.mockImplementation(
      (_cmd: any, _args: any, _opts: any, callback?: any) => {
        if (typeof callback === "function") {
          callback(null, "ok", "");
        }
        return {
          get stdin() { stdinAccessed = true; return null; },
        } as any;
      },
    );

    await memcoreExec(["status"], {});

    // stdin should not be written to
    expect(stdinAccessed).toBe(false);
  });

  it("uses default binary name when not configured", async () => {
    mockExecSuccess("ok");

    await memcoreExec(["status"], {});

    expect(mockedExecFile).toHaveBeenCalledWith(
      "memcore",
      ["status"],
      expect.any(Object),
      expect.any(Function),
    );
  });

  it("sets MEMCORE_DIR when configured", async () => {
    mockExecSuccess("ok");

    await memcoreExec(["status"], { memcoreDir: "/my/dir" });

    expect(mockedExecFile).toHaveBeenCalledWith(
      "memcore",
      ["status"],
      expect.objectContaining({
        env: expect.objectContaining({ MEMCORE_DIR: "/my/dir" }),
      }),
      expect.any(Function),
    );
  });

  it("returns code 0 on success", async () => {
    mockExecSuccess("output", "warning");

    const result = await memcoreExec(["status"], {});
    expect(result.code).toBe(0);
    expect(result.stdout).toBe("output");
    expect(result.stderr).toBe("warning");
  });

  it("returns non-zero code on failure", async () => {
    mockExecFailure("not found", 1);

    const result = await memcoreExec(["get", "nonexistent"], {});
    expect(result.code).toBe(1);
    expect(result.stderr).toContain("not found");
  });
});

// ---------------------------------------------------------------------------
// Live integration tests (gated)
// ---------------------------------------------------------------------------

const LIVE = process.env.MEMCORE_LIVE_TEST === "1";
const MEMCORE_BIN = process.env.MEMCORE_BIN || "memcore";
const MEMCORE_DIR = process.env.MEMCORE_TEST_DIR;

describe.skipIf(!LIVE)("live integration", () => {
  // These tests require:
  // MEMCORE_LIVE_TEST=1
  // MEMCORE_BIN=/path/to/memcore (or memcore on PATH)
  // MEMCORE_TEST_DIR=/path/to/test/memcore/dir (initialized)

  // Unmock for live tests
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("can check status", async () => {
    // Re-import without mock for live test
    const { execFile: realExecFile } = await vi.importActual<
      typeof import("node:child_process")
    >("node:child_process");
    const { promisify } = await vi.importActual<typeof import("node:util")>(
      "node:util",
    );
    const exec = promisify(realExecFile);

    const env: Record<string, string> = { ...process.env } as Record<string, string>;
    if (MEMCORE_DIR) env.MEMCORE_DIR = MEMCORE_DIR;

    const { stdout } = await exec(MEMCORE_BIN, ["status"], {
      env,
      encoding: "utf8",
      timeout: 10_000,
    });

    expect(stdout).toContain("pid:");
    expect(stdout).toContain("nodes:");
  });
});
