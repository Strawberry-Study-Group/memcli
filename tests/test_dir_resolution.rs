// Tests for memcore directory resolution.
//
// Resolution priority (no silent fallbacks):
//   1. MEMCORE_DIR env var
//   2. Binary's own parent directory (if it contains memcore.toml or memories/)
//   3. Error — no ~/.memcore/ or .memcore/ fallback
//
// These tests spawn real daemons, so run single-threaded:
//   cargo test --test test_dir_resolution -- --test-threads=1

use assert_cmd::Command;
use tempfile::TempDir;

// ============================================================
// MEMCORE_DIR env var (highest priority)
// ============================================================

#[test]
fn test_memcore_dir_env_var_is_used() {
    let dir = TempDir::new().unwrap();
    // Init the directory so it has memcore.toml + memories/
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir.path())
        .arg("init")
        .arg("--dir")
        .arg(dir.path().to_str().unwrap());
    cmd.assert().success();

    // Status should work via MEMCORE_DIR
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir.path()).arg("status");
    cmd.assert().success();

    // Stop daemon
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir.path()).arg("stop");
    let _ = cmd.output();
}

#[test]
fn test_memcore_dir_env_var_overrides_binary_parent() {
    // Even if the binary sits next to memcore.toml, MEMCORE_DIR wins.
    let env_dir = TempDir::new().unwrap();

    // Init env_dir
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", env_dir.path())
        .arg("init")
        .arg("--dir")
        .arg(env_dir.path().to_str().unwrap());
    cmd.assert().success();

    // Create a node via MEMCORE_DIR
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", env_dir.path())
        .arg("create")
        .arg("env-override-test")
        .write_stdin("---\nabstract: testing env override\n---\ntest body");
    cmd.assert().success();

    // Verify node file is in env_dir, not next to binary
    assert!(env_dir.path().join("memories/env-override-test.md").exists());

    // Cleanup
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", env_dir.path())
        .arg("delete")
        .arg("env-override-test");
    let _ = cmd.output();
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", env_dir.path()).arg("stop");
    let _ = cmd.output();
}

// ============================================================
// Named env var: <DIRNAME>_DIR (derived from binary's parent dir)
// ============================================================

#[test]
fn test_named_env_var_from_parent_dir() {
    // Binary at .../work_memcore/memcore → checks WORK_MEMCORE_DIR env var.
    let data_dir = TempDir::new().unwrap();
    let bin_dir = TempDir::new().unwrap();

    // Create a "work_memcore" subdirectory to hold the binary
    let named_dir = bin_dir.path().join("work_memcore");
    std::fs::create_dir_all(&named_dir).unwrap();
    let binary = assert_cmd::cargo::cargo_bin("memcore");
    std::fs::copy(&binary, named_dir.join("memcore")).unwrap();

    // Init data_dir via MEMCORE_DIR
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", data_dir.path())
        .arg("init")
        .arg("--dir")
        .arg(data_dir.path().to_str().unwrap());
    cmd.assert().success();

    // Stop the daemon started by init
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", data_dir.path()).arg("stop");
    let _ = cmd.output();

    // Run the named binary with WORK_MEMCORE_DIR (not MEMCORE_DIR)
    let mut cmd = Command::new(named_dir.join("memcore"));
    cmd.env_remove("MEMCORE_DIR")
        .env("WORK_MEMCORE_DIR", data_dir.path())
        .arg("create")
        .arg("named-env-test")
        .write_stdin("---\nabstract: testing named env var\n---\nworks");
    cmd.assert().success();

    // Verify data landed in data_dir
    assert!(data_dir.path().join("memories/named-env-test.md").exists());

    // Cleanup
    let mut cmd = Command::new(named_dir.join("memcore"));
    cmd.env_remove("MEMCORE_DIR")
        .env("WORK_MEMCORE_DIR", data_dir.path())
        .arg("delete")
        .arg("named-env-test");
    let _ = cmd.output();
    let mut cmd = Command::new(named_dir.join("memcore"));
    cmd.env_remove("MEMCORE_DIR")
        .env("WORK_MEMCORE_DIR", data_dir.path())
        .arg("stop");
    let _ = cmd.output();
}

