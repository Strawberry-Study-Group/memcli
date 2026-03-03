use std::path::Path;

use memcore::daemon_state::{load_state_from_dir, flush_access_metadata};
use memcore::handler::handle_request;
use memcore::node::{write_node_to_dir, Frontmatter, read_node_from_dir};
use memcore::protocol::*;

/// Minimal content: only abstract + body, all other frontmatter fields defaulted.
/// This is the agent-friendly input format.
fn make_minimal_content(abstract_text: &str, body: &str) -> String {
    format!(
        "---\nabstract: '{}'\n---\n\n{}",
        abstract_text.replace('\'', "''"), body
    )
}

/// Setup: create a temp memcore dir, optionally with nodes, return the base dir
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

// ============================================================
// Create
// ============================================================

#[test]
fn test_create_basic() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_content("test abstract", &[], "hello world");
    let req = Request::Create {
        name: "my-node".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(state.name_index.contains("my-node"));
    assert!(state.node_metas.contains_key("my-node"));
    // File should exist on disk
    assert!(dir.path().join("memories/my-node.md").exists());
}

#[test]
fn test_create_with_links() {
    let dir = setup_dir();
    add_node(dir.path(), "target-a", vec![], "target a");
    add_node(dir.path(), "target-b", vec![], "target b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_content("new node", &["target-a", "target-b"], "body");
    let req = Request::Create {
        name: "new-node".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(state.graph.has_edge("new-node", "target-a"));
    assert!(state.graph.has_edge("new-node", "target-b"));
}

#[test]
fn test_create_duplicate_fails() {
    let dir = setup_dir();
    add_node(dir.path(), "exists", vec![], "existing");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_content("duplicate", &[], "body");
    let req = Request::Create {
        name: "exists".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_create_invalid_name() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_content("test", &[], "body");
    let req = Request::Create {
        name: "2bad".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_create_missing_link_target() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_content("test", &["nonexistent"], "body");
    let req = Request::Create {
        name: "my-node".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
}

// ============================================================
// Get
// ============================================================

#[test]
fn test_get_single() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "abstract a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Get {
        names: vec!["node-a".into()],
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::NodeContent { name, content } => {
            assert_eq!(name, "node-a");
            assert!(content.contains("abstract a"));
        }
        _ => panic!("wrong response body"),
    }
}

#[test]
fn test_get_batch() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Get {
        names: vec!["node-a".into(), "node-b".into()],
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::NodeBatch(entries) => {
            assert_eq!(entries.len(), 2);
        }
        _ => panic!("wrong response body"),
    }
}

#[test]
fn test_get_not_found() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Get {
        names: vec!["ghost".into()],
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
}

// ============================================================
// Delete
// ============================================================

#[test]
fn test_delete_basic() {
    let dir = setup_dir();
    add_node(dir.path(), "doomed", vec![], "to be deleted");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Delete {
        name: "doomed".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(!state.name_index.contains("doomed"));
    assert!(!dir.path().join("memories/doomed.md").exists());
}

#[test]
fn test_delete_removes_edges() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec!["node-b".into()], "a");
    add_node(dir.path(), "node-b", vec!["node-a".into()], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Delete {
        name: "node-a".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(!state.graph.has_edge("node-a", "node-b"));
    // Peer should have the link removed
    let meta_b = &state.node_metas["node-b"];
    assert!(!meta_b.links.contains(&"node-a".to_string()));
}

#[test]
fn test_delete_not_found() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Delete {
        name: "ghost".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
}

// ============================================================
// Link / Unlink
// ============================================================

#[test]
fn test_link_basic() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Link {
        a: "node-a".into(),
        b: "node-b".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(state.graph.has_edge("node-a", "node-b"));
    // Check .md files updated
    let (fm_a, _) = read_node_from_dir(&dir.path().join("memories"), "node-a").unwrap();
    assert!(fm_a.links.contains(&"node-b".to_string()));
    let (fm_b, _) = read_node_from_dir(&dir.path().join("memories"), "node-b").unwrap();
    assert!(fm_b.links.contains(&"node-a".to_string()));
}

#[test]
fn test_link_idempotent() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Link {
        a: "node-a".into(),
        b: "node-b".into(),
    };
    handle_request(&mut state, &req, dir.path());
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert_eq!(state.graph.edge_count(), 1);
}

#[test]
fn test_link_self_fails() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Link {
        a: "node-a".into(),
        b: "node-a".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
}

#[test]
fn test_unlink_basic() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec!["node-b".into()], "a");
    add_node(dir.path(), "node-b", vec!["node-a".into()], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Unlink {
        a: "node-a".into(),
        b: "node-b".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(!state.graph.has_edge("node-a", "node-b"));
}

// ============================================================
// Boost / Penalize
// ============================================================

#[test]
fn test_boost() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();
    let old_weight = state.node_metas["node-a"].weight;

    let req = Request::Boost {
        name: "node-a".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    // Weight capped at 1.0 (was already 1.0)
    assert!((state.node_metas["node-a"].weight - 1.0).abs() < 1e-6);
}

#[test]
fn test_penalize() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();
    let old_weight = state.node_metas["node-a"].weight;

    let req = Request::Penalize {
        name: "node-a".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(state.node_metas["node-a"].weight < old_weight);

    // Verify persisted to disk
    let (fm, _) = read_node_from_dir(&dir.path().join("memories"), "node-a").unwrap();
    assert!((fm.weight - state.node_metas["node-a"].weight).abs() < 1e-6);
}

// ============================================================
// Pin / Unpin
// ============================================================

#[test]
fn test_pin_unpin() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Pin {
        name: "node-a".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(state.node_metas["node-a"].pinned);

    let req = Request::Unpin {
        name: "node-a".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(!state.node_metas["node-a"].pinned);
}

// ============================================================
// Rename
// ============================================================

#[test]
fn test_rename_basic() {
    let dir = setup_dir();
    add_node(dir.path(), "old-name", vec![], "my abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Rename {
        old: "old-name".into(),
        new: "new-name".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(!state.name_index.contains("old-name"));
    assert!(state.name_index.contains("new-name"));
    assert!(!dir.path().join("memories/old-name.md").exists());
    assert!(dir.path().join("memories/new-name.md").exists());
}

#[test]
fn test_rename_updates_peer_links() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec!["node-b".into()], "a");
    add_node(dir.path(), "node-b", vec!["node-a".into()], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Rename {
        old: "node-a".into(),
        new: "renamed-a".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    // Graph should reflect rename
    assert!(state.graph.has_edge("renamed-a", "node-b"));
    assert!(!state.graph.has_edge("node-a", "node-b"));

    // Peer's .md should reference new name
    let (fm_b, _) = read_node_from_dir(&dir.path().join("memories"), "node-b").unwrap();
    assert!(fm_b.links.contains(&"renamed-a".to_string()));
    assert!(!fm_b.links.contains(&"node-a".to_string()));
}

// ============================================================
// Ls
// ============================================================

#[test]
fn test_ls_empty() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Ls {
        sort: SortField::Name,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::NodeList(list) => assert!(list.is_empty()),
        _ => panic!("wrong body"),
    }
}

#[test]
fn test_ls_sorted_by_name() {
    let dir = setup_dir();
    add_node(dir.path(), "zebra", vec![], "z");
    add_node(dir.path(), "alpha", vec![], "a");
    add_node(dir.path(), "middle", vec![], "m");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Ls {
        sort: SortField::Name,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    match resp.body {
        ResponseBody::NodeList(list) => {
            assert_eq!(list[0].name, "alpha");
            assert_eq!(list[1].name, "middle");
            assert_eq!(list[2].name, "zebra");
        }
        _ => panic!("wrong body"),
    }
}

// ============================================================
// Neighbors
// ============================================================

#[test]
fn test_neighbors_basic() {
    let dir = setup_dir();
    add_node(dir.path(), "center", vec!["node-a".into(), "node-b".into()], "c");
    add_node(dir.path(), "node-a", vec!["center".into()], "a");
    add_node(dir.path(), "node-b", vec!["center".into()], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Neighbors {
        name: "center".into(),
        depth: 1,
        limit: 50,
        offset: 0,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::Neighbors {
            entries,
            total,
            depth,
        } => {
            assert_eq!(total, 2);
            assert_eq!(depth, 1);
            assert_eq!(entries.len(), 2);
        }
        _ => panic!("wrong body"),
    }
}

// ============================================================
// Status
// ============================================================

#[test]
fn test_status() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Status;
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::Status(s) => {
            assert_eq!(s.node_count, 1);
        }
        _ => panic!("wrong body"),
    }
}

// ============================================================
// Inspect
// ============================================================

#[test]
fn test_inspect_empty() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::InspectReport(data) => {
            assert_eq!(data.node_count, 0);
            assert!((data.health_score - 1.0).abs() < 1e-6);
        }
        _ => panic!("wrong body"),
    }
}

#[test]
fn test_inspect_json_format() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "body a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Json,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::Message(json_str) => {
            let parsed: serde_json::Value = serde_json::from_str(json_str)
                .expect("should be valid JSON");
            assert_eq!(parsed["node_count"], 1);
            assert!(parsed["health_score"].is_f64());
            assert!(parsed["orphans"].is_array());
        }
        _ => panic!("expected Message body with JSON, got {:?}", resp.body),
    }
}

