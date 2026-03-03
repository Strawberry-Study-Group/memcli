use memcore::node::*;
use chrono::{Datelike, Utc};
use std::fs;
use tempfile::TempDir;

// ============================================================
// validate_name — design.md §3.1
// ============================================================

#[test]
fn test_valid_names() {
    assert!(validate_name("user-profile").is_ok());
    assert!(validate_name("crawler").is_ok());
    assert!(validate_name("project-alpha-v2").is_ok());
    assert!(validate_name("a1b2c3").is_ok());
    assert!(validate_name("ab").is_ok()); // min 2 chars
}

#[test]
fn test_valid_name_mixed_case() {
    assert!(validate_name("User-Profile").is_ok());
    assert!(validate_name("UPPERCASE").is_ok());
    assert!(validate_name("How to deploy the app").is_ok());
    assert!(validate_name("Rust memory management tips").is_ok());
    assert!(validate_name("Project Alpha v2").is_ok());
}

#[test]
fn test_valid_name_128_chars() {
    // Max length: 128 chars total
    let name = format!("A{}", "b".repeat(127));
    assert_eq!(name.len(), 128);
    assert!(validate_name(&name).is_ok());
}

#[test]
fn test_valid_name_with_spaces() {
    assert!(validate_name("my node").is_ok());
    assert!(validate_name("a b").is_ok());
}

#[test]
fn test_invalid_name_underscore() {
    assert!(validate_name("my_note").is_err());
}

#[test]
fn test_invalid_name_digit_start() {
    assert!(validate_name("2fast").is_err());
}

#[test]
fn test_invalid_name_too_short() {
    assert!(validate_name("a").is_err());
}

#[test]
fn test_invalid_name_empty() {
    assert!(validate_name("").is_err());
}

#[test]
fn test_invalid_name_slash() {
    assert!(validate_name("projects/crawler").is_err());
}

#[test]
fn test_invalid_name_hyphen_start() {
    assert!(validate_name("-bad").is_err());
}

#[test]
fn test_invalid_name_too_long() {
    // 129 chars = over limit
    let name = format!("A{}", "b".repeat(128));
    assert_eq!(name.len(), 129);
    assert!(validate_name(&name).is_err());
}

#[test]
fn test_invalid_name_trailing_space() {
    assert!(validate_name("hello ").is_err());
}

#[test]
fn test_invalid_name_trailing_hyphen() {
    assert!(validate_name("hello-").is_err());
}

#[test]
fn test_invalid_name_dots() {
    assert!(validate_name("my.node").is_err());
}

// ============================================================
// parse_node_file — design.md §3.2
// ============================================================

#[test]
fn test_parse_full_frontmatter() {
    let content = r#"---
created: "2026-02-23T14:30:00Z"
updated: "2026-02-23T16:00:00Z"
weight: 0.85
last_accessed: "2026-02-23T16:00:00Z"
access_count: 4
pinned: false
links:
  - user-profile
  - data-pipeline
abstract: |
  Test abstract content.
---

# Test Node

Body content here.
"#;

    let (fm, body) = parse_node_file(content).expect("parse failed");
    assert!((fm.weight - 0.85).abs() < f32::EPSILON);
    assert_eq!(fm.access_count, 4);
    assert!(!fm.pinned);
    assert_eq!(fm.links, vec!["user-profile", "data-pipeline"]);
    assert!(fm.abstract_text.contains("Test abstract content."));
    assert!(body.contains("# Test Node"));
    assert!(body.contains("Body content here."));
}

#[test]
fn test_parse_minimal_frontmatter() {
    let content = r#"---
created: "2026-02-23T14:30:00Z"
updated: "2026-02-23T14:30:00Z"
last_accessed: "2026-02-23T14:30:00Z"
abstract: "Minimal node."
---

Body.
"#;

    let (fm, body) = parse_node_file(content).expect("parse failed");
    // Defaults
    assert!((fm.weight - 1.0).abs() < f32::EPSILON); // default weight
    assert_eq!(fm.access_count, 0);                    // default
    assert!(!fm.pinned);                                // default
    assert!(fm.links.is_empty());                       // default
    assert_eq!(fm.abstract_text, "Minimal node.");
    assert!(body.contains("Body."));
}

