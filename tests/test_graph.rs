use memcore::graph::{Graph, serialize_graph_idx, deserialize_graph_idx, GraphIdxError};

// ============================================================
// Basic edge operations
// ============================================================

#[test]
fn test_add_edge_bidirectional() {
    let mut g = Graph::new();
    assert!(g.add_edge("a", "b")); // returns true (new edge)
    assert!(g.has_edge("a", "b"));
    assert!(g.has_edge("b", "a"));
    assert_eq!(g.edge_count(), 1);
}

#[test]
fn test_add_edge_idempotent() {
    let mut g = Graph::new();
    assert!(g.add_edge("a", "b"));
    assert!(!g.add_edge("a", "b")); // returns false (already exists)
    assert_eq!(g.edge_count(), 1);
}

#[test]
fn test_add_edge_reverse_idempotent() {
    let mut g = Graph::new();
    g.add_edge("a", "b");
    assert!(!g.add_edge("b", "a")); // same edge in reverse
    assert_eq!(g.edge_count(), 1);
}

#[test]
fn test_remove_edge_bidirectional() {
    let mut g = Graph::new();
    g.add_edge("a", "b");
    assert!(g.remove_edge("a", "b"));
    assert!(!g.has_edge("a", "b"));
    assert!(!g.has_edge("b", "a"));
    assert_eq!(g.edge_count(), 0);
}

#[test]
fn test_remove_edge_reverse_direction() {
    let mut g = Graph::new();
    g.add_edge("a", "b");
    assert!(g.remove_edge("b", "a")); // remove from other direction
    assert!(!g.has_edge("a", "b"));
    assert_eq!(g.edge_count(), 0);
}

#[test]
fn test_remove_nonexistent_edge() {
    let mut g = Graph::new();
    g.ensure_node("a");
    assert!(!g.remove_edge("a", "b"));
    assert_eq!(g.edge_count(), 0);
}

#[test]
fn test_remove_edge_from_empty_graph() {
    let mut g = Graph::new();
    assert!(!g.remove_edge("a", "b"));
}

// ============================================================
// Node operations
// ============================================================

#[test]
fn test_remove_node_returns_neighbors() {
    let mut g = Graph::new();
    g.add_edge("a", "b");
    g.add_edge("a", "c");
    g.add_edge("b", "c");

    let mut removed = g.remove_node("a");
    removed.sort();
    assert_eq!(removed, vec!["b", "c"]);
    assert!(g.has_edge("b", "c"));
    assert!(!g.has_edge("a", "b"));
    assert!(!g.has_edge("a", "c"));
    assert_eq!(g.edge_count(), 1);
    assert_eq!(g.node_count(), 2); // a removed from adjacency
}

#[test]
fn test_remove_isolated_node() {
    let mut g = Graph::new();
    g.ensure_node("lonely");
    assert_eq!(g.node_count(), 1);

    let removed = g.remove_node("lonely");
    assert!(removed.is_empty());
    assert_eq!(g.node_count(), 0);
}

#[test]
fn test_remove_nonexistent_node() {
    let mut g = Graph::new();
    let removed = g.remove_node("ghost");
    assert!(removed.is_empty());
}

#[test]
fn test_ensure_node_creates_isolated() {
    let mut g = Graph::new();
    g.ensure_node("lonely");
    assert_eq!(g.node_count(), 1);
    assert_eq!(g.edge_count(), 0);
    assert!(g.neighbors("lonely").unwrap().is_empty());
}

#[test]
fn test_ensure_node_idempotent() {
    let mut g = Graph::new();
    g.add_edge("a", "b");
    g.ensure_node("a"); // already exists with edges
    assert_eq!(g.node_count(), 2);
    assert!(g.has_edge("a", "b")); // edge preserved
}

// ============================================================
// Neighbors
// ============================================================

#[test]
fn test_neighbors_existing() {
    let mut g = Graph::new();
    g.add_edge("a", "b");
    g.add_edge("a", "c");
    let neighbors = g.neighbors("a").unwrap();
    assert!(neighbors.contains("b"));
    assert!(neighbors.contains("c"));
    assert_eq!(neighbors.len(), 2);
}

#[test]
fn test_neighbors_nonexistent() {
    let g = Graph::new();
    assert!(g.neighbors("nope").is_none());
}

#[test]
fn test_has_edge_nonexistent_nodes() {
    let g = Graph::new();
    assert!(!g.has_edge("a", "b"));
}

