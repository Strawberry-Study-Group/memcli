use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Utc};

use crate::config::{self, Config};
use crate::graph::{deserialize_graph_idx, serialize_graph_idx, Graph};
use crate::index::VectorIndex;
use crate::name_index::NameIndex;
use crate::node::{self, NodeMeta};
use crate::wal::{self, WalOp, WalWriter};

/// All daemon state, loadable from disk and testable independently.
pub struct DaemonState {
    pub graph: Graph,
    pub name_index: NameIndex,
    pub vector_index: VectorIndex,
    pub node_metas: HashMap<String, NodeMeta>,
    pub wal: WalWriter,
    pub config: Config,
    pub started_at: DateTime<Utc>,
    /// Nodes whose access metadata (last_accessed, access_count) changed
    /// since last flush. Flushed to disk periodically by the daemon.
    pub access_dirty: std::collections::HashSet<String>,
    #[cfg(feature = "embedding")]
    pub embedding_model: Option<crate::index::EmbeddingModel>,
}

/// Load daemon state from a memcore directory.
///
/// Steps (per design.md §10.4):
///   1. Scan memories/*.md, load all frontmatter
///   2. Build NameIndex, NodeMeta cache
///   3. WAL recovery (rollback uncommitted transactions)
///   4. Load or rebuild graph from links
///   5. Consistency check (dangling refs, bidirectional repair)
pub fn load_state_from_dir(base_dir: &Path) -> anyhow::Result<DaemonState> {
    let memories_dir = base_dir.join("memories");
    let index_dir = base_dir.join("index");
    let wal_path = base_dir.join("wal.log");
    let graph_idx_path = base_dir.join("graph.idx");

    // Ensure directories exist
    std::fs::create_dir_all(&memories_dir)?;
    std::fs::create_dir_all(&index_dir)?;

    // Load config
    let config_path = base_dir.join("memcore.toml");
    let config = if config_path.exists() {
        config::load_config_from(&config_path)
    } else {
        Config::default()
    };

    // Step 1: WAL recovery — must happen BEFORE loading state
    wal_recovery(&memories_dir, &wal_path)?;

    // Step 2: Scan memories/*.md
    let mut name_index = NameIndex::new();
    let mut node_metas: HashMap<String, NodeMeta> = HashMap::new();

    let entries = std::fs::read_dir(&memories_dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        // Only process .md files
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let name = match path.file_stem().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Parse the file
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("skipping {}: read error: {}", name, e);
                continue;
            }
        };

        let (frontmatter, _body) = match node::parse_node_file(&content) {
            Ok(parsed) => parsed,
            Err(e) => {
                tracing::warn!("skipping {}: corrupt frontmatter: {}", name, e);
                continue;
            }
        };

        let meta = NodeMeta::from_frontmatter(&name, &frontmatter);
        name_index.insert(name.clone());
        node_metas.insert(name, meta);
    }

    // Step 3: Build graph
    let graph = build_graph(&graph_idx_path, &node_metas);

    // Step 4: Consistency check — already handled by build_graph
    // (dangling refs are skipped, unidirectional links become bidirectional edges)

    // Step 5: Load vector index
    let vector_index = VectorIndex::load_from_dir(&index_dir).unwrap_or_else(|_| VectorIndex::new());

    // Step 5b: Try to load embedding model
    #[cfg(feature = "embedding")]
    let embedding_model = {
        let models_dir = base_dir.join("models");
        match crate::index::EmbeddingModel::load(&models_dir) {
            Ok(model) => {
                tracing::info!("embedding model loaded from {:?}", models_dir);
                Some(model)
            }
            Err(e) => {
                tracing::warn!("embedding model not available: {}", e);
                None
            }
        }
    };

    // Step 6: Clear WAL after successful recovery
    let wal = WalWriter::at(wal_path);
    let _ = wal.clear();

    Ok(DaemonState {
        graph,
        name_index,
        vector_index,
        node_metas,
        wal,
        config,
        started_at: Utc::now(),
        access_dirty: std::collections::HashSet::new(),
        #[cfg(feature = "embedding")]
        embedding_model,
    })
}

/// Build the graph from node metadata links.
///
/// Tries to load graph.idx first for fast startup. Falls back to
/// scanning all nodes' links fields.
///
/// In both cases, applies consistency rules:
/// - Skip dangling references (links to non-existent nodes)
/// - Make all edges bidirectional (repair unidirectional links)
fn build_graph(graph_idx_path: &Path, node_metas: &HashMap<String, NodeMeta>) -> Graph {
    // Try loading graph.idx
    if graph_idx_path.exists() {
        if let Ok(buf) = std::fs::read(graph_idx_path) {
            // Build hash-to-name lookup
            let hash_to_name: HashMap<u64, String> = node_metas
                .keys()
                .map(|name| {
                    let hash = xxhash_rust::xxh64::xxh64(name.as_bytes(), 0);
                    (hash, name.clone())
                })
                .collect();

            if let Ok(graph) = deserialize_graph_idx(&buf, &hash_to_name) {
                // Validate: ensure all edges reference existing nodes
                // The deserialization already uses the hash_to_name map,
                // so unknown hashes are rejected. This is sufficient.
                return graph;
            }
        }
        // graph.idx is corrupt or outdated — fall through to rebuild
        tracing::warn!("graph.idx is invalid, rebuilding from .md files");
    }

    // Rebuild from links fields
    rebuild_graph_from_links(node_metas)
}