#[test]
fn test_parse_pinned_true() {
    let content = r#"---
created: "2026-02-23T14:30:00Z"
updated: "2026-02-23T14:30:00Z"
last_accessed: "2026-02-23T14:30:00Z"
pinned: true
abstract: "Core memory."
---

Important stuff.
"#;

    let (fm, _body) = parse_node_file(content).expect("parse failed");
    assert!(fm.pinned);
}

#[test]
fn test_parse_empty_links() {
    let content = r#"---
created: "2026-02-23T14:30:00Z"
updated: "2026-02-23T14:30:00Z"
last_accessed: "2026-02-23T14:30:00Z"
links: []
abstract: "No links."
---

Body.
"#;

    let (fm, _) = parse_node_file(content).expect("parse failed");
    assert!(fm.links.is_empty());
}

#[test]
fn test_parse_missing_opening_dashes() {
    let content = "no frontmatter\n---\nbody";
    let err = parse_node_file(content).unwrap_err();
    assert!(matches!(err, NodeError::CorruptedFrontmatter(_)));
}

#[test]
fn test_parse_missing_closing_dashes() {
    let content = "---\ncreated: bad\n\nbody without closing";
    let err = parse_node_file(content).unwrap_err();
    assert!(matches!(err, NodeError::CorruptedFrontmatter(_)));
}

#[test]
fn test_parse_invalid_yaml() {
    let content = "---\n: : : invalid yaml\n---\nbody";
    let err = parse_node_file(content).unwrap_err();
    assert!(matches!(err, NodeError::CorruptedFrontmatter(_)));
}

#[test]
fn test_parse_empty_body() {
    let content = r#"---
created: "2026-02-23T14:30:00Z"
updated: "2026-02-23T14:30:00Z"
last_accessed: "2026-02-23T14:30:00Z"
abstract: "Has abstract but no body."
---
"#;

    let (fm, body) = parse_node_file(content).expect("parse failed");
    assert_eq!(fm.abstract_text, "Has abstract but no body.");
    assert!(body.is_empty() || body.trim().is_empty());
}

// ============================================================
// serialize_node — roundtrip
// ============================================================

#[test]
fn test_serialize_roundtrip_preserves_fields() {
    let now = Utc::now();
    let fm = Frontmatter {
        created: now,
        updated: now,
        weight: 0.75,
        last_accessed: now,
        access_count: 10,
        pinned: true,
        links: vec!["node-a".into(), "node-b".into()],
        abstract_text: "A test abstract.".into(),
    };
    let body = "# Hello\n\nSome content.";

    let serialized = serialize_node(&fm, body);
    let (fm2, body2) = parse_node_file(&serialized).expect("roundtrip parse failed");

    assert!((fm2.weight - 0.75).abs() < f32::EPSILON);
    assert_eq!(fm2.access_count, 10);
    assert!(fm2.pinned);
    assert_eq!(fm2.links, vec!["node-a", "node-b"]);
    assert!(fm2.abstract_text.contains("A test abstract."));
    assert!(body2.contains("Some content."));
}

#[test]
fn test_serialize_empty_links_roundtrip() {
    let now = Utc::now();
    let fm = Frontmatter {
        created: now,
        updated: now,
        weight: 1.0,
        last_accessed: now,
        access_count: 0,
        pinned: false,
        links: vec![],
        abstract_text: "Lonely node.".into(),
    };

    let serialized = serialize_node(&fm, "Body.");
    let (fm2, _) = parse_node_file(&serialized).expect("roundtrip parse failed");
    assert!(fm2.links.is_empty());
}

// ============================================================
// hash_abstract
// ============================================================

#[test]
fn test_hash_abstract_deterministic() {
    let text = "test abstract";
    assert_eq!(hash_abstract(text), hash_abstract(text));
}

#[test]
fn test_hash_abstract_different_inputs() {
    assert_ne!(hash_abstract("text a"), hash_abstract("text b"));
}

#[test]
fn test_hash_abstract_empty_string() {
    // Should not panic
    let _ = hash_abstract("");
}

// ============================================================
// NodeMeta::from_frontmatter
// ============================================================

