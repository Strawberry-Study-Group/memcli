use std::collections::HashMap;

use memcore::config::{ProximityMetric, RecallConfig};
use memcore::graph::Graph;
use memcore::index::VectorIndex;
use memcore::node::NodeMeta;
use memcore::recall::{multi_recall, recall};

/// Helper: create a NodeMeta with given weight and abstract
fn make_meta(name: &str, weight: f32, abstract_text: &str) -> NodeMeta {
    use chrono::Utc;
    NodeMeta {
        name: name.to_string(),
        created: Utc::now(),
        updated: Utc::now(),
        weight,
        last_accessed: Utc::now(),
        access_count: 0,
        pinned: false,
        links: vec![],
        abstract_text: abstract_text.to_string(),
        abstract_hash: 0,
    }
}

fn default_config() -> RecallConfig {
    RecallConfig {
        alpha: 0.6,
        beta: 0.2,
        gamma: 0.2,
        default_depth: 1,
        proximity_metric: ProximityMetric::EdgeDistance,
    }
}

// ============================================================
// Basic recall
// ============================================================

#[test]
fn test_recall_empty_index() {
    let idx = VectorIndex::new();
    let graph = Graph::new();
    let metas: HashMap<String, NodeMeta> = HashMap::new();
    let query = vec![1.0, 0.0, 0.0];
    let config = default_config();

    let results = recall(&idx, &graph, &metas, &query, &config, 5, 1);
    assert!(results.is_empty());
}

#[test]
fn test_recall_single_node_exact_match() {
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0, 0.0]);

    let graph = Graph::new();
    let mut metas = HashMap::new();
    metas.insert("node-a".to_string(), make_meta("node-a", 1.0, "test abstract"));

    let query = vec![1.0, 0.0, 0.0];
    let config = default_config();

    let results = recall(&idx, &graph, &metas, &query, &config, 5, 1);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].node_name, "node-a");
    assert!(results[0].score > 0.0);
    assert!((results[0].similarity - 1.0).abs() < 1e-6);
}

#[test]
fn test_recall_scoring_formula() {
    // score = alpha * similarity + beta * weight + gamma * graph_proximity
    // For a seed node: similarity from search, weight from meta, graph_proximity = 0
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0]);

    let graph = Graph::new();
    let mut metas = HashMap::new();
    metas.insert("node-a".to_string(), make_meta("node-a", 0.5, "abstract"));

    let query = vec![1.0, 0.0];
    let config = RecallConfig {
        alpha: 0.6,
        beta: 0.2,
        gamma: 0.2,
        default_depth: 1,
        proximity_metric: ProximityMetric::EdgeDistance,
    };

    let results = recall(&idx, &graph, &metas, &query, &config, 5, 1);
    assert_eq!(results.len(), 1);

    // similarity = 1.0 (exact match), weight = 0.5, graph_proximity = 0.0 (seed)
    let expected_score = 0.6 * 1.0 + 0.2 * 0.5 + 0.2 * 0.0;
    assert!(
        (results[0].score - expected_score).abs() < 1e-5,
        "expected {}, got {}",
        expected_score,
        results[0].score
    );
}

#[test]
fn test_recall_with_graph_expansion() {
    // node-a is in the vector index, node-b is only reachable via graph
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0]);
    // node-b is NOT in the vector index but is a graph neighbor

    let mut graph = Graph::new();
    graph.add_edge("node-a", "node-b");

    let mut metas = HashMap::new();
    metas.insert("node-a".to_string(), make_meta("node-a", 1.0, "abstract a"));
    metas.insert("node-b".to_string(), make_meta("node-b", 0.8, "abstract b"));

    let query = vec![1.0, 0.0];
    let config = default_config();

    let results = recall(&idx, &graph, &metas, &query, &config, 5, 1);

    // Both nodes should appear: node-a from vector search, node-b from graph expansion
    assert_eq!(results.len(), 2);

    let names: Vec<&str> = results.iter().map(|r| r.node_name.as_str()).collect();
    assert!(names.contains(&"node-a"));
    assert!(names.contains(&"node-b"));

    // node-a should score higher (has similarity, node-b has only weight+graph_proximity)
    assert!(results[0].score > results[1].score);
}

