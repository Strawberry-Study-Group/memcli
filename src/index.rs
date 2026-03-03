use std::collections::HashMap;
use std::path::Path;

use crate::util::cosine_similarity;

// ============================================================
// Embedding model (requires `embedding` feature)
// ============================================================

#[cfg(feature = "embedding")]
pub struct EmbeddingModel {
    session: ort::session::Session,
    tokenizer: tokenizers::Tokenizer,
    pub dimensions: usize,
    max_tokens: usize,
}

#[cfg(feature = "embedding")]
impl EmbeddingModel {
    /// Load model from a directory containing model_quantized.onnx, tokenizer.json, config.json.
    pub fn load(models_dir: &Path) -> Result<Self, IndexError> {
        let model_path = models_dir.join("model_quantized.onnx");
        let tokenizer_path = models_dir.join("tokenizer.json");
        let config_path = models_dir.join("config.json");

        if !model_path.exists() {
            return Err(IndexError::ModelNotFound(model_path.display().to_string()));
        }
        if !tokenizer_path.exists() {
            return Err(IndexError::ModelNotFound(
                tokenizer_path.display().to_string(),
            ));
        }

        // Load config
        let (dimensions, max_tokens) = if config_path.exists() {
            let config_str = std::fs::read_to_string(&config_path)?;
            let config: serde_json::Value = serde_json::from_str(&config_str)
                .map_err(|e| IndexError::EmbeddingFailed(format!("config parse error: {}", e)))?;
            let dims = config["dimensions"].as_u64().unwrap_or(384) as usize;
            let max_tok = config["max_tokens"].as_u64().unwrap_or(256) as usize;
            (dims, max_tok)
        } else {
            (384, 256)
        };

        // Load ONNX model
        let session = ort::session::Session::builder()
            .map_err(|e| IndexError::EmbeddingFailed(format!("ort builder error: {}", e)))?
            .with_intra_threads(1)
            .map_err(|e| IndexError::EmbeddingFailed(format!("ort thread config error: {}", e)))?
            .commit_from_file(&model_path)
            .map_err(|e| IndexError::EmbeddingFailed(format!("ort session error: {}", e)))?;

        // Load tokenizer
        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| IndexError::EmbeddingFailed(format!("tokenizer error: {}", e)))?;

        tracing::info!(
            "loaded embedding model: dims={}, max_tokens={}",
            dimensions,
            max_tokens
        );

        Ok(Self {
            session,
            tokenizer,
            dimensions,
            max_tokens,
        })
    }

    /// Compute embedding for a text string.
    ///
    /// Pipeline: tokenize → truncate → ONNX inference → mean pooling → L2 normalize
    pub fn compute(&mut self, text: &str) -> Result<Vec<f32>, IndexError> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| IndexError::EmbeddingFailed(format!("tokenize error: {}", e)))?;

        let mut input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let mut attention_mask: Vec<i64> =
            encoding.get_attention_mask().iter().map(|&m| m as i64).collect();
        let mut token_type_ids: Vec<i64> =
            encoding.get_type_ids().iter().map(|&t| t as i64).collect();

        // Truncate to max_tokens
        if input_ids.len() > self.max_tokens {
            input_ids.truncate(self.max_tokens);
            attention_mask.truncate(self.max_tokens);
            token_type_ids.truncate(self.max_tokens);
        }

        let seq_len = input_ids.len();

        // Build input tensors [1, seq_len] using ort::Tensor::from_array with (shape, vec)
        let ids_tensor = ort::value::Tensor::from_array(([1usize, seq_len], input_ids))
            .map_err(|e| IndexError::EmbeddingFailed(format!("input_ids tensor error: {}", e)))?;
        let mask_tensor =
            ort::value::Tensor::from_array(([1usize, seq_len], attention_mask.clone()))
                .map_err(|e| {
                    IndexError::EmbeddingFailed(format!("attention_mask tensor error: {}", e))
                })?;
        let type_tensor = ort::value::Tensor::from_array(([1usize, seq_len], token_type_ids))
            .map_err(|e| {
                IndexError::EmbeddingFailed(format!("token_type_ids tensor error: {}", e))
            })?;

        let outputs = self
            .session
            .run(ort::inputs![ids_tensor, mask_tensor, type_tensor])
            .map_err(|e| IndexError::EmbeddingFailed(format!("inference error: {}", e)))?;

        // Output shape: [1, seq_len, hidden_size]
        let (output_shape, output_data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| IndexError::EmbeddingFailed(format!("output extract error: {}", e)))?;

        // Shape derefs to &[i64]
        let dims: &[i64] = &output_shape;
        if dims.len() != 3 {
            return Err(IndexError::EmbeddingFailed(format!(
                "unexpected output shape: {:?}",
                dims
            )));
        }

        let hidden_size = dims[2] as usize;

        // Mean pooling with attention mask
        // output_data layout: [batch=1][seq_len][hidden_size] flattened
        let mut pooled = vec![0.0f32; hidden_size];
        let mut mask_sum = 0.0f32;

        for tok in 0..seq_len {
            let mask_val = attention_mask[tok] as f32;
            mask_sum += mask_val;
            let offset = tok * hidden_size;
            for h in 0..hidden_size {
                pooled[h] += output_data[offset + h] * mask_val;
            }
        }

        if mask_sum > 0.0 {
            for h in 0..hidden_size {
                pooled[h] /= mask_sum;
            }
        }

        // L2 normalize
        let norm: f32 = pooled.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut pooled {
                *x /= norm;
            }
        }

        Ok(pooled)
    }
}

