use std::collections::HashMap;

use chrono::{Duration, Utc};

use memcore::config::RecallConfig;
use memcore::graph::Graph;
use memcore::index::VectorIndex;
use memcore::node::NodeMeta;
use memcore::recall::{multi_recall, recall, vitality};

/// Helper: create a NodeMeta with given weight and abstract.
/// Uses Utc::now() for timestamps and access_count=0 → vitality=1.0 (born hot).
fn make_meta(name: &str, weight: f32, abstract_text: &str) -> NodeMeta {
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

/// Helper: create a NodeMeta with specific access history for vitality testing.
fn make_meta_with_access(
    name: &str,
    weight: f32,
    abstract_text: &str,
    last_accessed: chrono::DateTime<Utc>,
    access_count: u32,
) -> NodeMeta {
    NodeMeta {
        name: name.to_string(),
        created: Utc::now(),
        updated: Utc::now(),
        weight,
        last_accessed,
        access_count,
        pinned: false,
        links: vec![],
        abstract_text: abstract_text.to_string(),
        abstract_hash: 0,
    }
}

fn default_config() -> RecallConfig {
    RecallConfig {
        vitality_floor: 0.05,
        default_depth: 1,
    }
}

// ============================================================
// Vitality function tests
// ============================================================

#[test]
fn test_vitality_brand_new_node() {
    // A node just created (last_accessed = now, access_count = 0) → vitality = 1.0
    let meta = make_meta("fresh", 1.0, "new node");
    let now = Utc::now();
    let v = vitality(&meta, 0.05, now);
    assert!(
        (v - 1.0).abs() < 0.01,
        "brand new node should have vitality ~1.0, got {}",
        v
    );
}

#[test]
fn test_vitality_one_day_no_access() {
    // 1 day old, never accessed → effective_age = 1 / (1 + ln(1)) = 1 / 1 = 1
    // vitality = 1 / (1 + 1) = 0.5
    let now = Utc::now();
    let meta = make_meta_with_access("old", 1.0, "test", now - Duration::days(1), 0);
    let v = vitality(&meta, 0.05, now);
    assert!(
        (v - 0.5).abs() < 0.01,
        "1-day-old unaccessed node should have vitality ~0.5, got {}",
        v
    );
}

#[test]
fn test_vitality_frequency_slows_decay() {
    // Same age (1 day), but with access_count=10
    // effective_age = 1 / (1 + ln(11)) ≈ 1 / (1 + 2.397) ≈ 0.294
    // vitality = 1 / (1 + 0.294) ≈ 0.773
    let now = Utc::now();
    let meta_no_access = make_meta_with_access("no-access", 1.0, "test", now - Duration::days(1), 0);
    let meta_frequent = make_meta_with_access("frequent", 1.0, "test", now - Duration::days(1), 10);

    let v_no = vitality(&meta_no_access, 0.05, now);
    let v_freq = vitality(&meta_frequent, 0.05, now);

    assert!(
        v_freq > v_no,
        "frequently accessed node should have higher vitality: freq={}, no_access={}",
        v_freq,
        v_no
    );
}

#[test]
fn test_vitality_floor_clamp() {
    // Very old node (365 days, no access) should not go below floor
    let now = Utc::now();
    let meta = make_meta_with_access("ancient", 1.0, "test", now - Duration::days(365), 0);
    let floor = 0.05;
    let v = vitality(&meta, floor, now);
    assert!(
        v >= floor,
        "vitality {} should be >= floor {}",
        v,
        floor
    );
    assert!(
        (v - floor).abs() < 0.01,
        "very old node should be near floor: got {}",
        v
    );
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
    idx.insert("node-a", &[1.0, 0.0, 0.0]).unwrap();

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
fn test_recall_multiplicative_scoring_formula() {
    // score = similarity × weight × vitality
    // For a brand-new seed node: sim=1.0, weight=0.5, vitality≈1.0
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0]).unwrap();

    let graph = Graph::new();
    let mut metas = HashMap::new();
    metas.insert("node-a".to_string(), make_meta("node-a", 0.5, "abstract"));

    let query = vec![1.0, 0.0];
    let config = default_config();

    let results = recall(&idx, &graph, &metas, &query, &config, 5, 1);
    assert_eq!(results.len(), 1);

    // similarity = 1.0, weight = 0.5, vitality ≈ 1.0 → score ≈ 0.5
    let expected_score = 1.0 * 0.5 * 1.0;
    assert!(
        (results[0].score - expected_score).abs() < 0.05,
        "expected ~{}, got {}",
        expected_score,
        results[0].score
    );
}

