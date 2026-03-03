use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Global configuration from memcore.toml
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub index: IndexConfig,
    #[serde(default)]
    pub weight: WeightConfig,
    #[serde(default)]
    pub recall: RecallConfig,
    #[serde(default)]
    pub inspect: InspectConfig,
    #[serde(default)]
    pub daemon: DaemonConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IndexConfig {
    #[serde(default = "default_engine")]
    pub engine: String,
    #[serde(default = "default_metric")]
    pub metric: String,
    #[serde(default = "default_ef_construction")]
    pub ef_construction: usize,
    #[serde(default = "default_m")]
    pub m: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WeightConfig {
    #[serde(default = "default_boost_amount")]
    pub boost_amount: f32,
    #[serde(default = "default_penalty_factor")]
    pub penalty_factor: f32,
    #[serde(default = "default_warn_threshold")]
    pub warn_threshold: f32,
}

/// How graph proximity is computed from BFS hop distance.
///
/// Add new variants here to plug in alternative proximity formulas.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProximityMetric {
    /// `1 / (1 + hops)` — simple inverse hop distance.
    /// Seed nodes (hops=0) get proximity 0 (they already have a similarity score).
    EdgeDistance,
    /// `1 / (1 + hops)^2` — sharper falloff, strongly favours direct neighbours.
    EdgeDistanceSquared,
}

impl Default for ProximityMetric {
    fn default() -> Self {
        Self::EdgeDistance
    }
}

impl ProximityMetric {
    /// Compute graph proximity score for a candidate at `hops` distance from a seed.
    ///
    /// Returns 0.0 for seed nodes (hops == 0) since their contribution comes
    /// from the similarity term instead.
    pub fn compute(&self, hops: usize) -> f32 {
        if hops == 0 {
            return 0.0;
        }
        match self {
            Self::EdgeDistance => 1.0 / (1.0 + hops as f32),
            Self::EdgeDistanceSquared => {
                let d = 1.0 + hops as f32;
                1.0 / (d * d)
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecallConfig {
    #[serde(default = "default_alpha")]
    pub alpha: f32,
    #[serde(default = "default_beta")]
    pub beta: f32,
    #[serde(default = "default_gamma")]
    pub gamma: f32,
    #[serde(default = "default_depth")]
    pub default_depth: usize,
    #[serde(default)]
    pub proximity_metric: ProximityMetric,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InspectConfig {
    #[serde(default = "default_max_cluster_full_scan")]
    pub max_cluster_full_scan: usize,
    #[serde(default = "default_similarity_top_pairs")]
    pub similarity_top_pairs: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonConfig {
    #[serde(default = "default_idle_timeout_minutes")]
    pub idle_timeout_minutes: u64,
    #[serde(default = "default_bind_host")]
    pub bind_host: String,
    #[serde(default)]
    pub port: u16,
}

/// Model metadata from models/config.json
#[derive(Debug, Serialize, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    pub dimensions: usize,
    pub max_tokens: usize,
}

// --- Defaults ---

fn default_engine() -> String { "usearch".into() }
fn default_metric() -> String { "cosine".into() }
fn default_ef_construction() -> usize { 128 }
fn default_m() -> usize { 16 }

fn default_boost_amount() -> f32 { 0.1 }
fn default_penalty_factor() -> f32 { 0.8 }
fn default_warn_threshold() -> f32 { 0.1 }

fn default_alpha() -> f32 { 0.6 }
fn default_beta() -> f32 { 0.2 }
fn default_gamma() -> f32 { 0.2 }
fn default_depth() -> usize { 1 }

fn default_max_cluster_full_scan() -> usize { 100 }
fn default_similarity_top_pairs() -> usize { 50 }

fn default_idle_timeout_minutes() -> u64 { 30 }
fn default_bind_host() -> String { "127.0.0.1".into() }

// --- Default trait impls ---

impl Default for Config {
    fn default() -> Self {
        Self {
            index: IndexConfig::default(),
            weight: WeightConfig::default(),
            recall: RecallConfig::default(),
            inspect: InspectConfig::default(),
            daemon: DaemonConfig::default(),
        }
    }
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            engine: default_engine(),
            metric: default_metric(),
            ef_construction: default_ef_construction(),
            m: default_m(),
        }
    }
}

impl Default for WeightConfig {
    fn default() -> Self {
        Self {
            boost_amount: default_boost_amount(),
            penalty_factor: default_penalty_factor(),
            warn_threshold: default_warn_threshold(),
        }
    }
}

impl Default for RecallConfig {
    fn default() -> Self {
        Self {
            alpha: default_alpha(),
            beta: default_beta(),
            gamma: default_gamma(),
            default_depth: default_depth(),
            proximity_metric: ProximityMetric::default(),
        }
    }
}

impl Default for InspectConfig {
    fn default() -> Self {
        Self {
            max_cluster_full_scan: default_max_cluster_full_scan(),
            similarity_top_pairs: default_similarity_top_pairs(),
        }
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            idle_timeout_minutes: default_idle_timeout_minutes(),
            bind_host: default_bind_host(),
            port: 0,
        }
    }
}

/// Resolve the memcore root directory
pub fn memcore_dir() -> PathBuf {
    std::env::var("MEMCORE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .expect("cannot determine home directory")
                .join(".memcore")
        })
}

/// Load config from memcore.toml, falling back to defaults
pub fn load_config() -> Config {
    load_config_from(&memcore_dir().join("memcore.toml"))
}

/// Load config from a specific path, falling back to defaults
pub fn load_config_from(path: &std::path::Path) -> Config {
    match std::fs::read_to_string(path) {
        Ok(content) => toml::from_str(&content).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

/// Load model config from models/config.json
pub fn load_model_config_from(path: &std::path::Path) -> Result<ModelConfig, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read model config: {}", e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("invalid model config JSON: {}", e))
}
