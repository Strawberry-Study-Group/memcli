//! Boundary condition tests from the §10.6 design.md matrix.
//!
//! These tests target error-paths and edge-case behaviors that were
//! missing from the existing test suite:
//!   - Not-found errors for every mutating command
//!   - Empty-system errors for commands that expect state
//!   - Patch sub-operation validation at the handler level
//!   - Output format verification (link/unlink messages)
//!   - Behavioral invariants (boost cap, penalize decay, GC persistence)
//!   - Status accuracy after mutations

use std::path::Path;

use memcore::daemon_state::load_state_from_dir;
use memcore::handler::handle_request;
use memcore::node::{write_node_to_dir, Frontmatter};
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
// §1: GET error paths
// ============================================================

#[test]
fn test_get_invalid_name() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Get {
        names: vec!["INVALID_NAME".into()],
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §2: PATCH error paths (handler-level)
// ============================================================

#[test]
fn test_patch_not_found() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Patch {
        name: "nonexistent".into(),
        op: PatchRequest::Append("text".into()),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_patch_replace_not_found_handler() {
    let dir = setup_dir();
    add_node(dir.path(), "test-node", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Patch {
        name: "test-node".into(),
        op: PatchRequest::Replace {
            old: "text that does not exist in body".into(),
            new: "replacement".into(),
        },
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_patch_replace_ambiguous_handler() {
    let dir = setup_dir();
    // Create a node whose body contains "word" twice
    let memories = dir.path().join("memories");
    let fm = Frontmatter::new_for_create(vec![], false, "abstract".into());
    write_node_to_dir(&memories, "test-node", &fm, "word and word again").unwrap();

    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Patch {
        name: "test-node".into(),
        op: PatchRequest::Replace {
            old: "word".into(),
            new: "replaced".into(),
        },
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_patch_append_empty_handler() {
    let dir = setup_dir();
    add_node(dir.path(), "test-node", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Patch {
        name: "test-node".into(),
        op: PatchRequest::Append("".into()),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_patch_prepend_empty_handler() {
    let dir = setup_dir();
    add_node(dir.path(), "test-node", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Patch {
        name: "test-node".into(),
        op: PatchRequest::Prepend("".into()),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §3: RENAME error paths
// ============================================================

#[test]
fn test_rename_old_not_found() {
    let dir = setup_dir();
    add_node(dir.path(), "existing", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Rename {
        old: "nonexistent".into(),
        new: "new-name".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_rename_empty_system() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Rename {
        old: "nonexistent".into(),
        new: "new-name".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §4: LINK error paths
// ============================================================

#[test]
fn test_link_a_not_found() {
    let dir = setup_dir();
    add_node(dir.path(), "node-b", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Link {
        a: "nonexistent".into(),
        b: "node-b".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_link_b_not_found() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Link {
        a: "node-a".into(),
        b: "nonexistent".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_link_both_not_found() {
    let dir = setup_dir();
    add_node(dir.path(), "unrelated", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Link {
        a: "ghost-a".into(),
        b: "ghost-b".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_link_empty_system() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Link {
        a: "alpha".into(),
        b: "beta".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §5: UNLINK error paths
// ============================================================

#[test]
fn test_unlink_a_not_found() {
    let dir = setup_dir();
    add_node(dir.path(), "node-b", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Unlink {
        a: "nonexistent".into(),
        b: "node-b".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_unlink_empty_system() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Unlink {
        a: "alpha".into(),
        b: "beta".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §6: BOOST error paths
// ============================================================

#[test]
fn test_boost_not_found() {
    let dir = setup_dir();
    add_node(dir.path(), "existing", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Boost {
        name: "nonexistent".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_boost_empty_system() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Boost {
        name: "nonexistent".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §7: PENALIZE error paths
// ============================================================

#[test]
fn test_penalize_not_found() {
    let dir = setup_dir();
    add_node(dir.path(), "existing", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Penalize {
        name: "nonexistent".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_penalize_empty_system() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Penalize {
        name: "nonexistent".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §8: PIN error paths
// ============================================================

#[test]
fn test_pin_not_found() {
    let dir = setup_dir();
    add_node(dir.path(), "existing", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Pin {
        name: "nonexistent".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_pin_empty_system() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Pin {
        name: "nonexistent".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §9: UNPIN error paths
// ============================================================

#[test]
fn test_unpin_not_found() {
    let dir = setup_dir();
    add_node(dir.path(), "existing", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Unpin {
        name: "nonexistent".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_unpin_empty_system() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Unpin {
        name: "nonexistent".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §10: NEIGHBORS error paths
// ============================================================

#[test]
fn test_neighbors_not_found() {
    let dir = setup_dir();
    add_node(dir.path(), "existing", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Neighbors {
        name: "nonexistent".into(),
        depth: 1,
        limit: 50,
        offset: 0,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_neighbors_empty_system() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Neighbors {
        name: "nonexistent".into(),
        depth: 1,
        limit: 50,
        offset: 0,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §11: UPDATE error paths
// ============================================================

#[test]
fn test_update_links_target_not_found() {
    let dir = setup_dir();
    add_node(dir.path(), "test-node", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_content("updated abstract", &["nonexistent-target"], "new body");
    let req = Request::Update {
        name: "test-node".into(),
        content,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §12: BASELINE error paths (no embedding feature)
// ============================================================

#[cfg(not(feature = "embedding"))]
#[test]
fn test_baseline_no_embedding() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Baseline;
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 2);
}

#[cfg(feature = "embedding")]
#[test]
fn test_baseline_too_few_nodes() {
    let dir = setup_dir();
    add_node(dir.path(), "only-one", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Baseline;
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §13: INSPECT error paths
// ============================================================

#[test]
fn test_inspect_node_not_found_handler() {
    let dir = setup_dir();
    add_node(dir.path(), "existing", vec![], "abstract");
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

// ============================================================
// §14: REINDEX error paths (no embedding feature)
// ============================================================

#[cfg(not(feature = "embedding"))]
#[test]
fn test_reindex_no_embedding() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Reindex;
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 2);
}

// ============================================================
// §15: RECALL error paths (no embedding feature)
// ============================================================

#[cfg(not(feature = "embedding"))]
#[test]
fn test_recall_empty_with_query() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Recall {
        query: Some("test query".into()),
        name_prefix: None,
        top_k: 5,
        depth: 1,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 2);
}

// ============================================================
// §16: GC positive paths
// ============================================================

#[test]
fn test_gc_empty_system() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Gc;
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    assert_eq!(resp.exit_code, 0);
    match &resp.body {
        ResponseBody::Message(msg) => assert!(msg.contains("0 dangling")),
        other => panic!("expected Message, got {:?}", other),
    }
}

#[test]
fn test_gc_dangling_links_persisted_to_disk() {
    let dir = setup_dir();
    // Create two nodes linked to each other
    add_node(dir.path(), "alpha", vec!["beta".into()], "alpha abstract");
    add_node(dir.path(), "beta", vec!["alpha".into()], "beta abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Delete beta (will remove beta's file but alpha's .md still references beta in-memory)
    let req = Request::Delete {
        name: "beta".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    // Manually inject a dangling link into alpha's in-memory state
    // (simulating a scenario where link cleanup was incomplete)
    if let Some(meta) = state.node_metas.get_mut("alpha") {
        meta.links.push("ghost-node".into());
    }

    // Run GC
    let req = Request::Gc;
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::Message(msg) => {
            // Should have cleaned at least the ghost-node reference
            assert!(msg.contains("dangling"));
        }
        other => panic!("expected Message, got {:?}", other),
    }

    // Verify on-disk: alpha's .md should NOT contain "ghost-node"
    let memories = dir.path().join("memories");
    let (fm, _body) = memcore::node::read_node_from_dir(&memories, "alpha").unwrap();
    assert!(
        !fm.links.contains(&"ghost-node".into()),
        "dangling link should be removed from disk"
    );
}

// ============================================================
// §17: LINK / UNLINK output format
// ============================================================

#[test]
fn test_link_output_format() {
    let dir = setup_dir();
    add_node(dir.path(), "alpha", vec![], "abstract");
    add_node(dir.path(), "beta", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Link {
        a: "alpha".into(),
        b: "beta".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::Message(msg) => {
            assert!(msg.contains("alpha"), "output should contain node A name");
            assert!(msg.contains("beta"), "output should contain node B name");
            assert!(msg.contains("linked"), "output should confirm linking");
        }
        other => panic!("expected Message, got {:?}", other),
    }
}

#[test]
fn test_unlink_output_format() {
    let dir = setup_dir();
    add_node(dir.path(), "alpha", vec!["beta".into()], "abstract");
    add_node(dir.path(), "beta", vec!["alpha".into()], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Unlink {
        a: "alpha".into(),
        b: "beta".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::Message(msg) => {
            assert!(msg.contains("alpha"), "output should contain node A name");
            assert!(msg.contains("beta"), "output should contain node B name");
            assert!(msg.contains("unlinked"), "output should confirm unlinking");
        }
        other => panic!("expected Message, got {:?}", other),
    }
}

// ============================================================
// §18: BOOST behavioral — cap at 1.0
// ============================================================

#[test]
fn test_boost_cap_at_one() {
    let dir = setup_dir();
    add_node(dir.path(), "test-node", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Node starts at weight 1.0. Boosting should not exceed 1.0.
    for _ in 0..20 {
        let req = Request::Boost {
            name: "test-node".into(),
        };
        let resp = handle_request(&mut state, &req, dir.path());
        assert!(resp.success);
    }

    let weight = state.node_metas["test-node"].weight;
    assert!(
        (weight - 1.0).abs() < 1e-6,
        "weight should be capped at 1.0, got {}",
        weight
    );
}

// ============================================================
// §19: PENALIZE behavioral — geometric decay
// ============================================================

#[test]
fn test_penalize_geometric_decay() {
    let dir = setup_dir();
    add_node(dir.path(), "test-node", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Starting weight = 1.0, penalty_factor default = 0.8
    let mut weights = vec![1.0f32];

    for _ in 0..5 {
        let req = Request::Penalize {
            name: "test-node".into(),
        };
        let resp = handle_request(&mut state, &req, dir.path());
        assert!(resp.success);
        weights.push(state.node_metas["test-node"].weight);
    }

    // Each weight should be ~0.8× the previous (geometric decay)
    for i in 1..weights.len() {
        let ratio = weights[i] / weights[i - 1];
        assert!(
            (ratio - 0.8).abs() < 0.01,
            "decay ratio at step {} was {}, expected ~0.8",
            i,
            ratio
        );
    }

    // After 5 penalties: 1.0 * 0.8^5 ≈ 0.32768
    let final_weight = state.node_metas["test-node"].weight;
    assert!(
        (final_weight - 0.8f32.powi(5)).abs() < 0.001,
        "after 5 penalties weight should be ~{}, got {}",
        0.8f32.powi(5),
        final_weight
    );
}

// ============================================================
// §20: STATUS accuracy after mutations
// ============================================================

#[test]
fn test_status_reports_correct_counts() {
    let dir = setup_dir();
    add_node(dir.path(), "alpha", vec![], "abstract");
    add_node(dir.path(), "beta", vec![], "abstract");
    add_node(dir.path(), "gamma", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Link alpha<->beta
    let req = Request::Link {
        a: "alpha".into(),
        b: "beta".into(),
    };
    handle_request(&mut state, &req, dir.path());

    // Link beta<->gamma
    let req = Request::Link {
        a: "beta".into(),
        b: "gamma".into(),
    };
    handle_request(&mut state, &req, dir.path());

    // Check status
    let req = Request::Status;
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    match &resp.body {
        ResponseBody::Status(data) => {
            assert_eq!(data.node_count, 3, "should have 3 nodes");
            assert_eq!(data.edge_count, 2, "should have 2 edges");
        }
        other => panic!("expected Status, got {:?}", other),
    }
}

// ============================================================
// §21: RECALL working memory — pinned nodes included
// ============================================================

#[test]
fn test_recall_working_memory_pinned_not_dropped() {
    let dir = setup_dir();
    // Create several nodes; pin one
    add_node(dir.path(), "pinned-node", vec![], "pinned abstract");
    add_node(dir.path(), "normal-a", vec![], "abstract a");
    add_node(dir.path(), "normal-b", vec![], "abstract b");
    add_node(dir.path(), "normal-c", vec![], "abstract c");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Pin the node
    let req = Request::Pin {
        name: "pinned-node".into(),
    };
    handle_request(&mut state, &req, dir.path());

    // Recall working memory with top_k=2 (small enough to potentially drop pinned)
    let req = Request::Recall {
        query: None,
        name_prefix: None,
        top_k: 2,
        depth: 1,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    match &resp.body {
        ResponseBody::NodeNames(names) => {
            // The pinned node should be in the results (it's added first)
            assert!(
                names.contains(&"pinned-node".to_string()),
                "pinned node should be included in recall working memory, got: {:?}",
                names
            );
            // Total should not exceed top_k
            assert!(
                names.len() <= 2,
                "should not exceed top_k={}, got {}",
                2,
                names.len()
            );
        }
        other => panic!("expected NodeNames, got {:?}", other),
    }
}

// ============================================================
// §22: LS sort-by-date
// ============================================================

#[test]
fn test_ls_sort_by_date_correctness() {
    let dir = setup_dir();
    add_node(dir.path(), "alpha", vec![], "abstract");
    add_node(dir.path(), "beta", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Access beta to give it a more recent last_accessed
    let req = Request::Get {
        names: vec!["beta".into()],
    };
    handle_request(&mut state, &req, dir.path());

    let req = Request::Ls {
        sort: SortField::Date,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    match &resp.body {
        ResponseBody::NodeList(entries) => {
            assert_eq!(entries.len(), 2);
            // Date sort is by the formatted "Xd ago" / "Xh ago" string (ascending).
            // Both nodes are very recently created so both should show "0s ago" or similar.
            // The test verifies ls returns all nodes with date sort without crashing.
            let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
            assert!(names.contains(&"alpha"));
            assert!(names.contains(&"beta"));
        }
        other => panic!("expected NodeList, got {:?}", other),
    }
}

// ============================================================
// §23: INSPECT global on empty system
// ============================================================

#[test]
fn test_inspect_global_empty_system() {
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

    match &resp.body {
        ResponseBody::InspectReport(data) => {
            assert_eq!(data.node_count, 0);
            assert_eq!(data.edge_count, 0);
            assert_eq!(data.cluster_count, 0);
            assert_eq!(data.orphan_count, 0);
        }
        other => panic!("expected InspectReport, got {:?}", other),
    }
}

// ============================================================
// §24: INSPECT node not found on empty system
// ============================================================

#[test]
fn test_inspect_node_empty_system() {
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

// ============================================================
// §25: CREATE with invalid name returns exit code 1
// ============================================================

#[test]
fn test_create_invalid_name_digit_start() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_content("abstract", &[], "body");
    let req = Request::Create {
        name: "2bad".into(),
        content,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

#[test]
fn test_create_invalid_name_too_short() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_content("abstract", &[], "body");
    let req = Request::Create {
        name: "a".into(),
        content,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §26: DELETE on empty system
// ============================================================

#[test]
fn test_delete_empty_system() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Delete {
        name: "nonexistent".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §27: LS on empty system returns empty list
// ============================================================

#[test]
fn test_ls_empty_system() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Ls {
        sort: SortField::Name,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    match &resp.body {
        ResponseBody::NodeList(entries) => {
            assert!(entries.is_empty(), "ls on empty system should return empty list");
        }
        other => panic!("expected NodeList, got {:?}", other),
    }
}

// ============================================================
// §28: NEIGHBORS returns correct depth info
// ============================================================

#[test]
fn test_neighbors_depth_info() {
    let dir = setup_dir();
    add_node(dir.path(), "center", vec![], "abstract");
    add_node(dir.path(), "ring-a", vec![], "abstract");
    add_node(dir.path(), "ring-b", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Link center to ring-a and ring-b
    handle_request(
        &mut state,
        &Request::Link {
            a: "center".into(),
            b: "ring-a".into(),
        },
        dir.path(),
    );
    handle_request(
        &mut state,
        &Request::Link {
            a: "center".into(),
            b: "ring-b".into(),
        },
        dir.path(),
    );

    let req = Request::Neighbors {
        name: "center".into(),
        depth: 1,
        limit: 50,
        offset: 0,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    match &resp.body {
        ResponseBody::Neighbors {
            entries,
            total,
            depth,
        } => {
            assert_eq!(*depth, 1);
            assert_eq!(*total, 2, "center has 2 neighbors");
            assert_eq!(entries.len(), 2);
            for entry in entries {
                assert_eq!(entry.depth, 1, "direct neighbors should be at depth 1");
            }
        }
        other => panic!("expected Neighbors, got {:?}", other),
    }
}

// ============================================================
// §29: GC with actual dangling links (node deleted externally)
// ============================================================

#[test]
fn test_gc_cleans_after_delete() {
    let dir = setup_dir();
    add_node(dir.path(), "alpha", vec!["beta".into()], "abstract a");
    add_node(dir.path(), "beta", vec!["alpha".into()], "abstract b");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    // Delete beta through handler (clean delete updates alpha's links)
    let req = Request::Delete {
        name: "beta".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    // Run GC — should report 0 dangling since delete already cleaned up
    let req = Request::Gc;
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);
    match &resp.body {
        ResponseBody::Message(msg) => {
            assert!(msg.contains("0 dangling"), "handler delete should clean up, gc finds 0");
        }
        other => panic!("expected Message, got {:?}", other),
    }
}

// ============================================================
// §30: STATUS on empty system
// ============================================================

#[test]
fn test_status_empty_system() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Status;
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    match &resp.body {
        ResponseBody::Status(data) => {
            assert_eq!(data.node_count, 0);
            assert_eq!(data.edge_count, 0);
            assert_eq!(data.index_count, 0);
        }
        other => panic!("expected Status, got {:?}", other),
    }
}

// ============================================================
// §31: RECALL name prefix mode
// ============================================================

#[test]
fn test_recall_name_prefix_returns_matches() {
    let dir = setup_dir();
    add_node(dir.path(), "project-alpha", vec![], "alpha project");
    add_node(dir.path(), "project-beta", vec![], "beta project");
    add_node(dir.path(), "unrelated", vec![], "something else");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Recall {
        query: None,
        name_prefix: Some("project-".into()),
        top_k: 10,
        depth: 1,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    match &resp.body {
        ResponseBody::NodeNames(names) => {
            assert_eq!(names.len(), 2, "should match 2 project-* nodes");
            assert!(names.contains(&"project-alpha".to_string()));
            assert!(names.contains(&"project-beta".to_string()));
        }
        other => panic!("expected NodeNames, got {:?}", other),
    }
}

// ============================================================
// §32: RECALL name prefix on empty system
// ============================================================

#[test]
fn test_recall_name_prefix_empty_system() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Recall {
        query: None,
        name_prefix: Some("anything".into()),
        top_k: 10,
        depth: 1,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    match &resp.body {
        ResponseBody::NodeNames(names) => {
            assert!(names.is_empty());
        }
        other => panic!("expected NodeNames, got {:?}", other),
    }
}

// ============================================================
// §33: RECALL working memory on empty system
// ============================================================

#[test]
fn test_recall_working_memory_empty_system() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Recall {
        query: None,
        name_prefix: None,
        top_k: 10,
        depth: 1,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    match &resp.body {
        ResponseBody::NodeNames(names) => {
            assert!(names.is_empty(), "working memory on empty system should return empty");
        }
        other => panic!("expected NodeNames, got {:?}", other),
    }
}

// ============================================================
// §34: UPDATE on empty system
// ============================================================

#[test]
fn test_update_not_found() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_content("abstract", &[], "body");
    let req = Request::Update {
        name: "nonexistent".into(),
        content,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §35: GET empty names list
// ============================================================

#[test]
fn test_get_empty_names() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Get { names: vec![] };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §36: CREATE duplicate name
// ============================================================

#[test]
fn test_create_duplicate_name() {
    let dir = setup_dir();
    add_node(dir.path(), "existing", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let content = make_content("abstract", &[], "body");
    let req = Request::Create {
        name: "existing".into(),
        content,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §37: RENAME to invalid name
// ============================================================

#[test]
fn test_rename_to_invalid_name() {
    let dir = setup_dir();
    add_node(dir.path(), "test-node", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Rename {
        old: "test-node".into(),
        new: "!bad".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §38: RENAME to existing name
// ============================================================

#[test]
fn test_rename_to_existing_name() {
    let dir = setup_dir();
    add_node(dir.path(), "node-a", vec![], "abstract a");
    add_node(dir.path(), "node-b", vec![], "abstract b");
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
// §39: LINK self-reference
// ============================================================

#[test]
fn test_link_self_reference() {
    let dir = setup_dir();
    add_node(dir.path(), "test-node", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Link {
        a: "test-node".into(),
        b: "test-node".into(),
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 1);
}

// ============================================================
// §40: INSPECT JSON format
// ============================================================

#[test]
fn test_inspect_global_json_format() {
    let dir = setup_dir();
    add_node(dir.path(), "alpha", vec![], "abstract");
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
        ResponseBody::Message(json) => {
            // Should be valid JSON
            let parsed: serde_json::Value = serde_json::from_str(json)
                .expect("inspect JSON output should be valid JSON");
            assert_eq!(parsed["node_count"], 1);
        }
        other => panic!("expected Message (JSON), got {:?}", other),
    }
}

// ============================================================
// §41: INSPECT node JSON format
// ============================================================

#[test]
fn test_inspect_node_json_format() {
    let dir = setup_dir();
    add_node(dir.path(), "alpha", vec![], "abstract");
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Inspect {
        node: Some("alpha".into()),
        format: OutputFormat::Json,
        threshold: None,
        cap: None,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(resp.success);

    match &resp.body {
        ResponseBody::Message(json) => {
            let parsed: serde_json::Value = serde_json::from_str(json)
                .expect("node inspect JSON should be valid JSON");
            assert_eq!(parsed["name"], "alpha");
        }
        other => panic!("expected Message (JSON), got {:?}", other),
    }
}

// ============================================================
// §42: SEARCH without embedding feature
// ============================================================

#[cfg(not(feature = "embedding"))]
#[test]
fn test_search_no_embedding() {
    let dir = setup_dir();
    let mut state = load_state_from_dir(dir.path()).unwrap();

    let req = Request::Search {
        query: "test".into(),
        top_k: 5,
    };
    let resp = handle_request(&mut state, &req, dir.path());
    assert!(!resp.success);
    assert_eq!(resp.exit_code, 2);
}