#[test]
fn test_recall_multiplicative_zero_weight_kills_score() {
    // weight=0 should make score=0 regardless of similarity or vitality
    let mut idx = VectorIndex::new();
    idx.insert("zero-weight", &[1.0, 0.0]).unwrap();

    let graph = Graph::new();
    let mut metas = HashMap::new();
    metas.insert("zero-weight".to_string(), make_meta("zero-weight", 0.0, "abstract"));

    let query = vec![1.0, 0.0];
    let config = default_config();

    let results = recall(&idx, &graph, &metas, &query, &config, 5, 1);
    assert_eq!(results.len(), 1);
    assert!(
        results[0].score.abs() < 1e-6,
        "zero weight should kill score, got {}",
        results[0].score
    );
}

#[test]
fn test_recall_graph_neighbor_gets_real_similarity() {
    // node-a is a seed, node-b is a graph neighbor WITH an embedding
    // node-b should get real cosine similarity, not 0
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0]).unwrap();
    idx.insert("node-b", &[0.8, 0.6]).unwrap(); // has embedding

    let mut graph = Graph::new();
    graph.add_edge("node-a", "node-b");

    let mut metas = HashMap::new();
    metas.insert("node-a".to_string(), make_meta("node-a", 1.0, "a"));
    metas.insert("node-b".to_string(), make_meta("node-b", 1.0, "b"));

    let query = vec![1.0, 0.0];
    let config = default_config();

    let results = recall(&idx, &graph, &metas, &query, &config, 5, 1);
    let b_result = results.iter().find(|r| r.node_name == "node-b").unwrap();

    // node-b embedding [0.8, 0.6] vs query [1.0, 0.0] → cosine = 0.8
    assert!(
        b_result.similarity > 0.7,
        "graph neighbor should get real similarity, got {}",
        b_result.similarity
    );
}

#[test]
fn test_recall_graph_neighbor_without_embedding_skipped() {
    // node-b is a graph neighbor but has NO embedding → should be skipped
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0]).unwrap();
    // node-b is NOT in the vector index

    let mut graph = Graph::new();
    graph.add_edge("node-a", "node-b");

    let mut metas = HashMap::new();
    metas.insert("node-a".to_string(), make_meta("node-a", 1.0, "a"));
    metas.insert("node-b".to_string(), make_meta("node-b", 1.0, "b"));

    let query = vec![1.0, 0.0];
    let config = default_config();

    let results = recall(&idx, &graph, &metas, &query, &config, 5, 1);

    // node-b should NOT appear — no embedding means no similarity
    let names: Vec<&str> = results.iter().map(|r| r.node_name.as_str()).collect();
    assert!(!names.contains(&"node-b"), "graph neighbor without embedding should be skipped");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].node_name, "node-a");
}

#[test]
fn test_recall_with_graph_expansion() {
    // node-a is in the vector index, node-b is reachable via graph AND has embedding
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0]).unwrap();
    idx.insert("node-b", &[0.7, 0.7]).unwrap(); // has embedding for real similarity

    let mut graph = Graph::new();
    graph.add_edge("node-a", "node-b");

    let mut metas = HashMap::new();
    metas.insert("node-a".to_string(), make_meta("node-a", 1.0, "abstract a"));
    metas.insert("node-b".to_string(), make_meta("node-b", 0.8, "abstract b"));

    let query = vec![1.0, 0.0];
    let config = default_config();

    let results = recall(&idx, &graph, &metas, &query, &config, 5, 1);

    // Both nodes should appear
    assert_eq!(results.len(), 2);
    let names: Vec<&str> = results.iter().map(|r| r.node_name.as_str()).collect();
    assert!(names.contains(&"node-a"));
    assert!(names.contains(&"node-b"));

    // node-a should score higher (exact match sim=1.0 vs node-b lower sim)
    assert!(results[0].score > results[1].score);
}

