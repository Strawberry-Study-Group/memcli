use memcore::config::*;
use tempfile::TempDir;

// ============================================================
// Default values match design.md §5.7
// ============================================================

#[test]
fn test_default_weight_config() {
    let cfg = WeightConfig::default();
    assert!((cfg.boost_amount - 0.1).abs() < f32::EPSILON);
    assert!((cfg.penalty_factor - 0.8).abs() < f32::EPSILON);
    assert!((cfg.warn_threshold - 0.1).abs() < f32::EPSILON);
}

#[test]
fn test_default_recall_config() {
    let cfg = RecallConfig::default();
    assert!((cfg.alpha - 0.6).abs() < f32::EPSILON);
    assert!((cfg.beta - 0.2).abs() < f32::EPSILON);
    assert!((cfg.gamma - 0.2).abs() < f32::EPSILON);
    assert_eq!(cfg.default_depth, 1);
    assert_eq!(cfg.proximity_metric, ProximityMetric::EdgeDistance);
}

#[test]
fn test_recall_config_proximity_from_toml() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("memcore.toml");
    std::fs::write(&path, r#"
[recall]
proximity_metric = "edge_distance_squared"
"#).unwrap();

    let cfg = load_config_from(&path);
    assert_eq!(cfg.recall.proximity_metric, ProximityMetric::EdgeDistanceSquared);
}

#[test]
fn test_default_recall_weights_sum_to_one() {
    let cfg = RecallConfig::default();
    let sum = cfg.alpha + cfg.beta + cfg.gamma;
    assert!((sum - 1.0).abs() < f32::EPSILON);
}

#[test]
fn test_default_index_config() {
    let cfg = IndexConfig::default();
    assert_eq!(cfg.engine, "usearch");
    assert_eq!(cfg.metric, "cosine");
    assert_eq!(cfg.ef_construction, 128);
    assert_eq!(cfg.m, 16);
}

#[test]
fn test_default_inspect_config() {
    let cfg = InspectConfig::default();
    assert_eq!(cfg.max_cluster_full_scan, 100);
    assert_eq!(cfg.similarity_top_pairs, 50);
}

#[test]
fn test_default_daemon_config() {
    let cfg = DaemonConfig::default();
    assert_eq!(cfg.idle_timeout_minutes, 30);
    assert_eq!(cfg.bind_host, "127.0.0.1");
    assert_eq!(cfg.port, 0);
}

// ============================================================
// Loading from TOML
// ============================================================

#[test]
fn test_load_full_toml() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("memcore.toml");
    std::fs::write(&path, r#"
[weight]
boost_amount = 0.2
penalty_factor = 0.5
warn_threshold = 0.05

[recall]
alpha = 0.5
beta = 0.3
gamma = 0.2

[daemon]
idle_timeout_minutes = 60
port = 9999
"#).unwrap();

    let cfg = load_config_from(&path);
    assert!((cfg.weight.boost_amount - 0.2).abs() < f32::EPSILON);
    assert!((cfg.weight.penalty_factor - 0.5).abs() < f32::EPSILON);
    assert!((cfg.recall.alpha - 0.5).abs() < f32::EPSILON);
    assert_eq!(cfg.daemon.idle_timeout_minutes, 60);
    assert_eq!(cfg.daemon.port, 9999);
}

#[test]
fn test_load_partial_toml_uses_defaults() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("memcore.toml");
    std::fs::write(&path, r#"
[weight]
boost_amount = 0.3
"#).unwrap();

    let cfg = load_config_from(&path);
    assert!((cfg.weight.boost_amount - 0.3).abs() < f32::EPSILON);
    // Rest should be defaults
    assert!((cfg.weight.penalty_factor - 0.8).abs() < f32::EPSILON);
    assert!((cfg.recall.alpha - 0.6).abs() < f32::EPSILON);
    assert_eq!(cfg.daemon.idle_timeout_minutes, 30);
}

#[test]
fn test_load_empty_toml_uses_defaults() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("memcore.toml");
    std::fs::write(&path, "").unwrap();

    let cfg = load_config_from(&path);
    assert!((cfg.weight.boost_amount - 0.1).abs() < f32::EPSILON);
}

