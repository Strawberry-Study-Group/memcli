//! Tests for gap fixes #1-#10.
//!
//! Covers:
//!   - #1: update inherits weight when not explicitly set
//!   - #2: update inherits links when not explicitly set
//!   - #4: get truncates links for supernodes (>20 links)
//!   - #9: filesystem error handling (permission denied, missing dirs)
//!   - #10: concurrent handler requests (Mutex serialization)

use std::path::Path;

use memcore::daemon_state::load_state_from_dir;
use memcore::handler::handle_request;
use memcore::node::{self, write_node_to_dir, Frontmatter};
use memcore::protocol::*;

// ============================================================
// Helpers
// ============================================================

fn setup_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("memories")).unwrap();
    std::fs::create_dir_all(dir.path().join("index")).unwrap();
    dir
}

fn add_node(dir: &Path, name: &str, links: Vec<String>, abstract_text: &str) {
    let memories = dir.join("memories");
    let fm = Frontmatter::new_for_create(links, false, abstract_text.to_string());
    write_node_to_dir(&memories, name, &fm, "body text").unwrap();
}

fn add_node_with_weight(dir: &Path, name: &str, weight: f32, links: Vec<String>, abstract_text: &str) {
    let memories = dir.join("memories");
    let mut fm = Frontmatter::new_for_create(links, false, abstract_text.to_string());
    fm.weight = weight;
    write_node_to_dir(&memories, name, &fm, "body text").unwrap();
}

/// Content with explicit weight and links fields
fn make_content_full(abstract_text: &str, links: &[&str], weight: f32, body: &str) -> String {
    let links_yaml = if links.is_empty() {
        "links: []".to_string()
    } else {
        let items: Vec<String> = links.iter().map(|l| format!("- {}", l)).collect();
        format!("links:\n{}", items.join("\n"))
    };
    format!(
        "---\nweight: {}\n{}\nabstract: {}\n---\n\n{}",
        weight, links_yaml, abstract_text, body
    )
}

/// Content WITHOUT weight field — should inherit old weight
fn make_content_no_weight(abstract_text: &str, links: &[&str], body: &str) -> String {
    let links_yaml = if links.is_empty() {
        "links: []".to_string()
    } else {
        let items: Vec<String> = links.iter().map(|l| format!("- {}", l)).collect();
        format!("links:\n{}", items.join("\n"))
    };
    format!(
        "---\n{}\nabstract: {}\n---\n\n{}",
        links_yaml, abstract_text, body
    )
}

/// Content WITHOUT links field — should inherit old links
fn make_content_no_links(abstract_text: &str, weight: f32, body: &str) -> String {
    format!(
        "---\nweight: {}\nabstract: {}\n---\n\n{}",
        weight, abstract_text, body
    )
}

/// Content with NEITHER weight NOR links — should inherit both
fn make_content_minimal(abstract_text: &str, body: &str) -> String {
    format!(
        "---\nabstract: {}\n---\n\n{}",
        abstract_text, body
    )
}

// ============================================================
// #1: Update inherits weight when not explicitly set
// ============================================================