#[test]
fn test_recall_graph_expansion_depth() {
    // Chain: a -- b -- c
    // All have embeddings. With large top_k, all are vector seeds.
    // But with top_k=1, only node-a (highest sim to query) is a seed.
    // BFS at depth=1 from node-a → finds node-b.
    // BFS at depth=2 from node-a → finds node-b and node-c.
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0]).unwrap();
    idx.insert("node-b", &[0.5, 0.866]).unwrap(); // ~60° from query, sim ≈ 0.5
    idx.insert("node-c", &[0.3, 0.954]).unwrap(); // ~72° from query, sim ≈ 0.3

    let mut graph = Graph::new();
    graph.add_edge("node-a", "node-b");
    graph.add_edge("node-b", "node-c");

    let mut metas = HashMap::new();
    metas.insert("node-a".to_string(), make_meta("node-a", 1.0, "a"));
    metas.insert("node-b".to_string(), make_meta("node-b", 1.0, "b"));
    metas.insert("node-c".to_string(), make_meta("node-c", 1.0, "c"));

    let query = vec![1.0, 0.0];
    let config = default_config();

    // top_k=1: only node-a is a seed. depth=1 → node-b added via BFS.
    // Final output truncated to top_k=1 → only node-a returned.
    let results_d1 = recall(&idx, &graph, &metas, &query, &config, 1, 1);
    assert_eq!(results_d1.len(), 1);
    assert_eq!(results_d1[0].node_name, "node-a");

    // top_k=3 + depth=0: all 3 from vector search (no graph expansion needed)
    let results_d0 = recall(&idx, &graph, &metas, &query, &config, 3, 0);
    assert_eq!(results_d0.len(), 3);

    // Verify that depth=2 with large top_k finds all nodes
    let results_d2 = recall(&idx, &graph, &metas, &query, &config, 10, 2);
    let names: Vec<&str> = results_d2.iter().map(|r| r.node_name.as_str()).collect();
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
    idx.insert("node-a", &[1.0, 0.0]).unwrap();
    idx.insert("node-b", &[0.9, 0.1]).unwrap();

    let mut graph = Graph::new();
    graph.add_edge("node-a", "node-b");

    let mut metas = HashMap::new();
    metas.insert("node-a".to_string(), make_meta("node-a", 1.0, "a"));
    metas.insert("node-b".to_string(), make_meta("node-b", 1.0, "b"));

    let query = vec![1.0, 0.0];
    let config = default_config();

    let results = recall(&idx, &graph, &metas, &query, &config, 5, 1);
    let names: Vec<&str> = results.iter().map(|r| r.node_name.as_str()).collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"node-a"));
    assert!(names.contains(&"node-b"));
}

#[test]
fn test_recall_missing_meta_skipped() {
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0]).unwrap();
    idx.insert("node-b", &[0.9, 0.1]).unwrap();

    let graph = Graph::new();
    let mut metas = HashMap::new();
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
    idx.insert("high-sim", &[1.0, 0.0]).unwrap();
    idx.insert("mid-sim", &[0.7, 0.7]).unwrap();
    idx.insert("low-sim", &[0.0, 1.0]).unwrap();

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
    idx.insert("heavy", &[1.0, 0.0]).unwrap();
    idx.insert("light", &[1.0, 0.0]).unwrap(); // same embedding

    let graph = Graph::new();
    let mut metas = HashMap::new();
    metas.insert("heavy".to_string(), make_meta("heavy", 1.0, "a"));
    metas.insert("light".to_string(), make_meta("light", 0.1, "b"));

    let query = vec![1.0, 0.0];
    let config = default_config();

    let results = recall(&idx, &graph, &metas, &query, &config, 2, 0);
    assert_eq!(results[0].node_name, "heavy");
    assert_eq!(results[1].node_name, "light");
}

#[test]
fn test_recall_result_fields() {
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0]).unwrap();

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
    // Brand new node → vitality ≈ 1.0
    assert!(results[0].vitality > 0.99);
}