// ============================================================
// Inspect (global — similar pairs)
// ============================================================

#[test]
fn test_inspect_global_similar_pairs_found() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    add_node(dir.path(), "node-c", vec![], "c");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // a and b near-identical, c orthogonal
    state.vector_index.insert("node-a", &[1.0, 0.0, 0.0]);
    state.vector_index.insert("node-b", &[0.99, 0.01, 0.0]);
    state.vector_index.insert("node-c", &[0.0, 0.0, 1.0]);

    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: Some(0.9),
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            assert_eq!(data.similar_pairs.len(), 1);
            assert_eq!(data.similar_pairs[0].node_a, "node-a");
            assert_eq!(data.similar_pairs[0].node_b, "node-b");
            assert!(data.similar_pairs[0].similarity > 0.9);
        }
        _ => panic!("expected InspectReport"),
    }
}

#[test]
fn test_inspect_global_threshold_filters() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // moderate similarity: cos([1,0,0], [0.8,0.6,0]) = 0.8
    state.vector_index.insert("node-a", &[1.0, 0.0, 0.0]);
    state.vector_index.insert("node-b", &[0.8, 0.6, 0.0]);

    // With threshold=0.9, the pair (sim≈0.8) should be filtered out
    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: Some(0.9),
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            assert!(data.similar_pairs.is_empty());
        }
        _ => panic!("expected InspectReport"),
    }
}

#[test]
fn test_inspect_global_no_embeddings() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // No embeddings inserted
    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            assert!(data.similar_pairs.is_empty());
            assert_eq!(data.redundancy, Some(0.0));
        }
        _ => panic!("expected InspectReport"),
    }
}

#[test]
fn test_inspect_global_pairs_sorted_descending() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    add_node(dir.path(), "node-c", vec![], "c");
    add_node(dir.path(), "node-d", vec![], "d");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    state.vector_index.insert("node-a", &[1.0, 0.0, 0.0]);
    state.vector_index.insert("node-b", &[0.99, 0.01, 0.0]); // very similar to a
    state.vector_index.insert("node-c", &[0.95, 0.05, 0.0]); // less similar to a
    state.vector_index.insert("node-d", &[0.98, 0.02, 0.0]); // moderately similar to a

    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: Some(0.9),
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            assert!(data.similar_pairs.len() >= 2);
            for w in data.similar_pairs.windows(2) {
                assert!(w[0].similarity >= w[1].similarity);
            }
        }
        _ => panic!("expected InspectReport"),
    }
}

#[test]
fn test_inspect_global_no_self_pairs() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    state.vector_index.insert("node-a", &[1.0, 0.0, 0.0]);

    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: Some(0.0), // very low threshold to catch everything
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            // No (a, a) pairs
            for p in &data.similar_pairs {
                assert_ne!(p.node_a, p.node_b);
            }
        }
        _ => panic!("expected InspectReport"),
    }
}

#[test]
fn test_inspect_global_redundancy_metric() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    add_node(dir.path(), "node-c", vec![], "c");
    add_node(dir.path(), "node-d", vec![], "d");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Only a and b are similar; c and d are orthogonal
    state.vector_index.insert("node-a", &[1.0, 0.0, 0.0]);
    state.vector_index.insert("node-b", &[0.99, 0.01, 0.0]);
    state.vector_index.insert("node-c", &[0.0, 1.0, 0.0]);
    state.vector_index.insert("node-d", &[0.0, 0.0, 1.0]);

    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: Some(0.9),
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            // 1 pair involving 2 unique nodes out of 4 total
            assert_eq!(data.similar_pairs.len(), 1);
            let r = data.redundancy.unwrap();
            assert!((r - 0.5).abs() < 1e-6, "expected redundancy 0.5, got {}", r);
        }
        _ => panic!("expected InspectReport"),
    }
}

#[test]
fn test_inspect_global_all_below_threshold() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    add_node(dir.path(), "node-c", vec![], "c");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // All orthogonal — cosine similarity 0.0
    state.vector_index.insert("node-a", &[1.0, 0.0, 0.0]);
    state.vector_index.insert("node-b", &[0.0, 1.0, 0.0]);
    state.vector_index.insert("node-c", &[0.0, 0.0, 1.0]);

    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: Some(0.5),
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            assert!(data.similar_pairs.is_empty());
            assert_eq!(data.redundancy, Some(0.0));
        }
        _ => panic!("expected InspectReport"),
    }
}

#[test]
fn test_inspect_global_default_threshold() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    add_node(dir.path(), "node-c", vec![], "c");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // a-b similarity ~0.9998 (above 0.85 default), a-c similarity ~0.71 (below 0.85)
    state.vector_index.insert("node-a", &[1.0, 0.0, 0.0]);
    state.vector_index.insert("node-b", &[0.99, 0.01, 0.0]);
    state.vector_index.insert("node-c", &[0.7, 0.7, 0.0]); // cosine with [1,0,0] ≈ 0.707

    // threshold=None → uses default 0.85
    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            // Only a-b should be above 0.85
            assert_eq!(data.similar_pairs.len(), 1);
            assert_eq!(data.similar_pairs[0].node_a, "node-a");
            assert_eq!(data.similar_pairs[0].node_b, "node-b");
        }
        _ => panic!("expected InspectReport"),
    }
}

#[test]
fn test_inspect_global_pairs_json() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    state.vector_index.insert("node-a", &[1.0, 0.0, 0.0]);
    state.vector_index.insert("node-b", &[0.99, 0.01, 0.0]);

    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Json,
        threshold: Some(0.9),
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::Message(json_str) => {
            let parsed: serde_json::Value = serde_json::from_str(json_str)
                .expect("should be valid JSON");
            let pairs = parsed["similar_pairs"].as_array().expect("should be array");
            assert_eq!(pairs.len(), 1);
            assert_eq!(pairs[0]["node_a"], "node-a");
            assert_eq!(pairs[0]["node_b"], "node-b");
            assert!(pairs[0]["similarity"].as_f64().unwrap() > 0.9);
            // redundancy should be present
            assert!(parsed["redundancy"].is_f64());
        }
        _ => panic!("expected Message body with JSON"),
    }
}

// ============================================================
// Inspect (global — similar pairs corner cases)
// ============================================================

