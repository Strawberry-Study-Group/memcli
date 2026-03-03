use std::collections::HashMap;

use crate::config::RecallConfig;
use crate::graph::Graph;
use crate::index::VectorIndex;
use crate::node::NodeMeta;

/// A recall result combining vector similarity, weight, and graph proximity
#[derive(Debug, Clone)]
pub struct RecallResult {
    pub node_name: String,
    pub score: f32,
    pub similarity: f32,
    pub weight: f32,
    pub abstract_text: String,
}

/// Comprehensive memory retrieval: vector search + graph expansion + scoring
///
/// Algorithm:
///   1. search <query> top-k → seed nodes
///   2. for each seed: neighbors(depth=D) → graph-expanded candidates
///   3. score = α×similarity + β×weight + γ×graph_proximity
///   4. deduplicate, sort, return top-k
pub fn recall(
    index: &VectorIndex,
    graph: &Graph,
    metas: &HashMap<String, NodeMeta>,
    query_embedding: &[f32],
    config: &RecallConfig,
    top_k: usize,
    depth: usize,
) -> Vec<RecallResult> {
    // Step 1: vector search for seed nodes
    let seeds = index.search(query_embedding, top_k);

    // Step 2: graph expansion
    let mut candidates: HashMap<String, (f32, usize)> = HashMap::new(); // name → (similarity, graph_distance)

    for seed in &seeds {
        candidates
            .entry(seed.node_name.clone())
            .or_insert((seed.similarity, 0));

        // BFS expansion
        let expanded = graph.bfs(&seed.node_name, depth);
        for (neighbor, dist) in expanded {
            candidates.entry(neighbor).or_insert((0.0, dist));
        }
    }

    // Step 3: scoring
    let mut results: Vec<RecallResult> = candidates
        .into_iter()
        .filter_map(|(name, (similarity, graph_dist))| {
            let meta = metas.get(&name)?;

            let graph_proximity = config.proximity_metric.compute(graph_dist);

            let score = config.alpha * similarity
                + config.beta * meta.weight
                + config.gamma * graph_proximity;

            Some(RecallResult {
                node_name: name,
                score,
                similarity,
                weight: meta.weight,
                abstract_text: meta.abstract_text.clone(),
            })
        })
        .collect();

    // Step 4: sort and truncate
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(top_k);

    results
}

/// Multi-term comprehensive retrieval: multiple queries + graph expansion + scoring
///
/// Algorithm:
///   1. For each query embedding: search top-k → seed nodes, BFS expand
///   2. Merge across all queries: max similarity, min graph distance per node
///   3. Score = α×max_sim + β×weight + γ×proximity(min_dist)
///   4. Sort descending, return all merged results (no global cap)
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

    // Merge candidates across all queries: node → (max_similarity, min_graph_dist)
    let mut candidates: HashMap<String, (f32, usize)> = HashMap::new();

    for emb in query_embeddings {
        let seeds = index.search(emb, top_k);

        for seed in &seeds {
            let entry = candidates
                .entry(seed.node_name.clone())
                .or_insert((0.0, 0));
            if seed.similarity > entry.0 {
                entry.0 = seed.similarity;
            }

            let expanded = graph.bfs(&seed.node_name, depth);
            for (neighbor, dist) in expanded {
                let entry = candidates.entry(neighbor).or_insert((0.0, dist));
                if dist < entry.1 {
                    entry.1 = dist;
                }
            }
        }
    }

    // Score all candidates
    let mut results: Vec<RecallResult> = candidates
        .into_iter()
        .filter_map(|(name, (similarity, graph_dist))| {
            let meta = metas.get(&name)?;

            let graph_proximity = config.proximity_metric.compute(graph_dist);

            let score = config.alpha * similarity
                + config.beta * meta.weight
                + config.gamma * graph_proximity;

            Some(RecallResult {
                node_name: name,
                score,
                similarity,
                weight: meta.weight,
                abstract_text: meta.abstract_text.clone(),
            })
        })
        .collect();

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    results
}
