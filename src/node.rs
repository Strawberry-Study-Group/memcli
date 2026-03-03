use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::config::memcore_dir;
use crate::util::atomic_write;

#[derive(Error, Debug)]
pub enum NodeError {
    #[error("invalid node name '{0}' (must start with a letter, end with letter/digit, 2-128 chars, only letters/digits/spaces/hyphens)")]
    InvalidName(String),
    #[error("node '{0}' not found")]
    NotFound(String),
    #[error("node '{0}' already exists")]
    AlreadyExists(String),
    #[error("self-reference not allowed in links")]
    SelfReference,
    #[error("link target '{0}' does not exist (create it first, then update links)")]
    LinkTargetNotFound(String),
    #[error("corrupted frontmatter in node '{0}'")]
    CorruptedFrontmatter(String),
    #[error("text not found in body of '{0}'")]
    PatchTextNotFound(String),
    #[error("ambiguous match: text appears {1} times in '{0}'")]
    PatchAmbiguous(String, usize),
    #[error("patch operates on body only, use 'update' for frontmatter")]
    PatchInFrontmatter,
    #[error("empty patch content")]
    EmptyPatch,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// YAML frontmatter fields
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frontmatter {
    #[serde(default = "default_now")]
    pub created: DateTime<Utc>,
    #[serde(default = "default_now")]
    pub updated: DateTime<Utc>,
    #[serde(default = "default_weight")]
    pub weight: f32,
    #[serde(default = "default_now")]
    pub last_accessed: DateTime<Utc>,
    #[serde(default)]
    pub access_count: u32,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub links: Vec<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: String,
}

fn default_weight() -> f32 {
    1.0
}

fn default_now() -> DateTime<Utc> {
    Utc::now()
}

impl Frontmatter {
    /// Create a new Frontmatter with system-managed defaults for `create` command.
    /// LLM provides: links, pinned, abstract_text.
    /// System sets: created, updated, weight=1.0, last_accessed, access_count=0.
    pub fn new_for_create(
        links: Vec<String>,
        pinned: bool,
        abstract_text: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            created: now,
            updated: now,
            weight: 1.0,
            last_accessed: now,
            access_count: 0,
            pinned,
            links,
            abstract_text,
        }
    }
}

/// In-memory node metadata (no body, saves memory)
#[derive(Debug, Clone)]
pub struct NodeMeta {
    pub name: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub weight: f32,
    pub last_accessed: DateTime<Utc>,
    pub access_count: u32,
    pub pinned: bool,
    pub links: Vec<String>,
    pub abstract_text: String,
    pub abstract_hash: u64,
}

impl NodeMeta {
    /// Build NodeMeta from a parsed Frontmatter and a node name.
    pub fn from_frontmatter(name: &str, fm: &Frontmatter) -> Self {
        Self {
            name: name.to_string(),
            created: fm.created,
            updated: fm.updated,
            weight: fm.weight,
            last_accessed: fm.last_accessed,
            access_count: fm.access_count,
            pinned: fm.pinned,
            links: fm.links.clone(),
            abstract_text: fm.abstract_text.clone(),
            abstract_hash: hash_abstract(&fm.abstract_text),
        }
    }
}

/// Validate a node name against the naming rules (design.md §3.1)
pub fn validate_name(name: &str) -> Result<(), NodeError> {
    let re = Regex::new(r"^[a-zA-Z][a-zA-Z0-9 \-]{0,126}[a-zA-Z0-9]$").unwrap();
    if !re.is_match(name) {
        return Err(NodeError::InvalidName(name.into()));
    }
    Ok(())
}

/// Validate a links list: reject self-references, deduplicate (design.md §3.2)
pub fn validate_links(self_name: &str, links: &[String]) -> Result<Vec<String>, NodeError> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();

    for link in links {
        if link == self_name {
            return Err(NodeError::SelfReference);
        }
        if seen.insert(link.clone()) {
            result.push(link.clone());
        }
    }

    Ok(result)
}

/// Patch operations for the `patch` command (design.md §6.2)
pub enum PatchOp {
    Replace { old: String, new: String },
    Append(String),
    Prepend(String),
}

