/// System tests: true end-to-end tests that spawn the compiled memcore binary.
///
/// Each test creates an isolated MEMCORE_DIR, runs CLI commands via `assert_cmd`,
/// and verifies output. The daemon is auto-started by the first command and
/// stopped on cleanup.
mod system;

use system::helpers::TestEnv;
use predicates::prelude::*;

// ============================================================
// Init
// ============================================================

#[test]
fn test_init_creates_directory_structure() {
    let env = TestEnv::new();
    assert!(env.dir.path().join("memories").is_dir());
    assert!(env.dir.path().join("index").is_dir());
    assert!(env.dir.path().join("models").is_dir());
    assert!(env.dir.path().join("memcore.toml").is_file());
}

#[test]
fn test_init_idempotent() {
    let env = TestEnv::new();
    // Run init again — should succeed without error
    env.cmd()
        .args(["init", "--dir"])
        .arg(env.dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("initialized"));
}

// ============================================================
// Create + Get
// ============================================================

#[test]
fn test_create_and_get_node() {
    let env = TestEnv::new();
    let content = TestEnv::make_content("a test node", &[], "Hello world");
    let file = env.write_content_file("test-node", &content);

    // Create
    env.cmd()
        .args(["create", "test-node", "-f"])
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("created: test-node"));

    // Get
    env.cmd()
        .args(["get", "test-node"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Hello world"))
        .stdout(predicate::str::contains("a test node"));
}

#[test]
fn test_create_invalid_name_fails() {
    let env = TestEnv::new();
    let content = TestEnv::make_content("bad name", &[], "body");
    let file = env.write_content_file("bad", &content);

    env.cmd()
        .args(["create", "INVALID_NAME", "-f"])
        .arg(&file)
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

#[test]
fn test_create_duplicate_fails() {
    let env = TestEnv::new();
    let content = TestEnv::make_content("dup test", &[], "body");
    let file = env.write_content_file("dup-node", &content);

    env.cmd()
        .args(["create", "dup-node", "-f"])
        .arg(&file)
        .assert()
        .success();

    // Second create with same name should fail
    env.cmd()
        .args(["create", "dup-node", "-f"])
        .arg(&file)
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn test_get_nonexistent_fails() {
    let env = TestEnv::new();

    env.cmd()
        .args(["get", "no-such-node"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

// ============================================================
// Update
// ============================================================

#[test]
fn test_update_node() {
    let env = TestEnv::new();
    let content = TestEnv::make_content("original", &[], "original body");
    let file = env.write_content_file("upd-node", &content);

    env.cmd()
        .args(["create", "upd-node", "-f"])
        .arg(&file)
        .assert()
        .success();

    let new_content = TestEnv::make_content("updated abstract", &[], "new body");
    let new_file = env.write_content_file("upd-node-new", &new_content);

    env.cmd()
        .args(["update", "upd-node", "-f"])
        .arg(&new_file)
        .assert()
        .success()
        .stdout(predicate::str::contains("updated"));

    // Verify
    env.cmd()
        .args(["get", "upd-node"])
        .assert()
        .success()
        .stdout(predicate::str::contains("new body"));
}

// ============================================================
// Delete
// ============================================================

#[test]
fn test_delete_node() {
    let env = TestEnv::new();
    let content = TestEnv::make_content("to delete", &[], "body");
    let file = env.write_content_file("del-node", &content);

    env.cmd()
        .args(["create", "del-node", "-f"])
        .arg(&file)
        .assert()
        .success();

    env.cmd()
        .args(["delete", "del-node"])
        .assert()
        .success()
        .stdout(predicate::str::contains("deleted"));

    // Get should now fail
    env.cmd()
        .args(["get", "del-node"])
        .assert()
        .failure();
}

// ============================================================
// Rename
// ============================================================

#[test]
fn test_rename_node() {
    let env = TestEnv::new();
    let content = TestEnv::make_content("rename me", &[], "body");
    let file = env.write_content_file("old-name", &content);

    env.cmd()
        .args(["create", "old-name", "-f"])
        .arg(&file)
        .assert()
        .success();

    env.cmd()
        .args(["rename", "old-name", "new-name"])
        .assert()
        .success()
        .stdout(predicate::str::contains("renamed"));

    // Old name gone
    env.cmd()
        .args(["get", "old-name"])
        .assert()
        .failure();

    // New name works
    env.cmd()
        .args(["get", "new-name"])
        .assert()
        .success()
        .stdout(predicate::str::contains("body"));
}

// ============================================================
// Patch
// ============================================================

#[test]
fn test_patch_append() {
    let env = TestEnv::new();
    let content = TestEnv::make_content("patch me", &[], "original");
    let file = env.write_content_file("patch-node", &content);

    env.cmd()
        .args(["create", "patch-node", "-f"])
        .arg(&file)
        .assert()
        .success();

    env.cmd()
        .args(["patch", "patch-node", "--append", "\nappended text"])
        .assert()
        .success()
        .stdout(predicate::str::contains("patched"));

    env.cmd()
        .args(["get", "patch-node"])
        .assert()
        .success()
        .stdout(predicate::str::contains("original"))
        .stdout(predicate::str::contains("appended text"));
}

#[test]
fn test_patch_replace() {
    let env = TestEnv::new();
    let content = TestEnv::make_content("replace test", &[], "hello world");
    let file = env.write_content_file("rep-node", &content);

    env.cmd()
        .args(["create", "rep-node", "-f"])
        .arg(&file)
        .assert()
        .success();

    env.cmd()
        .args(["patch", "rep-node", "--replace", "hello", "goodbye"])
        .assert()
        .success();

    env.cmd()
        .args(["get", "rep-node"])
        .assert()
        .success()
        .stdout(predicate::str::contains("goodbye world"));
}

// ============================================================
// Link + Unlink + Neighbors
// ============================================================

#[test]
fn test_link_and_neighbors() {
    let env = TestEnv::new();

    let ca = TestEnv::make_content("node a", &[], "A");
    let cb = TestEnv::make_content("node b", &[], "B");
    let fa = env.write_content_file("node-a", &ca);
    let fb = env.write_content_file("node-b", &cb);

    env.cmd()
        .args(["create", "node-a", "-f"])
        .arg(&fa)
        .assert()
        .success();
    env.cmd()
        .args(["create", "node-b", "-f"])
        .arg(&fb)
        .assert()
        .success();

    // Link
    env.cmd()
        .args(["link", "node-a", "node-b"])
        .assert()
        .success()
        .stdout(predicate::str::contains("linked"));

    // Neighbors of node-a
    env.cmd()
        .args(["neighbors", "node-a"])
        .assert()
        .success()
        .stdout(predicate::str::contains("node-b"));

    // Unlink
    env.cmd()
        .args(["unlink", "node-a", "node-b"])
        .assert()
        .success()
        .stdout(predicate::str::contains("unlinked"));
}

// ============================================================
// Boost + Penalize
// ============================================================

#[test]
fn test_boost_and_penalize() {
    let env = TestEnv::new();
    let content = TestEnv::make_content("feedback test", &[], "body");
    let file = env.write_content_file("fb-node", &content);

    env.cmd()
        .args(["create", "fb-node", "-f"])
        .arg(&file)
        .assert()
        .success();

    // Node starts at weight 1.0 — boost should say "already at maximum"
    env.cmd()
        .args(["boost", "fb-node"])
        .assert()
        .success()
        .stdout(predicate::str::contains("already at maximum"));

    // Penalize to get below max
    env.cmd()
        .args(["penalize", "fb-node"])
        .assert()
        .success()
        .stdout(predicate::str::contains("penalized"));

    // Now boost works (below max)
    env.cmd()
        .args(["boost", "fb-node"])
        .assert()
        .success()
        .stdout(predicate::str::contains("boosted"));
}

// ============================================================
// Pin + Unpin
// ============================================================

#[test]
fn test_pin_and_unpin() {
    let env = TestEnv::new();
    let content = TestEnv::make_content("pin test", &[], "body");
    let file = env.write_content_file("pin-node", &content);

    env.cmd()
        .args(["create", "pin-node", "-f"])
        .arg(&file)
        .assert()
        .success();

    env.cmd()
        .args(["pin", "pin-node"])
        .assert()
        .success()
        .stdout(predicate::str::contains("pinned"));

    env.cmd()
        .args(["unpin", "pin-node"])
        .assert()
        .success()
        .stdout(predicate::str::contains("unpinned"));
}

// ============================================================
// Ls
// ============================================================

#[test]
fn test_ls_empty() {
    let env = TestEnv::new();

    env.cmd()
        .args(["ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no nodes"));
}

#[test]
fn test_ls_with_nodes() {
    let env = TestEnv::new();

    for name in &["alpha", "beta", "gamma"] {
        let content = TestEnv::make_content(&format!("{} node", name), &[], "body");
        let file = env.write_content_file(name, &content);
        env.cmd()
            .args(["create", name, "-f"])
            .arg(&file)
            .assert()
            .success();
    }

    env.cmd()
        .args(["ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha"))
        .stdout(predicate::str::contains("beta"))
        .stdout(predicate::str::contains("gamma"));
}

#[test]
fn test_ls_sort_by_weight() {
    let env = TestEnv::new();

    let content = TestEnv::make_content("ls sort test", &[], "body");
    let file = env.write_content_file("ls-node", &content);
    env.cmd()
        .args(["create", "ls-node", "-f"])
        .arg(&file)
        .assert()
        .success();

    env.cmd()
        .args(["ls", "--sort", "weight"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ls-node"));
}

// ============================================================
// Status
// ============================================================

#[test]
fn test_status() {
    let env = TestEnv::new();

    // Status triggers daemon start
    env.cmd()
        .args(["status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("pid"))
        .stdout(predicate::str::contains("nodes"))
        .stdout(predicate::str::contains("uptime"));
}

// ============================================================
// Inspect
// ============================================================

#[test]
fn test_inspect_empty() {
    let env = TestEnv::new();

    env.cmd()
        .args(["inspect"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Health"))
        .stdout(predicate::str::contains("100%"));
}

#[test]
fn test_inspect_with_nodes() {
    let env = TestEnv::new();

    let content = TestEnv::make_content("inspect test", &[], "body");
    let file = env.write_content_file("insp-node", &content);
    env.cmd()
        .args(["create", "insp-node", "-f"])
        .arg(&file)
        .assert()
        .success();

    env.cmd()
        .args(["inspect"])
        .assert()
        .success()
        .stdout(predicate::str::contains("nodes:"))
        .stdout(predicate::str::contains("1"));
}

// ============================================================
// GC
// ============================================================

#[test]
fn test_gc() {
    let env = TestEnv::new();

    env.cmd()
        .args(["gc"])
        .assert()
        .success()
        .stdout(predicate::str::contains("gc"));
}

// ============================================================
// Stop
// ============================================================

#[test]
fn test_stop_daemon() {
    let env = TestEnv::new();

    // Start daemon by running status
    env.cmd().args(["status"]).assert().success();

    // Stop
    env.cmd()
        .args(["stop"])
        .assert()
        .success()
        .stdout(predicate::str::contains("stopping"));
}

// ============================================================
// Recall (name prefix mode)
// ============================================================

#[test]
fn test_recall_name_prefix() {
    let env = TestEnv::new();

    for name in &["proj-alpha", "proj-beta", "other-node"] {
        let content = TestEnv::make_content(&format!("{} abstract", name), &[], "body");
        let file = env.write_content_file(name, &content);
        env.cmd()
            .args(["create", name, "-f"])
            .arg(&file)
            .assert()
            .success();
    }

    env.cmd()
        .args(["recall", "--name", "proj"])
        .assert()
        .success()
        .stdout(predicate::str::contains("proj-alpha"))
        .stdout(predicate::str::contains("proj-beta"));
}

// ============================================================
// Full workflow: create → link → boost → inspect → delete
// ============================================================

#[test]
fn test_full_workflow() {
    let env = TestEnv::new();

    // Create two nodes
    let ca = TestEnv::make_content("workflow node a", &[], "Body A");
    let cb = TestEnv::make_content("workflow node b", &[], "Body B");
    let fa = env.write_content_file("wf-a", &ca);
    let fb = env.write_content_file("wf-b", &cb);

    env.cmd()
        .args(["create", "wf-a", "-f"])
        .arg(&fa)
        .assert()
        .success();
    env.cmd()
        .args(["create", "wf-b", "-f"])
        .arg(&fb)
        .assert()
        .success();

    // Link them
    env.cmd()
        .args(["link", "wf-a", "wf-b"])
        .assert()
        .success();

    // Boost wf-a
    env.cmd()
        .args(["boost", "wf-a"])
        .assert()
        .success();

    // Pin wf-b
    env.cmd()
        .args(["pin", "wf-b"])
        .assert()
        .success();

    // Ls should show both nodes
    env.cmd()
        .args(["ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("wf-a"))
        .stdout(predicate::str::contains("wf-b"));

    // Inspect should show 2 nodes, 1 edge
    env.cmd()
        .args(["inspect"])
        .assert()
        .success()
        .stdout(predicate::str::contains("nodes:"))
        .stdout(predicate::str::contains("edges:"));

    // Neighbors of wf-a should include wf-b
    env.cmd()
        .args(["neighbors", "wf-a"])
        .assert()
        .success()
        .stdout(predicate::str::contains("wf-b"));

    // Delete wf-a
    env.cmd()
        .args(["delete", "wf-a"])
        .assert()
        .success();

    // wf-b should still exist, no longer linked
    env.cmd()
        .args(["get", "wf-b"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Body B"));

    // wf-a should be gone
    env.cmd()
        .args(["get", "wf-a"])
        .assert()
        .failure();
}

// ============================================================
// Create with links
// ============================================================

#[test]
fn test_create_with_links() {
    let env = TestEnv::new();

    // Create target node first
    let ct = TestEnv::make_content("target node", &[], "target body");
    let ft = env.write_content_file("target", &ct);
    env.cmd()
        .args(["create", "target", "-f"])
        .arg(&ft)
        .assert()
        .success();

    // Create node with link to target
    let cl = TestEnv::make_content("linking node", &["target"], "linked body");
    let fl = env.write_content_file("linker", &cl);
    env.cmd()
        .args(["create", "linker", "-f"])
        .arg(&fl)
        .assert()
        .success();

    // Both should be neighbors
    env.cmd()
        .args(["neighbors", "linker"])
        .assert()
        .success()
        .stdout(predicate::str::contains("target"));

    env.cmd()
        .args(["neighbors", "target"])
        .assert()
        .success()
        .stdout(predicate::str::contains("linker"));
}

// ============================================================
// Create with minimal content (agent-friendly format)
// ============================================================

#[test]
fn test_create_with_minimal_content() {
    let env = TestEnv::new();
    let content = TestEnv::make_minimal_content("minimal abstract", "minimal body");
    let file = env.write_content_file("min-node", &content);

    env.cmd()
        .args(["create", "min-node", "-f"])
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("created: min-node"));

    env.cmd()
        .args(["get", "min-node"])
        .assert()
        .success()
        .stdout(predicate::str::contains("minimal body"))
        .stdout(predicate::str::contains("minimal abstract"));
}

// ============================================================
// Rename updates peer links
// ============================================================

#[test]
fn test_rename_preserves_links() {
    let env = TestEnv::new();

    let ca = TestEnv::make_content("node a", &[], "A");
    let cb = TestEnv::make_content("node b", &[], "B");
    let fa = env.write_content_file("ra", &ca);
    let fb = env.write_content_file("rb", &cb);

    env.cmd()
        .args(["create", "ra", "-f"])
        .arg(&fa)
        .assert()
        .success();
    env.cmd()
        .args(["create", "rb", "-f"])
        .arg(&fb)
        .assert()
        .success();

    env.cmd()
        .args(["link", "ra", "rb"])
        .assert()
        .success();

    // Rename ra to ra-new
    env.cmd()
        .args(["rename", "ra", "ra-new"])
        .assert()
        .success();

    // rb should now link to ra-new
    env.cmd()
        .args(["neighbors", "rb"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ra-new"));
}

// ============================================================
// BUG-4 repro: create-with-links after rename+update+patch
// ============================================================

#[test]
fn test_create_with_links_after_rename_and_patch() {
    let env = TestEnv::new();

    // Create base node
    let c1 = TestEnv::make_content("base node", &[], "base body");
    let f1 = env.write_content_file("base-node", &c1);
    env.cmd()
        .args(["create", "base-node", "-f"])
        .arg(&f1)
        .assert()
        .success();

    // Create second node linked to base
    let c2 = TestEnv::make_content("second node", &["base-node"], "second body");
    let f2 = env.write_content_file("second-node", &c2);
    env.cmd()
        .args(["create", "second-node", "-f"])
        .arg(&f2)
        .assert()
        .success();

    // Create third node, then rename it
    let c3 = TestEnv::make_content("third node", &["second-node"], "third body");
    let f3 = env.write_content_file("old-name", &c3);
    env.cmd()
        .args(["create", "old-name", "-f"])
        .arg(&f3)
        .assert()
        .success();
    env.cmd()
        .args(["rename", "old-name", "new-name"])
        .assert()
        .success();

    // Patch base-node
    env.cmd()
        .args(["patch", "base-node", "--append", "extra text"])
        .assert()
        .success();

    // Update second-node
    let c4 = TestEnv::make_content("updated second", &["base-node"], "updated body");
    let f4 = env.write_content_file("second-updated", &c4);
    env.cmd()
        .args(["update", "second-node", "-f"])
        .arg(&f4)
        .assert()
        .success();

    // NOW: create a new node with links to base-node (should succeed)
    let c5 = TestEnv::make_content("linker node", &["base-node"], "linker body");
    let f5 = env.write_content_file("linker-node", &c5);
    env.cmd()
        .args(["create", "linker-node", "-f"])
        .arg(&f5)
        .assert()
        .success()
        .stdout(predicate::str::contains("created: linker-node"));
}

// ============================================================
// Pattern 4: Output formatting tests
// ============================================================

#[test]
fn test_format_node_batch_separator() {
    let env = TestEnv::new();

    let ca = TestEnv::make_content("node a abstract", &[], "Body of A");
    let cb = TestEnv::make_content("node b abstract", &[], "Body of B");
    let fa = env.write_content_file("fmt-a", &ca);
    let fb = env.write_content_file("fmt-b", &cb);

    env.cmd()
        .args(["create", "fmt-a", "-f"])
        .arg(&fa)
        .assert()
        .success();
    env.cmd()
        .args(["create", "fmt-b", "-f"])
        .arg(&fb)
        .assert()
        .success();

    // Multi-get should have "---" separator between nodes
    let output = env.cmd()
        .args(["get", "fmt-a", "fmt-b"])
        .output()
        .expect("failed to run get");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Body of A"), "should contain first node body");
    assert!(stdout.contains("Body of B"), "should contain second node body");
    // Separator: named separator "=== <name> ===" between entries
    assert!(stdout.contains("=== fmt-b ==="), "should have named separator between nodes");
}

#[test]
fn test_format_node_list_empty() {
    let env = TestEnv::new();

    let output = env.cmd()
        .args(["ls"])
        .output()
        .expect("failed to run ls");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("(no nodes)"), "empty ls should print '(no nodes)'");
}

#[test]
fn test_format_search_results_empty() {
    let env = TestEnv::new();

    // Recall with no nodes should return empty
    let output = env.cmd()
        .args(["recall"])
        .output()
        .expect("failed to run recall");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("(no results)"), "empty recall should print '(no results)'");
}

#[test]
fn test_format_error_to_stderr() {
    let env = TestEnv::new();

    // Get a nonexistent node — error should go to stderr
    let output = env.cmd()
        .args(["get", "ghost"])
        .output()
        .expect("failed to run get");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error:"), "errors should go to stderr");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("error:"), "errors should NOT be on stdout");
}

#[test]
fn test_inspect_json_format_system() {
    let env = TestEnv::new();

    let content = TestEnv::make_content("json test", &[], "body");
    let file = env.write_content_file("json-node", &content);
    env.cmd()
        .args(["create", "json-node", "-f"])
        .arg(&file)
        .assert()
        .success();

    let output = env.cmd()
        .args(["inspect", "--format", "json"])
        .output()
        .expect("failed to run inspect");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should be valid JSON
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(stdout.trim());
    assert!(parsed.is_ok(), "inspect --format json should output valid JSON, got: {}", stdout);
    let data = parsed.unwrap();
    assert_eq!(data["node_count"], 1);
}

// ============================================================
// Exit code validation (design spec: 0=success, 1=user error)
// ============================================================

#[test]
fn test_exit_code_success() {
    let env = TestEnv::new();

    let output = env.cmd()
        .args(["status"])
        .output()
        .expect("failed to run status");
    assert_eq!(output.status.code(), Some(0), "success should exit with code 0");
}

#[test]
fn test_exit_code_user_error_not_found() {
    let env = TestEnv::new();

    let output = env.cmd()
        .args(["get", "nonexistent-node"])
        .output()
        .expect("failed to run get");
    assert_eq!(output.status.code(), Some(1), "user error (not found) should exit with code 1");
}

#[test]
fn test_exit_code_user_error_invalid_name() {
    let env = TestEnv::new();
    let content = TestEnv::make_content("test", &[], "body");
    let file = env.write_content_file("bad", &content);

    let output = env.cmd()
        .args(["create", "INVALID_NAME", "-f"])
        .arg(&file)
        .output()
        .expect("failed to run create");
    assert_eq!(output.status.code(), Some(1), "user error (invalid name) should exit with code 1");
}

#[test]
fn test_exit_code_user_error_duplicate() {
    let env = TestEnv::new();
    let content = TestEnv::make_content("dup", &[], "body");
    let file = env.write_content_file("dup-ec", &content);

    env.cmd()
        .args(["create", "dup-ec", "-f"])
        .arg(&file)
        .assert()
        .success();

    let output = env.cmd()
        .args(["create", "dup-ec", "-f"])
        .arg(&file)
        .output()
        .expect("failed to run create");
    assert_eq!(output.status.code(), Some(1), "user error (duplicate) should exit with code 1");
}

#[test]
fn test_exit_code_user_error_delete_not_found() {
    let env = TestEnv::new();

    let output = env.cmd()
        .args(["delete", "ghost-node"])
        .output()
        .expect("failed to run delete");
    assert_eq!(output.status.code(), Some(1), "user error (delete not found) should exit with code 1");
}

#[test]
fn test_exit_code_user_error_link_not_found() {
    let env = TestEnv::new();

    let output = env.cmd()
        .args(["link", "ghost-a", "ghost-b"])
        .output()
        .expect("failed to run link");
    assert_eq!(output.status.code(), Some(1), "user error (link not found) should exit with code 1");
}

#[test]
fn test_exit_code_user_error_boost_not_found() {
    let env = TestEnv::new();

    let output = env.cmd()
        .args(["boost", "ghost-node"])
        .output()
        .expect("failed to run boost");
    assert_eq!(output.status.code(), Some(1), "user error (boost not found) should exit with code 1");
}

#[test]
fn test_exit_code_user_error_rename_not_found() {
    let env = TestEnv::new();

    let output = env.cmd()
        .args(["rename", "ghost-a", "ghost-b"])
        .output()
        .expect("failed to run rename");
    assert_eq!(output.status.code(), Some(1), "user error (rename not found) should exit with code 1");
}

#[test]
fn test_exit_code_user_error_update_not_found() {
    let env = TestEnv::new();
    let content = TestEnv::make_content("test", &[], "body");
    let file = env.write_content_file("upd-ghost", &content);

    let output = env.cmd()
        .args(["update", "ghost-node", "-f"])
        .arg(&file)
        .output()
        .expect("failed to run update");
    assert_eq!(output.status.code(), Some(1), "user error (update not found) should exit with code 1");
}

#[test]
fn test_exit_code_success_ls_empty() {
    let env = TestEnv::new();

    let output = env.cmd()
        .args(["ls"])
        .output()
        .expect("failed to run ls");
    assert_eq!(output.status.code(), Some(0), "ls on empty system should succeed with code 0");
}

#[test]
fn test_exit_code_success_gc() {
    let env = TestEnv::new();

    let output = env.cmd()
        .args(["gc"])
        .output()
        .expect("failed to run gc");
    assert_eq!(output.status.code(), Some(0), "gc should succeed with code 0");
}

#[test]
fn test_exit_code_success_inspect() {
    let env = TestEnv::new();

    let output = env.cmd()
        .args(["inspect"])
        .output()
        .expect("failed to run inspect");
    assert_eq!(output.status.code(), Some(0), "inspect should succeed with code 0");
}

// ============================================================
// Daemon idle timeout (requires ~65s; skipped in regular CI)
// ============================================================

#[test]
#[ignore] // Takes >60s — run with: cargo test test_daemon_idle_timeout -- --ignored
fn test_daemon_idle_timeout() {
    let env = TestEnv::new();

    // Write config with idle_timeout_minutes = 0 (exits on first check at ~60s)
    let config = "[daemon]\nidle_timeout_minutes = 0\n";
    std::fs::write(env.dir.path().join("memcore.toml"), config).unwrap();

    // Start daemon by sending a status request
    let output = env.cmd()
        .args(["status"])
        .output()
        .expect("failed to run status");
    assert!(output.status.success(), "initial status should succeed");

    // Extract PID from status output
    let stdout = String::from_utf8_lossy(&output.stdout);
    let pid_line = stdout.lines().find(|l| l.starts_with("pid:")).unwrap();
    let pid: u32 = pid_line.split_whitespace().last().unwrap().parse().unwrap();

    // Daemon should still be alive right now
    assert!(
        std::path::Path::new(&format!("/proc/{}", pid)).exists(),
        "daemon should be alive immediately after start"
    );

    // Wait for the daemon to timeout (check interval is 60s, timeout is 0s)
    // Allow 70s for safety margin
    std::thread::sleep(std::time::Duration::from_secs(70));

    // Daemon should have exited
    assert!(
        !std::path::Path::new(&format!("/proc/{}", pid)).exists(),
        "daemon should have exited after idle timeout"
    );

    // PID file should be cleaned up
    assert!(
        !env.dir.path().join(".daemon.pid").exists(),
        "pid file should be cleaned up after idle timeout"
    );
}