#[test]
fn test_node_meta_from_frontmatter() {
    let now = Utc::now();
    let fm = Frontmatter {
        created: now,
        updated: now,
        weight: 0.5,
        last_accessed: now,
        access_count: 3,
        pinned: true,
        links: vec!["other".into()],
        abstract_text: "Test.".into(),
    };

    let meta = NodeMeta::from_frontmatter("test-node", &fm);
    assert_eq!(meta.name, "test-node");
    assert!((meta.weight - 0.5).abs() < f32::EPSILON);
    assert_eq!(meta.access_count, 3);
    assert!(meta.pinned);
    assert_eq!(meta.links, vec!["other"]);
    assert_eq!(meta.abstract_text, "Test.");
    assert_eq!(meta.abstract_hash, hash_abstract("Test."));
}

// ============================================================
// Frontmatter::new_for_create — system field initialization
// ============================================================

#[test]
fn test_new_frontmatter_defaults() {
    let fm = Frontmatter::new_for_create(
        vec!["link-a".into()],
        false,
        "An abstract.".into(),
    );

    assert!((fm.weight - 1.0).abs() < f32::EPSILON); // always 1.0 on create
    assert_eq!(fm.access_count, 0);
    assert!(!fm.pinned);
    assert_eq!(fm.links, vec!["link-a"]);
    assert_eq!(fm.abstract_text, "An abstract.");
    // created == updated on first write
    assert_eq!(fm.created, fm.updated);
    assert_eq!(fm.created, fm.last_accessed);
}

#[test]
fn test_new_frontmatter_pinned() {
    let fm = Frontmatter::new_for_create(vec![], true, "Pinned node.".into());
    assert!(fm.pinned);
}

// ============================================================
// validate_links
// ============================================================

#[test]
fn test_validate_links_self_reference() {
    let err = validate_links("my-node", &["my-node".to_string()]).unwrap_err();
    assert!(matches!(err, NodeError::SelfReference));
}

#[test]
fn test_validate_links_dedup() {
    let result = validate_links("my-node", &[
        "other".to_string(),
        "other".to_string(),
        "third".to_string(),
    ]).expect("should succeed");
    assert_eq!(result, vec!["other", "third"]);
}

#[test]
fn test_validate_links_empty() {
    let result = validate_links("my-node", &[]).expect("should succeed");
    assert!(result.is_empty());
}

// ============================================================
// patch_body — design.md §6.2 (patch command)
// ============================================================

#[test]
fn test_patch_replace_single_match() {
    let body = "Hello world, this is a test.";
    let result = patch_body(body, PatchOp::Replace {
        old: "world".into(),
        new: "universe".into(),
    }).expect("patch failed");
    assert_eq!(result, "Hello universe, this is a test.");
}

#[test]
fn test_patch_replace_not_found() {
    let body = "Hello world.";
    let err = patch_body(body, PatchOp::Replace {
        old: "missing".into(),
        new: "replacement".into(),
    }).unwrap_err();
    assert!(matches!(err, NodeError::PatchTextNotFound(_)));
}

#[test]
fn test_patch_replace_ambiguous() {
    let body = "foo bar foo baz foo";
    let err = patch_body(body, PatchOp::Replace {
        old: "foo".into(),
        new: "qux".into(),
    }).unwrap_err();
    assert!(matches!(err, NodeError::PatchAmbiguous(_, 3)));
}

#[test]
fn test_patch_append() {
    let body = "Existing content.";
    let result = patch_body(body, PatchOp::Append("New stuff.".into()))
        .expect("patch failed");
    assert_eq!(result, "Existing content.\nNew stuff.");
}

#[test]
fn test_patch_append_body_ends_with_newline() {
    let body = "Existing content.\n";
    let result = patch_body(body, PatchOp::Append("New stuff.".into()))
        .expect("patch failed");
    assert_eq!(result, "Existing content.\nNew stuff.");
}

#[test]
fn test_patch_prepend() {
    let body = "Existing content.";
    let result = patch_body(body, PatchOp::Prepend("Prefix.".into()))
        .expect("patch failed");
    assert_eq!(result, "Prefix.\nExisting content.");
}

#[test]
fn test_patch_prepend_text_ends_with_newline() {
    let body = "Existing content.";
    let result = patch_body(body, PatchOp::Prepend("Prefix.\n".into()))
        .expect("patch failed");
    assert_eq!(result, "Prefix.\nExisting content.");
}

#[test]
fn test_patch_empty_content() {
    let body = "Content.";
    let err = patch_body(body, PatchOp::Append("".into())).unwrap_err();
    assert!(matches!(err, NodeError::EmptyPatch));
}

