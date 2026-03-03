use std::collections::HashMap;
use std::path::Path;

use memcore::config::Config;
use memcore::graph::{serialize_graph_idx, Graph};
use memcore::index::VectorIndex;
use memcore::name_index::NameIndex;
use memcore::node::{Frontmatter, NodeMeta, serialize_node, write_node_to_dir, parse_node_file};
use memcore::wal::{WalOp, WalWriter};

// We need to test daemon state loading, which is essentially:
// 1. Scan memories/*.md → build NameIndex, NodeMeta cache, Graph
// 2. Load or rebuild graph.idx
// 3. WAL recovery
// 4. Consistency check (bidirectional repair, dangling reference cleanup)
//
// We test via the load_state_from_dir function which we'll add to lib.rs

use memcore::daemon_state::{load_state_from_dir, DaemonState};

// ============================================================
// Helper: create a node file in a temp dir
// ============================================================

fn write_test_node(memories_dir: &Path, name: &str, links: Vec<String>, abstract_text: &str) {
    let fm = Frontmatter::new_for_create(links, false, abstract_text.to_string());
    write_node_to_dir(memories_dir, name, &fm, "body text").unwrap();
}

fn write_test_node_with_weight(
    memories_dir: &Path,
    name: &str,
    links: Vec<String>,
    abstract_text: &str,
    weight: f32,
    pinned: bool,
) {
    use chrono::Utc;
    let fm = Frontmatter {
        created: Utc::now(),
        updated: Utc::now(),
        weight,
        last_accessed: Utc::now(),
        access_count: 0,
        pinned,
        links,
        abstract_text: abstract_text.to_string(),
    };
    write_node_to_dir(memories_dir, name, &fm, "body text").unwrap();
}

// ============================================================
// Basic loading
// ============================================================

#[test]
fn test_load_empty_dir() {
    let dir = tempfile::tempdir().unwrap();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    let state = load_state_from_dir(dir.path()).unwrap();
    assert_eq!(state.name_index.len(), 0);
    assert_eq!(state.graph.node_count(), 0);
    assert_eq!(state.graph.edge_count(), 0);
    assert!(state.node_metas.is_empty());
}

#[test]
fn test_load_single_node() {
    let dir = tempfile::tempdir().unwrap();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    write_test_node(&memories_dir, "my-node", vec![], "test abstract");

    let state = load_state_from_dir(dir.path()).unwrap();
    assert_eq!(state.name_index.len(), 1);
    assert!(state.name_index.contains("my-node"));
    assert!(state.node_metas.contains_key("my-node"));
    assert_eq!(state.node_metas["my-node"].abstract_text, "test abstract");
}

#[test]
fn test_load_multiple_nodes() {
    let dir = tempfile::tempdir().unwrap();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    write_test_node(&memories_dir, "alpha", vec![], "abstract a");
    write_test_node(&memories_dir, "beta", vec![], "abstract b");
    write_test_node(&memories_dir, "gamma", vec![], "abstract c");

    let state = load_state_from_dir(dir.path()).unwrap();
    assert_eq!(state.name_index.len(), 3);
    assert_eq!(state.node_metas.len(), 3);
}

// ============================================================
// Graph reconstruction from links
// ============================================================

#[test]
fn test_load_builds_graph_from_links() {
    let dir = tempfile::tempdir().unwrap();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    write_test_node(&memories_dir, "node-a", vec!["node-b".into()], "a");
    write_test_node(&memories_dir, "node-b", vec!["node-a".into()], "b");

    let state = load_state_from_dir(dir.path()).unwrap();
    assert!(state.graph.has_edge("node-a", "node-b"));
    assert!(state.graph.has_edge("node-b", "node-a"));
    assert_eq!(state.graph.edge_count(), 1);
}