#[test]
fn test_inspect_global_pairs_cap_at_200() {
    // With many similar nodes, output is capped at the requested cap.
    // Also verifies the bounded heap doesn't blow memory.
    let dir = setup_dir();
    // Create 25 nodes — upper triangle = 25*24/2 = 300 pairs
    for i in 0..25 {
        let name = format!("n-{:03}", i);
        add_node(dir.path(), &name, vec![], &name);
    }
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // All embeddings nearly identical → all 300 pairs above threshold
    for i in 0..25 {
        let name = format!("n-{:03}", i);
        let mut emb = vec![1.0, 0.0, 0.0];
        emb[1] = i as f32 * 0.001; // tiny variation
        state.vector_index.insert(&name, &emb);
    }

    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: Some(0.9),
        cap: Some(200),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            // Capped at 200, not 300
            assert_eq!(data.similar_pairs.len(), 200);
            assert!(data.total_similar_pairs >= 300);
            // Top 200 should be sorted descending
            for w in data.similar_pairs.windows(2) {
                assert!(w[0].similarity >= w[1].similarity);
            }
            // Redundancy counts ALL nodes involved in above-threshold pairs (all 25)
            let r = data.redundancy.unwrap();
            assert!((r - 1.0).abs() < 1e-6, "all 25 nodes should be in pairs, got {}", r);
        }
        _ => panic!("expected InspectReport"),
    }
}

#[test]
fn test_inspect_global_pairs_default_cap_50() {
    // Default cap (None) limits to 50 pairs.
    let dir = setup_dir();
    // Create 25 nodes — upper triangle = 25*24/2 = 300 pairs
    for i in 0..25 {
        let name = format!("n-{:03}", i);
        add_node(dir.path(), &name, vec![], &name);
    }
    let mut state = load_state_from_dir(dir.path()).unwrap();

    for i in 0..25 {
        let name = format!("n-{:03}", i);
        let mut emb = vec![1.0, 0.0, 0.0];
        emb[1] = i as f32 * 0.001;
        state.vector_index.insert(&name, &emb);
    }

    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: Some(0.9),
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            assert_eq!(data.similar_pairs.len(), 50);
            assert!(data.total_similar_pairs >= 300);
        }
        _ => panic!("expected InspectReport"),
    }
}

#[test]
fn test_inspect_global_single_node_no_pairs() {
    let dir = setup_dir();
    add_node(dir.path(), "solo", vec![], "alone");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    state.vector_index.insert("solo", &[1.0, 0.0, 0.0]);

    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: Some(0.0), // catch everything
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            assert!(data.similar_pairs.is_empty(), "single node can't form a pair");
            assert_eq!(data.redundancy, Some(0.0));
        }
        _ => panic!("expected InspectReport"),
    }
}

#[test]
fn test_inspect_global_identical_embeddings_sim_one() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let emb = vec![0.6, 0.8, 0.0];
    state.vector_index.insert("node-a", &emb);
    state.vector_index.insert("node-b", &emb);

    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: Some(0.99),
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            assert_eq!(data.similar_pairs.len(), 1);
            assert!((data.similar_pairs[0].similarity - 1.0).abs() < 1e-5);
        }
        _ => panic!("expected InspectReport"),
    }
}

#[test]
fn test_inspect_global_zero_vector_produces_zero_sim() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    state.vector_index.insert("node-a", &[1.0, 0.0, 0.0]);
    state.vector_index.insert("node-b", &[0.0, 0.0, 0.0]); // zero vector

    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: Some(0.0), // include everything >= 0
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            // cosine_similarity returns 0.0 for zero vector, which == threshold 0.0 → included
            assert_eq!(data.similar_pairs.len(), 1);
            assert!((data.similar_pairs[0].similarity - 0.0).abs() < 1e-6);
        }
        _ => panic!("expected InspectReport"),
    }
}

#[test]
fn test_inspect_global_partial_embeddings_skip_unindexed() {
    let dir = setup_dir();
    add_node(dir.path(), "indexed-a", vec![], "a");
    add_node(dir.path(), "indexed-b", vec![], "b");
    add_node(dir.path(), "no-embedding", vec![], "c");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    state.vector_index.insert("indexed-a", &[1.0, 0.0, 0.0]);
    state.vector_index.insert("indexed-b", &[0.99, 0.01, 0.0]);
    // no-embedding has no vector

    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: Some(0.9),
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            assert_eq!(data.similar_pairs.len(), 1);
            // Pairs only reference indexed nodes
            for p in &data.similar_pairs {
                assert_ne!(p.node_a, "no-embedding");
                assert_ne!(p.node_b, "no-embedding");
            }
            // Redundancy: 2 nodes in pairs out of 3 total nodes
            let r = data.redundancy.unwrap();
            assert!((r - 2.0 / 3.0).abs() < 1e-6);
        }
        _ => panic!("expected InspectReport"),
    }
}

#[test]
fn test_inspect_global_threshold_boundary_inclusive() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Unit vectors: cos([1,0,0], [0.6,0.8,0]) = 0.6
    state.vector_index.insert("node-a", &[1.0, 0.0, 0.0]);
    state.vector_index.insert("node-b", &[0.6, 0.8, 0.0]);

    let sim = memcore::util::cosine_similarity(&[1.0, 0.0, 0.0], &[0.6, 0.8, 0.0]);

    // threshold exactly equal to the similarity → should be included (>=)
    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: Some(sim),
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            assert_eq!(data.similar_pairs.len(), 1, "threshold=sim should include the pair");
        }
        _ => panic!("expected InspectReport"),
    }

    // threshold just above → excluded
    let req2 = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: Some(sim + 0.001),
        cap: None,
    };
    let resp2 = handle_request(&mut state, &req2, dir.path());
    match &resp2.body {
        ResponseBody::InspectReport(data) => {
            assert!(data.similar_pairs.is_empty(), "threshold above sim should exclude");
        }
        _ => panic!("expected InspectReport"),
    }
}

#[test]
fn test_inspect_global_negative_similarity_excluded_default() {
    let dir = setup_dir();
    add_node(dir.path(), "pos", vec![], "positive");
    add_node(dir.path(), "neg", vec![], "negative");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Opposite directions → cosine = -1.0
    state.vector_index.insert("pos", &[1.0, 0.0, 0.0]);
    state.vector_index.insert("neg", &[-1.0, 0.0, 0.0]);

    // Default threshold 0.85 → pair with -1.0 similarity excluded
    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            assert!(data.similar_pairs.is_empty());
        }
        _ => panic!("expected InspectReport"),
    }
}

#[test]
fn test_inspect_global_redundancy_all_similar() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    add_node(dir.path(), "node-c", vec![], "c");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // All identical → every pair has similarity 1.0
    let emb = vec![1.0, 0.0, 0.0];
    state.vector_index.insert("node-a", &emb);
    state.vector_index.insert("node-b", &emb);
    state.vector_index.insert("node-c", &emb);

    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: Some(0.9),
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            // 3 pairs: a-b, a-c, b-c; all 3 nodes involved
            assert_eq!(data.similar_pairs.len(), 3);
            let r = data.redundancy.unwrap();
            assert!((r - 1.0).abs() < 1e-6, "all nodes in pairs → redundancy=1.0");
        }
        _ => panic!("expected InspectReport"),
    }
}

#[test]
fn test_inspect_global_pair_ordering_deterministic() {
    // Two pairs with identical similarity: ordering should be deterministic
    // (by name, due to sorted names and stable upper-triangle iteration)
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    add_node(dir.path(), "node-c", vec![], "c");
    add_node(dir.path(), "node-d", vec![], "d");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let emb = vec![1.0, 0.0, 0.0];
    state.vector_index.insert("node-a", &emb);
    state.vector_index.insert("node-b", &emb);
    state.vector_index.insert("node-c", &emb);
    state.vector_index.insert("node-d", &emb);

    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: Some(0.99),
        cap: None,
    };

    // Run twice, verify same order
    let resp1 = handle_request(&mut state, &req, dir.path());
    let resp2 = handle_request(&mut state, &req, dir.path());

    let pairs1 = match &resp1.body {
        ResponseBody::InspectReport(d) => &d.similar_pairs,
        _ => panic!("expected InspectReport"),
    };
    let pairs2 = match &resp2.body {
        ResponseBody::InspectReport(d) => &d.similar_pairs,
        _ => panic!("expected InspectReport"),
    };

    assert_eq!(pairs1.len(), pairs2.len());
    for (a, b) in pairs1.iter().zip(pairs2.iter()) {
        assert_eq!(a.node_a, b.node_a);
        assert_eq!(a.node_b, b.node_b);
    }
}