#[test]
fn test_recall_graph_expansion_depth() {
    // Chain: a -- b -- c
    // depth=1 should only reach b from a, not c
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0]);

    let mut graph = Graph::new();
    graph.add_edge("node-a", "node-b");
    graph.add_edge("node-b", "node-c");

    let mut metas = HashMap::new();
    metas.insert("node-a".to_string(), make_meta("node-a", 1.0, "a"));
    metas.insert("node-b".to_string(), make_meta("node-b", 1.0, "b"));
    metas.insert("node-c".to_string(), make_meta("node-c", 1.0, "c"));

    let query = vec![1.0, 0.0];
    let config = default_config();

    // depth=1: only node-a and node-b
    let results = recall(&idx, &graph, &metas, &query, &config, 5, 1);
    let names: Vec<&str> = results.iter().map(|r| r.node_name.as_str()).collect();
    assert!(names.contains(&"node-a"));
    assert!(names.contains(&"node-b"));
    assert!(!names.contains(&"node-c"));

    // depth=2: all three
    let results = recall(&idx, &graph, &metas, &query, &config, 5, 2);
    let names: Vec<&str> = results.iter().map(|r| r.node_name.as_str()).collect();
    assert!(names.contains(&"node-a"));
    assert!(names.contains(&"node-b"));
    assert!(names.contains(&"node-c"));
}

#[test]
fn test_recall_top_k_truncation() {
    let mut idx = VectorIndex::new();
    for i in 0..10 {
        let angle = (i as f32) * 0.1;
        idx.insert(&format!("node-{}", i), &[angle.cos(), angle.sin()]);
    }

    let graph = Graph::new();
    let mut metas = HashMap::new();
    for i in 0..10 {
        let name = format!("node-{}", i);
        metas.insert(name.clone(), make_meta(&name, 1.0, "abstract"));
    }

    let query = vec![1.0, 0.0];
    let config = default_config();

    let results = recall(&idx, &graph, &metas, &query, &config, 3, 0);
    assert_eq!(results.len(), 3);
}

#[test]
fn test_recall_deduplication() {
    // node-b is both a vector search result AND a graph neighbor of node-a
    // It should appear only once
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0]);
    idx.insert("node-b", &[0.9, 0.1]);

    let mut graph = Graph::new();
    graph.add_edge("node-a", "node-b");

    let mut metas = HashMap::new();
    metas.insert("node-a".to_string(), make_meta("node-a", 1.0, "a"));
    metas.insert("node-b".to_string(), make_meta("node-b", 1.0, "b"));

    let query = vec![1.0, 0.0];
    let config = default_config();

    let results = recall(&idx, &graph, &metas, &query, &config, 5, 1);
    // Each node should appear exactly once
    let names: Vec<&str> = results.iter().map(|r| r.node_name.as_str()).collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"node-a"));
    assert!(names.contains(&"node-b"));
}

#[test]
fn test_recall_missing_meta_skipped() {
    // node-b is in vector index but has no meta entry — should be skipped
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0]);
    idx.insert("node-b", &[0.9, 0.1]);

    let graph = Graph::new();
    let mut metas = HashMap::new();
    // Only node-a has metadata
    metas.insert("node-a".to_string(), make_meta("node-a", 1.0, "a"));

    let query = vec![1.0, 0.0];
    let config = default_config();

    let results = recall(&idx, &graph, &metas, &query, &config, 5, 1);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].node_name, "node-a");
}

#[test]
fn test_recall_sorted_by_score_descending() {
    let mut idx = VectorIndex::new();
    idx.insert("high-sim", &[1.0, 0.0]);
    idx.insert("mid-sim", &[0.7, 0.7]);
    idx.insert("low-sim", &[0.0, 1.0]);

    let graph = Graph::new();
    let mut metas = HashMap::new();
    metas.insert("high-sim".to_string(), make_meta("high-sim", 1.0, "a"));
    metas.insert("mid-sim".to_string(), make_meta("mid-sim", 1.0, "b"));
    metas.insert("low-sim".to_string(), make_meta("low-sim", 1.0, "c"));

    let query = vec![1.0, 0.0];
    let config = default_config();

    let results = recall(&idx, &graph, &metas, &query, &config, 3, 0);
    for w in results.windows(2) {
        assert!(
            w[0].score >= w[1].score,
            "results not sorted: {} < {}",
            w[0].score,
            w[1].score
        );
    }
}