#[test]
fn test_load_multiple_edges() {
    let dir = tempfile::tempdir().unwrap();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    // Star topology: center links to a, b, c
    write_test_node(
        &memories_dir,
        "center",
        vec!["node-a".into(), "node-b".into(), "node-c".into()],
        "center",
    );
    write_test_node(&memories_dir, "node-a", vec!["center".into()], "a");
    write_test_node(&memories_dir, "node-b", vec!["center".into()], "b");
    write_test_node(&memories_dir, "node-c", vec!["center".into()], "c");

    let state = load_state_from_dir(dir.path()).unwrap();
    assert_eq!(state.graph.edge_count(), 3);
    assert!(state.graph.has_edge("center", "node-a"));
    assert!(state.graph.has_edge("center", "node-b"));
    assert!(state.graph.has_edge("center", "node-c"));
}

// ============================================================
// Consistency: dangling reference cleanup
// ============================================================

#[test]
fn test_load_removes_dangling_links() {
    let dir = tempfile::tempdir().unwrap();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    // node-a links to node-b which doesn't exist
    write_test_node(&memories_dir, "node-a", vec!["nonexistent".into()], "a");

    let state = load_state_from_dir(dir.path()).unwrap();
    // Dangling link should be ignored in graph
    assert_eq!(state.graph.edge_count(), 0);
    // The node should still be loaded
    assert!(state.name_index.contains("node-a"));
}

// ============================================================
// Consistency: bidirectional repair
// ============================================================

#[test]
fn test_load_repairs_unidirectional_links() {
    let dir = tempfile::tempdir().unwrap();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    // node-a links to node-b, but node-b does NOT link back
    write_test_node(&memories_dir, "node-a", vec!["node-b".into()], "a");
    write_test_node(&memories_dir, "node-b", vec![], "b");

    let state = load_state_from_dir(dir.path()).unwrap();
    // Should still create the edge (repair unidirectional → bidirectional)
    assert!(state.graph.has_edge("node-a", "node-b"));
    assert!(state.graph.has_edge("node-b", "node-a"));
    assert_eq!(state.graph.edge_count(), 1);
}

// ============================================================
// NodeMeta fields
// ============================================================

#[test]
fn test_load_preserves_weight_and_pinned() {
    let dir = tempfile::tempdir().unwrap();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    write_test_node_with_weight(&memories_dir, "pinned-node", vec![], "important", 0.5, true);
    write_test_node_with_weight(&memories_dir, "normal-node", vec![], "regular", 0.8, false);

    let state = load_state_from_dir(dir.path()).unwrap();
    assert!(state.node_metas["pinned-node"].pinned);
    assert!(!state.node_metas["normal-node"].pinned);
    assert!((state.node_metas["pinned-node"].weight - 0.5).abs() < 1e-6);
    assert!((state.node_metas["normal-node"].weight - 0.8).abs() < 1e-6);
}

// ============================================================
// WAL recovery
// ============================================================

#[test]
fn test_load_with_uncommitted_create_rolls_back() {
    let dir = tempfile::tempdir().unwrap();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    // Simulate: WAL says CREATE ghost-node was started but not committed
    // And the file exists (partially completed create)
    write_test_node(&memories_dir, "ghost-node", vec![], "ghost");
    write_test_node(&memories_dir, "real-node", vec![], "real");

    let mut wal = WalWriter::at(dir.path().join("wal.log"));
    wal.begin(&WalOp::Create("ghost-node".into())).unwrap();
    // No commit!

    let state = load_state_from_dir(dir.path()).unwrap();
    // ghost-node should be rolled back (deleted)
    assert!(!state.name_index.contains("ghost-node"));
    assert!(!state.node_metas.contains_key("ghost-node"));
    // real-node should still exist
    assert!(state.name_index.contains("real-node"));
}

#[test]
fn test_load_with_committed_create_keeps_node() {
    let dir = tempfile::tempdir().unwrap();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    write_test_node(&memories_dir, "good-node", vec![], "good");

    let mut wal = WalWriter::at(dir.path().join("wal.log"));
    let tx = wal.begin(&WalOp::Create("good-node".into())).unwrap();
    wal.commit(&tx).unwrap();

    let state = load_state_from_dir(dir.path()).unwrap();
    assert!(state.name_index.contains("good-node"));
}