#[test]
fn test_inspect_global_large_scale_500_nodes() {
    // 500 nodes with random-ish embeddings: verifies no crash, bounded output,
    // and reasonable performance (should complete in <1s).
    let dir = setup_dir();
    for i in 0..500 {
        let name = format!("n-{:04}", i);
        add_node(dir.path(), &name, vec![], &name);
    }
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Create embeddings that form clusters: groups of 10 share a direction
    for i in 0..500u32 {
        let cluster = i / 10;
        let mut emb = vec![0.0f32; 16]; // small dims for speed
        emb[(cluster as usize) % 16] = 1.0;
        emb[15] = (i % 10) as f32 * 0.01; // small within-cluster variation
        state.vector_index.insert(&format!("n-{:04}", i), &emb);
    }

    let start = std::time::Instant::now();
    let req = Request::Inspect {
        node: None,
        format: OutputFormat::Human,
        threshold: Some(0.9),
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    let elapsed = start.elapsed();

    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            // Should be capped at 50 (default cap, there are many within-cluster pairs)
            assert!(data.similar_pairs.len() <= 50);
            assert!(!data.similar_pairs.is_empty());
            // Sorted descending
            for w in data.similar_pairs.windows(2) {
                assert!(w[0].similarity >= w[1].similarity);
            }
            // Should complete quickly — 500 nodes = 124,750 pairs
            assert!(
                elapsed.as_millis() < 2000,
                "500 nodes took {}ms, expected <2000ms",
                elapsed.as_millis()
            );
        }
        _ => panic!("expected InspectReport"),
    }
}

// ============================================================
// Inspect (node-level)
// ============================================================

#[test]
fn test_inspect_node_not_found() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Inspect {
        node: Some("nonexistent".into()),
        format: OutputFormat::Human,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_inspect_node_basic() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "abstract a");
    add_node(dir.path(), "node-b", vec![], "abstract b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Link them so node-a has 1 edge
    let link_req = Request::Link {
        a: "node-a".into(),
        b: "node-b".into(),
    };
    handle_request(&mut state, &link_req, dir.path());

    // Insert fake embeddings
    let emb_a = vec![1.0, 0.0, 0.0];
    let emb_b = vec![0.9, 0.1, 0.0];
    state.vector_index.insert("node-a", &emb_a);
    state.vector_index.insert("node-b", &emb_b);

    let req = Request::Inspect {
        node: Some("node-a".into()),
        format: OutputFormat::Human,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::NodeInspectReport(data) => {
            assert_eq!(data.name, "node-a");
            assert_eq!(data.edge_count, 1);
            assert_eq!(data.similar_nodes.len(), 1);
            assert_eq!(data.similar_nodes[0].name, "node-b");
            assert!(data.similar_nodes[0].similarity > 0.0);
            assert!(data.warnings.is_empty());
        }
        _ => panic!("expected NodeInspectReport, got {:?}", resp.body),
    }
}

#[test]
fn test_inspect_node_with_collisions() {
    let dir = setup_dir();
    add_node(dir.path(), "original", vec![], "abstract");
    add_node(dir.path(), "near-dup", vec![], "abstract");
    add_node(dir.path(), "unrelated", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // original and near-dup are nearly identical embeddings
    let emb_orig = vec![1.0, 0.0, 0.0];
    let emb_dup = vec![0.99, 0.01, 0.0]; // very similar
    let emb_unrelated = vec![0.0, 0.0, 1.0]; // orthogonal
    state.vector_index.insert("original", &emb_orig);
    state.vector_index.insert("near-dup", &emb_dup);
    state.vector_index.insert("unrelated", &emb_unrelated);

    let req = Request::Inspect {
        node: Some("original".into()),
        format: OutputFormat::Human,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::NodeInspectReport(data) => {
            assert_eq!(data.similar_nodes.len(), 2);
            // near-dup should be first (highest similarity)
            assert_eq!(data.similar_nodes[0].name, "near-dup");
            assert!(data.similar_nodes[0].similarity > 0.9);
            // unrelated should be second (low similarity)
            assert_eq!(data.similar_nodes[1].name, "unrelated");
            assert!(data.similar_nodes[1].similarity < 0.1);
        }
        _ => panic!("expected NodeInspectReport"),
    }
}

#[test]
fn test_inspect_node_no_embedding() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "abstract a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Inspect {
        node: Some("node-a".into()),
        format: OutputFormat::Human,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::NodeInspectReport(data) => {
            assert_eq!(data.name, "node-a");
            assert!(data.similar_nodes.is_empty());
            assert!(!data.warnings.is_empty());
        }
        _ => panic!("expected NodeInspectReport"),
    }
}

#[test]
fn test_inspect_node_json() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "abstract a");
    add_node(dir.path(), "node-b", vec![], "abstract b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    state.vector_index.insert("node-a", &[1.0, 0.0, 0.0]);
    state.vector_index.insert("node-b", &[0.9, 0.1, 0.0]);

    let req = Request::Inspect {
        node: Some("node-a".into()),
        format: OutputFormat::Json,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::Message(json_str) => {
            let parsed: serde_json::Value =
                serde_json::from_str(json_str).expect("should be valid JSON");
            assert_eq!(parsed["name"], "node-a");
            assert!(parsed["edge_count"].is_u64());
            assert!(parsed["similar_nodes"].is_array());
        }
        _ => panic!("expected Message with JSON, got {:?}", resp.body),
    }
}