/// Rebuild graph by scanning all nodes' links fields.
///
/// Applies consistency rules:
/// - Skip links to non-existent nodes (dangling references)
/// - All edges are bidirectional (even if only one side has the link)
fn rebuild_graph_from_links(node_metas: &HashMap<String, NodeMeta>) -> Graph {
    let mut graph = Graph::new();

    // Ensure all nodes exist in the graph (even isolated ones)
    for name in node_metas.keys() {
        graph.ensure_node(name);
    }

    // Add edges from links
    for (name, meta) in node_metas {
        for link in &meta.links {
            // Skip dangling references
            if !node_metas.contains_key(link) {
                tracing::warn!("dangling link: {} -> {} (target not found)", name, link);
                continue;
            }
            // add_edge is idempotent and bidirectional
            graph.add_edge(name, link);
        }
    }

    graph
}

/// WAL recovery: rollback uncommitted transactions.
///
/// Per design.md §10.4:
/// - Incomplete CREATE: delete the file if it exists
/// - Incomplete DELETE: keep the file (rollback = no-op, file still there)
/// - Incomplete UPDATE: file is either old or new version (atomic rename)
/// - Incomplete LINK/UNLINK: handled by consistency check later
/// - Incomplete RENAME: complex, handle both states
fn wal_recovery(memories_dir: &Path, wal_path: &std::path::PathBuf) -> anyhow::Result<()> {
    let uncommitted = wal::find_uncommitted_at(wal_path)?;

    if uncommitted.is_empty() {
        return Ok(());
    }

    tracing::info!(
        "WAL recovery: {} uncommitted transactions",
        uncommitted.len()
    );

    for record in &uncommitted {
        match &record.op {
            WalOp::Create(name) => {
                // Incomplete CREATE: delete the partially-created file
                let path = memories_dir.join(format!("{}.md", name));
                if path.exists() {
                    tracing::info!("WAL rollback: removing incomplete CREATE {}", name);
                    let _ = std::fs::remove_file(&path);
                }
            }
            WalOp::Delete(name) => {
                // Incomplete DELETE: file should still exist (rollback = keep it)
                let path = memories_dir.join(format!("{}.md", name));
                if path.exists() {
                    tracing::info!(
                        "WAL rollback: keeping file for incomplete DELETE {}",
                        name
                    );
                }
                // If file is gone, nothing we can do — data is lost
            }
            WalOp::Update(_name) => {
                // File is either old or new version (atomic rename guarantees no corruption)
                // We just accept whatever version is on disk
            }
            WalOp::Link(_a, _b) | WalOp::Unlink(_a, _b) => {
                // Handled by the consistency check during graph rebuild
                // (bidirectional repair + dangling ref cleanup)
            }
            WalOp::Rename(old_name, new_name) => {
                let old_path = memories_dir.join(format!("{}.md", old_name));
                let new_path = memories_dir.join(format!("{}.md", new_name));

                if old_path.exists() && !new_path.exists() {
                    // Rename never started — keep old
                    tracing::info!("WAL rollback: keeping {} (rename incomplete)", old_name);
                } else if !old_path.exists() && new_path.exists() {
                    // Rename completed at file level but not committed
                    // Keep new file, peer links will be fixed by consistency check
                    tracing::info!(
                        "WAL rollback: keeping {} (rename completed at file level)",
                        new_name
                    );
                } else if old_path.exists() && new_path.exists() {
                    // Both exist — remove new (rollback to pre-rename state)
                    tracing::info!(
                        "WAL rollback: removing {} (both old and new exist)",
                        new_name
                    );
                    let _ = std::fs::remove_file(&new_path);
                }
            }
        }
    }

    Ok(())
}

/// Write graph.idx to disk (for caching after load or after mutations).
pub fn save_graph_idx(base_dir: &Path, graph: &Graph) -> std::io::Result<()> {
    let bytes = serialize_graph_idx(graph);
    let path = base_dir.join("graph.idx");
    std::fs::write(path, bytes)
}

/// Flush dirty access metadata by rewriting each dirty node's .md file.
///
/// Reads each dirty node's file, updates last_accessed and access_count
/// in frontmatter, and writes it back atomically.
///
/// Returns the number of dirty nodes that were flushed.
pub fn flush_access_metadata(state: &mut DaemonState, base_dir: &Path) -> usize {
    let count = state.access_dirty.len();
    if count == 0 {
        return 0;
    }

    let dirty: Vec<String> = state.access_dirty.drain().collect();
    let memories_dir = base_dir.join("memories");
    let mut flushed = 0;

    for name in &dirty {
        let meta = match state.node_metas.get(name) {
            Some(m) => m,
            None => continue,
        };

        // Read current file
        let (mut frontmatter, body) = match node::read_node_from_dir(&memories_dir, name) {
            Ok(parsed) => parsed,
            Err(e) => {
                tracing::warn!("flush access metadata: read {} failed: {}", name, e);
                continue;
            }
        };

        // Update access fields
        frontmatter.last_accessed = meta.last_accessed;
        frontmatter.access_count = meta.access_count;

        // Write back atomically
        if let Err(e) = node::write_node_to_dir(&memories_dir, name, &frontmatter, &body) {
            tracing::warn!("flush access metadata: write {} failed: {}", name, e);
            continue;
        }

        flushed += 1;
    }

    tracing::debug!("flushed access metadata ({} dirty nodes)", flushed);
    flushed
}
