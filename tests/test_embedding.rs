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
    assert_eq!(model.dimensions, 384);
}

#[test]
fn test_compute_embedding_basic() {
    let mut model = EmbeddingModel::load(&models_dir()).unwrap();
    let embedding = model.compute("hello world").unwrap();
    assert_eq!(embedding.len(), 384);

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
    let embedding = model.compute("").unwrap();
    assert_eq!(embedding.len(), 384);
}

#[test]
fn test_similar_texts_have_high_similarity() {
    let mut model = EmbeddingModel::load(&models_dir()).unwrap();

    let emb_a = model.compute("the cat sat on the mat").unwrap();
    let emb_b = model.compute("a cat is sitting on a mat").unwrap();
    let emb_c = model.compute("quantum physics equations").unwrap();

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

    let emb_a = model.compute("rust programming language").unwrap();
    let emb_b = model.compute("recipe for chocolate cake").unwrap();

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

    let emb1 = model.compute("reproducibility test").unwrap();
    let emb2 = model.compute("reproducibility test").unwrap();

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
    let embedding = model.compute(&long_text).unwrap();
    assert_eq!(embedding.len(), 384);

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
    let embedding = model
        .compute("\u{4E2D}\u{6587}\u{6587}\u{672C}\u{6D4B}\u{8BD5} \u{65E5}\u{672C}\u{8A9E}\u{30C6}\u{30B9}\u{30C8}")
        .unwrap();
    assert_eq!(embedding.len(), 384);
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
    let embedding = model
        .compute("<html><body>fn main() { println!(\"hello\"); }</body></html>")
        .unwrap();
    assert_eq!(embedding.len(), 384);
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
    let emb1 = model.compute(text).unwrap();
    let emb2 = model.compute(text).unwrap();
    let emb3 = model.compute(text).unwrap();

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

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}