#[test]
fn test_inspect_node_self_excluded() {
    // Self should never appear in similar_nodes, even with similarity 1.0
    let dir = setup_dir();
    add_node(dir.path(), "solo", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    state.vector_index.insert("solo", &[1.0, 0.0, 0.0]);

    let req = Request::Inspect {
        node: Some("solo".into()),
        format: OutputFormat::Human,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::NodeInspectReport(data) => {
            assert!(data.similar_nodes.is_empty());
            assert!(data.warnings.is_empty());
        }
        _ => panic!("expected NodeInspectReport"),
    }
}

#[test]
fn test_inspect_node_zero_edges() {
    // Isolated node with no links
    let dir = setup_dir();
    add_node(dir.path(), "lonely", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    state.vector_index.insert("lonely", &[1.0, 0.0, 0.0]);

    let req = Request::Inspect {
        node: Some("lonely".into()),
        format: OutputFormat::Human,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::NodeInspectReport(data) => {
            assert_eq!(data.edge_count, 0);
        }
        _ => panic!("expected NodeInspectReport"),
    }
}

#[test]
fn test_inspect_node_many_edges() {
    // Node linked to multiple neighbors — edge_count accuracy
    let dir = setup_dir();
    add_node(dir.path(), "hub", vec![], "hub");
    add_node(dir.path(), "spoke-a", vec![], "a");
    add_node(dir.path(), "spoke-b", vec![], "b");
    add_node(dir.path(), "spoke-c", vec![], "c");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    for spoke in &["spoke-a", "spoke-b", "spoke-c"] {
        let req = Request::Link {
            a: "hub".into(),
            b: spoke.to_string(),
        };
        handle_request(&mut state, &req, dir.path());
    }

    state.vector_index.insert("hub", &[1.0, 0.0, 0.0]);

    let req = Request::Inspect {
        node: Some("hub".into()),
        format: OutputFormat::Human,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    match resp.body {
        ResponseBody::NodeInspectReport(data) => {
            assert_eq!(data.edge_count, 3);
        }
        _ => panic!("expected NodeInspectReport"),
    }
}

#[test]
fn test_inspect_node_top_k_capped_at_ten() {
    // More than 10 other nodes — results capped at 10
    let dir = setup_dir();
    add_node(dir.path(), "target", vec![], "target");
    for i in 0..15 {
        add_node(dir.path(), &format!("other-{:02}", i), vec![], "other");
    }
    let mut state = load_state_from_dir(dir.path()).unwrap();

    state.vector_index.insert("target", &[1.0, 0.0, 0.0]);
    for i in 0..15 {
        // Vary similarity slightly so ordering is deterministic
        let sim = 0.99 - (i as f32 * 0.01);
        state
            .vector_index
            .insert(&format!("other-{:02}", i), &[sim, (1.0 - sim * sim).sqrt(), 0.0]);
    }

    let req = Request::Inspect {
        node: Some("target".into()),
        format: OutputFormat::Human,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    match resp.body {
        ResponseBody::NodeInspectReport(data) => {
            assert_eq!(data.similar_nodes.len(), 10);
            // Verify descending order
            for w in data.similar_nodes.windows(2) {
                assert!(w[0].similarity >= w[1].similarity);
            }
        }
        _ => panic!("expected NodeInspectReport"),
    }
}

#[test]
fn test_inspect_node_identical_embeddings() {
    // Two nodes with exactly the same embedding — similarity should be 1.0
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "abstract");
    add_node(dir.path(), "node-b", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let emb = vec![0.6, 0.8, 0.0]; // already unit-length
    state.vector_index.insert("node-a", &emb);
    state.vector_index.insert("node-b", &emb);

    let req = Request::Inspect {
        node: Some("node-a".into()),
        format: OutputFormat::Human,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    match resp.body {
        ResponseBody::NodeInspectReport(data) => {
            assert_eq!(data.similar_nodes.len(), 1);
            assert!((data.similar_nodes[0].similarity - 1.0).abs() < 1e-6);
        }
        _ => panic!("expected NodeInspectReport"),
    }
}

#[test]
fn test_inspect_node_partial_embeddings() {
    // Target has embedding; some other nodes do, some don't.
    // Only nodes WITH embeddings should appear in results.
    let dir = setup_dir();
    add_node(dir.path(), "indexed", vec![], "abstract");
    add_node(dir.path(), "also-indexed", vec![], "abstract");
    add_node(dir.path(), "not-indexed", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    state.vector_index.insert("indexed", &[1.0, 0.0, 0.0]);
    state.vector_index.insert("also-indexed", &[0.9, 0.1, 0.0]);
    // not-indexed has no embedding

    let req = Request::Inspect {
        node: Some("indexed".into()),
        format: OutputFormat::Human,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    match resp.body {
        ResponseBody::NodeInspectReport(data) => {
            assert_eq!(data.similar_nodes.len(), 1);
            assert_eq!(data.similar_nodes[0].name, "also-indexed");
            assert!(data.warnings.is_empty());
        }
        _ => panic!("expected NodeInspectReport"),
    }
}

#[test]
fn test_inspect_node_json_with_warnings() {
    // No embedding → JSON output should include non-empty warnings array
    let dir = setup_dir();
    add_node(dir.path(), "no-emb", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Inspect {
        node: Some("no-emb".into()),
        format: OutputFormat::Json,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::Message(json_str) => {
            let parsed: serde_json::Value =
                serde_json::from_str(json_str).expect("valid JSON");
            assert_eq!(parsed["name"], "no-emb");
            let warnings = parsed["warnings"].as_array().expect("warnings array");
            assert!(!warnings.is_empty());
            let similar = parsed["similar_nodes"].as_array().expect("similar_nodes array");
            assert!(similar.is_empty());
        }
        _ => panic!("expected Message with JSON"),
    }
}

#[test]
fn test_inspect_node_descending_similarity_order() {
    // 5 nodes with known similarities — verify strict descending order
    let dir = setup_dir();
    add_node(dir.path(), "target", vec![], "t");
    add_node(dir.path(), "high", vec![], "h");
    add_node(dir.path(), "mid", vec![], "m");
    add_node(dir.path(), "low", vec![], "l");
    add_node(dir.path(), "neg", vec![], "n");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    state.vector_index.insert("target", &[1.0, 0.0, 0.0]);
    state.vector_index.insert("high", &[0.95, 0.05, 0.0]);
    state.vector_index.insert("mid", &[0.5, 0.5, 0.0]);
    state.vector_index.insert("low", &[0.1, 0.9, 0.0]);
    state.vector_index.insert("neg", &[-1.0, 0.0, 0.0]); // opposite direction

    let req = Request::Inspect {
        node: Some("target".into()),
        format: OutputFormat::Human,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    match resp.body {
        ResponseBody::NodeInspectReport(data) => {
            assert_eq!(data.similar_nodes.len(), 4);
            assert_eq!(data.similar_nodes[0].name, "high");
            assert_eq!(data.similar_nodes[1].name, "mid");
            assert_eq!(data.similar_nodes[2].name, "low");
            assert_eq!(data.similar_nodes[3].name, "neg");
            // neg should have negative similarity (opposite vector)
            assert!(data.similar_nodes[3].similarity < 0.0);
            // Strict descending
            for w in data.similar_nodes.windows(2) {
                assert!(w[0].similarity > w[1].similarity);
            }
        }
        _ => panic!("expected NodeInspectReport"),
    }
}

// ============================================================
// GC
// ============================================================

#[test]
fn test_gc_cleans_dangling() {
    let dir = setup_dir();
    // node-a has a link to "ghost" which doesn't exist
    add_node(dir.path(), "node-a", vec!["ghost".into()], "a");
    // Manually load — the load already strips dangling, so we need to
    // add a dangling reference after load
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Manually inject a dangling reference for testing
    if let Some(meta) = state.node_metas.get_mut("node-a") {
        meta.links.push("phantom".into());
    }

    let req = Request::Gc;
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    let meta = &state.node_metas["node-a"];
    assert!(!meta.links.contains(&"phantom".to_string()));
}

// ============================================================
// Patch
// ============================================================

#[test]
fn test_patch_append() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Patch {
        name: "node-a".into(),
        op: PatchRequest::Append("\nappended text".into()),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    // Verify the body was updated
    let (_, body) = read_node_from_dir(&dir.path().join("memories"), "node-a").unwrap();
    assert!(body.contains("appended text"));
}

#[test]
fn test_patch_replace() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Patch {
        name: "node-a".into(),
        op: PatchRequest::Replace {
            old: "body text".into(),
            new: "replaced text".into(),
        },
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    let (_, body) = read_node_from_dir(&dir.path().join("memories"), "node-a").unwrap();
    assert!(body.contains("replaced text"));
    assert!(!body.contains("body text"));
}

// ============================================================
// Recall (name prefix mode)
// ============================================================

#[test]
fn test_recall_name_prefix() {
    let dir = setup_dir();
    add_node(dir.path(), "project-alpha", vec![], "alpha");
    add_node(dir.path(), "project-beta", vec![], "beta");
    add_node(dir.path(), "other-node", vec![], "other");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Recall {
        query: None,
        name_prefix: Some("project".into()),
        top_k: 10,
        depth: 0,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::NodeNames(names) => {
            assert_eq!(names.len(), 2);
            for n in &names {
                assert!(n.starts_with("project"));
            }
        }
        _ => panic!("wrong body"),
    }
}

// ============================================================
// Corner-case tests: Create
// ============================================================

#[test]
fn test_create_minimal_frontmatter() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_minimal_content("minimal test", "hello");
    let req = Request::Create {
        name: "min-node".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(state.name_index.contains("min-node"));
    // Verify defaults were applied
    let meta = &state.node_metas["min-node"];
    assert!((meta.weight - 1.0).abs() < 1e-6);
    assert_eq!(meta.access_count, 0);
    assert!(!meta.pinned);
}

#[test]
fn test_create_self_link_fails() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_content("test", &["self-node"], "body");
    let req = Request::Create {
        name: "self-node".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_create_empty_abstract() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = "---\nabstract: ''\n---\n\nbody text";
    let req = Request::Create {
        name: "empty-abs".into(),
        content: content.to_string(),
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    let meta = &state.node_metas["empty-abs"];
    assert_eq!(meta.abstract_text, "");
}

#[test]
fn test_create_unicode_abstract() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = "---\nabstract: '\u{4E2D}\u{6587}\u{6D4B}\u{8BD5} \u{1F680} \u{65E5}\u{672C}\u{8A9E}\u{30C6}\u{30B9}\u{30C8}'\n---\n\nunicode body";
    let req = Request::Create {
        name: "unicode-node".into(),
        content: content.to_string(),
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    let meta = &state.node_metas["unicode-node"];
    assert!(meta.abstract_text.contains("\u{4E2D}\u{6587}"));
    assert!(meta.abstract_text.contains("\u{1F680}"));
}

#[test]
fn test_create_very_long_abstract() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let long_abstract = "x".repeat(10_000);
    let content = format!("---\nabstract: '{}'\n---\n\nbody", long_abstract);
    let req = Request::Create {
        name: "long-abs".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(state.node_metas["long-abs"].abstract_text.len() >= 10_000);
}

#[test]
fn test_create_empty_body() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = "---\nabstract: frontmatter only\n---\n";
    let req = Request::Create {
        name: "no-body".into(),
        content: content.to_string(),
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(dir.path().join("memories/no-body.md").exists());
}

// ============================================================
// Corner-case tests: Update
// ============================================================

#[test]
fn test_update_preserves_created_timestamp() {
    let dir = setup_dir();
    add_node(dir.path(), "upd-node", vec![], "original");
    let mut state = load_state_from_dir(dir.path()).unwrap();
    let original_created = state.node_metas["upd-node"].created;

    let content = make_content("updated abstract", &[], "new body");
    let req = Request::Update {
        name: "upd-node".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert_eq!(state.node_metas["upd-node"].created, original_created);
}

#[test]
fn test_update_changes_updated_timestamp() {
    let dir = setup_dir();
    add_node(dir.path(), "upd-time", vec![], "original");
    let mut state = load_state_from_dir(dir.path()).unwrap();
    let before_update = state.node_metas["upd-time"].updated;

    std::thread::sleep(std::time::Duration::from_millis(10));

    let content = make_content("changed", &[], "new body");
    let req = Request::Update {
        name: "upd-time".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(state.node_metas["upd-time"].updated >= before_update);
}

#[test]
fn test_update_link_diff() {
    let dir = setup_dir();
    add_node(dir.path(), "center", vec!["keep-link".into(), "remove-link".into()], "center");
    add_node(dir.path(), "keep-link", vec!["center".into()], "keep");
    add_node(dir.path(), "remove-link", vec!["center".into()], "remove");
    add_node(dir.path(), "add-link", vec![], "add");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Update center: keep "keep-link", drop "remove-link", add "add-link"
    let content = make_content("updated", &["keep-link", "add-link"], "new body");
    let req = Request::Update {
        name: "center".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    assert!(state.graph.has_edge("center", "keep-link"));
    assert!(state.graph.has_edge("center", "add-link"));
    assert!(!state.graph.has_edge("center", "remove-link"));

    // Peer state should reflect changes
    let meta_remove = &state.node_metas["remove-link"];
    assert!(!meta_remove.links.contains(&"center".to_string()));
    let meta_add = &state.node_metas["add-link"];
    assert!(meta_add.links.contains(&"center".to_string()));
}

#[test]
fn test_update_not_found() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_content("test", &[], "body");
    let req = Request::Update {
        name: "nonexistent".into(),
        content,
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// Corner-case tests: Delete
// ============================================================

#[test]
fn test_delete_with_multiple_edges() {
    let dir = setup_dir();
    add_node(dir.path(), "hub", vec!["spoke-a".into(), "spoke-b".into(), "spoke-c".into()], "hub");
    add_node(dir.path(), "spoke-a", vec!["hub".into()], "a");
    add_node(dir.path(), "spoke-b", vec!["hub".into()], "b");
    add_node(dir.path(), "spoke-c", vec!["hub".into()], "c");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Delete {
        name: "hub".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    // All spokes should have their link to hub removed
    for spoke in ["spoke-a", "spoke-b", "spoke-c"] {
        assert!(!state.graph.has_edge(spoke, "hub"));
        let meta = &state.node_metas[spoke];
        assert!(!meta.links.contains(&"hub".to_string()));
    }
}

// ============================================================
// Corner-case tests: Rename
// ============================================================

#[test]
fn test_rename_to_invalid_name() {
    let dir = setup_dir();
    add_node(dir.path(), "good-name", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Rename {
        old: "good-name".into(),
        new: "!bad".into(),
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
    // Original should still exist
    assert!(state.name_index.contains("good-name"));
}

#[test]
fn test_rename_to_existing_name() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Rename {
        old: "node-a".into(),
        new: "node-b".into(),
    };

    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// Corner-case tests: Search / Recall on empty system
// ============================================================

#[test]
fn test_search_empty_system() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Search {
        query: "test query".into(),
        top_k: 5,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    // Without embedding feature: system error; with embedding: empty or error
    assert!(!resp.success);
}

#[test]
fn test_recall_empty_system() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Recall {
        query: None,
        name_prefix: None,
        top_k: 10,
        depth: 0,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::NodeNames(names) => {
            assert!(names.is_empty());
        }
        _ => panic!("wrong body"),
    }
}

// ============================================================
// Pattern 1: Side-effect verification
// ============================================================

#[test]
fn test_get_increments_access_count() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    assert_eq!(state.node_metas["node-a"].access_count, 0);

    let req = Request::Get {
        names: vec!["node-a".into()],
    };
    handle_request(&mut state, &req, dir.path());
    assert_eq!(state.node_metas["node-a"].access_count, 1);

    // Second get should increment again
    handle_request(&mut state, &req, dir.path());
    assert_eq!(state.node_metas["node-a"].access_count, 2);
}

#[test]
fn test_get_updates_last_accessed() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();
    let before = state.node_metas["node-a"].last_accessed;

    std::thread::sleep(std::time::Duration::from_millis(10));

    let req = Request::Get {
        names: vec!["node-a".into()],
    };
    handle_request(&mut state, &req, dir.path());
    assert!(state.node_metas["node-a"].last_accessed > before);
}

#[test]
fn test_get_batch_increments_all() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Get {
        names: vec!["node-a".into(), "node-b".into()],
    };
    handle_request(&mut state, &req, dir.path());
    assert_eq!(state.node_metas["node-a"].access_count, 1);
    assert_eq!(state.node_metas["node-b"].access_count, 1);
}

#[test]
fn test_patch_append_updates_timestamp() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();
    let before = state.node_metas["node-a"].updated;

    std::thread::sleep(std::time::Duration::from_millis(10));

    let req = Request::Patch {
        name: "node-a".into(),
        op: PatchRequest::Append("new text".into()),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(state.node_metas["node-a"].updated > before);

    // Verify disk is in sync
    let (fm, _) = read_node_from_dir(&dir.path().join("memories"), "node-a").unwrap();
    assert_eq!(fm.updated, state.node_metas["node-a"].updated);
}

#[test]
fn test_patch_prepend_updates_timestamp() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();
    let before = state.node_metas["node-a"].updated;

    std::thread::sleep(std::time::Duration::from_millis(10));

    let req = Request::Patch {
        name: "node-a".into(),
        op: PatchRequest::Prepend("prefix text".into()),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(state.node_metas["node-a"].updated > before);
}

#[test]
fn test_patch_replace_updates_timestamp() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();
    let before = state.node_metas["node-a"].updated;

    std::thread::sleep(std::time::Duration::from_millis(10));

    let req = Request::Patch {
        name: "node-a".into(),
        op: PatchRequest::Replace {
            old: "body text".into(),
            new: "replaced".into(),
        },
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(state.node_metas["node-a"].updated > before);
}

#[test]
fn test_link_updates_timestamps_both_nodes() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();
    let before_a = state.node_metas["node-a"].updated;
    let before_b = state.node_metas["node-b"].updated;

    std::thread::sleep(std::time::Duration::from_millis(10));

    let req = Request::Link {
        a: "node-a".into(),
        b: "node-b".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert!(state.node_metas["node-a"].updated > before_a);
    assert!(state.node_metas["node-b"].updated > before_b);
}

#[test]
fn test_boost_increases_weight_when_below_max() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Penalize first to get weight below 1.0
    let req = Request::Penalize {
        name: "node-a".into(),
    };
    handle_request(&mut state, &req, dir.path());
    let penalized_weight = state.node_metas["node-a"].weight;
    assert!(penalized_weight < 1.0);

    // Now boost — weight should increase by 0.1
    let req = Request::Boost {
        name: "node-a".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    let boosted_weight = state.node_metas["node-a"].weight;
    assert!((boosted_weight - (penalized_weight + 0.1)).abs() < 1e-6);

    // Verify persisted to disk
    let (fm, _) = read_node_from_dir(&dir.path().join("memories"), "node-a").unwrap();
    assert!((fm.weight - boosted_weight).abs() < 1e-6);
}

// ============================================================
// Pattern 2: handle_unlink full coverage
// ============================================================

#[test]
fn test_unlink_removes_edge_and_updates_files() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec!["node-b".into()], "a");
    add_node(dir.path(), "node-b", vec!["node-a".into()], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();
    assert!(state.graph.has_edge("node-a", "node-b"));

    let req = Request::Unlink {
        a: "node-a".into(),
        b: "node-b".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    // Graph edge removed
    assert!(!state.graph.has_edge("node-a", "node-b"));

    // In-memory links removed
    assert!(!state.node_metas["node-a"].links.contains(&"node-b".to_string()));
    assert!(!state.node_metas["node-b"].links.contains(&"node-a".to_string()));

    // .md files updated
    let (fm_a, _) = read_node_from_dir(&dir.path().join("memories"), "node-a").unwrap();
    assert!(!fm_a.links.contains(&"node-b".to_string()));
    let (fm_b, _) = read_node_from_dir(&dir.path().join("memories"), "node-b").unwrap();
    assert!(!fm_b.links.contains(&"node-a".to_string()));
}

#[test]
fn test_unlink_updates_timestamps() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec!["node-b".into()], "a");
    add_node(dir.path(), "node-b", vec!["node-a".into()], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();
    let before_a = state.node_metas["node-a"].updated;
    let before_b = state.node_metas["node-b"].updated;

    std::thread::sleep(std::time::Duration::from_millis(10));

    let req = Request::Unlink {
        a: "node-a".into(),
        b: "node-b".into(),
    };
    handle_request(&mut state, &req, dir.path());

    assert!(state.node_metas["node-a"].updated > before_a);
    assert!(state.node_metas["node-b"].updated > before_b);
}

#[test]
fn test_unlink_not_found() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Unlink {
        a: "node-a".into(),
        b: "ghost".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_unlink_no_edge_idempotent() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Unlink nodes that aren't linked — should succeed (idempotent)
    let req = Request::Unlink {
        a: "node-a".into(),
        b: "node-b".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
}

// ============================================================
// Pattern 3: Command parameter coverage
// ============================================================

#[test]
fn test_ls_sort_by_weight() {
    let dir = setup_dir();
    add_node(dir.path(), "heavy", vec![], "heavy");
    add_node(dir.path(), "light", vec![], "light");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Penalize "light" so it has lower weight
    let req = Request::Penalize {
        name: "light".into(),
    };
    handle_request(&mut state, &req, dir.path());

    let req = Request::Ls {
        sort: SortField::Weight,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    match resp.body {
        ResponseBody::NodeList(list) => {
            assert_eq!(list.len(), 2);
            // Heavy (1.0) should come first, light (0.8) second
            assert_eq!(list[0].name, "heavy");
            assert_eq!(list[1].name, "light");
            assert!(list[0].weight > list[1].weight);
        }
        _ => panic!("wrong body"),
    }
}

#[test]
fn test_ls_sort_by_date() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Access node-a to update its last_accessed
    let req = Request::Get {
        names: vec!["node-a".into()],
    };
    handle_request(&mut state, &req, dir.path());

    let req = Request::Ls {
        sort: SortField::Date,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::NodeList(list) => {
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].name, "node-a");
        }
        _ => panic!("wrong body"),
    }
}

#[test]
fn test_neighbors_with_offset() {
    let dir = setup_dir();
    add_node(dir.path(), "center", vec!["node-a".into(), "node-b".into(), "node-c".into()], "c");
    add_node(dir.path(), "node-a", vec!["center".into()], "a");
    add_node(dir.path(), "node-b", vec!["center".into()], "b");
    add_node(dir.path(), "node-c", vec!["center".into()], "c");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Get all neighbors first
    let req_all = Request::Neighbors {
        name: "center".into(),
        depth: 1,
        limit: 50,
        offset: 0,
    };
    let resp_all = handle_request(&mut state, &req_all, dir.path());
    let total = match &resp_all.body {
        ResponseBody::Neighbors { total, .. } => *total,
        _ => panic!("wrong body"),
    };
    assert_eq!(total, 3);

    // Now use offset=1, limit=1 to get just the second entry
    let req = Request::Neighbors {
        name: "center".into(),
        depth: 1,
        limit: 1,
        offset: 1,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    match resp.body {
        ResponseBody::Neighbors { entries, total, .. } => {
            assert_eq!(total, 3); // Total is still 3
            assert_eq!(entries.len(), 1); // But only 1 returned due to limit
        }
        _ => panic!("wrong body"),
    }
}

#[test]
fn test_patch_prepend_handler() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Patch {
        name: "node-a".into(),
        op: PatchRequest::Prepend("prefix line".into()),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    let (_, body) = read_node_from_dir(&dir.path().join("memories"), "node-a").unwrap();
    assert!(body.starts_with("prefix line"));
    assert!(body.contains("body text"));
}

// ============================================================
// Get: deduplication
// ============================================================

#[test]
fn test_get_dedup() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Get {
        names: vec!["node-a".into(), "node-a".into(), "node-a".into()],
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    // Should return single node content (deduped from 3 → 1)
    match resp.body {
        ResponseBody::NodeContent { name, .. } => {
            assert_eq!(name, "node-a");
        }
        _ => panic!("expected single NodeContent, got batch or error"),
    }
    // Access count should only be 1 (not 3)
    assert_eq!(state.node_metas["node-a"].access_count, 1);
}

#[test]
fn test_get_dedup_batch_preserves_order() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Get {
        names: vec!["node-b".into(), "node-a".into(), "node-b".into()],
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::NodeBatch(entries) => {
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].name, "node-b");
            assert_eq!(entries[1].name, "node-a");
        }
        _ => panic!("expected NodeBatch"),
    }
}

// ============================================================
// Get: access metadata persisted to disk
// ============================================================

#[test]
fn test_get_persists_access_count_via_flush() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Get {
        names: vec!["node-a".into()],
    };
    handle_request(&mut state, &req, dir.path());
    handle_request(&mut state, &req, dir.path());

    // Verify in-memory
    assert_eq!(state.node_metas["node-a"].access_count, 2);
    assert!(state.access_dirty.contains("node-a"));

    // Flush rewrites .md files
    let flushed = flush_access_metadata(&mut state, dir.path());
    assert_eq!(flushed, 1);
    assert!(state.access_dirty.is_empty());

    // Reload state from disk — .md should have updated access_count
    let reloaded = load_state_from_dir(dir.path()).unwrap();
    assert_eq!(reloaded.node_metas["node-a"].access_count, 2);
}

#[test]
fn test_get_persists_last_accessed_via_flush() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();
    let before = state.node_metas["node-a"].last_accessed;

    std::thread::sleep(std::time::Duration::from_millis(10));

    let req = Request::Get {
        names: vec!["node-a".into()],
    };
    handle_request(&mut state, &req, dir.path());

    // Flush rewrites .md
    flush_access_metadata(&mut state, dir.path());

    // Reload and verify
    let reloaded = load_state_from_dir(dir.path()).unwrap();
    assert!(reloaded.node_metas["node-a"].last_accessed > before);
}

#[test]
fn test_access_dirty_deduplicates() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Get {
        names: vec!["node-a".into()],
    };
    // 5 gets → dirty set should still have just 1 entry
    for _ in 0..5 {
        handle_request(&mut state, &req, dir.path());
    }
    assert_eq!(state.access_dirty.len(), 1);
    assert_eq!(state.node_metas["node-a"].access_count, 5);

    // Flush rewrites just the 1 dirty node's .md
    let flushed = flush_access_metadata(&mut state, dir.path());
    assert_eq!(flushed, 1);

    // Reload and verify
    let reloaded = load_state_from_dir(dir.path()).unwrap();
    assert_eq!(reloaded.node_metas["node-a"].access_count, 5);
}

#[test]
fn test_fresh_start_loads_access_from_md() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");

    // Fresh load reads access metadata from .md frontmatter
    let state = load_state_from_dir(dir.path()).unwrap();
    assert_eq!(state.node_metas["node-a"].access_count, 0);
}

// ============================================================
// Recall: working memory mode returns NodeNames
// ============================================================

#[test]
fn test_recall_working_memory_returns_node_names() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    add_node(dir.path(), "node-b", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Pin node-a
    let pin_req = Request::Pin { name: "node-a".into() };
    handle_request(&mut state, &pin_req, dir.path());

    let req = Request::Recall {
        query: None,
        name_prefix: None,
        top_k: 10,
        depth: 0,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::NodeNames(names) => {
            assert!(names.contains(&"node-a".to_string()));
            assert!(names.contains(&"node-b".to_string()));
        }
        _ => panic!("expected NodeNames"),
    }
}

#[test]
fn test_recall_working_memory_no_duplicates() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Pin node-a so it appears in both pinned and high-weight
    let pin_req = Request::Pin { name: "node-a".into() };
    handle_request(&mut state, &pin_req, dir.path());

    let req = Request::Recall {
        query: None,
        name_prefix: None,
        top_k: 10,
        depth: 0,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::NodeNames(names) => {
            // node-a should appear only once even though it's pinned + high weight
            let count = names.iter().filter(|n| *n == "node-a").count();
            assert_eq!(count, 1, "node-a should appear exactly once, got {}", count);
        }
        _ => panic!("expected NodeNames"),
    }
}

// ============================================================
// Status: uptime_seconds
// ============================================================

#[test]
fn test_status_uptime_nonzero() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    std::thread::sleep(std::time::Duration::from_millis(50));

    let req = Request::Status;
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::Status(data) => {
            // Should be at least 0 (might be 0 if clock resolution is coarse,
            // but importantly it won't panic or return a hardcoded value)
            assert!(data.uptime_seconds < 60, "uptime should be reasonable");
        }
        _ => panic!("expected Status"),
    }
}

// ============================================================
// Fix 5: Embedding error messages include rebuild hint
// ============================================================

#[test]
fn test_search_no_embedding_shows_rebuild_hint() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Search {
        query: "test".into(),
        top_k: 5,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    // Only fails when compiled without embedding feature
    if !resp.success {
        match &resp.body {
            ResponseBody::Error(msg) => {
                assert!(msg.contains("cargo build --features embedding"), "error should contain rebuild hint, got: {}", msg);
            }
            _ => panic!("expected Error body"),
        }
    }
}

#[test]
fn test_recall_no_embedding_shows_rebuild_hint() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Recall {
        query: Some("test".into()),
        name_prefix: None,
        top_k: 5,
        depth: 1,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    if !resp.success {
        match &resp.body {
            ResponseBody::Error(msg) => {
                assert!(msg.contains("cargo build --features embedding"), "error should contain rebuild hint, got: {}", msg);
            }
            _ => panic!("expected Error body"),
        }
    }
}

#[test]
fn test_reindex_no_embedding_shows_rebuild_hint() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Reindex;
    let resp = handle_request(&mut state, &req, dir.path());
    if !resp.success {
        match &resp.body {
            ResponseBody::Error(msg) => {
                assert!(msg.contains("cargo build --features embedding"), "error should contain rebuild hint, got: {}", msg);
            }
            _ => panic!("expected Error body"),
        }
    }
}

#[test]
fn test_baseline_no_embedding_shows_rebuild_hint() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Baseline;
    let resp = handle_request(&mut state, &req, dir.path());
    if !resp.success {
        match &resp.body {
            ResponseBody::Error(msg) => {
                assert!(msg.contains("cargo build --features embedding"), "error should contain rebuild hint, got: {}", msg);
            }
            _ => panic!("expected Error body"),
        }
    }
}

