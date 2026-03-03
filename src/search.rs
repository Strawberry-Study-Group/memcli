use std::collections::HashMap;

use crate::index::{SearchResult, VectorIndex};

/// Pure vector similarity search
///
/// Computes embedding for the query text and searches the global HNSW index.
/// This is the low-level search primitive; most callers should use recall instead.
pub fn vector_search(
    index: &VectorIndex,
    query_embedding: &[f32],
    top_k: usize,
) -> Vec<SearchResult> {
    index.search(query_embedding, top_k)
}

/// Multi-term vector similarity search
///
/// For each query embedding, searches for `top_k` results independently.
/// Merges results across all queries using max-similarity per unique node.
/// Returns all merged results sorted by descending similarity (no global cap).
pub fn multi_vector_search(
    index: &VectorIndex,
    query_embeddings: &[Vec<f32>],
    top_k: usize,
) -> Vec<SearchResult> {
    if query_embeddings.is_empty() {
        return Vec::new();
    }

    let mut merged: HashMap<String, f32> = HashMap::new();

    for emb in query_embeddings {
        let hits = index.search(emb, top_k);
        for hit in hits {
            let entry = merged.entry(hit.node_name).or_insert(0.0);
            if hit.similarity > *entry {
                *entry = hit.similarity;
            }
        }
    }

    let mut results: Vec<SearchResult> = merged
        .into_iter()
        .map(|(node_name, similarity)| SearchResult {
            node_name,
            similarity,
        })
        .collect();

    results.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    results
}
