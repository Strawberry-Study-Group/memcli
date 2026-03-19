use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::config::RecallConfig;
use crate::graph::Graph;
use crate::index::VectorIndex;
use crate::node::NodeMeta;
use crate::util::cosine_similarity;

/// A recall result combining vector similarity, weight (trust), and vitality
#[derive(Debug, Clone)]
pub struct RecallResult {
    pub node_name: String,
    pub score: f32,
    pub similarity: f32,
    pub weight: f32,
    pub vitality: f32,
    pub abstract_text: String,
}

/// Compute vitality for a node based on age and access frequency.
///
/// Formula:
///   effective_age = days_since_last_access / (1 + ln(1 + access_count))
///   vitality = max(floor, 1 / (1 + effective_age))
///
/// New nodes (just created) have vitality = 1.0 ("born hot").
/// Frequently accessed nodes decay slower.
/// All nodes decay to at least `floor`.
pub fn vitality(meta: &NodeMeta, floor: f32, now: DateTime<Utc>) -> f32 {
    let days_since = now
        .signed_duration_since(meta.last_accessed)
        .num_seconds()
        .max(0) as f64
        / 86400.0;
    let effective_age = days_since / (1.0 + (1.0 + meta.access_count as f64).ln());
    let v = 1.0 / (1.0 + effective_age);
    (v as f32).max(floor)
}

/// Comprehensive memory retrieval: vector search + graph expansion + multiplicative scoring
///
/// Algorithm:
///   1. search <query> top-k → seed nodes (with similarity)
///   2. for each seed: neighbors(depth=D) → graph-expanded candidates
///   3. for graph neighbors: compute real similarity via stored embeddings
///   4. score = similarity × weight × vitality
///   5. deduplicate, sort, return top-k
pub fn recall(
    index: &VectorIndex,
    graph: &Graph,
    metas: &HashMap<String, NodeMeta>,
    query_embedding: &[f32],
    config: &RecallConfig,
    top_k: usize,
    depth: usize,
) -> Vec<RecallResult> {
    let now = Utc::now();

    // Step 1: vector search for seed nodes
    let seeds = index.search(query_embedding, top_k);

    // Step 2: collect candidates with similarity
    // name → max similarity
    let mut candidates: HashMap<String, f32> = HashMap::new();

    for seed in &seeds {
        candidates
            .entry(seed.node_name.clone())
            .or_insert(seed.similarity);

        // BFS expansion
        let expanded = graph.bfs(&seed.node_name, depth);
        for (neighbor, _dist) in expanded {
            candidates.entry(neighbor.clone()).or_insert_with(|| {
                // Compute real similarity for graph neighbors
                match index.get_embedding(&neighbor) {
                    Some(emb) => cosine_similarity(query_embedding, emb),
                    None => f32::NEG_INFINITY, // sentinel: no embedding, will be skipped
                }
            });
        }
    }

    // Step 3: multiplicative scoring
    let mut results: Vec<RecallResult> = candidates
        .into_iter()
        .filter_map(|(name, similarity)| {
            if similarity == f32::NEG_INFINITY {
                return None; // no embedding — skip
            }
            let meta = metas.get(&name)?;
            let v = vitality(meta, config.vitality_floor, now);
            let score = similarity * meta.weight * v;

            Some(RecallResult {
                node_name: name,
                score,
                similarity,
                weight: meta.weight,
                vitality: v,
                abstract_text: meta.abstract_text.clone(),
            })
        })
        .collect();

    // Step 4: sort and truncate
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(top_k);

    results
}

/// Multi-term comprehensive retrieval: multiple queries + graph expansion + multiplicative scoring
///
/// Algorithm:
///   1. For each query embedding: search top-k → seed nodes, BFS expand
///   2. Merge across all queries: max similarity per node
///   3. For graph neighbors: compute similarity against the query embedding that found the seed
///   4. Score = similarity × weight × vitality
///   5. Sort descending, return all merged results (no global cap)
pub fn multi_recall(
    index: &VectorIndex,
    graph: &Graph,
    metas: &HashMap<String, NodeMeta>,
    query_embeddings: &[Vec<f32>],
    config: &RecallConfig,
    top_k: usize,
    depth: usize,
) -> Vec<RecallResult> {
    if query_embeddings.is_empty() {
        return Vec::new();
    }

    let now = Utc::now();

    // Merge candidates across all queries: node → max similarity
    let mut candidates: HashMap<String, f32> = HashMap::new();

    for emb in query_embeddings {
        let seeds = index.search(emb, top_k);

        for seed in &seeds {
            let entry = candidates
                .entry(seed.node_name.clone())
                .or_insert(0.0);
            if seed.similarity > *entry {
                *entry = seed.similarity;
            }

            let expanded = graph.bfs(&seed.node_name, depth);
            for (neighbor, _dist) in expanded {
                let sim = match index.get_embedding(&neighbor) {
                    Some(neighbor_emb) => cosine_similarity(emb, neighbor_emb),
                    None => continue, // skip neighbors without embeddings
                };
                let entry = candidates.entry(neighbor).or_insert(0.0);
                if sim > *entry {
                    *entry = sim;
                }
            }
        }
    }

    // Score all candidates
    let mut results: Vec<RecallResult> = candidates
        .into_iter()
        .filter_map(|(name, similarity)| {
            let meta = metas.get(&name)?;
            let v = vitality(meta, config.vitality_floor, now);
            let score = similarity * meta.weight * v;

            Some(RecallResult {
                node_name: name,
                score,
                similarity,
                weight: meta.weight,
                vitality: v,
                abstract_text: meta.abstract_text.clone(),
            })
        })
        .collect();

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    results
}