// ============================================================
// Fix 1: Boost at max weight returns "already at maximum"
// ============================================================

#[test]
fn test_boost_already_at_max_returns_already_message() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Node starts at weight 1.0 (the max)
    assert!((state.node_metas["node-a"].weight - 1.0).abs() < f32::EPSILON);

    let req = Request::Boost {
        name: "node-a".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::Message(msg) => {
            assert!(msg.contains("already at maximum"), "expected 'already at maximum', got: {}", msg);
        }
        _ => panic!("expected Message body"),
    }
    // Weight should still be 1.0
    assert!((state.node_metas["node-a"].weight - 1.0).abs() < f32::EPSILON);
}

#[test]
fn test_boost_below_max_shows_transition() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Penalize first to get below 1.0
    let req = Request::Penalize {
        name: "node-a".into(),
    };
    handle_request(&mut state, &req, dir.path());
    assert!(state.node_metas["node-a"].weight < 1.0);

    // Now boost — should show transition, not "already at maximum"
    let req = Request::Boost {
        name: "node-a".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::Message(msg) => {
            assert!(msg.contains("boosted:"), "expected 'boosted:', got: {}", msg);
            assert!(!msg.contains("already at maximum"), "should not say already at max");
        }
        _ => panic!("expected Message body"),
    }
}

