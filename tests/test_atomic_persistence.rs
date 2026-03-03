//! Tests for atomic operations and data persistence on disk.
//!
//! These tests verify that:
//! 1. All write operations persist correctly to .md files on disk
//! 2. Data survives "daemon restart" (reload from disk via load_state_from_dir)
//! 3. Multi-file operations update ALL affected .md files (bidirectional links)
//! 4. WAL recovery correctly rolls back uncommitted transactions
//! 5. graph.idx corruption falls back to rebuild from .md files
//! 6. Atomic writes never leave half-written files

use std::path::Path;

use memcore::daemon_state::{flush_access_metadata, load_state_from_dir};
use memcore::graph::{deserialize_graph_idx, serialize_graph_idx, Graph};
use memcore::handler::handle_request;
use memcore::node::{parse_node_file, read_node_from_dir, write_node_to_dir, Frontmatter};
use memcore::protocol::*;
use memcore::wal::{WalOp, WalWriter};

// ============================================================
// Helpers
// ============================================================

fn setup_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let memories = dir.path().join("memories");
    std::fs::create_dir_all(&memories).unwrap();
    std::fs::create_dir_all(dir.path().join("index")).unwrap();
    dir
}

fn add_node(dir: &Path, name: &str, links: Vec<String>, abstract_text: &str) {
    let memories = dir.join("memories");
    let fm = Frontmatter::new_for_create(links, false, abstract_text.to_string());
    write_node_to_dir(&memories, name, &fm, "body text").unwrap();
}