/// Apply a patch operation to a node body.
/// Only operates on body text, never on frontmatter.
/// --replace requires exactly one match (literal string, not regex).
pub fn patch_body(body: &str, op: PatchOp) -> Result<String, NodeError> {
    match op {
        PatchOp::Replace { old, new } => {
            let count = body.matches(&old).count();
            match count {
                0 => Err(NodeError::PatchTextNotFound("node".into())),
                1 => Ok(body.replacen(&old, &new, 1)),
                n => Err(NodeError::PatchAmbiguous("node".into(), n)),
            }
        }
        PatchOp::Append(text) => {
            if text.is_empty() {
                return Err(NodeError::EmptyPatch);
            }
            let sep = if body.ends_with('\n') { "" } else { "\n" };
            Ok(format!("{}{}{}", body, sep, text))
        }
        PatchOp::Prepend(text) => {
            if text.is_empty() {
                return Err(NodeError::EmptyPatch);
            }
            let sep = if text.ends_with('\n') { "" } else { "\n" };
            Ok(format!("{}{}{}", text, sep, body))
        }
    }
}

/// Get the file path for a node in the default memcore dir
pub fn node_path(name: &str) -> PathBuf {
    memcore_dir().join("memories").join(format!("{}.md", name))
}

/// Check if a node exists on disk in the default memcore dir
pub fn node_exists(name: &str) -> bool {
    node_path(name).exists()
}

/// Parse a .md file into frontmatter + body
pub fn parse_node_file(content: &str) -> Result<(Frontmatter, String), NodeError> {
    let content = content.trim();
    if !content.starts_with("---") {
        return Err(NodeError::CorruptedFrontmatter("missing opening ---".into()));
    }

    let rest = &content[3..];
    let end = rest
        .find("\n---")
        .ok_or_else(|| NodeError::CorruptedFrontmatter("missing closing ---".into()))?;

    let yaml_str = &rest[..end];
    let body = rest[end + 4..].trim_start_matches('\n').to_string();

    let frontmatter: Frontmatter = serde_yaml::from_str(yaml_str)
        .map_err(|_| NodeError::CorruptedFrontmatter("yaml parse error".into()))?;

    Ok((frontmatter, body))
}

/// Serialize frontmatter + body into .md file content
pub fn serialize_node(frontmatter: &Frontmatter, body: &str) -> String {
    let yaml = serde_yaml::to_string(frontmatter).expect("frontmatter serialization failed");
    format!("---\n{}---\n\n{}", yaml, body)
}

/// Compute a fast hash of a string (for abstract change detection)
pub fn hash_abstract(text: &str) -> u64 {
    xxhash_rust::xxh64::xxh64(text.as_bytes(), 0)
}

// ============================================================
// Disk operations (dir-parameterized for testability)
// ============================================================

/// Write a node file atomically to a given memories directory
pub fn write_node_to_dir(
    memories_dir: &Path,
    name: &str,
    frontmatter: &Frontmatter,
    body: &str,
) -> Result<(), NodeError> {
    let content = serialize_node(frontmatter, body);
    let path = memories_dir.join(format!("{}.md", name));
    atomic_write(&path, content.as_bytes())?;
    Ok(())
}

/// Read and parse a node file from a given memories directory
pub fn read_node_from_dir(
    memories_dir: &Path,
    name: &str,
) -> Result<(Frontmatter, String), NodeError> {
    let path = memories_dir.join(format!("{}.md", name));
    if !path.exists() {
        return Err(NodeError::NotFound(name.into()));
    }
    let content = std::fs::read_to_string(&path)?;
    parse_node_file(&content)
}

/// Delete a node file from a given memories directory
pub fn delete_node_from_dir(memories_dir: &Path, name: &str) -> Result<(), NodeError> {
    let path = memories_dir.join(format!("{}.md", name));
    if !path.exists() {
        return Err(NodeError::NotFound(name.into()));
    }
    std::fs::remove_file(&path)?;
    Ok(())
}

/// List all node names in a given memories directory (files ending in .md)
pub fn list_nodes_in_dir(memories_dir: &Path) -> Result<Vec<String>, NodeError> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(memories_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_string());
            }
        }
    }
    Ok(names)
}
