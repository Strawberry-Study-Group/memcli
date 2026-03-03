use memcore::index::{VectorIndex, SearchResult, IndexError};
use std::path::Path;

// ============================================================
// Construction
// ============================================================

#[test]
fn test_new_vector_index_empty() {
    let idx = VectorIndex::new();
    assert_eq!(idx.node_count(), 0);
    assert_eq!(idx.dimensions(), None);
}

#[test]
fn test_with_dimensions() {
    let idx = VectorIndex::with_dimensions(1024);
    assert_eq!(idx.node_count(), 0);
    assert_eq!(idx.dimensions(), Some(1024));
}

// ============================================================
// Insert
// ============================================================

#[test]
fn test_insert_single() {
    let mut idx = VectorIndex::new();
    let id = idx.insert("node-a", &[1.0, 0.0, 0.0]);
    assert_eq!(id, 0);
    assert_eq!(idx.node_count(), 1);
    assert!(idx.contains("node-a"));
    assert_eq!(idx.dimensions(), Some(3));
}

#[test]
fn test_insert_multiple() {
    let mut idx = VectorIndex::new();
    let id0 = idx.insert("node-a", &[1.0, 0.0, 0.0]);
    let id1 = idx.insert("node-b", &[0.0, 1.0, 0.0]);
    let id2 = idx.insert("node-c", &[0.0, 0.0, 1.0]);
    assert_eq!(id0, 0);
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert_eq!(idx.node_count(), 3);
}

#[test]
fn test_insert_sets_dimensions() {
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 2.0, 3.0, 4.0]);
    assert_eq!(idx.dimensions(), Some(4));
}

#[test]
#[should_panic(expected = "dimension mismatch")]
fn test_insert_dimension_mismatch_panics() {
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0, 0.0]);
    idx.insert("node-b", &[1.0, 0.0]); // wrong dimensions
}

#[test]
fn test_insert_overwrites_existing() {
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0, 0.0]);
    let id = idx.insert("node-a", &[0.0, 1.0, 0.0]); // overwrite
    assert_eq!(idx.node_count(), 1);
    // embedding should be updated
    let emb = idx.get_embedding("node-a").unwrap();
    assert_eq!(emb, &[0.0, 1.0, 0.0]);
    // id should be new
    assert_eq!(id, 1);
}

// ============================================================
// Remove
// ============================================================

#[test]
fn test_remove_existing() {
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0, 0.0]);
    assert!(idx.remove("node-a"));
    assert_eq!(idx.node_count(), 0);
    assert!(!idx.contains("node-a"));
}

#[test]
fn test_remove_nonexistent() {
    let mut idx = VectorIndex::new();
    assert!(!idx.remove("node-a"));
}

#[test]
fn test_remove_does_not_affect_others() {
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0]);
    idx.insert("node-b", &[0.0, 1.0]);
    idx.remove("node-a");
    assert_eq!(idx.node_count(), 1);
    assert!(idx.contains("node-b"));
    assert!(!idx.contains("node-a"));
}

// ============================================================
// Search (brute-force cosine similarity)
// ============================================================

#[test]
fn test_search_empty_index() {
    let idx = VectorIndex::new();
    let results = idx.search(&[1.0, 0.0, 0.0], 5);
    assert!(results.is_empty());
}

#[test]
fn test_search_single_node() {
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0, 0.0]);
    let results = idx.search(&[1.0, 0.0, 0.0], 5);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].node_name, "node-a");
    assert!((results[0].similarity - 1.0).abs() < 1e-6); // identical vectors
}

#[test]
fn test_search_returns_sorted_by_similarity() {
    let mut idx = VectorIndex::new();
    idx.insert("exact-match", &[1.0, 0.0, 0.0]);
    idx.insert("partial-match", &[0.7, 0.7, 0.0]);
    idx.insert("no-match", &[0.0, 0.0, 1.0]);

    let results = idx.search(&[1.0, 0.0, 0.0], 3);
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].node_name, "exact-match");
    assert!(results[0].similarity > results[1].similarity);
    assert!(results[1].similarity > results[2].similarity);
}