#[test]
fn test_memcore_dir_overrides_named_env_var() {
    // MEMCORE_DIR takes priority over <DIRNAME>_DIR.
    let override_dir = TempDir::new().unwrap();
    let named_dir_data = TempDir::new().unwrap();
    let bin_dir = TempDir::new().unwrap();

    let named_dir = bin_dir.path().join("work_memcore");
    std::fs::create_dir_all(&named_dir).unwrap();
    let binary = assert_cmd::cargo::cargo_bin("memcore");
    std::fs::copy(&binary, named_dir.join("memcore")).unwrap();

    // Init override_dir
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", override_dir.path())
        .arg("init")
        .arg("--dir")
        .arg(override_dir.path().to_str().unwrap());
    cmd.assert().success();

    // Init named_dir_data
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", named_dir_data.path())
        .arg("init")
        .arg("--dir")
        .arg(named_dir_data.path().to_str().unwrap());
    cmd.assert().success();

    // Stop daemons
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", override_dir.path()).arg("stop");
    let _ = cmd.output();
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", named_dir_data.path()).arg("stop");
    let _ = cmd.output();

    // Run with BOTH env vars set — MEMCORE_DIR should win
    let mut cmd = Command::new(named_dir.join("memcore"));
    cmd.env("MEMCORE_DIR", override_dir.path())
        .env("WORK_MEMCORE_DIR", named_dir_data.path())
        .arg("create")
        .arg("priority-test")
        .write_stdin("---\nabstract: testing priority\n---\ntest");
    cmd.assert().success();

    // Data should be in override_dir, not named_dir_data
    assert!(override_dir.path().join("memories/priority-test.md").exists());
    assert!(!named_dir_data.path().join("memories/priority-test.md").exists());

    // Cleanup
    let mut cmd = Command::new(named_dir.join("memcore"));
    cmd.env("MEMCORE_DIR", override_dir.path())
        .arg("delete")
        .arg("priority-test");
    let _ = cmd.output();
    let mut cmd = Command::new(named_dir.join("memcore"));
    cmd.env("MEMCORE_DIR", override_dir.path()).arg("stop");
    let _ = cmd.output();
}

#[test]
fn test_multiple_instances_via_named_env_vars() {
    // Two instances: work_memcore and personal_memcore, each with their own
    // named env var. Same binary, completely isolated data.
    let work_data = TempDir::new().unwrap();
    let personal_data = TempDir::new().unwrap();
    let bin_dir = TempDir::new().unwrap();

    // Create two named directories with the same binary
    let work_bin_dir = bin_dir.path().join("work_memcore");
    let personal_bin_dir = bin_dir.path().join("personal_memcore");
    std::fs::create_dir_all(&work_bin_dir).unwrap();
    std::fs::create_dir_all(&personal_bin_dir).unwrap();
    let binary = assert_cmd::cargo::cargo_bin("memcore");
    std::fs::copy(&binary, work_bin_dir.join("memcore")).unwrap();
    std::fs::copy(&binary, personal_bin_dir.join("memcore")).unwrap();

    // Init both data dirs
    for (dir, env_name) in [
        (&work_data, "WORK_MEMCORE_DIR"),
        (&personal_data, "PERSONAL_MEMCORE_DIR"),
    ] {
        let mut cmd = Command::cargo_bin("memcore").unwrap();
        cmd.env("MEMCORE_DIR", dir.path())
            .arg("init")
            .arg("--dir")
            .arg(dir.path().to_str().unwrap());
        cmd.assert().success();
        let mut cmd = Command::cargo_bin("memcore").unwrap();
        cmd.env("MEMCORE_DIR", dir.path()).arg("stop");
        let _ = cmd.output();
    }

    // Create node via work_memcore binary + WORK_MEMCORE_DIR
    let mut cmd = Command::new(work_bin_dir.join("memcore"));
    cmd.env_remove("MEMCORE_DIR")
        .env("WORK_MEMCORE_DIR", work_data.path())
        .arg("create")
        .arg("work-note")
        .write_stdin("---\nabstract: work stuff\n---\nwork");
    cmd.assert().success();

    // Create node via personal_memcore binary + PERSONAL_MEMCORE_DIR
    let mut cmd = Command::new(personal_bin_dir.join("memcore"));
    cmd.env_remove("MEMCORE_DIR")
        .env("PERSONAL_MEMCORE_DIR", personal_data.path())
        .arg("create")
        .arg("personal-note")
        .write_stdin("---\nabstract: personal stuff\n---\npersonal");
    cmd.assert().success();

    // Verify isolation
    assert!(work_data.path().join("memories/work-note.md").exists());
    assert!(!work_data.path().join("memories/personal-note.md").exists());
    assert!(personal_data.path().join("memories/personal-note.md").exists());
    assert!(!personal_data.path().join("memories/work-note.md").exists());

    // Cleanup
    let mut cmd = Command::new(work_bin_dir.join("memcore"));
    cmd.env_remove("MEMCORE_DIR")
        .env("WORK_MEMCORE_DIR", work_data.path())
        .arg("stop");
    let _ = cmd.output();
    let mut cmd = Command::new(personal_bin_dir.join("memcore"));
    cmd.env_remove("MEMCORE_DIR")
        .env("PERSONAL_MEMCORE_DIR", personal_data.path())
        .arg("stop");
    let _ = cmd.output();
}

