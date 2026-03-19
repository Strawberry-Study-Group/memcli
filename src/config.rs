use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Global configuration from memcore.toml
#[derive(Debug, Default, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize)]
pub struct RecallConfig {
    #[serde(default = "default_vitality_floor")]
    pub vitality_floor: f32,
    #[serde(default = "default_depth")]
    pub default_depth: usize,
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
    pub query_prefix: Option<String>,
    pub passage_prefix: Option<String>,
}

// --- Defaults ---

fn default_engine() -> String { "usearch".into() }
fn default_metric() -> String { "cosine".into() }
fn default_ef_construction() -> usize { 128 }
fn default_m() -> usize { 16 }

fn default_boost_amount() -> f32 { 0.1 }
fn default_penalty_factor() -> f32 { 0.8 }
fn default_warn_threshold() -> f32 { 0.1 }

fn default_vitality_floor() -> f32 { 0.05 }
fn default_depth() -> usize { 1 }

fn default_max_cluster_full_scan() -> usize { 100 }
fn default_similarity_top_pairs() -> usize { 50 }

fn default_idle_timeout_minutes() -> u64 { 30 }
fn default_bind_host() -> String { "127.0.0.1".into() }

// --- Default trait impls ---


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
            vitality_floor: default_vitality_floor(),
            default_depth: default_depth(),
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

/// Resolve the memcore root directory. No implicit fallbacks — explicit only.
///
/// Priority:
///   1. `MEMCORE_DIR` env var (generic override, always wins)
///   2. `<DIRNAME>_DIR` env var (derived from binary's parent directory name)
///      e.g. binary in `work_memcore/` → checks `WORK_MEMCORE_DIR`
///   3. Binary's own parent directory (if it contains memcore.toml or memories/)
///
/// If none resolves, prints an error and exits. No silent fallback to
/// `~/.memcore/` or hidden `.memcore/` directories.
pub fn memcore_dir() -> PathBuf {
    match try_memcore_dir() {
        Some(dir) => dir,
        None => {
            let hint = exe_parent_dir_name()
                .map(|name| {
                    let env_name = dir_name_to_env_var(&name);
                    format!(
                        "Set {env_name} or MEMCORE_DIR to the path of your memcore directory, \
                         or run the binary from within it.\n\
                         Example: export {env_name}=\"/path/to/{name}\""
                    )
                })
                .unwrap_or_else(|| {
                    "Set MEMCORE_DIR to the path of your memcore directory, \
                     or run the binary from within it.\n\
                     Example: export MEMCORE_DIR=\"/path/to/your/memcore\""
                        .to_string()
                });
            eprintln!("error: cannot find memcore directory.\n{hint}");
            std::process::exit(1);
        }
    }
}

/// Try to resolve the memcore root directory, returning None if not found.
pub fn try_memcore_dir() -> Option<PathBuf> {
    // 1. MEMCORE_DIR env var (generic override)
    if let Ok(dir) = std::env::var("MEMCORE_DIR") {
        return Some(PathBuf::from(dir));
    }

    // 2. <DIRNAME>_DIR env var (derived from binary's parent directory name)
    //    e.g. binary at /home/user/work_memcore/memcore → WORK_MEMCORE_DIR
    if let Some(name) = exe_parent_dir_name() {
        let env_name = dir_name_to_env_var(&name);
        if let Ok(dir) = std::env::var(&env_name) {
            return Some(PathBuf::from(dir));
        }
    }

    // 3. Binary's own parent directory (self-contained release layout)
    if let Ok(exe) = std::env::current_exe()
        && let Ok(canonical) = exe.canonicalize()
        && let Some(parent) = canonical.parent()
        && (parent.join("memcore.toml").exists() || parent.join("memories").is_dir())
    {
        return Some(parent.to_path_buf());
    }

    None
}

/// Get the binary's parent directory name (e.g. "work_memcore").
fn exe_parent_dir_name() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let canonical = exe.canonicalize().ok()?;
    let parent = canonical.parent()?;
    parent.file_name()?.to_str().map(|s| s.to_string())
}

/// Convert a directory name to an env var name.
/// "work_memcore" → "WORK_MEMCORE_DIR"
/// "my-project-memcore" → "MY_PROJECT_MEMCORE_DIR"
/// "memcore" → "MEMCORE_DIR"
pub fn dir_name_to_env_var(name: &str) -> String {
    let upper: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
        .collect();
    format!("{}_DIR", upper)
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