#[test]
fn test_search_top_k_limits_results() {
    let mut idx = VectorIndex::new();
    idx.insert("a", &[1.0, 0.0]);
    idx.insert("b", &[0.9, 0.1]);
    idx.insert("c", &[0.8, 0.2]);
    idx.insert("d", &[0.0, 1.0]);

    let results = idx.search(&[1.0, 0.0], 2);
    assert_eq!(results.len(), 2);
}

#[test]
fn test_search_top_k_larger_than_index() {
    let mut idx = VectorIndex::new();
    idx.insert("a", &[1.0, 0.0]);
    idx.insert("b", &[0.0, 1.0]);

    let results = idx.search(&[1.0, 0.0], 10);
    assert_eq!(results.len(), 2);
}

#[test]
fn test_search_orthogonal_vectors() {
    let mut idx = VectorIndex::new();
    idx.insert("x-axis", &[1.0, 0.0]);
    idx.insert("y-axis", &[0.0, 1.0]);

    let results = idx.search(&[1.0, 0.0], 2);
    assert_eq!(results[0].node_name, "x-axis");
    assert!((results[0].similarity - 1.0).abs() < 1e-6);
    assert!(results[1].similarity.abs() < 1e-6); // orthogonal = 0
}

#[test]
fn test_search_negative_similarity() {
    let mut idx = VectorIndex::new();
    idx.insert("same-dir", &[1.0, 0.0]);
    idx.insert("opposite", &[-1.0, 0.0]);

    let results = idx.search(&[1.0, 0.0], 2);
    assert_eq!(results[0].node_name, "same-dir");
    assert!((results[0].similarity - 1.0).abs() < 1e-6);
    assert!((results[1].similarity - (-1.0)).abs() < 1e-6);
}

#[test]
fn test_search_after_remove() {
    let mut idx = VectorIndex::new();
    idx.insert("keep", &[1.0, 0.0]);
    idx.insert("gone", &[0.9, 0.1]);
    idx.remove("gone");

    let results = idx.search(&[1.0, 0.0], 5);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].node_name, "keep");
}

#[test]
fn test_search_top_k_zero() {
    let mut idx = VectorIndex::new();
    idx.insert("a", &[1.0, 0.0]);
    let results = idx.search(&[1.0, 0.0], 0);
    assert!(results.is_empty());
}

// ============================================================
// Get embedding
// ============================================================

#[test]
fn test_get_embedding_exists() {
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 2.0, 3.0]);
    let emb = idx.get_embedding("node-a").unwrap();
    assert_eq!(emb, &[1.0, 2.0, 3.0]);
}

#[test]
fn test_get_embedding_not_found() {
    let idx = VectorIndex::new();
    assert!(idx.get_embedding("nonexistent").is_none());
}

#[test]
fn test_get_embedding_after_remove() {
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 2.0, 3.0]);
    idx.remove("node-a");
    assert!(idx.get_embedding("node-a").is_none());
}

// ============================================================
// Contains
// ============================================================

#[test]
fn test_contains_present() {
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0]);
    assert!(idx.contains("node-a"));
}

#[test]
fn test_contains_absent() {
    let idx = VectorIndex::new();
    assert!(!idx.contains("nonexistent"));
}

// ============================================================
// Rename
// ============================================================

#[test]
fn test_rename_existing() {
    let mut idx = VectorIndex::new();
    idx.insert("old-name", &[1.0, 2.0]);
    assert!(idx.rename("old-name", "new-name"));
    assert!(!idx.contains("old-name"));
    assert!(idx.contains("new-name"));
    assert_eq!(idx.get_embedding("new-name").unwrap(), &[1.0, 2.0]);
    assert_eq!(idx.node_count(), 1);
}

#[test]
fn test_rename_nonexistent() {
    let mut idx = VectorIndex::new();
    assert!(!idx.rename("ghost", "new-name"));
}