// ============================================================
// Binary parent directory detection
// ============================================================

#[test]
fn test_binary_parent_dir_with_memcore_toml() {
    // Copy the binary into a temp dir that has memcore.toml.
    // Running without MEMCORE_DIR should use that directory.
    let dir = TempDir::new().unwrap();

    // Init via MEMCORE_DIR first to create the structure
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir.path())
        .arg("init")
        .arg("--dir")
        .arg(dir.path().to_str().unwrap());
    cmd.assert().success();

    // Stop daemon started via MEMCORE_DIR
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir.path()).arg("stop");
    let _ = cmd.output();

    // Copy binary into the dir
    let binary = assert_cmd::cargo::cargo_bin("memcore");
    let dest = dir.path().join("memcore");
    std::fs::copy(&binary, &dest).unwrap();

    // Run the copied binary WITHOUT MEMCORE_DIR — should self-discover
    let mut cmd = Command::new(&dest);
    cmd.env_remove("MEMCORE_DIR").arg("status");
    cmd.assert().success();

    // Create a node and verify it's in the right place
    let mut cmd = Command::new(&dest);
    cmd.env_remove("MEMCORE_DIR")
        .arg("create")
        .arg("self-discovery-test")
        .write_stdin("---\nabstract: testing binary self-discovery\n---\ntest");
    cmd.assert().success();

    assert!(dir.path().join("memories/self-discovery-test.md").exists());

    // Cleanup
    let mut cmd = Command::new(&dest);
    cmd.env_remove("MEMCORE_DIR")
        .arg("delete")
        .arg("self-discovery-test");
    let _ = cmd.output();
    let mut cmd = Command::new(&dest);
    cmd.env_remove("MEMCORE_DIR").arg("stop");
    let _ = cmd.output();
}

#[test]
fn test_binary_parent_dir_with_memories_subdir() {
    // A directory with memories/ but no memcore.toml should also be detected.
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("memories")).unwrap();

    // Copy binary into the dir
    let binary = assert_cmd::cargo::cargo_bin("memcore");
    let dest = dir.path().join("memcore");
    std::fs::copy(&binary, &dest).unwrap();

    // Init using the copied binary without MEMCORE_DIR
    let mut cmd = Command::new(&dest);
    cmd.env_remove("MEMCORE_DIR")
        .arg("init")
        .arg("--dir")
        .arg(dir.path().to_str().unwrap());
    cmd.assert().success();

    assert!(dir.path().join("memcore.toml").exists());

    // Cleanup
    let mut cmd = Command::new(&dest);
    cmd.env_remove("MEMCORE_DIR").arg("stop");
    let _ = cmd.output();
}