#[test]
fn test_patch_replace_multiline() {
    let body = "line 1\nold line\nline 3";
    let result = patch_body(body, PatchOp::Replace {
        old: "old line".into(),
        new: "new line\nextra line".into(),
    }).expect("patch failed");
    assert_eq!(result, "line 1\nnew line\nextra line\nline 3");
}

// ============================================================
// Pattern 5: Patch body edge cases
// ============================================================

#[test]
fn test_patch_append_to_empty_body() {
    let body = "";
    let result = patch_body(body, PatchOp::Append("new content".into()))
        .expect("patch failed");
    assert_eq!(result, "\nnew content");
}

#[test]
fn test_patch_prepend_to_empty_body() {
    let body = "";
    let result = patch_body(body, PatchOp::Prepend("new content".into()))
        .expect("patch failed");
    assert_eq!(result, "new content\n");
}

#[test]
fn test_patch_append_text_with_leading_newline() {
    let body = "Existing.";
    let result = patch_body(body, PatchOp::Append("\nNew stuff.".into()))
        .expect("patch failed");
    // Body doesn't end with \n, so separator \n is added, then text starts with \n
    assert_eq!(result, "Existing.\n\nNew stuff.");
}

#[test]
fn test_patch_replace_spanning_lines() {
    let body = "line 1\nold start\nold end\nline 4";
    let result = patch_body(body, PatchOp::Replace {
        old: "old start\nold end".into(),
        new: "replaced block".into(),
    }).expect("patch failed");
    assert_eq!(result, "line 1\nreplaced block\nline 4");
}

// ============================================================
// Disk operations: write_node / read_node
// ============================================================

#[test]
fn test_write_and_read_node() {
    let dir = TempDir::new().unwrap();
    let memories_dir = dir.path().join("memories");
    fs::create_dir_all(&memories_dir).unwrap();

    let fm = Frontmatter::new_for_create(vec![], false, "Test abstract.".into());
    let body = "# My Node\n\nContent here.";

    write_node_to_dir(&memories_dir, "test-node", &fm, body).expect("write failed");

    // File should exist
    let file_path = memories_dir.join("test-node.md");
    assert!(file_path.exists());

    // Read it back
    let (fm2, body2) = read_node_from_dir(&memories_dir, "test-node").expect("read failed");
    assert_eq!(fm2.abstract_text, "Test abstract.");
    assert!(body2.contains("Content here."));
}

#[test]
fn test_read_nonexistent_node() {
    let dir = TempDir::new().unwrap();
    let memories_dir = dir.path().join("memories");
    fs::create_dir_all(&memories_dir).unwrap();

    let err = read_node_from_dir(&memories_dir, "nope").unwrap_err();
    assert!(matches!(err, NodeError::NotFound(_)));
}

#[test]
fn test_write_node_atomic_no_corruption() {
    let dir = TempDir::new().unwrap();
    let memories_dir = dir.path().join("memories");
    fs::create_dir_all(&memories_dir).unwrap();

    let fm = Frontmatter::new_for_create(vec![], false, "First version.".into());
    write_node_to_dir(&memories_dir, "test-node", &fm, "v1").expect("write failed");

    // Overwrite
    let fm2 = Frontmatter::new_for_create(vec![], false, "Second version.".into());
    write_node_to_dir(&memories_dir, "test-node", &fm2, "v2").expect("write failed");

    let (fm_read, body_read) = read_node_from_dir(&memories_dir, "test-node").expect("read failed");
    assert_eq!(fm_read.abstract_text, "Second version.");
    assert_eq!(body_read, "v2");
}

#[test]
fn test_delete_node_from_dir() {
    let dir = TempDir::new().unwrap();
    let memories_dir = dir.path().join("memories");
    fs::create_dir_all(&memories_dir).unwrap();

    let fm = Frontmatter::new_for_create(vec![], false, "To be deleted.".into());
    write_node_to_dir(&memories_dir, "doomed", &fm, "bye").unwrap();
    assert!(memories_dir.join("doomed.md").exists());

    delete_node_from_dir(&memories_dir, "doomed").expect("delete failed");
    assert!(!memories_dir.join("doomed.md").exists());
}