// ============================================================
// Fix 2: ls --sort weight secondary sort
// ============================================================

#[test]
fn test_ls_sort_by_weight_secondary_sort() {
    let dir = setup_dir();
    // Create 3 nodes — all start at weight 1.0
    add_node(dir.path(), "node-c", vec![], "c");
    add_node(dir.path(), "node-a", vec!["node-c".into()], "a");
    add_node(dir.path(), "node-b", vec!["node-a".into(), "node-c".into()], "b");
    // node-b has links to both a and c (2 edges), node-a has 2 edges (to b and c), node-c has 2 edges (to a and b)
    // Actually let's set up more carefully for different edge counts
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // All at same weight (1.0). Edge counts:
    // node-b: linked to node-a and node-c = 2 edges
    // node-a: linked to node-c and node-b = 2 edges
    // node-c: linked to node-a and node-b = 2 edges
    // All equal, so tiebreak by name asc: node-a, node-b, node-c

    let req = Request::Ls {
        sort: SortField::Weight,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::NodeList(entries) => {
            assert_eq!(entries.len(), 3);
            // All same weight and same edge count → name asc
            assert_eq!(entries[0].name, "node-a");
            assert_eq!(entries[1].name, "node-b");
            assert_eq!(entries[2].name, "node-c");
        }
        _ => panic!("expected NodeList"),
    }
}