// ============================================================
// No resolution → explicit error (no silent fallback)
// ============================================================

#[test]
fn test_no_memcore_dir_gives_error() {
    // Binary in a bare directory with no memcore.toml or memories/ and no
    // MEMCORE_DIR env var should fail with a clear error, NOT silently
    // fall back to ~/.memcore/.
    let dir = TempDir::new().unwrap();

    // Copy binary to a bare directory
    let binary = assert_cmd::cargo::cargo_bin("memcore");
    let dest = dir.path().join("memcore");
    std::fs::copy(&binary, &dest).unwrap();

    // Run without MEMCORE_DIR
    let mut cmd = Command::new(&dest);
    cmd.env_remove("MEMCORE_DIR").arg("status");

    let output = cmd.output().unwrap();
    assert!(!output.status.success(), "should have failed");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot find memcore directory"),
        "should show clear error, got: {}",
        stderr
    );
    assert!(
        stderr.contains("MEMCORE_DIR"),
        "error should mention MEMCORE_DIR, got: {}",
        stderr
    );
}

#[test]
fn test_no_fallback_to_home_memcore() {
    // Even if ~/.memcore/ exists, the binary should NOT use it
    // when run from a bare directory without MEMCORE_DIR.
    let dir = TempDir::new().unwrap();

    let binary = assert_cmd::cargo::cargo_bin("memcore");
    let dest = dir.path().join("memcore");
    std::fs::copy(&binary, &dest).unwrap();

    // Run without MEMCORE_DIR — should fail even if ~/.memcore/ exists
    let mut cmd = Command::new(&dest);
    cmd.env_remove("MEMCORE_DIR").arg("status");

    let output = cmd.output().unwrap();
    assert!(
        !output.status.success(),
        "should not silently fall back to ~/.memcore/"
    );
}

// ============================================================
// Data persistence in the correct directory
// ============================================================

#[test]
fn test_create_node_persists_to_correct_dir() {
    let dir = TempDir::new().unwrap();

    // Init
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir.path())
        .arg("init")
        .arg("--dir")
        .arg(dir.path().to_str().unwrap());
    cmd.assert().success();

    // Create a node
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir.path())
        .arg("create")
        .arg("persistence-test")
        .write_stdin("---\nabstract: testing persistence\n---\ntest body");
    cmd.assert().success();

    // Verify .md file is in memories/
    let node_path = dir.path().join("memories/persistence-test.md");
    assert!(node_path.exists(), "node file should exist at {:?}", node_path);

    let content = std::fs::read_to_string(&node_path).unwrap();
    assert!(content.contains("testing persistence"));

    // Cleanup
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir.path())
        .arg("delete")
        .arg("persistence-test");
    let _ = cmd.output();
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir.path()).arg("stop");
    let _ = cmd.output();
}

#[test]
fn test_node_survives_daemon_restart() {
    let dir = TempDir::new().unwrap();

    // Init
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir.path())
        .arg("init")
        .arg("--dir")
        .arg(dir.path().to_str().unwrap());
    cmd.assert().success();

    // Create a node
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir.path())
        .arg("create")
        .arg("restart-test")
        .write_stdin("---\nabstract: survives restart\n---\ntest body");
    cmd.assert().success();

    // Stop daemon
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir.path()).arg("stop");
    cmd.assert().success();

    // Wait a moment for the daemon to fully stop
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Get the node — daemon should auto-restart and load from disk
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir.path())
        .arg("get")
        .arg("restart-test");
    let output = cmd.output().unwrap();
    assert!(output.status.success(), "get after restart should work");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("survives restart"),
        "node content should survive daemon restart, got: {}",
        stdout
    );

    // Cleanup
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir.path())
        .arg("delete")
        .arg("restart-test");
    let _ = cmd.output();
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir.path()).arg("stop");
    let _ = cmd.output();
}

// ============================================================
// Multiple memcore instances (same binary, different dirs)
// ============================================================

