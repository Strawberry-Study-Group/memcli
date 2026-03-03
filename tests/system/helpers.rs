use assert_cmd::Command;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A test environment with an isolated memcore directory and daemon lifecycle.
pub struct TestEnv {
    pub dir: TempDir,
}

impl TestEnv {
    /// Create a new isolated test environment and run `memcore init`.
    pub fn new() -> Self {
        let dir = TempDir::new().expect("failed to create temp dir");
        // Run init to set up directory structure
        let mut cmd = Self::memcore_at(dir.path());
        cmd.arg("init")
            .arg("--dir")
            .arg(dir.path().to_str().unwrap());
        let output = cmd.output().expect("failed to run memcore init");
        assert!(
            output.status.success(),
            "init failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Self { dir }
    }

    /// Get a Command pre-configured with MEMCORE_DIR pointing to this env.
    pub fn cmd(&self) -> Command {
        Self::memcore_at(self.dir.path())
    }

    /// Build a memcore Command targeting a specific directory.
    fn memcore_at(dir: &Path) -> Command {
        let mut cmd = Command::cargo_bin("memcore").expect("binary not found");
        cmd.env("MEMCORE_DIR", dir);
        cmd
    }

    /// Path to the memories directory.
    pub fn memories_dir(&self) -> PathBuf {
        self.dir.path().join("memories")
    }

    /// Write a valid .md node content string with given abstract, links and body.
    pub fn make_content(abstract_text: &str, links: &[&str], body: &str) -> String {
        let links_yaml = if links.is_empty() {
            "links: []".to_string()
        } else {
            let items: Vec<String> = links.iter().map(|l| format!("- {}", l)).collect();
            format!("links:\n{}", items.join("\n"))
        };
        format!(
            "---\ncreated: '2025-01-01T00:00:00Z'\nupdated: '2025-01-01T00:00:00Z'\nweight: 1.0\nlast_accessed: '2025-01-01T00:00:00Z'\naccess_count: 0\npinned: false\n{}\nabstract: {}\n---\n\n{}",
            links_yaml, abstract_text, body
        )
    }

    /// Write content to a temp file and return the path.
    pub fn write_content_file(&self, name: &str, content: &str) -> PathBuf {
        let path = self.dir.path().join(format!("{}.input.md", name));
        std::fs::write(&path, content).expect("failed to write content file");
        path
    }

    /// Write minimal content: only abstract + body, all other fields defaulted.
    /// This is the agent-friendly input format.
    pub fn make_minimal_content(abstract_text: &str, body: &str) -> String {
        format!(
            "---\nabstract: '{}'\n---\n\n{}",
            abstract_text.replace('\'', "''"), body
        )
    }

    /// Stop the daemon for this environment (best-effort).
    pub fn stop_daemon(&self) {
        let _ = self.cmd().arg("stop").output();
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        self.stop_daemon();
    }
}