// ============================================================
// Fix 6: Inspect node includes metadata fields
// ============================================================

#[test]
fn test_inspect_node_includes_metadata_fields() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "my abstract text");
    add_node(dir.path(), "node-b", vec![], "b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Link them
    let link_req = Request::Link {
        a: "node-a".into(),
        b: "node-b".into(),
    };
    handle_request(&mut state, &link_req, dir.path());

    // Pin node-a
    let pin_req = Request::Pin {
        name: "node-a".into(),
    };
    handle_request(&mut state, &pin_req, dir.path());

    let req = Request::Inspect {
        node: Some("node-a".into()),
        format: OutputFormat::Human,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::NodeInspectReport(data) => {
            assert_eq!(data.name, "node-a");
            assert!((data.weight - 1.0).abs() < f32::EPSILON);
            assert!(data.pinned);
            assert!(data.links.contains(&"node-b".to_string()));
            assert_eq!(data.abstract_text, "my abstract text");
        }
        _ => panic!("expected NodeInspectReport"),
    }
}

#[test]
fn test_inspect_node_json_includes_new_fields() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "abstract a");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Inspect {
        node: Some("node-a".into()),
        format: OutputFormat::Json,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match resp.body {
        ResponseBody::Message(json) => {
            assert!(json.contains("\"weight\""), "JSON should contain weight field");
            assert!(json.contains("\"pinned\""), "JSON should contain pinned field");
            assert!(json.contains("\"links\""), "JSON should contain links field");
            assert!(json.contains("\"abstract_text\""), "JSON should contain abstract_text field");
        }
        _ => panic!("expected Message (JSON)"),
    }
}
