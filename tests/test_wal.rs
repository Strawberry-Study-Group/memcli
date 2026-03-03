use memcore::wal::{WalOp, WalWriter, find_uncommitted_at};
use tempfile::TempDir;

// ============================================================
// WalOp basics
// ============================================================

#[test]
fn test_wal_op_equality() {
    assert_eq!(
        WalOp::Create("node".into()),
        WalOp::Create("node".into())
    );
    assert_ne!(
        WalOp::Create("a".into()),
        WalOp::Create("b".into())
    );
    assert_ne!(
        WalOp::Create("a".into()),
        WalOp::Delete("a".into())
    );
}

#[test]
fn test_wal_op_link_fields() {
    let op = WalOp::Link("node-a".to_string(), "node-b".to_string());
    match op {
        WalOp::Link(a, b) => {
            assert_eq!(a, "node-a");
            assert_eq!(b, "node-b");
        }
        _ => panic!("wrong variant"),
    }
}

// ============================================================
// WalWriter begin + commit cycle
// ============================================================

#[test]
fn test_begin_commit_no_uncommitted() {
    let dir = TempDir::new().unwrap();
    let wal_path = dir.path().join("wal.log");
    let mut wal = WalWriter::at(wal_path.clone());

    let tx = wal.begin(&WalOp::Create("test-node".into())).unwrap();
    wal.commit(&tx).unwrap();

    let uncommitted = find_uncommitted_at(&wal_path).unwrap();
    assert!(uncommitted.is_empty());
}

#[test]
fn test_begin_without_commit_is_uncommitted() {
    let dir = TempDir::new().unwrap();
    let wal_path = dir.path().join("wal.log");
    let mut wal = WalWriter::at(wal_path.clone());

    let _tx = wal.begin(&WalOp::Create("orphan".into())).unwrap();
    // No commit!

    let uncommitted = find_uncommitted_at(&wal_path).unwrap();
    assert_eq!(uncommitted.len(), 1);
    assert_eq!(uncommitted[0].op, WalOp::Create("orphan".into()));
}

#[test]
fn test_multiple_transactions_mixed() {
    let dir = TempDir::new().unwrap();
    let wal_path = dir.path().join("wal.log");
    let mut wal = WalWriter::at(wal_path.clone());

    // tx1: committed
    let tx1 = wal.begin(&WalOp::Create("alpha".into())).unwrap();
    wal.commit(&tx1).unwrap();

    // tx2: NOT committed (crash simulation)
    let _tx2 = wal.begin(&WalOp::Delete("beta".into())).unwrap();

    // tx3: committed
    let tx3 = wal.begin(&WalOp::Link("aa".into(), "bb".into())).unwrap();
    wal.commit(&tx3).unwrap();

    let uncommitted = find_uncommitted_at(&wal_path).unwrap();
    assert_eq!(uncommitted.len(), 1);
    assert_eq!(uncommitted[0].op, WalOp::Delete("beta".into()));
}

// ============================================================
// All operation types round-trip through WAL
// ============================================================

#[test]
fn test_all_op_types_roundtrip() {
    let dir = TempDir::new().unwrap();
    let wal_path = dir.path().join("wal.log");
    let mut wal = WalWriter::at(wal_path.clone());

    // Write all op types WITHOUT committing
    wal.begin(&WalOp::Create("node-a".into())).unwrap();
    wal.begin(&WalOp::Delete("node-b".into())).unwrap();
    wal.begin(&WalOp::Update("node-c".into())).unwrap();
    wal.begin(&WalOp::Link("xx".into(), "yy".into())).unwrap();
    wal.begin(&WalOp::Unlink("pp".into(), "qq".into())).unwrap();
    wal.begin(&WalOp::Rename("old".into(), "new".into())).unwrap();

    let uncommitted = find_uncommitted_at(&wal_path).unwrap();
    assert_eq!(uncommitted.len(), 6);

    assert_eq!(uncommitted[0].op, WalOp::Create("node-a".into()));
    assert_eq!(uncommitted[1].op, WalOp::Delete("node-b".into()));
    assert_eq!(uncommitted[2].op, WalOp::Update("node-c".into()));
    assert_eq!(uncommitted[3].op, WalOp::Link("xx".into(), "yy".into()));
    assert_eq!(uncommitted[4].op, WalOp::Unlink("pp".into(), "qq".into()));
    assert_eq!(uncommitted[5].op, WalOp::Rename("old".into(), "new".into()));
}