#[test]
fn test_rename_searchable_under_new_name() {
    let mut idx = VectorIndex::new();
    idx.insert("old-name", &[1.0, 0.0]);
    idx.rename("old-name", "new-name");
    let results = idx.search(&[1.0, 0.0], 1);
    assert_eq!(results[0].node_name, "new-name");
}

// ============================================================
// All node names
// ============================================================

#[test]
fn test_all_node_names_empty() {
    let idx = VectorIndex::new();
    assert!(idx.all_node_names().is_empty());
}

#[test]
fn test_all_node_names() {
    let mut idx = VectorIndex::new();
    idx.insert("alpha", &[1.0]);
    idx.insert("beta", &[2.0]);
    idx.insert("gamma", &[3.0]);
    let mut names = idx.all_node_names();
    names.sort();
    assert_eq!(names, vec!["alpha", "beta", "gamma"]);
}

// ============================================================
// Persistence (save/load roundtrip)
// ============================================================

#[test]
fn test_save_load_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let index_dir = dir.path().join("index");
    std::fs::create_dir_all(&index_dir).unwrap();

    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[1.0, 0.0, 0.5]);
    idx.insert("node-b", &[0.0, 1.0, 0.3]);
    idx.insert("node-c", &[0.2, 0.8, 1.0]);

    idx.save_to_dir(&index_dir).unwrap();

    let loaded = VectorIndex::load_from_dir(&index_dir).unwrap();
    assert_eq!(loaded.node_count(), 3);
    assert_eq!(loaded.dimensions(), Some(3));
    assert!(loaded.contains("node-a"));
    assert!(loaded.contains("node-b"));
    assert!(loaded.contains("node-c"));
    assert_eq!(loaded.get_embedding("node-a").unwrap(), &[1.0, 0.0, 0.5]);
    assert_eq!(loaded.get_embedding("node-b").unwrap(), &[0.0, 1.0, 0.3]);
}

#[test]
fn test_save_load_empty_index() {
    let dir = tempfile::tempdir().unwrap();
    let index_dir = dir.path().join("index");
    std::fs::create_dir_all(&index_dir).unwrap();

    let idx = VectorIndex::new();
    idx.save_to_dir(&index_dir).unwrap();

    let loaded = VectorIndex::load_from_dir(&index_dir).unwrap();
    assert_eq!(loaded.node_count(), 0);
    assert_eq!(loaded.dimensions(), None);
}

#[test]
fn test_load_nonexistent_dir() {
    let result = VectorIndex::load_from_dir(Path::new("/tmp/nonexistent-memcore-test-dir"));
    // Should return empty index when dir doesn't have index files
    assert!(result.is_ok());
    assert_eq!(result.unwrap().node_count(), 0);
}

#[test]
fn test_save_load_preserves_search_results() {
    let dir = tempfile::tempdir().unwrap();
    let index_dir = dir.path().join("index");
    std::fs::create_dir_all(&index_dir).unwrap();

    let mut idx = VectorIndex::new();
    idx.insert("close", &[0.9, 0.1]);
    idx.insert("far", &[0.0, 1.0]);
    idx.save_to_dir(&index_dir).unwrap();

    let loaded = VectorIndex::load_from_dir(&index_dir).unwrap();
    let results = loaded.search(&[1.0, 0.0], 2);
    assert_eq!(results[0].node_name, "close");
    assert_eq!(results[1].node_name, "far");
}

#[test]
fn test_save_load_after_remove() {
    let dir = tempfile::tempdir().unwrap();
    let index_dir = dir.path().join("index");
    std::fs::create_dir_all(&index_dir).unwrap();

    let mut idx = VectorIndex::new();
    idx.insert("keep", &[1.0, 0.0]);
    idx.insert("gone", &[0.0, 1.0]);
    idx.remove("gone");
    idx.save_to_dir(&index_dir).unwrap();

    let loaded = VectorIndex::load_from_dir(&index_dir).unwrap();
    assert_eq!(loaded.node_count(), 1);
    assert!(loaded.contains("keep"));
    assert!(!loaded.contains("gone"));
}

