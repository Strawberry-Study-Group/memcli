use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use crate::config::memcore_dir;

/// WAL operation types
#[derive(Debug, Clone, PartialEq)]
pub enum WalOp {
    Create(String),
    Delete(String),
    Update(String),
    Link(String, String),
    Unlink(String, String),
    Rename(String, String),
}

/// A single WAL record
#[derive(Debug, Clone)]
pub struct WalRecord {
    pub tx_id: String,
    pub op: WalOp,
    pub committed: bool,
}

/// WAL writer for append-only log
pub struct WalWriter {
    path: PathBuf,
    tx_counter: u64,
}

impl WalWriter {
    /// Create a WalWriter using the default memcore dir
    pub fn new() -> Self {
        Self::at(memcore_dir().join("wal.log"))
    }

    /// Create a WalWriter at a specific path (for testing)
    pub fn at(path: PathBuf) -> Self {
        Self {
            path,
            tx_counter: 0,
        }
    }

    /// Generate a new transaction ID
    fn next_tx_id(&mut self) -> String {
        self.tx_counter += 1;
        format!("tx_{:06}", self.tx_counter)
    }

    /// Write a BEGIN record and return the tx_id
    pub fn begin(&mut self, op: &WalOp) -> std::io::Result<String> {
        let tx_id = self.next_tx_id();
        let op_str = match op {
            WalOp::Create(n) => format!("CREATE {}", n),
            WalOp::Delete(n) => format!("DELETE {}", n),
            WalOp::Update(n) => format!("UPDATE {}", n),
            WalOp::Link(a, b) => format!("LINK {} {}", a, b),
            WalOp::Unlink(a, b) => format!("UNLINK {} {}", a, b),
            WalOp::Rename(o, n) => format!("RENAME {} {}", o, n),
        };

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "BEGIN {} {}", tx_id, op_str)?;
        file.flush()?;
        Ok(tx_id)
    }

    /// Write a COMMIT record for a transaction
    pub fn commit(&mut self, tx_id: &str) -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "COMMIT {}", tx_id)?;
        file.flush()?;
        Ok(())
    }

    /// Clear the WAL file
    pub fn clear(&self) -> std::io::Result<()> {
        fs::write(&self.path, "")?;
        Ok(())
    }

    /// Get the path of the WAL file
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

/// Read and parse a WAL file, returning uncommitted transactions.
/// Uses the default memcore_dir path.
pub fn find_uncommitted_transactions() -> std::io::Result<Vec<WalRecord>> {
    find_uncommitted_at(&memcore_dir().join("wal.log"))
}

/// Read and parse a WAL file at a specific path, returning uncommitted transactions.
pub fn find_uncommitted_at(path: &PathBuf) -> std::io::Result<Vec<WalRecord>> {
    if !path.exists() {
        return Ok(vec![]);
    }

    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);

    let mut begins: Vec<WalRecord> = Vec::new();
    let mut committed: std::collections::HashSet<String> = std::collections::HashSet::new();

    for line in reader.lines() {
        let line = line?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "BEGIN" if parts.len() >= 3 => {
                let tx_id = parts[1].to_string();
                let op = match parts[2] {
                    "CREATE" if parts.len() >= 4 => WalOp::Create(parts[3].to_string()),
                    "DELETE" if parts.len() >= 4 => WalOp::Delete(parts[3].to_string()),
                    "UPDATE" if parts.len() >= 4 => WalOp::Update(parts[3].to_string()),
                    "LINK" if parts.len() >= 5 => {
                        WalOp::Link(parts[3].to_string(), parts[4].to_string())
                    }
                    "UNLINK" if parts.len() >= 5 => {
                        WalOp::Unlink(parts[3].to_string(), parts[4].to_string())
                    }
                    "RENAME" if parts.len() >= 5 => {
                        WalOp::Rename(parts[3].to_string(), parts[4].to_string())
                    }
                    _ => continue,
                };
                begins.push(WalRecord {
                    tx_id,
                    op,
                    committed: false,
                });
            }
            "COMMIT" if parts.len() >= 2 => {
                committed.insert(parts[1].to_string());
            }
            _ => {}
        }
    }

    Ok(begins
        .into_iter()
        .filter(|r| !committed.contains(&r.tx_id))
        .collect())
}