// ============================================================
// tx_id uniqueness
// ============================================================

#[test]
fn test_tx_ids_are_unique() {
    let dir = TempDir::new().unwrap();
    let wal_path = dir.path().join("wal.log");
    let mut wal = WalWriter::at(wal_path.clone());

    let tx1 = wal.begin(&WalOp::Create("a".into())).unwrap();
    let tx2 = wal.begin(&WalOp::Create("b".into())).unwrap();
    let tx3 = wal.begin(&WalOp::Create("c".into())).unwrap();

    assert_ne!(tx1, tx2);
    assert_ne!(tx2, tx3);
    assert_ne!(tx1, tx3);
}

// ============================================================
// Clear
// ============================================================

#[test]
fn test_clear_removes_all_records() {
    let dir = TempDir::new().unwrap();
    let wal_path = dir.path().join("wal.log");
    let mut wal = WalWriter::at(wal_path.clone());

    wal.begin(&WalOp::Create("a".into())).unwrap();
    wal.begin(&WalOp::Create("b".into())).unwrap();

    wal.clear().unwrap();

    let uncommitted = find_uncommitted_at(&wal_path).unwrap();
    assert!(uncommitted.is_empty());
}

#[test]
fn test_clear_then_write_works() {
    let dir = TempDir::new().unwrap();
    let wal_path = dir.path().join("wal.log");
    let mut wal = WalWriter::at(wal_path.clone());

    wal.begin(&WalOp::Create("old".into())).unwrap();
    wal.clear().unwrap();

    // Writing after clear should work
    wal.begin(&WalOp::Create("new".into())).unwrap();

    let uncommitted = find_uncommitted_at(&wal_path).unwrap();
    assert_eq!(uncommitted.len(), 1);
    assert_eq!(uncommitted[0].op, WalOp::Create("new".into()));
}

// ============================================================
// Edge cases
// ============================================================

#[test]
fn test_find_uncommitted_no_file() {
    let dir = TempDir::new().unwrap();
    let wal_path = dir.path().join("nonexistent_wal.log");
    let uncommitted = find_uncommitted_at(&wal_path).unwrap();
    assert!(uncommitted.is_empty());
}

#[test]
fn test_find_uncommitted_empty_file() {
    let dir = TempDir::new().unwrap();
    let wal_path = dir.path().join("wal.log");
    std::fs::write(&wal_path, "").unwrap();

    let uncommitted = find_uncommitted_at(&wal_path).unwrap();
    assert!(uncommitted.is_empty());
}

#[test]
fn test_find_uncommitted_blank_lines_ignored() {
    let dir = TempDir::new().unwrap();
    let wal_path = dir.path().join("wal.log");
    std::fs::write(&wal_path, "\n\n\nBEGIN tx_001 CREATE test\n\n\n").unwrap();

    let uncommitted = find_uncommitted_at(&wal_path).unwrap();
    assert_eq!(uncommitted.len(), 1);
}

#[test]
fn test_find_uncommitted_malformed_lines_skipped() {
    let dir = TempDir::new().unwrap();
    let wal_path = dir.path().join("wal.log");
    std::fs::write(&wal_path, "GARBAGE LINE\nBEGIN tx_001 CREATE test\nMORE GARBAGE\n").unwrap();

    let uncommitted = find_uncommitted_at(&wal_path).unwrap();
    assert_eq!(uncommitted.len(), 1);
    assert_eq!(uncommitted[0].op, WalOp::Create("test".into()));
}

#[test]
fn test_commit_for_wrong_tx_doesnt_affect_others() {
    let dir = TempDir::new().unwrap();
    let wal_path = dir.path().join("wal.log");
    std::fs::write(&wal_path,
        "BEGIN tx_001 CREATE alpha\nBEGIN tx_002 CREATE beta\nCOMMIT tx_999\n"
    ).unwrap();

    let uncommitted = find_uncommitted_at(&wal_path).unwrap();
    assert_eq!(uncommitted.len(), 2); // tx_999 commit doesn't match either
}
