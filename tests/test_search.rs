use memcore::index::VectorIndex;
use memcore::search::{multi_vector_search, vector_search};

#[test]
fn test_vector_search_empty() {
    let idx = VectorIndex::new();
    let results = vector_search(&idx, &[1.0, 0.0], 5);
    assert!(results.is_empty());
}

#[test]
fn test_vector_search_basic() {
    let mut idx = VectorIndex::new();
    idx.insert("close", &[0.9, 0.1]);
    idx.insert("far", &[0.0, 1.0]);

    let results = vector_search(&idx, &[1.0, 0.0], 2);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].node_name, "close");
    assert!(results[0].similarity > results[1].similarity);
}

#[test]
fn test_vector_search_top_k() {
    let mut idx = VectorIndex::new();
    idx.insert("a", &[1.0, 0.0]);
    idx.insert("b", &[0.9, 0.1]);
    idx.insert("c", &[0.5, 0.5]);
    idx.insert("d", &[0.0, 1.0]);

    let results = vector_search(&idx, &[1.0, 0.0], 2);
    assert_eq!(results.len(), 2);
}

#[test]
fn test_vector_search_sorted_by_similarity() {
    let mut idx = VectorIndex::new();
    idx.insert("best", &[1.0, 0.0]);
    idx.insert("good", &[0.8, 0.2]);
    idx.insert("ok", &[0.5, 0.5]);

    let results = vector_search(&idx, &[1.0, 0.0], 3);
    for w in results.windows(2) {
        assert!(w[0].similarity >= w[1].similarity);
    }
}

// ============================================================
// Multi-term vector search
// ============================================================

#[test]
fn test_multi_vector_search_empty_queries() {
    let idx = VectorIndex::new();
    let embeddings: Vec<Vec<f32>> = vec![];
    let results = multi_vector_search(&idx, &embeddings, 5);
    assert!(results.is_empty());
}

#[test]
fn test_multi_vector_search_single_query_matches_vector_search() {
    let mut idx = VectorIndex::new();
    idx.insert("close", &[0.9, 0.1]);
    idx.insert("far", &[0.0, 1.0]);

    let query = vec![1.0, 0.0];
    let single = vector_search(&idx, &query, 2);
    let multi = multi_vector_search(&idx, &[query], 2);

    assert_eq!(single.len(), multi.len());
    for (s, m) in single.iter().zip(multi.iter()) {
        assert_eq!(s.node_name, m.node_name);
        assert!((s.similarity - m.similarity).abs() < 1e-6);
    }
}

#[test]
fn test_multi_vector_search_two_queries_union() {
    let mut idx = VectorIndex::new();
    // Cluster A: close to [1, 0]
    idx.insert("a1", &[1.0, 0.0]);
    idx.insert("a2", &[0.9, 0.1]);
    // Cluster B: close to [0, 1]
    idx.insert("b1", &[0.0, 1.0]);
    idx.insert("b2", &[0.1, 0.9]);

    let q1 = vec![1.0, 0.0]; // hits cluster A
    let q2 = vec![0.0, 1.0]; // hits cluster B

    let results = multi_vector_search(&idx, &[q1, q2], 2);
    let names: Vec<&str> = results.iter().map(|r| r.node_name.as_str()).collect();

    // Should get nodes from both clusters
    assert!(names.contains(&"a1"));
    assert!(names.contains(&"b1"));
    assert!(results.len() >= 3); // at least 3 unique nodes (likely 4)
}

#[test]
fn test_multi_vector_search_shared_node_max_similarity() {
    let mut idx = VectorIndex::new();
    idx.insert("shared", &[0.7, 0.7]); // similar to both queries
    idx.insert("only-a", &[1.0, 0.0]);
    idx.insert("only-b", &[0.0, 1.0]);

    let q1 = vec![1.0, 0.0];
    let q2 = vec![0.0, 1.0];

    let results = multi_vector_search(&idx, &[q1.clone(), q2.clone()], 3);

    // "shared" should appear exactly once
    let shared_results: Vec<_> = results.iter().filter(|r| r.node_name == "shared").collect();
    assert_eq!(shared_results.len(), 1);

    // Its similarity should be the max across the two queries
    let sim_q1 = vector_search(&idx, &q1, 3)
        .iter()
        .find(|r| r.node_name == "shared")
        .unwrap()
        .similarity;
    let sim_q2 = vector_search(&idx, &q2, 3)
        .iter()
        .find(|r| r.node_name == "shared")
        .unwrap()
        .similarity;
    let expected_max = sim_q1.max(sim_q2);

    assert!(
        (shared_results[0].similarity - expected_max).abs() < 1e-6,
        "expected max similarity {}, got {}",
        expected_max,
        shared_results[0].similarity
    );
}

#[test]
fn test_multi_vector_search_per_term_top_k() {
    let mut idx = VectorIndex::new();
    // 6 nodes: 3 close to [1,0], 3 close to [0,1]
    idx.insert("a1", &[1.0, 0.0]);
    idx.insert("a2", &[0.95, 0.05]);
    idx.insert("a3", &[0.9, 0.1]);
    idx.insert("b1", &[0.0, 1.0]);
    idx.insert("b2", &[0.05, 0.95]);
    idx.insert("b3", &[0.1, 0.9]);

    let q1 = vec![1.0, 0.0];
    let q2 = vec![0.0, 1.0];

    // top_k=2 per term → each term returns 2 results → merged can be up to 4
    let results = multi_vector_search(&idx, &[q1, q2], 2);
    assert!(
        results.len() >= 3 && results.len() <= 4,
        "expected 3-4 results, got {}",
        results.len()
    );
}

#[test]
fn test_multi_vector_search_sorted_descending() {
    let mut idx = VectorIndex::new();
    idx.insert("a", &[1.0, 0.0]);
    idx.insert("b", &[0.7, 0.7]);
    idx.insert("c", &[0.0, 1.0]);

    let q1 = vec![1.0, 0.0];
    let q2 = vec![0.0, 1.0];

    let results = multi_vector_search(&idx, &[q1, q2], 3);
    for w in results.windows(2) {
        assert!(
            w[0].similarity >= w[1].similarity,
            "not sorted: {} < {}",
            w[0].similarity,
            w[1].similarity
        );
    }
}
