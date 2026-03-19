//! Tests for embedding model loading and inference.
//! Only compiled with `--features embedding`.
#![cfg(feature = "embedding")]

use memcore::index::EmbeddingModel;
use std::path::PathBuf;

fn models_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models")
}

#[test]
fn test_load_model() {
    let model = EmbeddingModel::load(&models_dir());
    assert!(model.is_ok(), "failed to load model: {:?}", model.err());
    let model = model.unwrap();
    // dimensions are set by probe, not config.json
    assert!(model.dimensions > 0, "probed dimensions should be > 0");
}

#[test]
fn test_compute_embedding_basic() {
    let mut model = EmbeddingModel::load(&models_dir()).unwrap();
    let dims = model.dimensions;
    let embedding = model.compute_passage("hello world").unwrap();
    assert_eq!(embedding.len(), dims);

    // Check L2 normalization: magnitude should be ~1.0
    let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!(
        (norm - 1.0).abs() < 0.01,
        "embedding not normalized: norm={}",
        norm
    );
}

#[test]
fn test_compute_embedding_empty_string() {
    let mut model = EmbeddingModel::load(&models_dir()).unwrap();
    let dims = model.dimensions;
    let embedding = model.compute_passage("").unwrap();
    assert_eq!(embedding.len(), dims);
}

#[test]
fn test_similar_texts_have_high_similarity() {
    let mut model = EmbeddingModel::load(&models_dir()).unwrap();

    let emb_a = model.compute_passage("the cat sat on the mat").unwrap();
    let emb_b = model.compute_passage("a cat is sitting on a mat").unwrap();
    let emb_c = model.compute_passage("quantum physics equations").unwrap();

    let sim_ab = cosine(&emb_a, &emb_b);
    let sim_ac = cosine(&emb_a, &emb_c);

    assert!(
        sim_ab > sim_ac,
        "similar texts should have higher similarity: ab={} ac={}",
        sim_ab,
        sim_ac
    );
    assert!(sim_ab > 0.5, "similar texts should have sim > 0.5: {}", sim_ab);
}

#[test]
fn test_different_texts_have_lower_similarity() {
    let mut model = EmbeddingModel::load(&models_dir()).unwrap();

    let emb_a = model.compute_passage("rust programming language").unwrap();
    let emb_b = model.compute_passage("recipe for chocolate cake").unwrap();

    let sim = cosine(&emb_a, &emb_b);
    assert!(
        sim < 0.5,
        "unrelated texts should have sim < 0.5: {}",
        sim
    );
}

#[test]
fn test_same_text_has_identical_embedding() {
    let mut model = EmbeddingModel::load(&models_dir()).unwrap();

    let emb1 = model.compute_passage("reproducibility test").unwrap();
    let emb2 = model.compute_passage("reproducibility test").unwrap();

    let sim = cosine(&emb1, &emb2);
    assert!(
        (sim - 1.0).abs() < 0.001,
        "same text should produce identical embeddings: sim={}",
        sim
    );
}

#[test]
fn test_long_text_truncation() {
    let mut model = EmbeddingModel::load(&models_dir()).unwrap();

    // Generate a very long text (well over 256 tokens)
    let long_text = "the quick brown fox jumps over the lazy dog ".repeat(100);
    let embedding = model.compute_passage(&long_text).unwrap();
    assert_eq!(embedding.len(), model.dimensions);

    let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!(
        (norm - 1.0).abs() < 0.01,
        "long text embedding not normalized: norm={}",
        norm
    );
}

#[test]
fn test_model_not_found() {
    let result = EmbeddingModel::load(&PathBuf::from("/nonexistent/path"));
    assert!(result.is_err());
}

#[test]
fn test_compute_unicode_text() {
    let mut model = EmbeddingModel::load(&models_dir()).unwrap();
    let dims = model.dimensions;
    let embedding = model
        .compute_passage("\u{4E2D}\u{6587}\u{6587}\u{672C}\u{6D4B}\u{8BD5} \u{65E5}\u{672C}\u{8A9E}\u{30C6}\u{30B9}\u{30C8}")
        .unwrap();
    assert_eq!(embedding.len(), dims);
    let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!(
        (norm - 1.0).abs() < 0.01,
        "unicode embedding not normalized: norm={}",
        norm
    );
}

#[test]
fn test_compute_special_characters() {
    let mut model = EmbeddingModel::load(&models_dir()).unwrap();
    let dims = model.dimensions;
    let embedding = model
        .compute_passage("<html><body>fn main() { println!(\"hello\"); }</body></html>")
        .unwrap();
    assert_eq!(embedding.len(), dims);
    let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!(
        (norm - 1.0).abs() < 0.01,
        "special chars embedding not normalized: norm={}",
        norm
    );
}