// ============================================================
// BFS
// ============================================================

#[test]
fn test_bfs_depth_1() {
    let mut g = Graph::new();
    g.add_edge("a", "b");
    g.add_edge("b", "c");
    g.add_edge("c", "d");

    let result = g.bfs("a", 1);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], ("b".to_string(), 1));
}

#[test]
fn test_bfs_depth_2() {
    let mut g = Graph::new();
    g.add_edge("a", "b");
    g.add_edge("b", "c");
    g.add_edge("c", "d");

    let result = g.bfs("a", 2);
    assert_eq!(result.len(), 2);
    // b at depth 1, c at depth 2
    let names: Vec<&str> = result.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"b"));
    assert!(names.contains(&"c"));
}

#[test]
fn test_bfs_depth_0_returns_empty() {
    let mut g = Graph::new();
    g.add_edge("a", "b");
    let result = g.bfs("a", 0);
    assert!(result.is_empty()); // start node excluded, depth 0 = no traversal
}

#[test]
fn test_bfs_excludes_start_node() {
    let mut g = Graph::new();
    g.add_edge("a", "b");
    let result = g.bfs("a", 10);
    let names: Vec<&str> = result.iter().map(|(n, _)| n.as_str()).collect();
    assert!(!names.contains(&"a")); // start never in result
}

#[test]
fn test_bfs_cycle_no_infinite_loop() {
    let mut g = Graph::new();
    g.add_edge("a", "b");
    g.add_edge("b", "c");
    g.add_edge("c", "a"); // cycle

    let result = g.bfs("a", 10);
    assert_eq!(result.len(), 2); // b and c, no duplicates
}

#[test]
fn test_bfs_isolated_node() {
    let mut g = Graph::new();
    g.ensure_node("lonely");
    let result = g.bfs("lonely", 5);
    assert!(result.is_empty());
}

#[test]
fn test_bfs_nonexistent_start() {
    let g = Graph::new();
    let result = g.bfs("ghost", 5);
    assert!(result.is_empty());
}

// ============================================================
// Connected components
// ============================================================

#[test]
fn test_components_two_clusters() {
    let mut g = Graph::new();
    g.add_edge("a", "b");
    g.add_edge("c", "d");

    let components = g.connected_components();
    assert_eq!(components.len(), 2);
}

#[test]
fn test_components_with_isolated() {
    let mut g = Graph::new();
    g.add_edge("a", "b");
    g.ensure_node("lonely");

    let components = g.connected_components();
    assert_eq!(components.len(), 2);
}

#[test]
fn test_components_fully_connected() {
    let mut g = Graph::new();
    g.add_edge("a", "b");
    g.add_edge("b", "c");
    g.add_edge("c", "d");

    let components = g.connected_components();
    assert_eq!(components.len(), 1);
    assert_eq!(components[0].len(), 4);
}

#[test]
fn test_components_empty_graph() {
    let g = Graph::new();
    let components = g.connected_components();
    assert!(components.is_empty());
}

#[test]
fn test_components_all_isolated() {
    let mut g = Graph::new();
    g.ensure_node("a");
    g.ensure_node("b");
    g.ensure_node("c");
    let components = g.connected_components();
    assert_eq!(components.len(), 3);
}

// ============================================================
// Rename node
// ============================================================

#[test]
fn test_rename_node_basic() {
    let mut g = Graph::new();
    g.add_edge("old-name", "neighbor");

    g.rename_node("old-name", "new-name");

    assert!(!g.has_edge("old-name", "neighbor"));
    assert!(g.has_edge("new-name", "neighbor"));
    assert!(g.has_edge("neighbor", "new-name"));
    assert_eq!(g.edge_count(), 1);
}

#[test]
fn test_rename_node_multiple_neighbors() {
    let mut g = Graph::new();
    g.add_edge("old", "aa");
    g.add_edge("old", "bb");
    g.add_edge("old", "cc");
    g.add_edge("aa", "bb"); // unrelated edge

    g.rename_node("old", "new");

    assert!(g.has_edge("new", "aa"));
    assert!(g.has_edge("new", "bb"));
    assert!(g.has_edge("new", "cc"));
    assert!(g.has_edge("aa", "bb")); // unrelated preserved
    assert_eq!(g.edge_count(), 4);
    assert!(!g.has_edge("old", "aa")); // old name gone
}

