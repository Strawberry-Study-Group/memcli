use memcore::name_index::NameIndex;

// ============================================================
// Construction
// ============================================================

#[test]
fn test_new_is_empty() {
    let idx = NameIndex::new();
    assert!(idx.is_empty());
    assert_eq!(idx.len(), 0);
}

#[test]
fn test_from_iter_sorts() {
    let idx = NameIndex::from_iter(vec![
        "crawler".into(),
        "alpha".into(),
        "beta".into(),
    ]);
    assert_eq!(idx.all(), &["alpha", "beta", "crawler"]);
}

#[test]
fn test_from_iter_empty() {
    let idx = NameIndex::from_iter(Vec::<String>::new());
    assert!(idx.is_empty());
}

// ============================================================
// Insert
// ============================================================

#[test]
fn test_insert_maintains_sorted_order() {
    let mut idx = NameIndex::new();
    idx.insert("delta".into());
    idx.insert("alpha".into());
    idx.insert("charlie".into());
    idx.insert("bravo".into());
    assert_eq!(idx.all(), &["alpha", "bravo", "charlie", "delta"]);
}

#[test]
fn test_insert_dedup() {
    let mut idx = NameIndex::new();
    idx.insert("alpha".into());
    idx.insert("alpha".into());
    assert_eq!(idx.len(), 1);
}

#[test]
fn test_insert_at_beginning() {
    let mut idx = NameIndex::from_iter(vec!["beta".into(), "gamma".into()]);
    idx.insert("alpha".into());
    assert_eq!(idx.all()[0], "alpha");
}

#[test]
fn test_insert_at_end() {
    let mut idx = NameIndex::from_iter(vec!["alpha".into(), "beta".into()]);
    idx.insert("zeta".into());
    assert_eq!(idx.all().last().unwrap(), "zeta");
}

// ============================================================
// Remove
// ============================================================

#[test]
fn test_remove_existing() {
    let mut idx = NameIndex::from_iter(vec![
        "alpha".into(),
        "beta".into(),
        "gamma".into(),
    ]);
    idx.remove("beta");
    assert_eq!(idx.all(), &["alpha", "gamma"]);
    assert_eq!(idx.len(), 2);
}

#[test]
fn test_remove_nonexistent_is_noop() {
    let mut idx = NameIndex::from_iter(vec!["alpha".into()]);
    idx.remove("nope");
    assert_eq!(idx.len(), 1);
}

#[test]
fn test_remove_first() {
    let mut idx = NameIndex::from_iter(vec!["alpha".into(), "beta".into()]);
    idx.remove("alpha");
    assert_eq!(idx.all(), &["beta"]);
}

#[test]
fn test_remove_last() {
    let mut idx = NameIndex::from_iter(vec!["alpha".into(), "beta".into()]);
    idx.remove("beta");
    assert_eq!(idx.all(), &["alpha"]);
}

#[test]
fn test_remove_only_element() {
    let mut idx = NameIndex::from_iter(vec!["alpha".into()]);
    idx.remove("alpha");
    assert!(idx.is_empty());
}

// ============================================================
// Contains
// ============================================================

#[test]
fn test_contains_existing() {
    let idx = NameIndex::from_iter(vec!["alpha".into(), "beta".into()]);
    assert!(idx.contains("alpha"));
    assert!(idx.contains("beta"));
}

#[test]
fn test_contains_missing() {
    let idx = NameIndex::from_iter(vec!["alpha".into()]);
    assert!(!idx.contains("beta"));
}

#[test]
fn test_contains_empty_index() {
    let idx = NameIndex::new();
    assert!(!idx.contains("anything"));
}

// ============================================================
// Prefix search — core feature for `recall --name`
// ============================================================

#[test]
fn test_prefix_search_exact_match() {
    let idx = NameIndex::from_iter(vec![
        "crawler".into(),
        "crawler-v2".into(),
        "user-profile".into(),
    ]);
    let results = idx.prefix_search("crawler");
    assert_eq!(results, vec!["crawler", "crawler-v2"]);
}

#[test]
fn test_prefix_search_partial() {
    let idx = NameIndex::from_iter(vec![
        "project-alpha".into(),
        "project-beta".into(),
        "project-gamma".into(),
        "user-profile".into(),
    ]);
    let results = idx.prefix_search("project");
    assert_eq!(results, vec!["project-alpha", "project-beta", "project-gamma"]);
}

#[test]
fn test_prefix_search_no_match() {
    let idx = NameIndex::from_iter(vec!["alpha".into(), "beta".into()]);
    let results = idx.prefix_search("zz");
    assert!(results.is_empty());
}

#[test]
fn test_prefix_search_empty_prefix_returns_all() {
    let idx = NameIndex::from_iter(vec!["alpha".into(), "beta".into()]);
    let results = idx.prefix_search("");
    assert_eq!(results.len(), 2);
}

#[test]
fn test_prefix_search_single_char() {
    let idx = NameIndex::from_iter(vec![
        "alpha".into(),
        "app".into(),
        "beta".into(),
        "buzz".into(),
    ]);
    let results = idx.prefix_search("a");
    assert_eq!(results, vec!["alpha", "app"]);
}

#[test]
fn test_prefix_search_full_name_only_matches_itself() {
    let idx = NameIndex::from_iter(vec![
        "note".into(),
        "notebook".into(),
        "nothing".into(),
    ]);
    // "note" is a prefix of "notebook" and itself
    let results = idx.prefix_search("note");
    assert_eq!(results, vec!["note", "notebook"]);
}

#[test]
fn test_prefix_search_on_empty_index() {
    let idx = NameIndex::new();
    let results = idx.prefix_search("anything");
    assert!(results.is_empty());
}

// ============================================================
// Insert + Remove + Search interplay
// ============================================================

#[test]
fn test_insert_then_search() {
    let mut idx = NameIndex::new();
    idx.insert("crawler-v1".into());
    idx.insert("crawler-v2".into());
    idx.insert("user-profile".into());

    assert_eq!(idx.prefix_search("crawl"), vec!["crawler-v1", "crawler-v2"]);
    assert!(idx.contains("crawler-v1"));
}

#[test]
fn test_remove_then_search() {
    let mut idx = NameIndex::from_iter(vec![
        "crawler-v1".into(),
        "crawler-v2".into(),
    ]);
    idx.remove("crawler-v1");

    assert_eq!(idx.prefix_search("crawl"), vec!["crawler-v2"]);
    assert!(!idx.contains("crawler-v1"));
}

// ============================================================
// Larger scale
// ============================================================

#[test]
fn test_hundred_nodes() {
    let names: Vec<String> = (0..100).map(|i| format!("node-{:03}", i)).collect();
    let idx = NameIndex::from_iter(names);
    assert_eq!(idx.len(), 100);

    // Prefix search for "node-05" should get node-050..node-059
    let results = idx.prefix_search("node-05");
    assert_eq!(results.len(), 10);

    assert!(idx.contains("node-000"));
    assert!(idx.contains("node-099"));
    assert!(!idx.contains("node-100"));
}