#[test]
fn test_recall_depth_zero_no_graph_expansion() {
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0]).unwrap();
    idx.insert("node-b", &[0.5, 0.5]).unwrap();

    let mut graph = Graph::new();
    graph.add_edge("node-a", "node-b");

    let mut metas = HashMap::new();
    metas.insert("node-a".to_string(), make_meta("node-a", 1.0, "a"));
    metas.insert("node-b".to_string(), make_meta("node-b", 1.0, "b"));

    let query = vec![1.0, 0.0];
    let config = default_config();

    // depth=0 means no graph expansion — only vector search seeds
    let results = recall(&idx, &graph, &metas, &query, &config, 1, 0);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].node_name, "node-a");
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
    idx.insert("node-a", &[1.0, 0.0]).unwrap();
    idx.insert("node-b", &[0.5, 0.5]).unwrap();

    let graph = Graph::new();
    let mut metas = HashMap::new();
    metas.insert("node-a".to_string(), make_meta("node-a", 1.0, "a"));
    metas.insert("node-b".to_string(), make_meta("node-b", 0.8, "b"));

    let query = vec![1.0, 0.0];
    let config = default_config();

    let single = recall(&idx, &graph, &metas, &query, &config, 5, 0);
    let multi = multi_recall(&idx, &graph, &metas, &[query], &config, 5, 0);

    assert_eq!(single.len(), multi.len());
    for (s, m) in single.iter().zip(multi.iter()) {
        assert_eq!(s.node_name, m.node_name);
        assert!((s.score - m.score).abs() < 1e-5);
    }
}

#[test]
fn test_multi_recall_two_queries_wider_net() {
    let mut idx = VectorIndex::new();
    idx.insert("a1", &[1.0, 0.0]).unwrap();
    idx.insert("a2", &[0.9, 0.1]).unwrap();
    idx.insert("b1", &[0.0, 1.0]).unwrap();
    idx.insert("b2", &[0.1, 0.9]).unwrap();

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

    assert!(names.contains(&"a1"));
    assert!(names.contains(&"b1"));
    assert!(results.len() >= 3);
}

#[test]
fn test_multi_recall_shared_node_max_similarity() {
    let mut idx = VectorIndex::new();
    idx.insert("shared", &[0.7, 0.7]).unwrap();
    idx.insert("only-a", &[1.0, 0.0]).unwrap();
    idx.insert("only-b", &[0.0, 1.0]).unwrap();

    let graph = Graph::new();
    let mut metas = HashMap::new();
    metas.insert("shared".to_string(), make_meta("shared", 0.5, "shared"));
    metas.insert("only-a".to_string(), make_meta("only-a", 0.5, "a"));
    metas.insert("only-b".to_string(), make_meta("only-b", 0.5, "b"));

    let q1 = vec![1.0, 0.0];
    let q2 = vec![0.0, 1.0];
    let config = default_config();

    let results = multi_recall(&idx, &graph, &metas, &[q1, q2], &config, 3, 0);

    // "shared" should appear exactly once
    let shared_results: Vec<_> = results.iter().filter(|r| r.node_name == "shared").collect();
    assert_eq!(shared_results.len(), 1);
}

#[test]
fn test_multi_recall_graph_expansion_across_seeds() {
    // q1 hits seed-a, q2 hits seed-b
    // seed-a is linked to graph-neighbor (which has an embedding)
    let mut idx = VectorIndex::new();
    idx.insert("seed-a", &[1.0, 0.0]).unwrap();
    idx.insert("seed-b", &[0.0, 1.0]).unwrap();
    idx.insert("graph-neighbor", &[0.9, 0.1]).unwrap(); // has embedding

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
    idx.insert("a1", &[1.0, 0.0]).unwrap();
    idx.insert("a2", &[0.95, 0.05]).unwrap();
    idx.insert("a3", &[0.9, 0.1]).unwrap();
    idx.insert("b1", &[0.0, 1.0]).unwrap();
    idx.insert("b2", &[0.05, 0.95]).unwrap();
    idx.insert("b3", &[0.1, 0.9]).unwrap();

    let graph = Graph::new();
    let mut metas = HashMap::new();
    for name in &["a1", "a2", "a3", "b1", "b2", "b3"] {
        metas.insert(name.to_string(), make_meta(name, 1.0, name));
    }

    let q1 = vec![1.0, 0.0];
    let q2 = vec![0.0, 1.0];
    let config = default_config();

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
    idx.insert("a", &[1.0, 0.0]).unwrap();
    idx.insert("b", &[0.7, 0.7]).unwrap();
    idx.insert("c", &[0.0, 1.0]).unwrap();

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