#[test]
fn test_load_with_uncommitted_delete_keeps_node() {
    let dir = tempfile::tempdir().unwrap();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    // File still exists because delete wasn't completed
    write_test_node(&memories_dir, "survivor", vec![], "still here");

    let mut wal = WalWriter::at(dir.path().join("wal.log"));
    wal.begin(&WalOp::Delete("survivor".into())).unwrap();
    // No commit — rollback: keep the file

    let state = load_state_from_dir(dir.path()).unwrap();
    assert!(state.name_index.contains("survivor"));
}

#[test]
fn test_load_clears_wal_after_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    write_test_node(&memories_dir, "node-a", vec![], "a");

    let wal_path = dir.path().join("wal.log");
    let mut wal = WalWriter::at(wal_path.clone());
    let tx = wal.begin(&WalOp::Create("node-a".into())).unwrap();
    wal.commit(&tx).unwrap();

    let _state = load_state_from_dir(dir.path()).unwrap();

    // WAL should be cleared after successful recovery
    let content = std::fs::read_to_string(&wal_path).unwrap_or_default();
    assert!(content.is_empty(), "WAL should be cleared after recovery");
}

// ============================================================
// graph.idx loading
// ============================================================

#[test]
fn test_load_uses_graph_idx_when_present() {
    let dir = tempfile::tempdir().unwrap();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    write_test_node(&memories_dir, "node-a", vec!["node-b".into()], "a");
    write_test_node(&memories_dir, "node-b", vec!["node-a".into()], "b");

    // Pre-build graph.idx
    let mut graph = Graph::new();
    graph.add_edge("node-a", "node-b");
    let idx_bytes = serialize_graph_idx(&graph);
    std::fs::write(dir.path().join("graph.idx"), &idx_bytes).unwrap();

    let state = load_state_from_dir(dir.path()).unwrap();
    assert!(state.graph.has_edge("node-a", "node-b"));
    assert_eq!(state.graph.edge_count(), 1);
}

#[test]
fn test_load_rebuilds_graph_when_no_idx() {
    let dir = tempfile::tempdir().unwrap();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    write_test_node(&memories_dir, "node-a", vec!["node-b".into()], "a");
    write_test_node(&memories_dir, "node-b", vec!["node-a".into()], "b");
    // No graph.idx file

    let state = load_state_from_dir(dir.path()).unwrap();
    assert!(state.graph.has_edge("node-a", "node-b"));
    assert_eq!(state.graph.edge_count(), 1);
}

// ============================================================
// Edge cases
// ============================================================

#[test]
fn test_load_skips_non_md_files() {
    let dir = tempfile::tempdir().unwrap();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    write_test_node(&memories_dir, "valid-node", vec![], "ok");
    std::fs::write(memories_dir.join("readme.txt"), "not a node").unwrap();
    std::fs::write(memories_dir.join(".hidden"), "not a node").unwrap();

    let state = load_state_from_dir(dir.path()).unwrap();
    assert_eq!(state.name_index.len(), 1);
    assert!(state.name_index.contains("valid-node"));
}

#[test]
fn test_load_skips_corrupt_md_files() {
    let dir = tempfile::tempdir().unwrap();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    write_test_node(&memories_dir, "good-node", vec![], "ok");
    // Write a corrupt .md file (no valid frontmatter)
    std::fs::write(memories_dir.join("corrupt-node.md"), "no frontmatter here").unwrap();

    let state = load_state_from_dir(dir.path()).unwrap();
    assert_eq!(state.name_index.len(), 1);
    assert!(state.name_index.contains("good-node"));
    assert!(!state.name_index.contains("corrupt-node"));
}

#[test]
fn test_load_creates_memories_dir_if_missing() {
    let dir = tempfile::tempdir().unwrap();
    // Don't create memories/ dir

    let state = load_state_from_dir(dir.path()).unwrap();
    assert_eq!(state.name_index.len(), 0);
    // memories dir should now exist
    assert!(dir.path().join("memories").exists());
}
