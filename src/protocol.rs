use serde::{Deserialize, Serialize};

/// Wire protocol between CLI client and daemon.
///
/// Framing: length-prefixed JSON over TCP.
///   - 4 bytes (u32 LE): message length
///   - N bytes: JSON-encoded Request or Response
///
/// Handshake (text, before framing):
///   - Client sends: `MEMCORE_PING <nonce>\n`
///   - Daemon sends: `MEMCORE_PONG <version>\n`

// ============================================================
// Request
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    Create {
        name: String,
        content: String,
    },
    Get {
        names: Vec<String>,
    },
    Update {
        name: String,
        content: String,
    },
    Patch {
        name: String,
        op: PatchRequest,
    },
    Delete {
        name: String,
    },
    Rename {
        old: String,
        new: String,
    },
    Ls {
        sort: SortField,
    },
    Inspect {
        node: Option<String>,
        format: OutputFormat,
        threshold: Option<f32>,
        #[serde(default)]
        cap: Option<usize>,
    },
    Recall {
        query: Option<String>,
        name_prefix: Option<String>,
        top_k: usize,
        depth: usize,
    },
    Search {
        query: String,
        top_k: usize,
    },
    MultiSearch {
        queries: Vec<String>,
        top_k: usize,
    },
    MultiRecall {
        queries: Vec<String>,
        top_k: usize,
        depth: usize,
    },
    Neighbors {
        name: String,
        depth: usize,
        limit: usize,
        offset: usize,
    },
    Link {
        a: String,
        b: String,
    },
    Unlink {
        a: String,
        b: String,
    },
    Boost {
        name: String,
    },
    Penalize {
        name: String,
    },
    Pin {
        name: String,
    },
    Unpin {
        name: String,
    },
    Status,
    Reindex,
    Gc,
    Baseline,
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PatchRequest {
    Replace { old: String, new: String },
    Append(String),
    Prepend(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SortField {
    Name,
    Weight,
    Date,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum OutputFormat {
    Human,
    Json,
}

// ============================================================
// Response
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub success: bool,
    pub exit_code: u8,
    pub body: ResponseBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseBody {
    /// Simple message (create, delete, link, unlink, boost, penalize, pin, unpin, etc.)
    Message(String),

    /// Single node content (get single)
    NodeContent {
        name: String,
        content: String,
    },

    /// Multiple node contents (get batch)
    NodeBatch(Vec<NodeEntry>),

    /// Node listing (ls)
    NodeList(Vec<NodeListEntry>),

    /// Search results (search)
    SearchResults(Vec<SearchResultEntry>),

    /// Node name list (recall)
    NodeNames(Vec<String>),

    /// Neighbor listing
    Neighbors {
        entries: Vec<NeighborEntry>,
        total: usize,
        depth: usize,
    },

    /// Inspect report (system-wide, no node specified)
    InspectReport(InspectData),

    /// Inspect report for a single node
    NodeInspectReport(NodeInspectData),

    /// System status
    Status(StatusData),

    /// Error message
    Error(String),

    /// Empty (for stop, etc.)
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeEntry {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeListEntry {
    pub name: String,
    pub weight: f32,
    pub edge_count: usize,
    pub last_accessed: String,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultEntry {
    pub node_name: String,
    pub score: f32,
    pub similarity: f32,
    pub weight: f32,
    pub abstract_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeighborEntry {
    pub name: String,
    pub depth: usize,
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectData {
    pub node_count: usize,
    pub edge_count: usize,
    pub cluster_count: usize,
    pub orphan_count: usize,
    pub health_score: f32,
    pub redundancy: Option<f32>,
    pub orphan_ratio: f32,
    pub graveyard_ratio: f32,
    pub density: f32,
    pub similar_pairs: Vec<SimilarPair>,
    #[serde(default)]
    pub total_similar_pairs: usize,
    pub orphans: Vec<String>,
    pub low_weight: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarPair {
    pub node_a: String,
    pub node_b: String,
    pub similarity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarNode {
    pub name: String,
    pub similarity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInspectData {
    pub name: String,
    pub edge_count: usize,
    #[serde(default)]
    pub weight: f32,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub links: Vec<String>,
    #[serde(default)]
    pub abstract_text: String,
    pub similar_nodes: Vec<SimilarNode>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusData {
    pub pid: u32,
    pub port: u16,
    pub node_count: usize,
    pub edge_count: usize,
    pub index_count: usize,
    pub uptime_seconds: u64,
}

impl Response {
    pub fn ok(body: ResponseBody) -> Self {
        Self {
            success: true,
            exit_code: 0,
            body,
        }
    }

    pub fn user_error(msg: String) -> Self {
        Self {
            success: false,
            exit_code: 1,
            body: ResponseBody::Error(msg),
        }
    }

    pub fn system_error(msg: String) -> Self {
        Self {
            success: false,
            exit_code: 2,
            body: ResponseBody::Error(msg),
        }
    }

    pub fn connection_error(msg: String) -> Self {
        Self {
            success: false,
            exit_code: 3,
            body: ResponseBody::Error(msg),
        }
    }
}

// ============================================================
// Framing helpers
// ============================================================

/// Encode a message as length-prefixed JSON bytes.
pub fn encode_message<T: Serialize>(msg: &T) -> Result<Vec<u8>, serde_json::Error> {
    let json = serde_json::to_vec(msg)?;
    let len = json.len() as u32;
    let mut buf = Vec::with_capacity(4 + json.len());
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(&json);
    Ok(buf)
}

/// Read a length prefix from a 4-byte buffer.
pub fn read_length(buf: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*buf)
}

/// Decode a JSON message from bytes.
pub fn decode_message<T: for<'de> Deserialize<'de>>(buf: &[u8]) -> Result<T, serde_json::Error> {
    serde_json::from_slice(buf)
}

/// Handshake ping line
pub fn handshake_ping(nonce: &str) -> String {
    format!("MEMCORE_PING {}\n", nonce)
}

/// Handshake pong line
pub fn handshake_pong(version: &str) -> String {
    format!("MEMCORE_PONG {}\n", version)
}

/// Parse a handshake ping, returning the nonce if valid.
pub fn parse_ping(line: &str) -> Option<&str> {
    line.strip_prefix("MEMCORE_PING ")
        .map(|s| s.trim_end())
}

/// Parse a handshake pong, returning the version if valid.
pub fn parse_pong(line: &str) -> Option<&str> {
    line.strip_prefix("MEMCORE_PONG ")
        .map(|s| s.trim_end())
}