#[test]
fn test_rename_isolated_node() {
    let mut g = Graph::new();
    g.ensure_node("old");
    g.rename_node("old", "new");

    assert!(g.neighbors("old").is_none());
    assert!(g.neighbors("new").is_some());
    assert_eq!(g.node_count(), 1);
}

#[test]
fn test_rename_nonexistent_is_noop() {
    let mut g = Graph::new();
    g.ensure_node("existing");
    g.rename_node("ghost", "new"); // should not panic or alter graph
    assert_eq!(g.node_count(), 1);
    assert!(g.neighbors("existing").is_some());
}

// ============================================================
// graph.idx serialization / deserialization roundtrip
// ============================================================

#[test]
fn test_serialize_empty_graph() {
    let g = Graph::new();
    let buf = serialize_graph_idx(&g);
    // Header: 4 (magic) + 2 (version) + 4 (edge_count) = 10 bytes
    assert_eq!(buf.len(), 10);
    assert_eq!(&buf[0..4], b"MCGI");
}

#[test]
fn test_serialize_deserialize_roundtrip() {
    let mut g = Graph::new();
    g.add_edge("alpha", "beta");
    g.add_edge("beta", "gamma");

    let buf = serialize_graph_idx(&g);

    // Header (10) + 2 edges * 16 bytes = 42
    assert_eq!(buf.len(), 10 + 2 * 16);

    // Deserialize and verify
    let name_to_hash = |name: &str| xxhash_rust::xxh64::xxh64(name.as_bytes(), 0);
    let hash_to_name: std::collections::HashMap<u64, String> = [
        (name_to_hash("alpha"), "alpha".to_string()),
        (name_to_hash("beta"), "beta".to_string()),
        (name_to_hash("gamma"), "gamma".to_string()),
    ].into();

    let g2 = deserialize_graph_idx(&buf, &hash_to_name).expect("deserialize failed");
    assert_eq!(g2.edge_count(), 2);
    assert!(g2.has_edge("alpha", "beta"));
    assert!(g2.has_edge("beta", "gamma"));
}

#[test]
fn test_deserialize_bad_magic() {
    let buf = b"BADx\x01\x00\x00\x00\x00\x00";
    let result = deserialize_graph_idx(buf, &std::collections::HashMap::new());
    assert!(matches!(result, Err(GraphIdxError::BadMagic)));
}

#[test]
fn test_deserialize_truncated() {
    let buf = b"MCGI"; // too short, missing version + count
    let result = deserialize_graph_idx(buf, &std::collections::HashMap::new());
    assert!(matches!(result, Err(GraphIdxError::Truncated)));
}

#[test]
fn test_serialize_single_edge() {
    let mut g = Graph::new();
    g.add_edge("aa", "bb");

    let buf = serialize_graph_idx(&g);
    assert_eq!(buf.len(), 10 + 16); // header + 1 edge record

    let name_to_hash = |name: &str| xxhash_rust::xxh64::xxh64(name.as_bytes(), 0);
    let hash_to_name: std::collections::HashMap<u64, String> = [
        (name_to_hash("aa"), "aa".to_string()),
        (name_to_hash("bb"), "bb".to_string()),
    ].into();

    let g2 = deserialize_graph_idx(&buf, &hash_to_name).expect("deserialize failed");
    assert!(g2.has_edge("aa", "bb"));
    assert!(g2.has_edge("bb", "aa"));
    assert_eq!(g2.edge_count(), 1);
}

// ============================================================
// Empty graph / edge counts
// ============================================================

#[test]
fn test_empty_graph() {
    let g = Graph::new();
    assert_eq!(g.node_count(), 0);
    assert_eq!(g.edge_count(), 0);
    assert!(g.neighbors("nonexistent").is_none());
}

#[test]
fn test_node_count_tracks_adds() {
    let mut g = Graph::new();
    g.add_edge("a", "b");
    assert_eq!(g.node_count(), 2);
    g.add_edge("b", "c");
    assert_eq!(g.node_count(), 3);
}

#[test]
fn test_edge_count_after_complex_ops() {
    let mut g = Graph::new();
    g.add_edge("a", "b");  // 1
    g.add_edge("b", "c");  // 2
    g.add_edge("c", "a");  // 3
    assert_eq!(g.edge_count(), 3);

    g.remove_edge("a", "b"); // 2
    assert_eq!(g.edge_count(), 2);

    g.remove_node("c"); // removes c-b and c-a = 0
    assert_eq!(g.edge_count(), 0);
}