#[test]
fn test_delete_nonexistent_node() {
    let dir = TempDir::new().unwrap();
    let memories_dir = dir.path().join("memories");
    fs::create_dir_all(&memories_dir).unwrap();

    let err = delete_node_from_dir(&memories_dir, "ghost").unwrap_err();
    assert!(matches!(err, NodeError::NotFound(_)));
}

// ============================================================
// list_nodes
// ============================================================

#[test]
fn test_list_nodes_empty_dir() {
    let dir = TempDir::new().unwrap();
    let memories_dir = dir.path().join("memories");
    fs::create_dir_all(&memories_dir).unwrap();

    let names = list_nodes_in_dir(&memories_dir).expect("list failed");
    assert!(names.is_empty());
}

#[test]
fn test_list_nodes_multiple() {
    let dir = TempDir::new().unwrap();
    let memories_dir = dir.path().join("memories");
    fs::create_dir_all(&memories_dir).unwrap();

    for name in &["alpha", "beta", "gamma"] {
        let fm = Frontmatter::new_for_create(vec![], false, format!("{} node.", name));
        write_node_to_dir(&memories_dir, name, &fm, "body").unwrap();
    }

    let mut names = list_nodes_in_dir(&memories_dir).expect("list failed");
    names.sort();
    assert_eq!(names, vec!["alpha", "beta", "gamma"]);
}

#[test]
fn test_list_nodes_ignores_non_md_files() {
    let dir = TempDir::new().unwrap();
    let memories_dir = dir.path().join("memories");
    fs::create_dir_all(&memories_dir).unwrap();

    // Create a .md file and a non-.md file
    let fm = Frontmatter::new_for_create(vec![], false, "Real node.".into());
    write_node_to_dir(&memories_dir, "real-node", &fm, "body").unwrap();
    fs::write(memories_dir.join("notes.txt"), "not a node").unwrap();

    let names = list_nodes_in_dir(&memories_dir).expect("list failed");
    assert_eq!(names, vec!["real-node"]);
}

// ============================================================
// Frontmatter defaults — agent-friendly minimal input
// ============================================================

#[test]
fn test_parse_minimal_frontmatter_no_dates() {
    let content = "---\nabstract: 'Just an abstract, no dates.'\n---\n\nBody.";
    let (fm, body) = parse_node_file(content).expect("parse failed");
    assert_eq!(fm.abstract_text, "Just an abstract, no dates.");
    // Defaults should be applied
    assert!((fm.weight - 1.0).abs() < f32::EPSILON);
    assert_eq!(fm.access_count, 0);
    assert!(!fm.pinned);
    assert!(fm.links.is_empty());
    assert!(body.contains("Body."));
}

#[test]
fn test_parse_frontmatter_partial_dates() {
    let content = "---\ncreated: '2026-01-15T10:00:00Z'\nabstract: 'Partial dates.'\n---\n\nBody.";
    let (fm, _body) = parse_node_file(content).expect("parse failed");
    // created should be the explicit value
    assert_eq!(fm.created.year(), 2026);
    assert_eq!(fm.created.month(), 1);
    // updated and last_accessed should be defaults (Utc::now())
    assert!(fm.updated.year() >= 2026);
    assert!(fm.last_accessed.year() >= 2026);
}

#[test]
fn test_parse_frontmatter_with_extra_fields() {
    let content = "---\nabstract: 'Has extra fields.'\ncustom_field: should be ignored\nunknown_key: 42\n---\n\nBody.";
    let (fm, body) = parse_node_file(content).expect("parse should succeed with extra fields");
    assert_eq!(fm.abstract_text, "Has extra fields.");
    assert!(body.contains("Body."));
}

#[test]
fn test_parse_unicode_body() {
    let content = "---\nabstract: 'Unicode test'\n---\n\n# \u{4E2D}\u{6587}\u{6807}\u{9898}\n\n\u{65E5}\u{672C}\u{8A9E}\u{306E}\u{30C6}\u{30B9}\u{30C8} \u{1F389}\n\nEmoji: \u{1F680}\u{1F525}\u{1F4A1}";
    let (fm, body) = parse_node_file(content).expect("parse failed");
    assert_eq!(fm.abstract_text, "Unicode test");
    assert!(body.contains("\u{4E2D}\u{6587}\u{6807}\u{9898}"));
    assert!(body.contains("\u{65E5}\u{672C}\u{8A9E}"));
    assert!(body.contains("\u{1F680}"));
}