// ============================================================
// Edge cases
// ============================================================

#[test]
fn test_high_dimensional_vectors() {
    let mut idx = VectorIndex::new();
    let dim = 1024;
    let v1: Vec<f32> = (0..dim).map(|i| (i as f32) / dim as f32).collect();
    let v2: Vec<f32> = (0..dim).map(|i| 1.0 - (i as f32) / dim as f32).collect();

    idx.insert("node-a", &v1);
    idx.insert("node-b", &v2);

    let results = idx.search(&v1, 2);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].node_name, "node-a");
    assert!((results[0].similarity - 1.0).abs() < 1e-5);
}

#[test]
fn test_many_nodes_search() {
    let mut idx = VectorIndex::new();
    // Insert 100 nodes with varying similarity to query
    for i in 0..100 {
        let angle = (i as f32) * std::f32::consts::PI / 100.0;
        let embedding = vec![angle.cos(), angle.sin()];
        idx.insert(&format!("node-{:03}", i), &embedding);
    }

    let results = idx.search(&[1.0, 0.0], 5);
    assert_eq!(results.len(), 5);
    // First result should be node-000 (cos(0)=1, sin(0)=0)
    assert_eq!(results[0].node_name, "node-000");
    // Results should be sorted by descending similarity
    for w in results.windows(2) {
        assert!(w[0].similarity >= w[1].similarity);
    }
}

// ============================================================
// Additional edge cases
// ============================================================

#[test]
fn test_search_identical_vectors() {
    let mut idx = VectorIndex::new();
    idx.insert("node-a", &[0.5, 0.5, 0.5]);
    idx.insert("node-b", &[0.5, 0.5, 0.5]);
    idx.insert("node-c", &[0.5, 0.5, 0.5]);

    let results = idx.search(&[0.5, 0.5, 0.5], 3);
    assert_eq!(results.len(), 3);
    // All should have similarity ~1.0
    for r in &results {
        assert!(
            (r.similarity - 1.0).abs() < 1e-5,
            "identical vector should have sim ~1.0, got {}",
            r.similarity
        );
    }
}

#[test]
fn test_insert_zero_vector() {
    let mut idx = VectorIndex::new();
    idx.insert("zero-node", &[0.0, 0.0, 0.0]);
    assert_eq!(idx.node_count(), 1);
    assert!(idx.contains("zero-node"));

    // Search against non-zero query — cosine with zero vector returns 0.0
    let results = idx.search(&[1.0, 0.0, 0.0], 1);
    assert_eq!(results.len(), 1);
    assert!(
        results[0].similarity.abs() < 1e-6,
        "zero vector cosine should be 0.0, got {}",
        results[0].similarity
    );
}

#[test]
fn test_save_load_large_index() {
    let dir = tempfile::tempdir().unwrap();
    let index_dir = dir.path().join("index");
    std::fs::create_dir_all(&index_dir).unwrap();

    let mut idx = VectorIndex::new();
    for i in 0..150 {
        let angle = (i as f32) * std::f32::consts::PI / 150.0;
        let embedding = vec![angle.cos(), angle.sin(), (angle * 2.0).cos()];
        idx.insert(&format!("node-{:03}", i), &embedding);
    }

    idx.save_to_dir(&index_dir).unwrap();

    let loaded = VectorIndex::load_from_dir(&index_dir).unwrap();
    assert_eq!(loaded.node_count(), 150);
    assert_eq!(loaded.dimensions(), Some(3));

    // Spot check a few embeddings survive roundtrip
    for i in [0, 50, 100, 149] {
        let name = format!("node-{:03}", i);
        assert!(loaded.contains(&name));
        assert_eq!(
            loaded.get_embedding(&name).unwrap(),
            idx.get_embedding(&name).unwrap()
        );
    }
}