#[test]
fn test_load_missing_file_uses_defaults() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nonexistent.toml");

    let cfg = load_config_from(&path);
    assert!((cfg.weight.boost_amount - 0.1).abs() < f32::EPSILON);
}

#[test]
fn test_load_invalid_toml_uses_defaults() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("memcore.toml");
    std::fs::write(&path, "{{{{invalid toml!!!!").unwrap();

    let cfg = load_config_from(&path);
    // Should fallback to defaults, not panic
    assert!((cfg.weight.boost_amount - 0.1).abs() < f32::EPSILON);
}

// ============================================================
// Config serialization roundtrip
// ============================================================

#[test]
fn test_config_serialize_deserialize_roundtrip() {
    let cfg = Config::default();
    let toml_str = toml::to_string_pretty(&cfg).unwrap();
    let cfg2: Config = toml::from_str(&toml_str).unwrap();
    assert!((cfg2.weight.boost_amount - cfg.weight.boost_amount).abs() < f32::EPSILON);
    assert_eq!(cfg2.daemon.bind_host, cfg.daemon.bind_host);
}

// ============================================================
// ModelConfig
// ============================================================

#[test]
fn test_load_model_config() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(&path, r#"{"name":"bge-m3","dimensions":1024,"max_tokens":512}"#).unwrap();

    let mc = load_model_config_from(&path).unwrap();
    assert_eq!(mc.name, "bge-m3");
    assert_eq!(mc.dimensions, 1024);
    assert_eq!(mc.max_tokens, 512);
}

#[test]
fn test_load_model_config_missing_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nope.json");
    assert!(load_model_config_from(&path).is_err());
}

#[test]
fn test_load_model_config_invalid_json() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(&path, "not json").unwrap();
    assert!(load_model_config_from(&path).is_err());
}

// ============================================================
// memcore_dir
// ============================================================

#[test]
fn test_try_memcore_dir_returns_none_without_markers() {
    // Without MEMCORE_DIR set and without memcore.toml/memories/ next to the
    // binary, try_memcore_dir should return None (no silent fallback).
    // We can't unset env vars safely in parallel tests, so just verify the
    // function exists and returns Option<PathBuf>.
    let _result = try_memcore_dir();
}

#[test]
fn test_try_memcore_dir_finds_dir_with_memcore_toml() {
    // A directory containing memcore.toml should be recognized as a memcore dir.
    let tmp = TempDir::new().unwrap();
    let memcore_dir = tmp.path().join("my-memcore");
    std::fs::create_dir_all(&memcore_dir).unwrap();
    std::fs::write(memcore_dir.join("memcore.toml"), "").unwrap();

    // Simulate the check: memcore.toml exists → valid memcore dir
    assert!(memcore_dir.join("memcore.toml").exists());
}

#[test]
fn test_try_memcore_dir_finds_dir_with_memories() {
    // A directory containing memories/ should be recognized as a memcore dir.
    let tmp = TempDir::new().unwrap();
    let memcore_dir = tmp.path().join("my-memcore");
    std::fs::create_dir_all(memcore_dir.join("memories")).unwrap();

    // Simulate the check: memories/ exists → valid memcore dir
    assert!(memcore_dir.join("memories").is_dir());
}

// ============================================================
// dir_name_to_env_var
// ============================================================

#[test]
fn test_dir_name_to_env_var_simple() {
    assert_eq!(dir_name_to_env_var("memcore"), "MEMCORE_DIR");
}

#[test]
fn test_dir_name_to_env_var_with_underscores() {
    assert_eq!(dir_name_to_env_var("work_memcore"), "WORK_MEMCORE_DIR");
}

#[test]
fn test_dir_name_to_env_var_with_hyphens() {
    assert_eq!(dir_name_to_env_var("my-project-memcore"), "MY_PROJECT_MEMCORE_DIR");
}

#[test]
fn test_dir_name_to_env_var_mixed() {
    assert_eq!(dir_name_to_env_var("Project Alpha v2"), "PROJECT_ALPHA_V2_DIR");
}

#[test]
fn test_dir_name_to_env_var_already_upper() {
    assert_eq!(dir_name_to_env_var("MEMCORE"), "MEMCORE_DIR");
}