#[test]
fn test_recall_weight_affects_ranking() {
    // Two nodes with equal similarity but different weights
    let mut idx = VectorIndex::new();
    idx.insert("heavy", &[1.0, 0.0]);
    idx.insert("light", &[1.0, 0.0]); // same embedding

    let graph = Graph::new();
    let mut metas = HashMap::new();
    metas.insert("heavy".to_string(), make_meta("heavy", 1.0, "a"));
    metas.insert("light".to_string(), make_meta("light", 0.1, "b"));

    let query = vec![1.0, 0.0];
    let config = RecallConfig {
        alpha: 0.0, // ignore similarity
        beta: 1.0,  // only weight matters
        gamma: 0.0,
        default_depth: 0,
        proximity_metric: ProximityMetric::EdgeDistance,
    };

    let results = recall(&idx, &graph, &metas, &query, &config, 2, 0);
    assert_eq!(results[0].node_name, "heavy");
    assert_eq!(results[1].node_name, "light");
}

#[test]
fn test_recall_result_fields() {
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0]);

    let graph = Graph::new();
    let mut metas = HashMap::new();
    metas.insert(
        "node-a".to_string(),
        make_meta("node-a", 0.75, "my abstract text"),
    );

    let query = vec![1.0, 0.0];
    let config = default_config();

    let results = recall(&idx, &graph, &metas, &query, &config, 5, 0);
    assert_eq!(results[0].node_name, "node-a");
    assert_eq!(results[0].weight, 0.75);
    assert_eq!(results[0].abstract_text, "my abstract text");
    assert!((results[0].similarity - 1.0).abs() < 1e-6);
}

#[test]
fn test_recall_depth_zero_no_graph_expansion() {
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0]);

    let mut graph = Graph::new();
    graph.add_edge("node-a", "node-b");

    let mut metas = HashMap::new();
    metas.insert("node-a".to_string(), make_meta("node-a", 1.0, "a"));
    metas.insert("node-b".to_string(), make_meta("node-b", 1.0, "b"));

    let query = vec![1.0, 0.0];
    let config = default_config();

    // depth=0 means no graph expansion
    let results = recall(&idx, &graph, &metas, &query, &config, 5, 0);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].node_name, "node-a");
}

// ============================================================
// ProximityMetric unit tests
// ============================================================

#[test]
fn test_proximity_edge_distance_seed_is_zero() {
    let m = ProximityMetric::EdgeDistance;
    assert_eq!(m.compute(0), 0.0);
}

#[test]
fn test_proximity_edge_distance_values() {
    let m = ProximityMetric::EdgeDistance;
    assert!((m.compute(1) - 0.5).abs() < 1e-6);       // 1/(1+1)
    assert!((m.compute(2) - 1.0 / 3.0).abs() < 1e-6); // 1/(1+2)
    assert!((m.compute(3) - 0.25).abs() < 1e-6);       // 1/(1+3)
}

#[test]
fn test_proximity_edge_distance_squared_values() {
    let m = ProximityMetric::EdgeDistanceSquared;
    assert_eq!(m.compute(0), 0.0);
    assert!((m.compute(1) - 0.25).abs() < 1e-6);        // 1/(2^2)
    assert!((m.compute(2) - 1.0 / 9.0).abs() < 1e-6);   // 1/(3^2)
}