#[test]
fn test_batch_compute_consistency() {
    let mut model = EmbeddingModel::load(&models_dir()).unwrap();
    let text = "consistency test across multiple calls";
    let emb1 = model.compute_passage(text).unwrap();
    let emb2 = model.compute_passage(text).unwrap();
    let emb3 = model.compute_passage(text).unwrap();

    assert_eq!(emb1.len(), emb2.len());
    assert_eq!(emb2.len(), emb3.len());
    for i in 0..emb1.len() {
        assert!(
            (emb1[i] - emb2[i]).abs() < 1e-6,
            "embedding mismatch at dim {}: {} vs {}",
            i, emb1[i], emb2[i]
        );
        assert!(
            (emb2[i] - emb3[i]).abs() < 1e-6,
            "embedding mismatch at dim {}: {} vs {}",
            i, emb2[i], emb3[i]
        );
    }
}

/// Verify that the model detects whether token_type_ids is accepted.
/// Our bundled e5-small model has 3 inputs (including token_type_ids).
#[test]
fn test_model_detects_token_type_ids() {
    let model = EmbeddingModel::load(&models_dir()).unwrap();
    // The bundled e5-small ONNX model accepts token_type_ids
    assert!(
        model.has_token_type_ids,
        "bundled e5-small model should have token_type_ids input"
    );
}

/// Verify that compute works regardless of token_type_ids presence.
/// This is a regression test: before the fix, models without token_type_ids
/// would fail with "3 inputs provided but model only accepts 2".
#[test]
fn test_compute_works_with_token_type_ids_flag() {
    let mut model = EmbeddingModel::load(&models_dir()).unwrap();

    // Compute should succeed whether has_token_type_ids is true or false.
    // Test the true path (our bundled model).
    assert!(model.has_token_type_ids);
    let dims = model.dimensions;
    let emb = model.compute_passage("token type ids test").unwrap();
    assert_eq!(emb.len(), dims);

    // Force the false path: even though the model actually accepts it,
    // e5 models work correctly without token_type_ids (all zeros = default).
    model.has_token_type_ids = false;
    let emb_without = model.compute_passage("token type ids test").unwrap();
    assert_eq!(emb_without.len(), dims);

    // Both paths should produce valid normalized embeddings
    let norm: f32 = emb_without.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!(
        (norm - 1.0).abs() < 0.01,
        "embedding without token_type_ids not normalized: norm={}",
        norm
    );

    // For e5-small, token_type_ids are all zeros anyway, so results
    // should be very similar (though not necessarily identical due to
    // ONNX graph differences)
    let sim = cosine(&emb, &emb_without);
    assert!(
        sim > 0.99,
        "with/without token_type_ids should be near-identical for e5: sim={}",
        sim
    );
}

// ============================================================
// Probe-at-load tests
// ============================================================

/// Probed dimensions must match actual compute output length.
#[test]
fn test_probed_dimensions_match_output() {
    let mut model = EmbeddingModel::load(&models_dir()).unwrap();
    let embedding = model.compute_passage("test text").unwrap();
    assert_eq!(
        model.dimensions,
        embedding.len(),
        "probed dimensions should match actual compute output length"
    );
}

/// Query and passage prefixes produce different embeddings for e5 models.
#[test]
fn test_compute_query_vs_passage_differ() {
    let mut model = EmbeddingModel::load(&models_dir()).unwrap();
    // Only meaningful if prefixes are configured
    if model.query_prefix.is_none() && model.passage_prefix.is_none() {
        return; // skip — no prefixes configured
    }
    let query_emb = model.compute_query("machine learning").unwrap();
    let passage_emb = model.compute_passage("machine learning").unwrap();

    let sim = cosine(&query_emb, &passage_emb);
    assert!(
        sim < 1.0,
        "query and passage embeddings should differ with prefixes: sim={}",
        sim
    );
}

/// Without prefixes, compute_query and compute_passage produce identical results.
#[test]
fn test_no_prefix_backward_compat() {
    let mut model = EmbeddingModel::load(&models_dir()).unwrap();
    model.query_prefix = None;
    model.passage_prefix = None;

    let query_emb = model.compute_query("test text").unwrap();
    let passage_emb = model.compute_passage("test text").unwrap();

    let sim = cosine(&query_emb, &passage_emb);
    assert!(
        (sim - 1.0).abs() < 0.001,
        "without prefixes, query and passage should be identical: sim={}",
        sim
    );
}

/// Corrupt model files should cause load() to fail (not compute()).
#[test]
fn test_probe_catches_broken_model() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("model_quantized.onnx"), b"not a real model").unwrap();
    std::fs::write(tmp.path().join("tokenizer.json"), b"{}").unwrap();
    let result = EmbeddingModel::load(tmp.path());
    assert!(
        result.is_err(),
        "corrupt model should fail at load time, not at compute time"
    );
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}