/// Mapping from vector ID to node name
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeMapping {
    pub vector_id: u64,
    pub node_name: String,
}

/// Vector index with in-memory brute-force cosine search.
///
/// Stores embeddings in a HashMap keyed by node name.
/// Search is O(N) brute-force cosine similarity — sufficient for
/// thousands of nodes. Can be swapped to usearch HNSW later.
pub struct VectorIndex {
    /// node_name → (vector_id, embedding)
    embeddings: HashMap<String, (u64, Vec<f32>)>,
    next_id: u64,
    dimensions: Option<usize>,
}

impl VectorIndex {
    pub fn new() -> Self {
        Self {
            embeddings: HashMap::new(),
            next_id: 0,
            dimensions: None,
        }
    }

    /// Create a VectorIndex with a fixed dimensionality.
    pub fn with_dimensions(dim: usize) -> Self {
        Self {
            embeddings: HashMap::new(),
            next_id: 0,
            dimensions: Some(dim),
        }
    }

    /// Insert an embedding for a node. Returns the assigned vector ID.
    ///
    /// If the node already exists, its embedding is overwritten.
    /// Panics if embedding dimensions don't match previously inserted vectors.
    pub fn insert(&mut self, node_name: &str, embedding: &[f32]) -> u64 {
        match self.dimensions {
            Some(dim) => {
                assert_eq!(
                    embedding.len(),
                    dim,
                    "dimension mismatch: expected {}, got {}",
                    dim,
                    embedding.len()
                );
            }
            None => {
                self.dimensions = Some(embedding.len());
            }
        }

        // Remove old entry if overwriting
        self.embeddings.remove(node_name);

        let id = self.next_id;
        self.next_id += 1;
        self.embeddings
            .insert(node_name.to_string(), (id, embedding.to_vec()));
        id
    }

    /// Remove a node from the vector index. Returns true if it existed.
    pub fn remove(&mut self, node_name: &str) -> bool {
        self.embeddings.remove(node_name).is_some()
    }

    /// Search for top-k most similar nodes using brute-force cosine similarity.
    ///
    /// Returns results sorted by descending similarity.
    pub fn search(&self, query_embedding: &[f32], top_k: usize) -> Vec<SearchResult> {
        if top_k == 0 || self.embeddings.is_empty() {
            return Vec::new();
        }

        let mut scored: Vec<SearchResult> = self
            .embeddings
            .iter()
            .map(|(name, (_id, emb))| SearchResult {
                node_name: name.clone(),
                similarity: cosine_similarity(query_embedding, emb),
            })
            .collect();

        // Sort by descending similarity
        scored.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        scored.truncate(top_k);
        scored
    }

    /// Get the stored embedding for a node.
    pub fn get_embedding(&self, node_name: &str) -> Option<&[f32]> {
        self.embeddings
            .get(node_name)
            .map(|(_id, emb)| emb.as_slice())
    }

    /// Check if a node exists in the index.
    pub fn contains(&self, node_name: &str) -> bool {
        self.embeddings.contains_key(node_name)
    }

    /// Rename a node in the index. Returns true if the old name existed.
    pub fn rename(&mut self, old_name: &str, new_name: &str) -> bool {
        if let Some(entry) = self.embeddings.remove(old_name) {
            self.embeddings.insert(new_name.to_string(), entry);
            true
        } else {
            false
        }
    }

    /// Return all node names in the index.
    pub fn all_node_names(&self) -> Vec<&str> {
        self.embeddings.keys().map(|s| s.as_str()).collect()
    }

    pub fn node_count(&self) -> usize {
        self.embeddings.len()
    }

    /// Get the dimensionality of stored vectors (None if no vectors inserted yet).
    pub fn dimensions(&self) -> Option<usize> {
        self.dimensions
    }