#[test]
fn test_recall_with_squared_proximity_favours_direct_neighbours() {
    // Chain: a -- b -- c, search hits a.
    // With EdgeDistanceSquared, b (hops=1) should get much more proximity
    // than with EdgeDistance, relative to c (hops=2).
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0]);

    let mut graph = Graph::new();
    graph.add_edge("node-a", "node-b");
    graph.add_edge("node-b", "node-c");

    let mut metas = HashMap::new();
    metas.insert("node-a".to_string(), make_meta("node-a", 0.5, "a"));
    metas.insert("node-b".to_string(), make_meta("node-b", 0.5, "b"));
    metas.insert("node-c".to_string(), make_meta("node-c", 0.5, "c"));

    let config_linear = RecallConfig {
        alpha: 0.0,
        beta: 0.0,
        gamma: 1.0, // only proximity matters
        default_depth: 2,
        proximity_metric: ProximityMetric::EdgeDistance,
    };
    let config_squared = RecallConfig {
        alpha: 0.0,
        beta: 0.0,
        gamma: 1.0,
        default_depth: 2,
        proximity_metric: ProximityMetric::EdgeDistanceSquared,
    };

    let query = vec![1.0, 0.0];

    let linear = recall(&idx, &graph, &metas, &query, &config_linear, 5, 2);
    let squared = recall(&idx, &graph, &metas, &query, &config_squared, 5, 2);

    // Both should return b and c (a is seed with proximity=0)
    let b_linear = linear.iter().find(|r| r.node_name == "node-b").unwrap().score;
    let c_linear = linear.iter().find(|r| r.node_name == "node-c").unwrap().score;
    let b_squared = squared.iter().find(|r| r.node_name == "node-b").unwrap().score;
    let c_squared = squared.iter().find(|r| r.node_name == "node-c").unwrap().score;

    // Squared metric should have a larger gap between b and c
    let ratio_linear = b_linear / c_linear;
    let ratio_squared = b_squared / c_squared;
    assert!(
        ratio_squared > ratio_linear,
        "squared should favour direct neighbours more: linear ratio={}, squared ratio={}",
        ratio_linear,
        ratio_squared
    );
}

// ============================================================
// Multi-term recall
// ============================================================

#[test]
fn test_multi_recall_empty_queries() {
    let idx = VectorIndex::new();
    let graph = Graph::new();
    let metas: HashMap<String, NodeMeta> = HashMap::new();
    let embeddings: Vec<Vec<f32>> = vec![];
    let config = default_config();

    let results = multi_recall(&idx, &graph, &metas, &embeddings, &config, 5, 1);
    assert!(results.is_empty(), "expected empty results for empty queries");
}

#[test]
fn test_multi_recall_single_query_matches_recall() {
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0]);
    idx.insert("node-b", &[0.5, 0.5]);

    let graph = Graph::new();
    let mut metas = HashMap::new();
    metas.insert("node-a".to_string(), make_meta("node-a", 1.0, "a"));
    metas.insert("node-b".to_string(), make_meta("node-b", 0.8, "b"));

    let query = vec![1.0, 0.0];
    let config = default_config();

    let single = recall(&idx, &graph, &metas, &query, &config, 5, 1);
    let multi = multi_recall(&idx, &graph, &metas, &[query], &config, 5, 1);

    assert_eq!(single.len(), multi.len());
    for (s, m) in single.iter().zip(multi.iter()) {
        assert_eq!(s.node_name, m.node_name);
        assert!((s.score - m.score).abs() < 1e-5);
    }
}

#[test]
fn test_multi_recall_two_queries_wider_net() {
    let mut idx = VectorIndex::new();
    // Cluster A: close to [1, 0]
    idx.insert("a1", &[1.0, 0.0]);
    idx.insert("a2", &[0.9, 0.1]);
    // Cluster B: close to [0, 1]
    idx.insert("b1", &[0.0, 1.0]);
    idx.insert("b2", &[0.1, 0.9]);

    let graph = Graph::new();
    let mut metas = HashMap::new();
    metas.insert("a1".to_string(), make_meta("a1", 1.0, "a1"));
    metas.insert("a2".to_string(), make_meta("a2", 1.0, "a2"));
    metas.insert("b1".to_string(), make_meta("b1", 1.0, "b1"));
    metas.insert("b2".to_string(), make_meta("b2", 1.0, "b2"));

    let q1 = vec![1.0, 0.0];
    let q2 = vec![0.0, 1.0];
    let config = default_config();

    let results = multi_recall(&idx, &graph, &metas, &[q1, q2], &config, 2, 0);
    let names: Vec<&str> = results.iter().map(|r| r.node_name.as_str()).collect();

    // Should get nodes from both clusters
    assert!(names.contains(&"a1"));
    assert!(names.contains(&"b1"));
    assert!(results.len() >= 3);
}