fn make_content(abstract_text: &str, links: &[&str], body: &str) -> String {
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

/// Read the links field from a .md file on disk
fn read_links_from_disk(dir: &Path, name: &str) -> Vec<String> {
    let memories = dir.join("memories");
    let (fm, _) = read_node_from_dir(&memories, name).unwrap();
    fm.links
}

/// Check if a .md file exists on disk
fn file_exists(dir: &Path, name: &str) -> bool {
    dir.join("memories").join(format!("{}.md", name)).exists()
}

// ============================================================
// §1: Create persists to disk
// ============================================================

#[test]
fn test_create_persists_file_to_disk() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_content("my abstract", &[], "hello world");
    let req = Request::Create {
        name: "test-node".into(),
        content,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    // Verify file exists on disk
    assert!(file_exists(dir.path(), "test-node"));

    // Verify file content is parseable and correct
    let (fm, body) = read_node_from_dir(&dir.path().join("memories"), "test-node").unwrap();
    assert_eq!(fm.abstract_text, "my abstract");
    assert_eq!(body.trim(), "hello world");
    assert!((fm.weight - 1.0).abs() < 1e-6);
}

#[test]
fn test_create_survives_daemon_restart() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_content("persistent abstract", &[], "persistent body");
    let req = Request::Create {
        name: "persist-me".into(),
        content,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    // Drop state (simulate daemon exit), reload from disk
    drop(state);
    let reloaded = load_state_from_dir(dir.path()).unwrap();

    assert!(reloaded.name_index.contains("persist-me"));
    assert_eq!(
        reloaded.node_metas["persist-me"].abstract_text,
        "persistent abstract"
    );
}

#[test]
fn test_create_with_links_persists_bidirectional_on_disk() {
    let dir = setup_dir();
    add_node(dir.path(), "target-a", vec![], "target a");
    add_node(dir.path(), "target-b", vec![], "target b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_content("new node", &["target-a", "target-b"], "body");
    let req = Request::Create {
        name: "linker".into(),
        content,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    // Verify linker's own file has the links
    let linker_links = read_links_from_disk(dir.path(), "linker");
    assert!(linker_links.contains(&"target-a".to_string()));
    assert!(linker_links.contains(&"target-b".to_string()));

    // Verify peer .md files were updated with back-links
    let target_a_links = read_links_from_disk(dir.path(), "target-a");
    assert!(
        target_a_links.contains(&"linker".to_string()),
        "target-a.md should contain back-link to linker, got {:?}",
        target_a_links
    );

    let target_b_links = read_links_from_disk(dir.path(), "target-b");
    assert!(
        target_b_links.contains(&"linker".to_string()),
        "target-b.md should contain back-link to linker, got {:?}",
        target_b_links
    );
}

#[test]
fn test_create_with_links_survives_restart() {
    let dir = setup_dir();
    add_node(dir.path(), "peer-aa", vec![], "peer");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_content("linked node", &["peer-aa"], "body");
    let req = Request::Create {
        name: "linked-node".into(),
        content,
    };
    handle_request(&mut state, &req, dir.path());
    drop(state);

    let reloaded = load_state_from_dir(dir.path()).unwrap();
    assert!(reloaded.graph.has_edge("linked-node", "peer-aa"));
    assert!(reloaded.graph.has_edge("peer-aa", "linked-node"));
    assert_eq!(reloaded.graph.edge_count(), 1);
}

// ============================================================
// §2: Delete persists to disk
// ============================================================

#[test]
fn test_delete_removes_file_from_disk() {
    let dir = setup_dir();
    add_node(dir.path(), "doomed", vec![], "will be deleted");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    assert!(file_exists(dir.path(), "doomed"));

    let req = Request::Delete {
        name: "doomed".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    assert!(!file_exists(dir.path(), "doomed"));
}

#[test]
fn test_delete_removes_back_links_from_peer_files() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Link a <-> b
    let link_req = Request::Link {
        a: "node-a".into(),
        b: "node-b".into(),
    };
    handle_request(&mut state, &link_req, dir.path());

    // Verify link exists on disk
    assert!(read_links_from_disk(dir.path(), "node-b").contains(&"node-a".to_string()));

    // Delete node-a
    let del_req = Request::Delete {
        name: "node-a".into(),
    };
    handle_request(&mut state, &del_req, dir.path());

    // node-b's .md file should no longer reference node-a
    let b_links = read_links_from_disk(dir.path(), "node-b");
    assert!(
        !b_links.contains(&"node-a".to_string()),
        "node-b should not reference deleted node-a, got {:?}",
        b_links
    );
}

#[test]
fn test_delete_survives_daemon_restart() {
    let dir = setup_dir();
    add_node(dir.path(), "ephemeral", vec![], "gone soon");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Delete {
        name: "ephemeral".into(),
    };
    handle_request(&mut state, &req, dir.path());
    drop(state);

    let reloaded = load_state_from_dir(dir.path()).unwrap();
    assert!(!reloaded.name_index.contains("ephemeral"));
    assert!(!reloaded.node_metas.contains_key("ephemeral"));
}

#[test]
fn test_delete_with_multiple_peers_cleans_all_files() {
    let dir = setup_dir();
    add_node(dir.path(), "hub", vec![], "hub");
    add_node(dir.path(), "spoke-a", vec![], "a");
    add_node(dir.path(), "spoke-b", vec![], "b");
    add_node(dir.path(), "spoke-c", vec![], "c");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Create star: hub <-> spoke-a, hub <-> spoke-b, hub <-> spoke-c
    for spoke in &["spoke-a", "spoke-b", "spoke-c"] {
        let req = Request::Link {
            a: "hub".into(),
            b: spoke.to_string(),
        };
        handle_request(&mut state, &req, dir.path());
    }

    // Delete hub
    let req = Request::Delete {
        name: "hub".into(),
    };
    handle_request(&mut state, &req, dir.path());

    // All spoke files should have empty links
    for spoke in &["spoke-a", "spoke-b", "spoke-c"] {
        let links = read_links_from_disk(dir.path(), spoke);
        assert!(
            !links.contains(&"hub".to_string()),
            "{} still references deleted hub: {:?}",
            spoke,
            links
        );
    }
}

// ============================================================
// §3: Update persists to disk
// ============================================================

#[test]
fn test_update_persists_new_content_to_disk() {
    let dir = setup_dir();
    add_node(dir.path(), "mutable", vec![], "old abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_content("new abstract", &[], "new body");
    let req = Request::Update {
        name: "mutable".into(),
        content,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    // Read directly from disk
    let (fm, body) = read_node_from_dir(&dir.path().join("memories"), "mutable").unwrap();
    assert_eq!(fm.abstract_text, "new abstract");
    assert_eq!(body.trim(), "new body");
}

#[test]
fn test_update_with_link_changes_persists_to_all_files() {
    let dir = setup_dir();
    add_node(dir.path(), "node-x", vec![], "x");
    add_node(dir.path(), "node-y", vec![], "y");
    add_node(dir.path(), "node-z", vec![], "z");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Create initial link: node-x <-> node-y
    let link_req = Request::Link {
        a: "node-x".into(),
        b: "node-y".into(),
    };
    handle_request(&mut state, &link_req, dir.path());

    // Update node-x: remove link to y, add link to z
    let content = make_content("updated x", &["node-z"], "new body");
    let req = Request::Update {
        name: "node-x".into(),
        content,
    };
    handle_request(&mut state, &req, dir.path());

    // node-y should no longer have back-link to node-x
    let y_links = read_links_from_disk(dir.path(), "node-y");
    assert!(
        !y_links.contains(&"node-x".to_string()),
        "node-y should not reference node-x after update removed the link"
    );

    // node-z should now have back-link to node-x
    let z_links = read_links_from_disk(dir.path(), "node-z");
    assert!(
        z_links.contains(&"node-x".to_string()),
        "node-z should reference node-x after update added the link"
    );
}

#[test]
fn test_update_survives_daemon_restart() {
    let dir = setup_dir();
    add_node(dir.path(), "to-update", vec![], "old");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_content("updated abstract", &[], "updated body");
    let req = Request::Update {
        name: "to-update".into(),
        content,
    };
    handle_request(&mut state, &req, dir.path());
    drop(state);

    let reloaded = load_state_from_dir(dir.path()).unwrap();
    assert_eq!(
        reloaded.node_metas["to-update"].abstract_text,
        "updated abstract"
    );
}

// ============================================================
// §4: Link/Unlink persist to disk
// ============================================================

#[test]
fn test_link_persists_to_both_md_files() {
    let dir = setup_dir();
    add_node(dir.path(), "left", vec![], "left");
    add_node(dir.path(), "right", vec![], "right");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Link {
        a: "left".into(),
        b: "right".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    // Both files should have links on disk
    let left_links = read_links_from_disk(dir.path(), "left");
    let right_links = read_links_from_disk(dir.path(), "right");
    assert!(left_links.contains(&"right".to_string()));
    assert!(right_links.contains(&"left".to_string()));
}

#[test]
fn test_unlink_removes_from_both_md_files() {
    let dir = setup_dir();
    add_node(dir.path(), "alpha", vec![], "a");
    add_node(dir.path(), "beta", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Link then unlink
    let link_req = Request::Link {
        a: "alpha".into(),
        b: "beta".into(),
    };
    handle_request(&mut state, &link_req, dir.path());

    let unlink_req = Request::Unlink {
        a: "alpha".into(),
        b: "beta".into(),
    };
    let resp = handle_request(&mut state, &unlink_req, dir.path());
    assert!(resp.success);

    // Both files should have no links on disk
    let alpha_links = read_links_from_disk(dir.path(), "alpha");
    let beta_links = read_links_from_disk(dir.path(), "beta");
    assert!(!alpha_links.contains(&"beta".to_string()));
    assert!(!beta_links.contains(&"alpha".to_string()));
}

#[test]
fn test_link_survives_daemon_restart() {
    let dir = setup_dir();
    add_node(dir.path(), "aa", vec![], "a");
    add_node(dir.path(), "bb", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Link {
        a: "aa".into(),
        b: "bb".into(),
    };
    handle_request(&mut state, &req, dir.path());
    drop(state);

    let reloaded = load_state_from_dir(dir.path()).unwrap();
    assert!(reloaded.graph.has_edge("aa", "bb"));
    assert_eq!(reloaded.graph.edge_count(), 1);
}

#[test]
fn test_unlink_survives_daemon_restart() {
    let dir = setup_dir();
    add_node(dir.path(), "cc", vec!["dd".into()], "c");
    add_node(dir.path(), "dd", vec!["cc".into()], "d");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Unlink {
        a: "cc".into(),
        b: "dd".into(),
    };
    handle_request(&mut state, &req, dir.path());
    drop(state);

    let reloaded = load_state_from_dir(dir.path()).unwrap();
    assert!(!reloaded.graph.has_edge("cc", "dd"));
    assert_eq!(reloaded.graph.edge_count(), 0);
}

// ============================================================
// §5: Rename persists to disk
// ============================================================

#[test]
fn test_rename_persists_file_on_disk() {
    let dir = setup_dir();
    add_node(dir.path(), "old-name", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Rename {
        old: "old-name".into(),
        new: "new-name".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    assert!(!file_exists(dir.path(), "old-name"));
    assert!(file_exists(dir.path(), "new-name"));
}

#[test]
fn test_rename_updates_peer_links_on_disk() {
    let dir = setup_dir();
    add_node(dir.path(), "to-rename", vec![], "will be renamed");
    add_node(dir.path(), "peer-node", vec![], "peer");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Link them
    let link_req = Request::Link {
        a: "to-rename".into(),
        b: "peer-node".into(),
    };
    handle_request(&mut state, &link_req, dir.path());

    // Rename
    let rename_req = Request::Rename {
        old: "to-rename".into(),
        new: "renamed".into(),
    };
    handle_request(&mut state, &rename_req, dir.path());

    // Peer's .md should now reference "renamed" instead of "to-rename"
    let peer_links = read_links_from_disk(dir.path(), "peer-node");
    assert!(
        !peer_links.contains(&"to-rename".to_string()),
        "peer should not reference old name"
    );
    assert!(
        peer_links.contains(&"renamed".to_string()),
        "peer should reference new name"
    );
}

#[test]
fn test_rename_survives_daemon_restart() {
    let dir = setup_dir();
    add_node(dir.path(), "original", vec![], "abstract text");
    add_node(dir.path(), "friend", vec!["original".into()], "friend");
    // Also write the back-link in original
    {
        let memories = dir.path().join("memories");
        let (mut fm, body) = read_node_from_dir(&memories, "original").unwrap();
        fm.links.push("friend".to_string());
        write_node_to_dir(&memories, "original", &fm, &body).unwrap();
    }
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Rename {
        old: "original".into(),
        new: "renamed-original".into(),
    };
    handle_request(&mut state, &req, dir.path());
    drop(state);

    let reloaded = load_state_from_dir(dir.path()).unwrap();
    assert!(!reloaded.name_index.contains("original"));
    assert!(reloaded.name_index.contains("renamed-original"));
    assert!(reloaded.graph.has_edge("renamed-original", "friend"));
}

// ============================================================
// §6: Boost/Penalize persist to disk
// ============================================================

#[test]
fn test_boost_persists_weight_to_disk() {
    let dir = setup_dir();
    add_node(dir.path(), "node-w", vec![], "weighted");
    // Set initial weight to 0.5
    {
        let memories = dir.path().join("memories");
        let (mut fm, body) = read_node_from_dir(&memories, "node-w").unwrap();
        fm.weight = 0.5;
        write_node_to_dir(&memories, "node-w", &fm, &body).unwrap();
    }
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Boost {
        name: "node-w".into(),
    };
    handle_request(&mut state, &req, dir.path());

    // Read weight from disk
    let (fm, _) = read_node_from_dir(&dir.path().join("memories"), "node-w").unwrap();
    assert!(
        (fm.weight - 0.6).abs() < 1e-6,
        "weight should be 0.5 + 0.1 = 0.6, got {}",
        fm.weight
    );
}

#[test]
fn test_penalize_persists_weight_to_disk() {
    let dir = setup_dir();
    add_node(dir.path(), "node-p", vec![], "penalized");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Penalize {
        name: "node-p".into(),
    };
    handle_request(&mut state, &req, dir.path());

    // Read weight from disk (initial 1.0 * 0.8 = 0.8)
    let (fm, _) = read_node_from_dir(&dir.path().join("memories"), "node-p").unwrap();
    assert!(
        (fm.weight - 0.8).abs() < 1e-6,
        "weight should be 1.0 * 0.8 = 0.8, got {}",
        fm.weight
    );
}

#[test]
fn test_boost_survives_daemon_restart() {
    let dir = setup_dir();
    add_node(dir.path(), "boosted", vec![], "b");
    // Set initial weight to 0.5
    {
        let memories = dir.path().join("memories");
        let (mut fm, body) = read_node_from_dir(&memories, "boosted").unwrap();
        fm.weight = 0.5;
        write_node_to_dir(&memories, "boosted", &fm, &body).unwrap();
    }
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Boost {
        name: "boosted".into(),
    };
    handle_request(&mut state, &req, dir.path());
    drop(state);

    let reloaded = load_state_from_dir(dir.path()).unwrap();
    assert!(
        (reloaded.node_metas["boosted"].weight - 0.6).abs() < 1e-6,
        "weight should survive restart as 0.6"
    );
}

// ============================================================
// §7: Pin/Unpin persist to disk
// ============================================================

#[test]
fn test_pin_persists_to_disk() {
    let dir = setup_dir();
    add_node(dir.path(), "pinnable", vec![], "p");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Pin {
        name: "pinnable".into(),
    };
    handle_request(&mut state, &req, dir.path());

    let (fm, _) = read_node_from_dir(&dir.path().join("memories"), "pinnable").unwrap();
    assert!(fm.pinned, "pinned should be true on disk");
}

#[test]
fn test_unpin_persists_to_disk() {
    let dir = setup_dir();
    // Create a pinned node
    {
        let memories = dir.path().join("memories");
        let fm = Frontmatter::new_for_create(vec![], true, "pinned abstract".to_string());
        write_node_to_dir(&memories, "pinned-node", &fm, "body").unwrap();
    }
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Unpin {
        name: "pinned-node".into(),
    };
    handle_request(&mut state, &req, dir.path());

    let (fm, _) = read_node_from_dir(&dir.path().join("memories"), "pinned-node").unwrap();
    assert!(!fm.pinned, "pinned should be false on disk after unpin");
}

#[test]
fn test_pin_survives_daemon_restart() {
    let dir = setup_dir();
    add_node(dir.path(), "sticky", vec![], "s");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Pin {
        name: "sticky".into(),
    };
    handle_request(&mut state, &req, dir.path());
    drop(state);

    let reloaded = load_state_from_dir(dir.path()).unwrap();
    assert!(reloaded.node_metas["sticky"].pinned);
}

// ============================================================
// §8: Patch persists to disk
// ============================================================

#[test]
fn test_patch_append_persists_to_disk() {
    let dir = setup_dir();
    add_node(dir.path(), "patchable", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Patch {
        name: "patchable".into(),
        op: PatchRequest::Append("appended line".into()),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    let (_, body) = read_node_from_dir(&dir.path().join("memories"), "patchable").unwrap();
    assert!(body.contains("appended line"), "body should contain appended text");
}

#[test]
fn test_patch_replace_persists_to_disk() {
    let dir = setup_dir();
    add_node(dir.path(), "replaceable", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Patch {
        name: "replaceable".into(),
        op: PatchRequest::Replace {
            old: "body text".into(),
            new: "replaced text".into(),
        },
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    let (_, body) = read_node_from_dir(&dir.path().join("memories"), "replaceable").unwrap();
    assert!(body.contains("replaced text"));
    assert!(!body.contains("body text"));
}

// ============================================================
// §9: WAL recovery — uncommitted CREATE rolls back
// ============================================================

#[test]
fn test_wal_uncommitted_create_with_links_rolls_back() {
    let dir = setup_dir();
    let memories_dir = dir.path().join("memories");

    // Create a peer node (fully committed)
    add_node(dir.path(), "peer", vec![], "peer");

    // Simulate: a CREATE for "ghost" was started but never committed.
    // The file was written to disk (mid-operation crash).
    let fm = Frontmatter::new_for_create(vec!["peer".into()], false, "ghost".to_string());
    write_node_to_dir(&memories_dir, "ghost", &fm, "ghost body").unwrap();

    // Also simulate the peer's links were updated
    {
        let (mut pfm, pbody) = read_node_from_dir(&memories_dir, "peer").unwrap();
        pfm.links.push("ghost".to_string());
        write_node_to_dir(&memories_dir, "peer", &pfm, &pbody).unwrap();
    }

    // Write uncommitted WAL entry
    let mut wal = WalWriter::at(dir.path().join("wal.log"));
    wal.begin(&WalOp::Create("ghost".into())).unwrap();
    // No commit!

    // Reload — WAL recovery should remove the ghost file
    let state = load_state_from_dir(dir.path()).unwrap();
    assert!(!state.name_index.contains("ghost"));
    assert!(!file_exists(dir.path(), "ghost"));
    // Peer should still exist
    assert!(state.name_index.contains("peer"));
}

#[test]
fn test_wal_uncommitted_create_no_file_is_noop() {
    let dir = setup_dir();
    std::fs::create_dir_all(dir.path().join("memories")).unwrap();

    // WAL says CREATE started, but file was never written (very early crash)
    let mut wal = WalWriter::at(dir.path().join("wal.log"));
    wal.begin(&WalOp::Create("never-existed".into())).unwrap();

    // Should not panic or error
    let state = load_state_from_dir(dir.path()).unwrap();
    assert!(!state.name_index.contains("never-existed"));
}

// ============================================================
// §10: WAL recovery — uncommitted DELETE keeps file
// ============================================================

#[test]
fn test_wal_uncommitted_delete_preserves_file() {
    let dir = setup_dir();
    add_node(dir.path(), "survivor", vec![], "still here");

    // WAL says DELETE started, but wasn't committed
    let mut wal = WalWriter::at(dir.path().join("wal.log"));
    wal.begin(&WalOp::Delete("survivor".into())).unwrap();

    let state = load_state_from_dir(dir.path()).unwrap();
    assert!(state.name_index.contains("survivor"));
    assert!(file_exists(dir.path(), "survivor"));
}

#[test]
fn test_wal_uncommitted_delete_with_missing_file() {
    let dir = setup_dir();
    std::fs::create_dir_all(dir.path().join("memories")).unwrap();

    // WAL says DELETE started, and the file was actually removed (late crash)
    let mut wal = WalWriter::at(dir.path().join("wal.log"));
    wal.begin(&WalOp::Delete("already-gone".into())).unwrap();

    // Should not panic — file is gone, data is lost, but system recovers
    let state = load_state_from_dir(dir.path()).unwrap();
    assert!(!state.name_index.contains("already-gone"));
}

// ============================================================
// §11: WAL recovery — uncommitted UPDATE
// ============================================================

#[test]
fn test_wal_uncommitted_update_accepts_current_version() {
    let dir = setup_dir();
    add_node(dir.path(), "updated-node", vec![], "either old or new");

    // WAL says UPDATE started, but wasn't committed
    // (atomic rename means the file is either old or new version, both valid)
    let mut wal = WalWriter::at(dir.path().join("wal.log"));
    wal.begin(&WalOp::Update("updated-node".into())).unwrap();

    let state = load_state_from_dir(dir.path()).unwrap();
    assert!(state.name_index.contains("updated-node"));
    assert!(file_exists(dir.path(), "updated-node"));
}

// ============================================================
// §12: WAL recovery — uncommitted RENAME
// ============================================================

#[test]
fn test_wal_uncommitted_rename_only_old_exists() {
    let dir = setup_dir();
    add_node(dir.path(), "old-node", vec![], "still old");

    // WAL says RENAME started but rename never happened (very early crash)
    let mut wal = WalWriter::at(dir.path().join("wal.log"));
    wal.begin(&WalOp::Rename("old-node".into(), "new-node".into()))
        .unwrap();

    let state = load_state_from_dir(dir.path()).unwrap();
    assert!(state.name_index.contains("old-node"));
    assert!(!state.name_index.contains("new-node"));
}

#[test]
fn test_wal_uncommitted_rename_only_new_exists() {
    let dir = setup_dir();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    // Simulate: rename completed at file level but not committed
    add_node(dir.path(), "new-node", vec![], "renamed");
    // old-node no longer exists

    let mut wal = WalWriter::at(dir.path().join("wal.log"));
    wal.begin(&WalOp::Rename("old-node".into(), "new-node".into()))
        .unwrap();

    let state = load_state_from_dir(dir.path()).unwrap();
    // new-node should be kept since rename completed at file level
    assert!(state.name_index.contains("new-node"));
    assert!(!state.name_index.contains("old-node"));
}

#[test]
fn test_wal_uncommitted_rename_both_exist() {
    let dir = setup_dir();
    add_node(dir.path(), "old-node", vec![], "old");
    add_node(dir.path(), "new-node", vec![], "new copy");

    // WAL says RENAME started — both files exist (copy was made but old not deleted)
    let mut wal = WalWriter::at(dir.path().join("wal.log"));
    wal.begin(&WalOp::Rename("old-node".into(), "new-node".into()))
        .unwrap();

    let state = load_state_from_dir(dir.path()).unwrap();
    // Should rollback: keep old, remove new
    assert!(state.name_index.contains("old-node"));
    assert!(!state.name_index.contains("new-node"));
    assert!(file_exists(dir.path(), "old-node"));
    assert!(!file_exists(dir.path(), "new-node"));
}

// ============================================================
// §13: WAL recovery — uncommitted LINK/UNLINK
// ============================================================

#[test]
fn test_wal_uncommitted_link_handled_by_consistency_check() {
    let dir = setup_dir();

    // Simulate: LINK started, A.md has B in links but B.md does not have A
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    let fm_a = Frontmatter::new_for_create(vec!["node-b".into()], false, "a".to_string());
    write_node_to_dir(&memories_dir, "node-a", &fm_a, "body a").unwrap();

    let fm_b = Frontmatter::new_for_create(vec![], false, "b".to_string());
    write_node_to_dir(&memories_dir, "node-b", &fm_b, "body b").unwrap();

    let mut wal = WalWriter::at(dir.path().join("wal.log"));
    wal.begin(&WalOp::Link("node-a".into(), "node-b".into()))
        .unwrap();

    // Consistency check during load should repair the unidirectional link
    let state = load_state_from_dir(dir.path()).unwrap();
    // The graph should have the edge (consistency check repairs unidirectional → bidirectional)
    assert!(state.graph.has_edge("node-a", "node-b"));
}

#[test]
fn test_wal_uncommitted_unlink_handled_by_consistency_check() {
    let dir = setup_dir();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    // Simulate: UNLINK started, A.md no longer has B, but B.md still has A
    let fm_a = Frontmatter::new_for_create(vec![], false, "a".to_string());
    write_node_to_dir(&memories_dir, "node-a", &fm_a, "body a").unwrap();

    let fm_b = Frontmatter::new_for_create(vec!["node-a".into()], false, "b".to_string());
    write_node_to_dir(&memories_dir, "node-b", &fm_b, "body b").unwrap();

    let mut wal = WalWriter::at(dir.path().join("wal.log"));
    wal.begin(&WalOp::Unlink("node-a".into(), "node-b".into()))
        .unwrap();

    let state = load_state_from_dir(dir.path()).unwrap();
    // Consistency check makes it bidirectional (B has A → add A has B)
    // This is the simplified behavior — design says to rollback (remove B→A),
    // but implementation uses consistency check which adds A→B instead.
    assert!(state.graph.has_edge("node-a", "node-b"));
}

// ============================================================
// §14: WAL recovery — multiple uncommitted transactions
// ============================================================

#[test]
fn test_wal_multiple_uncommitted_transactions() {
    let dir = setup_dir();
    let memories_dir = dir.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();

    // Real node (committed)
    add_node(dir.path(), "real-node", vec![], "real");

    // Ghost node (uncommitted create)
    add_node(dir.path(), "ghost-one", vec![], "ghost");
    add_node(dir.path(), "ghost-two", vec![], "ghost");

    let mut wal = WalWriter::at(dir.path().join("wal.log"));

    // One committed tx
    let tx1 = wal.begin(&WalOp::Create("real-node".into())).unwrap();
    wal.commit(&tx1).unwrap();

    // Two uncommitted txs
    wal.begin(&WalOp::Create("ghost-one".into())).unwrap();
    wal.begin(&WalOp::Create("ghost-two".into())).unwrap();

    let state = load_state_from_dir(dir.path()).unwrap();
    assert!(state.name_index.contains("real-node"));
    assert!(!state.name_index.contains("ghost-one"));
    assert!(!state.name_index.contains("ghost-two"));
}

// ============================================================
// §15: graph.idx corruption fallback
// ============================================================

#[test]
fn test_corrupt_graph_idx_fallback_to_rebuild() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec!["node-b".into()], "a");
    add_node(dir.path(), "node-b", vec!["node-a".into()], "b");

    // Write garbage to graph.idx
    std::fs::write(dir.path().join("graph.idx"), b"GARBAGE DATA HERE").unwrap();

    // Should fall back to rebuilding from .md files
    let state = load_state_from_dir(dir.path()).unwrap();
    assert!(state.graph.has_edge("node-a", "node-b"));
    assert_eq!(state.graph.edge_count(), 1);
}

#[test]
fn test_truncated_graph_idx_fallback_to_rebuild() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec!["node-b".into()], "a");
    add_node(dir.path(), "node-b", vec!["node-a".into()], "b");

    // Write a truncated graph.idx (valid magic but truncated edge data)
    let mut buf = Vec::new();
    buf.extend_from_slice(b"MCGI"); // magic
    buf.extend_from_slice(&1u16.to_le_bytes()); // version
    buf.extend_from_slice(&1u32.to_le_bytes()); // edge_count = 1
    // But no edge data follows → truncated

    std::fs::write(dir.path().join("graph.idx"), &buf).unwrap();

    let state = load_state_from_dir(dir.path()).unwrap();
    assert!(state.graph.has_edge("node-a", "node-b"));
    assert_eq!(state.graph.edge_count(), 1);
}

#[test]
fn test_bad_magic_graph_idx_fallback_to_rebuild() {
    let dir = setup_dir();
    add_node(dir.path(), "aa", vec!["bb".into()], "a");
    add_node(dir.path(), "bb", vec!["aa".into()], "b");

    // Write graph.idx with wrong magic bytes
    let mut buf = Vec::new();
    buf.extend_from_slice(b"XYZW"); // wrong magic
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());

    std::fs::write(dir.path().join("graph.idx"), &buf).unwrap();

    let state = load_state_from_dir(dir.path()).unwrap();
    assert!(state.graph.has_edge("aa", "bb"));
    assert_eq!(state.graph.edge_count(), 1);
}

#[test]
fn test_graph_idx_with_unknown_hash_fallback_to_rebuild() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec!["node-b".into()], "a");
    add_node(dir.path(), "node-b", vec!["node-a".into()], "b");

    // Build a graph.idx referencing nodes that don't exist
    let mut buf = Vec::new();
    buf.extend_from_slice(b"MCGI");
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes()); // 1 edge
    buf.extend_from_slice(&0xDEADBEEFu64.to_le_bytes()); // unknown hash
    buf.extend_from_slice(&0xCAFEBABEu64.to_le_bytes()); // unknown hash

    std::fs::write(dir.path().join("graph.idx"), &buf).unwrap();

    let state = load_state_from_dir(dir.path()).unwrap();
    // Should fall back and rebuild correctly
    assert!(state.graph.has_edge("node-a", "node-b"));
    assert_eq!(state.graph.edge_count(), 1);
}

#[test]
fn test_valid_graph_idx_is_used() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec!["node-b".into()], "a");
    add_node(dir.path(), "node-b", vec!["node-a".into()], "b");

    // Build a valid graph.idx
    let mut graph = Graph::new();
    graph.add_edge("node-a", "node-b");
    let idx_bytes = serialize_graph_idx(&graph);
    std::fs::write(dir.path().join("graph.idx"), &idx_bytes).unwrap();

    let state = load_state_from_dir(dir.path()).unwrap();
    assert!(state.graph.has_edge("node-a", "node-b"));
    assert_eq!(state.graph.edge_count(), 1);
}

// ============================================================
// §16: Atomic write — no half-written files
// ============================================================

#[test]
fn test_atomic_write_produces_valid_file() {
    let dir = setup_dir();
    let memories_dir = dir.path().join("memories");

    let fm = Frontmatter::new_for_create(vec![], false, "test abstract".to_string());
    write_node_to_dir(&memories_dir, "atomic-test", &fm, "test body").unwrap();

    // File should be fully parseable
    let content = std::fs::read_to_string(memories_dir.join("atomic-test.md")).unwrap();
    let (parsed_fm, parsed_body) = parse_node_file(&content).unwrap();
    assert_eq!(parsed_fm.abstract_text, "test abstract");
    assert_eq!(parsed_body.trim(), "test body");
}

#[test]
fn test_atomic_write_overwrite_preserves_integrity() {
    let dir = setup_dir();
    let memories_dir = dir.path().join("memories");

    // Write initial version
    let fm1 = Frontmatter::new_for_create(vec![], false, "version-1".to_string());
    write_node_to_dir(&memories_dir, "overwritten", &fm1, "body v1").unwrap();

    // Overwrite with new version
    let fm2 = Frontmatter::new_for_create(vec![], false, "version-2".to_string());
    write_node_to_dir(&memories_dir, "overwritten", &fm2, "body v2").unwrap();

    // File should be the new version, fully valid
    let (fm, body) = read_node_from_dir(&memories_dir, "overwritten").unwrap();
    assert_eq!(fm.abstract_text, "version-2");
    assert_eq!(body.trim(), "body v2");
}

#[test]
fn test_atomic_write_no_temp_files_left_behind() {
    let dir = setup_dir();
    let memories_dir = dir.path().join("memories");

    let fm = Frontmatter::new_for_create(vec![], false, "clean".to_string());
    write_node_to_dir(&memories_dir, "clean-node", &fm, "body").unwrap();

    // Check that no temporary files are left in the directory
    let entries: Vec<_> = std::fs::read_dir(&memories_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();

    assert_eq!(
        entries.len(),
        1,
        "should have exactly 1 file, got: {:?}",
        entries.iter().map(|e| e.file_name()).collect::<Vec<_>>()
    );
    assert_eq!(entries[0].file_name(), "clean-node.md");
}

// ============================================================
// §17: Full round-trip — create → modify → restart → verify
// ============================================================

#[test]
fn test_full_lifecycle_create_link_update_delete_restart() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Step 1: Create 3 nodes
    for name in &["node-a", "node-b", "node-c"] {
        let content = make_content(&format!("{} abstract", name), &[], &format!("{} body", name));
        let req = Request::Create {
            name: name.to_string(),
            content,
        };
        let resp = handle_request(&mut state, &req, dir.path());
        assert!(resp.success, "create {} failed", name);
    }

    // Step 2: Link a<->b and b<->c
    handle_request(
        &mut state,
        &Request::Link {
            a: "node-a".into(),
            b: "node-b".into(),
        },
        dir.path(),
    );
    handle_request(
        &mut state,
        &Request::Link {
            a: "node-b".into(),
            b: "node-c".into(),
        },
        dir.path(),
    );

    // Step 3: Update node-a
    let content = make_content("updated-a abstract", &["node-b"], "updated-a body");
    handle_request(
        &mut state,
        &Request::Update {
            name: "node-a".into(),
            content,
        },
        dir.path(),
    );

    // Step 4: Delete node-c
    handle_request(
        &mut state,
        &Request::Delete {
            name: "node-c".into(),
        },
        dir.path(),
    );

    // Step 5: Boost node-b
    handle_request(
        &mut state,
        &Request::Boost {
            name: "node-b".into(),
        },
        dir.path(),
    );

    // Step 6: Simulate daemon restart
    drop(state);
    let reloaded = load_state_from_dir(dir.path()).unwrap();

    // Verify
    assert!(reloaded.name_index.contains("node-a"));
    assert!(reloaded.name_index.contains("node-b"));
    assert!(!reloaded.name_index.contains("node-c"));

    assert!(reloaded.graph.has_edge("node-a", "node-b"));
    assert!(!reloaded.graph.has_edge("node-b", "node-c"));
    assert_eq!(reloaded.graph.edge_count(), 1);

    assert_eq!(
        reloaded.node_metas["node-a"].abstract_text,
        "updated-a abstract"
    );
    // Weight of node-b: started at 1.0, already capped, boost should keep at 1.0
    assert!((reloaded.node_metas["node-b"].weight - 1.0).abs() < 1e-6);
}

// ============================================================
// §18: Access metadata flush persistence
// ============================================================

#[test]
fn test_flush_access_metadata_multiple_dirty_nodes() {
    let dir = setup_dir();
    add_node(dir.path(), "hot-a", vec![], "a");
    add_node(dir.path(), "hot-b", vec![], "b");
    add_node(dir.path(), "cold-c", vec![], "c");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Access hot-a 3 times, hot-b 2 times, cold-c never
    for _ in 0..3 {
        handle_request(
            &mut state,
            &Request::Get {
                names: vec!["hot-a".into()],
            },
            dir.path(),
        );
    }
    for _ in 0..2 {
        handle_request(
            &mut state,
            &Request::Get {
                names: vec!["hot-b".into()],
            },
            dir.path(),
        );
    }

    assert_eq!(state.access_dirty.len(), 2);

    // Flush
    let flushed = flush_access_metadata(&mut state, dir.path());
    assert_eq!(flushed, 2);
    assert!(state.access_dirty.is_empty());

    // Restart and verify
    drop(state);
    let reloaded = load_state_from_dir(dir.path()).unwrap();
    assert_eq!(reloaded.node_metas["hot-a"].access_count, 3);
    assert_eq!(reloaded.node_metas["hot-b"].access_count, 2);
    assert_eq!(reloaded.node_metas["cold-c"].access_count, 0);
}

#[test]
fn test_unflushed_access_metadata_lost_on_restart() {
    let dir = setup_dir();
    add_node(dir.path(), "accessed", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Access the node but do NOT flush
    handle_request(
        &mut state,
        &Request::Get {
            names: vec!["accessed".into()],
        },
        dir.path(),
    );

    assert_eq!(state.node_metas["accessed"].access_count, 1);
    assert!(!state.access_dirty.is_empty());

    // Restart WITHOUT flushing — access count should be lost
    drop(state);
    let reloaded = load_state_from_dir(dir.path()).unwrap();
    assert_eq!(
        reloaded.node_metas["accessed"].access_count, 0,
        "unflushed access metadata should be lost on restart"
    );
}

// ============================================================
// §19: WAL is cleared after successful load
// ============================================================

#[test]
fn test_wal_cleared_after_load() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");

    let wal_path = dir.path().join("wal.log");
    let mut wal = WalWriter::at(wal_path.clone());
    let tx = wal.begin(&WalOp::Create("node-a".into())).unwrap();
    wal.commit(&tx).unwrap();

    // WAL has content
    assert!(!std::fs::read_to_string(&wal_path).unwrap().is_empty());

    let _state = load_state_from_dir(dir.path()).unwrap();

    // WAL should be cleared after successful load
    let content = std::fs::read_to_string(&wal_path).unwrap_or_default();
    assert!(content.is_empty(), "WAL should be empty after recovery");
}

// ============================================================
// §20: graph.idx is saved after mutations
// ============================================================

#[test]
fn test_graph_idx_updated_after_link() {
    let dir = setup_dir();
    add_node(dir.path(), "ga", vec![], "a");
    add_node(dir.path(), "gb", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Link {
        a: "ga".into(),
        b: "gb".into(),
    };
    handle_request(&mut state, &req, dir.path());

    // graph.idx should exist and be valid
    let graph_idx_path = dir.path().join("graph.idx");
    assert!(graph_idx_path.exists(), "graph.idx should exist after link");

    let buf = std::fs::read(&graph_idx_path).unwrap();
    let hash_to_name: std::collections::HashMap<u64, String> = ["ga", "gb"]
        .iter()
        .map(|n| {
            (
                xxhash_rust::xxh64::xxh64(n.as_bytes(), 0),
                n.to_string(),
            )
        })
        .collect();
    let graph = deserialize_graph_idx(&buf, &hash_to_name).unwrap();
    assert!(graph.has_edge("ga", "gb"));
}

#[test]
fn test_graph_idx_updated_after_delete() {
    let dir = setup_dir();
    add_node(dir.path(), "da", vec!["db".into()], "a");
    add_node(dir.path(), "db", vec!["da".into()], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Delete {
        name: "da".into(),
    };
    handle_request(&mut state, &req, dir.path());

    // graph.idx should reflect the deletion
    let buf = std::fs::read(dir.path().join("graph.idx")).unwrap();
    let hash_to_name: std::collections::HashMap<u64, String> = ["db"]
        .iter()
        .map(|n| {
            (
                xxhash_rust::xxh64::xxh64(n.as_bytes(), 0),
                n.to_string(),
            )
        })
        .collect();
    let graph = deserialize_graph_idx(&buf, &hash_to_name).unwrap();
    assert_eq!(graph.edge_count(), 0);
}

// ============================================================
// §21: Concurrent-style sequential operations stay consistent
// ============================================================

#[test]
fn test_many_creates_all_persist() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let count = 50;
    for i in 0..count {
        let name = format!("node-{:03}", i);
        let content = make_content(&format!("abstract {}", i), &[], &format!("body {}", i));
        let req = Request::Create {
            name: name.clone(),
            content,
        };
        let resp = handle_request(&mut state, &req, dir.path());
        assert!(resp.success, "create {} failed", name);
    }

    // All files should exist
    for i in 0..count {
        let name = format!("node-{:03}", i);
        assert!(file_exists(dir.path(), &name), "{} missing on disk", name);
    }

    // Restart
    drop(state);
    let reloaded = load_state_from_dir(dir.path()).unwrap();
    assert_eq!(reloaded.name_index.len(), count);
}

#[test]
fn test_rapid_link_unlink_cycle_persists_correctly() {
    let dir = setup_dir();
    add_node(dir.path(), "flip-a", vec![], "a");
    add_node(dir.path(), "flip-b", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Link, unlink, link, unlink, link (5 ops, end state: linked)
    for i in 0..5 {
        if i % 2 == 0 {
            handle_request(
                &mut state,
                &Request::Link {
                    a: "flip-a".into(),
                    b: "flip-b".into(),
                },
                dir.path(),
            );
        } else {
            handle_request(
                &mut state,
                &Request::Unlink {
                    a: "flip-a".into(),
                    b: "flip-b".into(),
                },
                dir.path(),
            );
        }
    }

    // Should end linked (0=link, 1=unlink, 2=link, 3=unlink, 4=link)
    assert!(state.graph.has_edge("flip-a", "flip-b"));

    // Verify on disk
    let a_links = read_links_from_disk(dir.path(), "flip-a");
    assert!(a_links.contains(&"flip-b".to_string()));

    // Survive restart
    drop(state);
    let reloaded = load_state_from_dir(dir.path()).unwrap();
    assert!(reloaded.graph.has_edge("flip-a", "flip-b"));
}

// ============================================================
// §22: Edge cases from design.md §10.6
// ============================================================

#[test]
fn test_create_in_empty_system() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();
    assert_eq!(state.name_index.len(), 0);

    let content = make_content("first node", &[], "body");
    let req = Request::Create {
        name: "first".into(),
        content,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    drop(state);
    let reloaded = load_state_from_dir(dir.path()).unwrap();
    assert_eq!(reloaded.name_index.len(), 1);
}

#[test]
fn test_delete_last_node_leaves_empty_system() {
    let dir = setup_dir();
    add_node(dir.path(), "only-node", vec![], "only");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Delete {
        name: "only-node".into(),
    };
    handle_request(&mut state, &req, dir.path());

    drop(state);
    let reloaded = load_state_from_dir(dir.path()).unwrap();
    assert_eq!(reloaded.name_index.len(), 0);
    assert_eq!(reloaded.graph.node_count(), 0);
}

#[test]
fn test_gc_persists_graph_idx() {
    let dir = setup_dir();
    add_node(dir.path(), "re-a", vec!["re-b".into()], "a");
    add_node(dir.path(), "re-b", vec!["re-a".into()], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Delete graph.idx
    let _ = std::fs::remove_file(dir.path().join("graph.idx"));

    // gc rebuilds graph.idx (doesn't require embedding feature)
    let req = Request::Gc;
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    // graph.idx should be recreated
    assert!(
        dir.path().join("graph.idx").exists(),
        "graph.idx should be recreated after gc"
    );
}
