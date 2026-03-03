use std::collections::{HashMap, HashSet, VecDeque};

/// Undirected graph stored as an adjacency list
#[derive(Debug, Default)]
pub struct Graph {
    adjacency: HashMap<String, HashSet<String>>,
    edge_count: usize,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a bidirectional edge between two nodes
    pub fn add_edge(&mut self, a: &str, b: &str) -> bool {
        let inserted_a = self
            .adjacency
            .entry(a.to_string())
            .or_default()
            .insert(b.to_string());
        self.adjacency
            .entry(b.to_string())
            .or_default()
            .insert(a.to_string());
        if inserted_a {
            self.edge_count += 1;
            true
        } else {
            false
        }
    }

    /// Remove a bidirectional edge between two nodes
    pub fn remove_edge(&mut self, a: &str, b: &str) -> bool {
        let removed = self
            .adjacency
            .get_mut(a)
            .map_or(false, |s| s.remove(b));
        if removed {
            if let Some(set) = self.adjacency.get_mut(b) {
                set.remove(a);
            }
            self.edge_count -= 1;
        }
        removed
    }

    /// Get neighbors of a node
    pub fn neighbors(&self, id: &str) -> Option<&HashSet<String>> {
        self.adjacency.get(id)
    }

    /// BFS traversal from a starting node up to max_depth
    pub fn bfs(&self, start: &str, max_depth: usize) -> Vec<(String, usize)> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut result = Vec::new();

        visited.insert(start.to_string());
        queue.push_back((start.to_string(), 0));

        while let Some((node, depth)) = queue.pop_front() {
            if depth > 0 {
                result.push((node.clone(), depth));
            }
            if depth < max_depth {
                if let Some(neighbors) = self.adjacency.get(&node) {
                    for neighbor in neighbors {
                        if visited.insert(neighbor.clone()) {
                            queue.push_back((neighbor.clone(), depth + 1));
                        }
                    }
                }
            }
        }

        result
    }

    /// Remove a node and all its edges, returning the list of former neighbors
    pub fn remove_node(&mut self, id: &str) -> Vec<String> {
        let neighbors: Vec<String> = self
            .adjacency
            .remove(id)
            .unwrap_or_default()
            .into_iter()
            .collect();

        for neighbor in &neighbors {
            if let Some(set) = self.adjacency.get_mut(neighbor) {
                set.remove(id);
            }
            self.edge_count -= 1;
        }

        neighbors
    }

    /// Ensure a node exists in the adjacency list (even if isolated)
    pub fn ensure_node(&mut self, id: &str) {
        self.adjacency.entry(id.to_string()).or_default();
    }

    /// Find all connected components via BFS
    pub fn connected_components(&self) -> Vec<Vec<String>> {
        let mut visited = HashSet::new();
        let mut components = Vec::new();

        for node in self.adjacency.keys() {
            if visited.contains(node) {
                continue;
            }

            let mut component = Vec::new();
            let mut queue = VecDeque::new();
            visited.insert(node.clone());
            queue.push_back(node.clone());

            while let Some(current) = queue.pop_front() {
                component.push(current.clone());
                if let Some(neighbors) = self.adjacency.get(&current) {
                    for neighbor in neighbors {
                        if visited.insert(neighbor.clone()) {
                            queue.push_back(neighbor.clone());
                        }
                    }
                }
            }

            components.push(component);
        }

        components
    }

    pub fn node_count(&self) -> usize {
        self.adjacency.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edge_count
    }

    /// Check if two nodes are connected by an edge
    pub fn has_edge(&self, a: &str, b: &str) -> bool {
        self.adjacency
            .get(a)
            .map_or(false, |s| s.contains(b))
    }

    /// Rename a node in the graph, updating all neighbor references.
    /// If old_name doesn't exist, this is a no-op.
    pub fn rename_node(&mut self, old_name: &str, new_name: &str) {
        let neighbors = match self.adjacency.remove(old_name) {
            Some(set) => set,
            None => return,
        };

        // Update all neighbors: remove old_name reference, add new_name
        for neighbor in &neighbors {
            if let Some(set) = self.adjacency.get_mut(neighbor) {
                set.remove(old_name);
                set.insert(new_name.to_string());
            }
        }

        // Insert new_name with the same neighbor set
        self.adjacency.insert(new_name.to_string(), neighbors);
    }
}

// --- graph.idx binary format ---

#[derive(Debug, thiserror::Error)]
pub enum GraphIdxError {
    #[error("bad magic bytes (expected MCGI)")]
    BadMagic,
    #[error("file truncated")]
    Truncated,
    #[error("unknown hash {0:#x} not in name map")]
    UnknownHash(u64),
}

const GRAPH_MAGIC: &[u8; 4] = b"MCGI";
const GRAPH_VERSION: u16 = 1;

/// Serialize the graph to graph.idx binary format
pub fn serialize_graph_idx(graph: &Graph) -> Vec<u8> {
    let mut buf = Vec::new();

    // Header
    buf.extend_from_slice(GRAPH_MAGIC);
    buf.extend_from_slice(&GRAPH_VERSION.to_le_bytes());
    buf.extend_from_slice(&(graph.edge_count() as u32).to_le_bytes());

    // Edge records (deduplicated: only write each edge once where a < b)
    let mut written = HashSet::new();
    for (node, neighbors) in &graph.adjacency {
        for neighbor in neighbors {
            let (lo, hi) = if node < neighbor {
                (node.as_str(), neighbor.as_str())
            } else {
                (neighbor.as_str(), node.as_str())
            };
            let key = (lo.to_string(), hi.to_string());
            if written.insert(key) {
                let hash_a = xxhash_rust::xxh64::xxh64(lo.as_bytes(), 0);
                let hash_b = xxhash_rust::xxh64::xxh64(hi.as_bytes(), 0);
                buf.extend_from_slice(&hash_a.to_le_bytes());
                buf.extend_from_slice(&hash_b.to_le_bytes());
            }
        }
    }

    buf
}

/// Deserialize a graph.idx binary buffer back into a Graph.
/// Requires a hash→name mapping (built from scanning .md filenames at startup).
pub fn deserialize_graph_idx(
    buf: &[u8],
    hash_to_name: &HashMap<u64, String>,
) -> Result<Graph, GraphIdxError> {
    const HEADER_SIZE: usize = 4 + 2 + 4; // magic + version + edge_count
    const EDGE_SIZE: usize = 16;           // two u64 hashes

    if buf.len() < HEADER_SIZE {
        return Err(GraphIdxError::Truncated);
    }

    if &buf[0..4] != GRAPH_MAGIC {
        return Err(GraphIdxError::BadMagic);
    }

    // skip version (bytes 4..6)
    let edge_count = u32::from_le_bytes(buf[6..10].try_into().unwrap()) as usize;

    let expected_len = HEADER_SIZE + edge_count * EDGE_SIZE;
    if buf.len() < expected_len {
        return Err(GraphIdxError::Truncated);
    }

    let mut graph = Graph::new();

    for i in 0..edge_count {
        let offset = HEADER_SIZE + i * EDGE_SIZE;
        let hash_a = u64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap());
        let hash_b = u64::from_le_bytes(buf[offset + 8..offset + 16].try_into().unwrap());

        let name_a = hash_to_name
            .get(&hash_a)
            .ok_or(GraphIdxError::UnknownHash(hash_a))?;
        let name_b = hash_to_name
            .get(&hash_b)
            .ok_or(GraphIdxError::UnknownHash(hash_b))?;

        graph.add_edge(name_a, name_b);
    }

    Ok(graph)
}