#[test]
fn test_multi_recall_shared_node_max_similarity() {
    let mut idx = VectorIndex::new();
    idx.insert("shared", &[0.7, 0.7]);
    idx.insert("only-a", &[1.0, 0.0]);
    idx.insert("only-b", &[0.0, 1.0]);

    let graph = Graph::new();
    let mut metas = HashMap::new();
    metas.insert("shared".to_string(), make_meta("shared", 0.5, "shared"));
    metas.insert("only-a".to_string(), make_meta("only-a", 0.5, "a"));
    metas.insert("only-b".to_string(), make_meta("only-b", 0.5, "b"));

    let q1 = vec![1.0, 0.0];
    let q2 = vec![0.0, 1.0];
    let config = RecallConfig {
        alpha: 1.0,
        beta: 0.0,
        gamma: 0.0,
        default_depth: 0,
        proximity_metric: ProximityMetric::EdgeDistance,
    };

    let results = multi_recall(&idx, &graph, &metas, &[q1, q2], &config, 3, 0);

    // "shared" should appear exactly once
    let shared_results: Vec<_> = results.iter().filter(|r| r.node_name == "shared").collect();
    assert_eq!(shared_results.len(), 1);
}

#[test]
fn test_multi_recall_graph_expansion_across_seeds() {
    // q1 hits seed-a, q2 hits seed-b
    // seed-a is linked to graph-neighbor
    // graph-neighbor should appear in results
    let mut idx = VectorIndex::new();
    idx.insert("seed-a", &[1.0, 0.0]);
    idx.insert("seed-b", &[0.0, 1.0]);

    let mut graph = Graph::new();
    graph.add_edge("seed-a", "graph-neighbor");

    let mut metas = HashMap::new();
    metas.insert("seed-a".to_string(), make_meta("seed-a", 1.0, "a"));
    metas.insert("seed-b".to_string(), make_meta("seed-b", 1.0, "b"));
    metas.insert(
        "graph-neighbor".to_string(),
        make_meta("graph-neighbor", 0.8, "neighbor"),
    );

    let q1 = vec![1.0, 0.0];
    let q2 = vec![0.0, 1.0];
    let config = default_config();

    let results = multi_recall(&idx, &graph, &metas, &[q1, q2], &config, 2, 1);
    let names: Vec<&str> = results.iter().map(|r| r.node_name.as_str()).collect();

    assert!(names.contains(&"seed-a"));
    assert!(names.contains(&"seed-b"));
    assert!(names.contains(&"graph-neighbor"));
}

#[test]
fn test_multi_recall_per_term_top_k() {
    let mut idx = VectorIndex::new();
    idx.insert("a1", &[1.0, 0.0]);
    idx.insert("a2", &[0.95, 0.05]);
    idx.insert("a3", &[0.9, 0.1]);
    idx.insert("b1", &[0.0, 1.0]);
    idx.insert("b2", &[0.05, 0.95]);
    idx.insert("b3", &[0.1, 0.9]);

    let graph = Graph::new();
    let mut metas = HashMap::new();
    for name in &["a1", "a2", "a3", "b1", "b2", "b3"] {
        metas.insert(name.to_string(), make_meta(name, 1.0, name));
    }

    let q1 = vec![1.0, 0.0];
    let q2 = vec![0.0, 1.0];
    let config = default_config();

    // top_k=2 per term, merged result can exceed 2
    let results = multi_recall(&idx, &graph, &metas, &[q1, q2], &config, 2, 0);
    assert!(
        results.len() >= 3,
        "expected at least 3 results (2 per term merged), got {}",
        results.len()
    );
}

#[test]
fn test_multi_recall_sorted_by_score_descending() {
    let mut idx = VectorIndex::new();
    idx.insert("a", &[1.0, 0.0]);
    idx.insert("b", &[0.7, 0.7]);
    idx.insert("c", &[0.0, 1.0]);

    let graph = Graph::new();
    let mut metas = HashMap::new();
    metas.insert("a".to_string(), make_meta("a", 1.0, "a"));
    metas.insert("b".to_string(), make_meta("b", 0.5, "b"));
    metas.insert("c".to_string(), make_meta("c", 1.0, "c"));

    let q1 = vec![1.0, 0.0];
    let q2 = vec![0.0, 1.0];
    let config = default_config();

    let results = multi_recall(&idx, &graph, &metas, &[q1, q2], &config, 3, 0);
    for w in results.windows(2) {
        assert!(
            w[0].score >= w[1].score,
            "not sorted: {} < {}",
            w[0].score,
            w[1].score
        );
    }
}