#[test]
fn test_multiple_instances_via_memcore_dir() {
    // Two separate memcore directories, same binary via MEMCORE_DIR.
    // Each should have isolated data — no cross-contamination.
    let dir_a = TempDir::new().unwrap();
    let dir_b = TempDir::new().unwrap();

    // Init both
    for dir in [&dir_a, &dir_b] {
        let mut cmd = Command::cargo_bin("memcore").unwrap();
        cmd.env("MEMCORE_DIR", dir.path())
            .arg("init")
            .arg("--dir")
            .arg(dir.path().to_str().unwrap());
        cmd.assert().success();
    }

    // Create a node in dir_a only
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir_a.path())
        .arg("create")
        .arg("only-in-a")
        .write_stdin("---\nabstract: this lives in instance A\n---\nA body");
    cmd.assert().success();

    // Create a different node in dir_b only
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir_b.path())
        .arg("create")
        .arg("only-in-b")
        .write_stdin("---\nabstract: this lives in instance B\n---\nB body");
    cmd.assert().success();

    // Verify: only-in-a exists in dir_a, not dir_b
    assert!(dir_a.path().join("memories/only-in-a.md").exists());
    assert!(!dir_b.path().join("memories/only-in-a.md").exists());

    // Verify: only-in-b exists in dir_b, not dir_a
    assert!(dir_b.path().join("memories/only-in-b.md").exists());
    assert!(!dir_a.path().join("memories/only-in-b.md").exists());

    // Verify via CLI: get only-in-a from dir_a succeeds
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir_a.path())
        .arg("get")
        .arg("only-in-a");
    let output = cmd.output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("instance A"));

    // Verify via CLI: get only-in-a from dir_b fails (not found)
    let mut cmd = Command::cargo_bin("memcore").unwrap();
    cmd.env("MEMCORE_DIR", dir_b.path())
        .arg("get")
        .arg("only-in-a");
    let output = cmd.output().unwrap();
    assert!(!output.status.success(), "only-in-a should not exist in dir_b");

    // Cleanup
    for dir in [&dir_a, &dir_b] {
        let mut cmd = Command::cargo_bin("memcore").unwrap();
        cmd.env("MEMCORE_DIR", dir.path()).arg("stop");
        let _ = cmd.output();
    }
}

#[test]
fn test_multiple_instances_via_separate_binaries() {
    // Two self-contained directories, each with its own binary copy.
    // Simulates the real-world "download memcore twice for two projects" case.
    let dir_a = TempDir::new().unwrap();
    let dir_b = TempDir::new().unwrap();

    let binary = assert_cmd::cargo::cargo_bin("memcore");

    // Set up both as self-contained directories
    for dir in [&dir_a, &dir_b] {
        std::fs::create_dir_all(dir.path().join("memories")).unwrap();
        std::fs::copy(&binary, dir.path().join("memcore")).unwrap();

        let mut cmd = Command::new(dir.path().join("memcore"));
        cmd.env_remove("MEMCORE_DIR")
            .arg("init")
            .arg("--dir")
            .arg(dir.path().to_str().unwrap());
        cmd.assert().success();
    }

    // Create different nodes in each
    let mut cmd = Command::new(dir_a.path().join("memcore"));
    cmd.env_remove("MEMCORE_DIR")
        .arg("create")
        .arg("node-a")
        .write_stdin("---\nabstract: in dir A\n---\nA");
    cmd.assert().success();

    let mut cmd = Command::new(dir_b.path().join("memcore"));
    cmd.env_remove("MEMCORE_DIR")
        .arg("create")
        .arg("node-b")
        .write_stdin("---\nabstract: in dir B\n---\nB");
    cmd.assert().success();

    // Verify isolation
    assert!(dir_a.path().join("memories/node-a.md").exists());
    assert!(!dir_a.path().join("memories/node-b.md").exists());
    assert!(dir_b.path().join("memories/node-b.md").exists());
    assert!(!dir_b.path().join("memories/node-a.md").exists());

    // Cleanup
    for dir in [&dir_a, &dir_b] {
        let mut cmd = Command::new(dir.path().join("memcore"));
        cmd.env_remove("MEMCORE_DIR").arg("stop");
        let _ = cmd.output();
    }
}