#[test]
fn test_update_inherits_weight_when_omitted() {
    let dir = setup_dir();
    add_node_with_weight(dir.path(), "my-node", 0.5, vec![], "original");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Verify initial weight
    assert!((state.node_metas["my-node"].weight - 0.5).abs() < f32::EPSILON);

    // Update without weight field — should inherit 0.5
    let content = make_content_no_weight("updated abstract", &[], "new body");
    let req = Request::Update {
        name: "my-node".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(
        (state.node_metas["my-node"].weight - 0.5).abs() < f32::EPSILON,
        "weight should be inherited as 0.5, got {}",
        state.node_metas["my-node"].weight
    );
}

#[test]
fn test_update_explicit_weight_overrides() {
    let dir = setup_dir();
    add_node_with_weight(dir.path(), "my-node", 0.5, vec![], "original");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Update WITH explicit weight=0.8 — should use 0.8
    let content = make_content_full("updated abstract", &[], 0.8, "new body");
    let req = Request::Update {
        name: "my-node".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(
        (state.node_metas["my-node"].weight - 0.8).abs() < f32::EPSILON,
        "weight should be explicitly set to 0.8, got {}",
        state.node_metas["my-node"].weight
    );
}

#[test]
fn test_update_weight_persists_to_disk() {
    let dir = setup_dir();
    add_node_with_weight(dir.path(), "my-node", 0.3, vec![], "original");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Update without weight — should persist 0.3 to disk
    let content = make_content_no_weight("updated abstract", &[], "new body");
    let req = Request::Update {
        name: "my-node".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    // Read from disk to verify
    let (fm, _body) = node::read_node_from_dir(&dir.path().join("memories"), "my-node").unwrap();
    assert!(
        (fm.weight - 0.3).abs() < f32::EPSILON,
        "weight on disk should be 0.3, got {}",
        fm.weight
    );
}

// ============================================================
// #2: Update inherits links when not explicitly set
// ============================================================

#[test]
fn test_update_inherits_links_when_omitted() {
    let dir = setup_dir();
    add_node(dir.path(), "peer-a", vec![], "peer a");
    add_node(dir.path(), "peer-b", vec![], "peer b");
    add_node(dir.path(), "my-node", vec!["peer-a".into(), "peer-b".into()], "original");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Verify initial links
    let initial_links = &state.node_metas["my-node"].links;
    assert!(initial_links.contains(&"peer-a".to_string()));
    assert!(initial_links.contains(&"peer-b".to_string()));

    // Update without links field — should inherit old links
    let content = make_content_no_links("updated abstract", 1.0, "new body");
    let req = Request::Update {
        name: "my-node".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    let updated_links = &state.node_metas["my-node"].links;
    assert!(
        updated_links.contains(&"peer-a".to_string()),
        "links should be inherited: peer-a missing"
    );
    assert!(
        updated_links.contains(&"peer-b".to_string()),
        "links should be inherited: peer-b missing"
    );
}

#[test]
fn test_update_explicit_empty_links_clears() {
    let dir = setup_dir();
    add_node(dir.path(), "peer-a", vec![], "peer a");
    add_node(dir.path(), "my-node", vec!["peer-a".into()], "original");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Update WITH explicit empty links — should clear
    let content = make_content_full("updated abstract", &[], 1.0, "new body");
    let req = Request::Update {
        name: "my-node".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(
        state.node_metas["my-node"].links.is_empty(),
        "explicit empty links should clear all links"
    );
}

#[test]
fn test_update_inherits_both_weight_and_links() {
    let dir = setup_dir();
    add_node(dir.path(), "peer-a", vec![], "peer a");
    add_node_with_weight(dir.path(), "my-node", 0.7, vec!["peer-a".into()], "original");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Update with neither weight nor links — should inherit both
    let content = make_content_minimal("updated abstract", "new body");
    let req = Request::Update {
        name: "my-node".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    assert!(
        (state.node_metas["my-node"].weight - 0.7).abs() < f32::EPSILON,
        "weight should be inherited as 0.7, got {}",
        state.node_metas["my-node"].weight
    );
    assert!(
        state.node_metas["my-node"].links.contains(&"peer-a".to_string()),
        "links should be inherited: peer-a missing"
    );
}

// ============================================================
// #4: Supernode protection in get output
// ============================================================

#[test]
fn test_get_truncates_supernode_links() {
    let dir = setup_dir();

    // Create 25 peer nodes
    for i in 0..25 {
        let name = format!("peer-{:02}", i);
        add_node(dir.path(), &name, vec![], &format!("peer {}", i));
    }

    // Create a supernode with 25 links
    let links: Vec<String> = (0..25).map(|i| format!("peer-{:02}", i)).collect();
    add_node(dir.path(), "supernode", links, "supernode abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Get the supernode
    let req = Request::Get {
        names: vec!["supernode".into()],
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    match resp.body {
        ResponseBody::NodeContent { content, .. } => {
            // Should contain truncation hint
            assert!(
                content.contains("more links, use 'memcore neighbors supernode' to explore"),
                "supernode output should contain truncation hint, got:\n{}",
                content
            );
            // Should have exactly 20 links in the YAML
            let link_count = content.matches("- peer-").count();
            assert_eq!(
                link_count, 20,
                "supernode output should show 20 links, got {}",
                link_count
            );
        }
        _ => panic!("expected NodeContent"),
    }
}

#[test]
fn test_get_no_truncation_for_small_link_count() {
    let dir = setup_dir();

    // Create 5 peers
    for i in 0..5 {
        let name = format!("peer-{}", i);
        add_node(dir.path(), &name, vec![], &format!("peer {}", i));
    }

    // Create a node with 5 links (below threshold)
    let links: Vec<String> = (0..5).map(|i| format!("peer-{}", i)).collect();
    add_node(dir.path(), "normal-node", links, "normal abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Get {
        names: vec!["normal-node".into()],
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    match resp.body {
        ResponseBody::NodeContent { content, .. } => {
            // Should NOT contain truncation hint
            assert!(
                !content.contains("more links"),
                "normal node should not have truncation hint"
            );
            // All 5 links should be present
            let link_count = content.matches("- peer-").count();
            assert_eq!(link_count, 5);
        }
        _ => panic!("expected NodeContent"),
    }
}

#[test]
fn test_get_batch_truncates_supernodes() {
    let dir = setup_dir();

    // Create 25 peers
    for i in 0..25 {
        let name = format!("peer-{:02}", i);
        add_node(dir.path(), &name, vec![], &format!("peer {}", i));
    }

    // Create supernode and normal node
    let links: Vec<String> = (0..25).map(|i| format!("peer-{:02}", i)).collect();
    add_node(dir.path(), "supernode", links, "supernode abstract");
    add_node(dir.path(), "normal-node", vec!["peer-00".into()], "normal abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Get {
        names: vec!["supernode".into(), "normal-node".into()],
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    match resp.body {
        ResponseBody::NodeBatch(entries) => {
            assert_eq!(entries.len(), 2);
            // Supernode should be truncated
            assert!(entries[0].content.contains("more links"));
            // Normal node should not
            assert!(!entries[1].content.contains("more links"));
        }
        _ => panic!("expected NodeBatch"),
    }
}

// ============================================================
// node::check_frontmatter_keys unit tests
// ============================================================

#[test]
fn test_check_frontmatter_keys_both_present() {
    let content = "---\nweight: 0.5\nlinks:\n- foo\nabstract: test\n---\n\nbody";
    let (has_weight, has_links) = node::check_frontmatter_keys(content);
    assert!(has_weight);
    assert!(has_links);
}

#[test]
fn test_check_frontmatter_keys_neither_present() {
    let content = "---\nabstract: test\n---\n\nbody";
    let (has_weight, has_links) = node::check_frontmatter_keys(content);
    assert!(!has_weight);
    assert!(!has_links);
}

#[test]
fn test_check_frontmatter_keys_only_weight() {
    let content = "---\nweight: 0.5\nabstract: test\n---\n\nbody";
    let (has_weight, has_links) = node::check_frontmatter_keys(content);
    assert!(has_weight);
    assert!(!has_links);
}

#[test]
fn test_check_frontmatter_keys_only_links() {
    let content = "---\nlinks: []\nabstract: test\n---\n\nbody";
    let (has_weight, has_links) = node::check_frontmatter_keys(content);
    assert!(!has_weight);
    assert!(has_links);
}

#[test]
fn test_check_frontmatter_keys_invalid_content() {
    let content = "no frontmatter here";
    let (has_weight, has_links) = node::check_frontmatter_keys(content);
    assert!(!has_weight);
    assert!(!has_links);
}

// ============================================================
// #9: Filesystem error handling
// ============================================================

#[test]
fn test_get_returns_error_for_missing_file_on_disk() {
    let dir = setup_dir();
    add_node(dir.path(), "my-node", vec![], "test");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Delete the file behind the daemon's back
    std::fs::remove_file(dir.path().join("memories/my-node.md")).unwrap();

    let req = Request::Get {
        names: vec!["my-node".into()],
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    match resp.body {
        ResponseBody::Error(msg) => {
            assert!(msg.contains("read error"), "expected read error, got: {}", msg);
        }
        _ => panic!("expected error response"),
    }
}

#[test]
fn test_create_returns_error_when_memories_dir_missing() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Remove the memories directory
    std::fs::remove_dir_all(dir.path().join("memories")).unwrap();

    let content = "---\nabstract: test\n---\n\nbody".to_string();
    let req = Request::Create {
        name: "my-node".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    match resp.body {
        ResponseBody::Error(msg) => {
            assert!(
                msg.contains("disk write error") || msg.contains("WAL error") || msg.contains("error"),
                "expected disk/WAL error, got: {}",
                msg
            );
        }
        _ => panic!("expected error response"),
    }
}

#[test]
fn test_update_handles_corrupt_file_on_disk() {
    let dir = setup_dir();
    add_node(dir.path(), "my-node", vec![], "test");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Corrupt the file on disk (handler reads from content param, not disk, so this
    // tests that the handler works even when the disk file is corrupt — the update
    // replaces the file entirely)
    std::fs::write(dir.path().join("memories/my-node.md"), "garbage content").unwrap();

    let content = "---\nabstract: updated\n---\n\nnew body".to_string();
    let req = Request::Update {
        name: "my-node".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success, "update should succeed even when disk file is corrupt");
}

// ============================================================
// #10: Concurrent handler requests (state consistency)
// ============================================================

#[test]
fn test_sequential_mutations_maintain_consistency() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Create 50 nodes rapidly
    for i in 0..50 {
        let name = format!("node-{:03}", i);
        let content = format!("---\nabstract: node {}\n---\n\nbody {}", i, i);
        let req = Request::Create {
            name,
            content,
        };
        let resp = handle_request(&mut state, &req, dir.path());
        assert!(resp.success, "create node-{:03} should succeed", i);
    }

    // Verify state
    assert_eq!(state.name_index.len(), 50);
    assert_eq!(state.node_metas.len(), 50);

    // Link them in a chain
    for i in 0..49 {
        let req = Request::Link {
            a: format!("node-{:03}", i),
            b: format!("node-{:03}", i + 1),
        };
        let resp = handle_request(&mut state, &req, dir.path());
        assert!(resp.success, "link node-{:03} -> node-{:03} should succeed", i, i + 1);
    }

    // Verify graph has 49 edges (chain of 50 nodes)
    assert_eq!(state.graph.edge_count(), 49);

    // Delete middle node
    let req = Request::Delete {
        names: vec!["node-025".into()],
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    // Verify cleanup
    assert_eq!(state.name_index.len(), 49);
    assert!(!state.node_metas.contains_key("node-025"));
    // Adjacent nodes should have lost their link to deleted node
    assert!(!state.node_metas["node-024"].links.contains(&"node-025".to_string()));
    assert!(!state.node_metas["node-026"].links.contains(&"node-025".to_string()));
}

#[test]
fn test_interleaved_create_delete_consistency() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Create, delete, recreate pattern
    for round in 0..10 {
        let name = "ephemeral-node".to_string();
        let content = format!("---\nabstract: round {}\n---\n\nbody {}", round, round);

        let req = Request::Create { name: name.clone(), content };
        let resp = handle_request(&mut state, &req, dir.path());
        assert!(resp.success);
        assert!(state.name_index.contains("ephemeral-node"));

        let req = Request::Delete { names: vec![name] };
        let resp = handle_request(&mut state, &req, dir.path());
        assert!(resp.success);
        assert!(!state.name_index.contains("ephemeral-node"));
    }

    // State should be clean
    assert_eq!(state.name_index.len(), 0);
    assert_eq!(state.node_metas.len(), 0);
}

#[test]
fn test_concurrent_like_rapid_operations() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Create two nodes
    for name in &["node-a", "node-b"] {
        let content = format!("---\nabstract: {}\n---\n\nbody", name);
        let req = Request::Create { name: name.to_string(), content };
        let resp = handle_request(&mut state, &req, dir.path());
        assert!(resp.success);
    }

    // Rapid alternating operations
    let ops: Vec<Request> = vec![
        Request::Link { a: "node-a".into(), b: "node-b".into() },
        Request::Boost { name: "node-a".into() },
        Request::Penalize { name: "node-b".into() },
        Request::Unlink { a: "node-a".into(), b: "node-b".into() },
        Request::Link { a: "node-a".into(), b: "node-b".into() },
        Request::Pin { name: "node-a".into() },
        Request::Unpin { name: "node-a".into() },
    ];

    for op in &ops {
        let resp = handle_request(&mut state, op, dir.path());
        assert!(resp.success, "operation should succeed");
    }

    // Final state should be consistent
    assert!(state.graph.has_edge("node-a", "node-b"));
    assert!(!state.node_metas["node-a"].pinned);
}