    /// Save the vector index to a directory (vectors.map + vectors.dat).
    ///
    /// - `vectors.map`: JSON array of `{ vector_id, node_name }` mappings
    /// - `vectors.dat`: raw f32 embeddings in insertion order, matched by vectors.map
    pub fn save_to_dir(&self, dir: &Path) -> Result<(), IndexError> {
        std::fs::create_dir_all(dir)?;

        // Build ordered list for deterministic output
        let mut entries: Vec<(&str, &u64, &Vec<f32>)> = self
            .embeddings
            .iter()
            .map(|(name, (id, emb))| (name.as_str(), id, emb))
            .collect();
        entries.sort_by_key(|(_, id, _)| *id);

        // Write vectors.map (JSON)
        let mappings: Vec<NodeMapping> = entries
            .iter()
            .map(|(name, id, _)| NodeMapping {
                vector_id: **id,
                node_name: name.to_string(),
            })
            .collect();
        let map_json = serde_json::to_string_pretty(&mappings)
            .map_err(|e| IndexError::EmbeddingFailed(e.to_string()))?;
        std::fs::write(dir.join("vectors.map"), map_json)?;

        // Write vectors.dat (binary: dimensions as u32, then f32 embeddings sequentially)
        let mut dat = Vec::new();
        let dim = self.dimensions.unwrap_or(0) as u32;
        dat.extend_from_slice(&dim.to_le_bytes());
        for (_, _, emb) in &entries {
            for &val in emb.iter() {
                dat.extend_from_slice(&val.to_le_bytes());
            }
        }
        std::fs::write(dir.join("vectors.dat"), dat)?;

        Ok(())
    }

    /// Load a vector index from a directory containing vectors.map + vectors.dat.
    ///
    /// Returns an empty index if the files don't exist.
    pub fn load_from_dir(dir: &Path) -> Result<Self, IndexError> {
        let map_path = dir.join("vectors.map");
        let dat_path = dir.join("vectors.dat");

        if !map_path.exists() || !dat_path.exists() {
            return Ok(Self::new());
        }

        // Read mappings
        let map_json = std::fs::read_to_string(&map_path)?;
        let mappings: Vec<NodeMapping> = serde_json::from_str(&map_json)
            .map_err(|e| IndexError::EmbeddingFailed(format!("corrupt vectors.map: {}", e)))?;

        // Read binary data
        let dat = std::fs::read(&dat_path)?;
        if dat.len() < 4 {
            return Err(IndexError::EmbeddingFailed(
                "vectors.dat too short".to_string(),
            ));
        }

        let dim = u32::from_le_bytes(dat[0..4].try_into().unwrap()) as usize;

        if dim == 0 && mappings.is_empty() {
            return Ok(Self::new());
        }

        let floats_start = 4;
        let floats_per_entry = dim;
        let bytes_per_entry = floats_per_entry * 4;
        let expected_len = floats_start + mappings.len() * bytes_per_entry;

        if dat.len() < expected_len {
            return Err(IndexError::EmbeddingFailed(format!(
                "vectors.dat truncated: expected {} bytes, got {}",
                expected_len,
                dat.len()
            )));
        }

        let mut idx = Self::with_dimensions(dim);
        let mut max_id: u64 = 0;

        for (i, mapping) in mappings.iter().enumerate() {
            let offset = floats_start + i * bytes_per_entry;
            let mut embedding = Vec::with_capacity(dim);
            for j in 0..dim {
                let byte_offset = offset + j * 4;
                let val =
                    f32::from_le_bytes(dat[byte_offset..byte_offset + 4].try_into().unwrap());
                embedding.push(val);
            }
            idx.embeddings.insert(
                mapping.node_name.clone(),
                (mapping.vector_id, embedding),
            );
            if mapping.vector_id >= max_id {
                max_id = mapping.vector_id + 1;
            }
        }

        idx.next_id = max_id;

        Ok(idx)
    }

    /// Compute embedding for an abstract text (requires `embedding` feature).
    ///
    /// Without the `embedding` feature, this always returns an error.
    /// With the feature, use `EmbeddingModel::compute()` directly instead.
    pub fn compute_embedding(&self, _abstract_text: &str) -> Result<Vec<f32>, IndexError> {
        Err(IndexError::EmbeddingFailed(
            "use EmbeddingModel::compute() instead".to_string(),
        ))
    }

    /// Path to the vector index directory
    pub fn index_dir() -> std::path::PathBuf {
        crate::config::memcore_dir().join("index")
    }
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub node_name: String,
    pub similarity: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("embedding computation failed: {0}")]
    EmbeddingFailed(String),
    #[error("abstract text exceeds max token limit")]
    AbstractTooLong,
    #[error("model not found at {0}")]
    ModelNotFound(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
