use std::path::Path;

use chrono::{Datelike, Utc};

use crate::daemon_state::{save_graph_idx, DaemonState};
use crate::feedback;
use crate::node::{self, NodeMeta, PatchOp};
use crate::protocol::*;
use crate::wal::WalOp;

/// Handle a request against daemon state. Pure logic, no I/O framing.
pub fn handle_request(
    state: &mut DaemonState,
    request: &Request,
    base_dir: &Path,
) -> Response {
    let memories_dir = base_dir.join("memories");

    match request {
        Request::Create { name, content } => {
            handle_create(state, name, content, &memories_dir, base_dir)
        }
        Request::Get { names } => handle_get(state, names, &memories_dir),
        Request::Update { name, content } => {
            handle_update(state, name, content, &memories_dir, base_dir)
        }
        Request::Patch { name, op } => handle_patch(state, name, op, &memories_dir),
        Request::Delete { name } => handle_delete(state, name, &memories_dir, base_dir),
        Request::Rename { old, new } => handle_rename(state, old, new, &memories_dir, base_dir),
        Request::Ls { sort } => handle_ls(state, *sort),
        Request::Link { a, b } => handle_link(state, a, b, &memories_dir, base_dir),
        Request::Unlink { a, b } => handle_unlink(state, a, b, &memories_dir, base_dir),
        Request::Boost { name } => handle_boost(state, name, &memories_dir),
        Request::Penalize { name } => handle_penalize(state, name, &memories_dir),
        Request::Pin { name } => handle_pin(state, name, &memories_dir),
        Request::Unpin { name } => handle_unpin(state, name, &memories_dir),
        Request::Neighbors {
            name,
            depth,
            limit,
            offset,
        } => handle_neighbors(state, name, *depth, *limit, *offset),
        Request::Search { query, top_k } => {
            handle_search(state, query, *top_k)
        }
        Request::MultiSearch { queries, top_k } => {
            handle_multi_search(state, queries, *top_k)
        }
        Request::Recall {
            query,
            name_prefix,
            top_k,
            depth,
        } => handle_recall(state, query, name_prefix, *top_k, *depth),
        Request::MultiRecall {
            queries,
            top_k,
            depth,
        } => handle_multi_recall(state, queries, *top_k, *depth),
        Request::Status => handle_status(state),
        Request::Inspect { node: Some(name), format, .. } => handle_inspect_node(state, name, format),
        Request::Inspect { node: None, format, threshold, cap } => handle_inspect(state, format, *threshold, *cap),
        Request::Reindex => handle_reindex(state, &memories_dir, base_dir),
        Request::Gc => handle_gc(state, &memories_dir, base_dir),
        Request::Baseline => handle_baseline(state, base_dir),
        Request::Stop => Response::ok(ResponseBody::Message("stopping daemon".into())),
    }
}

// ============================================================
// Create
// ============================================================

fn handle_create(
    state: &mut DaemonState,
    name: &str,
    content: &str,
    memories_dir: &Path,
    base_dir: &Path,
) -> Response {
    // Validate name
    if let Err(e) = node::validate_name(name) {
        return Response::user_error(e.to_string());
    }

    // Check node doesn't exist
    if state.name_index.contains(name) {
        return Response::user_error(format!("node '{}' already exists", name));
    }

    // Parse the content
    let (mut frontmatter, body) = match node::parse_node_file(content) {
        Ok(parsed) => parsed,
        Err(e) => return Response::user_error(format!("invalid content: {}", e)),
    };

    // Validate links
    let links = match node::validate_links(name, &frontmatter.links) {
        Ok(l) => l,
        Err(e) => return Response::user_error(e.to_string()),
    };

    // Check all link targets exist
    for link in &links {
        if !state.name_index.contains(link) {
            return Response::user_error(format!(
                "link target '{}' does not exist (create it first)",
                link
            ));
        }
    }

    frontmatter.links = links.clone();

    // Fill system-managed fields
    let now = Utc::now();
    frontmatter.created = now;
    frontmatter.updated = now;
    frontmatter.last_accessed = now;
    frontmatter.access_count = 0;
    if frontmatter.weight <= 0.0 || frontmatter.weight > 1.0 {
        frontmatter.weight = 1.0;
    }

    // WAL begin
    let tx_id = match state.wal.begin(&WalOp::Create(name.to_string())) {
        Ok(id) => id,
        Err(e) => return Response::system_error(format!("WAL error: {}", e)),
    };

    // Update in-memory state
    let meta = NodeMeta::from_frontmatter(name, &frontmatter);
    state.name_index.insert(name.to_string());
    state.node_metas.insert(name.to_string(), meta);

    // Update graph edges
    for link in &links {
        state.graph.add_edge(name, link);
        // Update peer's in-memory links
        if let Some(peer_meta) = state.node_metas.get_mut(link.as_str()) {
            if !peer_meta.links.contains(&name.to_string()) {
                peer_meta.links.push(name.to_string());
            }
        }
    }
    state.graph.ensure_node(name);

    // Persist to disk
    if let Err(e) = node::write_node_to_dir(memories_dir, name, &frontmatter, &body) {
        return Response::system_error(format!("disk write error: {}", e));
    }

    // Update peer .md files
    for link in &links {
        update_peer_links_on_disk(memories_dir, link, name, true);
    }

    // Compute and insert embedding if model is available
    #[cfg(feature = "embedding")]
    {
        if let Some(ref mut model) = state.embedding_model {
            match model.compute(&frontmatter.abstract_text) {
                Ok(embedding) => {
                    state.vector_index.insert(name, &embedding);
                }
                Err(e) => {
                    tracing::warn!("embedding failed for {}: {}", name, e);
                }
            }
        }
    }

    // Rewrite graph.idx
    let _ = save_graph_idx(base_dir, &state.graph);

    // WAL commit
    let _ = state.wal.commit(&tx_id);

    let link_count = links.len();
    Response::ok(ResponseBody::Message(format!(
        "created: {} [{} links]",
        name, link_count
    )))
}

// ============================================================
// Get
// ============================================================

fn handle_get(
    state: &mut DaemonState,
    names: &[String],
    memories_dir: &Path,
) -> Response {
    if names.is_empty() {
        return Response::user_error("no node names provided".into());
    }

    // Dedup while preserving order
    let mut seen = std::collections::HashSet::new();
    let deduped: Vec<&String> = names.iter().filter(|n| seen.insert((*n).clone())).collect();

    let mut entries = Vec::new();

    for name in deduped {
        if !state.name_index.contains(name) {
            return Response::user_error(format!("node '{}' not found", name));
        }

        let content = match std::fs::read_to_string(memories_dir.join(format!("{}.md", name))) {
            Ok(c) => c,
            Err(e) => return Response::system_error(format!("read error: {}", e)),
        };

        // Update access metadata in memory, mark dirty for async flush
        if let Some(meta) = state.node_metas.get_mut(name.as_str()) {
            meta.last_accessed = Utc::now();
            meta.access_count += 1;
            state.access_dirty.insert(name.clone());
        }

        entries.push(NodeEntry {
            name: name.clone(),
            content,
        });
    }

    if entries.len() == 1 {
        let entry = entries.remove(0);
        Response::ok(ResponseBody::NodeContent {
            name: entry.name,
            content: entry.content,
        })
    } else {
        Response::ok(ResponseBody::NodeBatch(entries))
    }
}

// ============================================================
// Update
// ============================================================

fn handle_update(
    state: &mut DaemonState,
    name: &str,
    content: &str,
    memories_dir: &Path,
    base_dir: &Path,
) -> Response {
    if !state.name_index.contains(name) {
        return Response::user_error(format!("node '{}' not found", name));
    }

    let (mut new_fm, new_body) = match node::parse_node_file(content) {
        Ok(parsed) => parsed,
        Err(e) => return Response::user_error(format!("invalid content: {}", e)),
    };

    let old_meta = state.node_metas.get(name).unwrap().clone();

    // Preserve system-managed fields
    new_fm.created = old_meta.created;
    new_fm.updated = Utc::now();
    new_fm.access_count = old_meta.access_count;

    // Weight: if not explicitly set (default 1.0), inherit historical
    // (We can't truly detect "not set" since YAML parse gives default,
    //  so we accept the declared value)

    // Validate links
    let new_links = match node::validate_links(name, &new_fm.links) {
        Ok(l) => l,
        Err(e) => return Response::user_error(e.to_string()),
    };

    for link in &new_links {
        if !state.name_index.contains(link) {
            return Response::user_error(format!(
                "link target '{}' does not exist",
                link
            ));
        }
    }

    new_fm.links = new_links.clone();

    // WAL begin
    let tx_id = match state.wal.begin(&WalOp::Update(name.to_string())) {
        Ok(id) => id,
        Err(e) => return Response::system_error(format!("WAL error: {}", e)),
    };

    // Compute link diff
    let old_links: std::collections::HashSet<&str> =
        old_meta.links.iter().map(|s| s.as_str()).collect();
    let new_links_set: std::collections::HashSet<&str> =
        new_links.iter().map(|s| s.as_str()).collect();

    let added: Vec<&str> = new_links_set.difference(&old_links).copied().collect();
    let removed: Vec<&str> = old_links.difference(&new_links_set).copied().collect();

    // Update graph
    for link in &added {
        state.graph.add_edge(name, link);
        if let Some(peer) = state.node_metas.get_mut(*link) {
            if !peer.links.contains(&name.to_string()) {
                peer.links.push(name.to_string());
            }
        }
    }
    for link in &removed {
        state.graph.remove_edge(name, link);
        if let Some(peer) = state.node_metas.get_mut(*link) {
            peer.links.retain(|l| l != name);
        }
    }

    // Update in-memory meta
    let meta = NodeMeta::from_frontmatter(name, &new_fm);
    state.node_metas.insert(name.to_string(), meta);

    // Persist
    if let Err(e) = node::write_node_to_dir(memories_dir, name, &new_fm, &new_body) {
        return Response::system_error(format!("disk write error: {}", e));
    }

    // Update peer .md files
    for link in &added {
        update_peer_links_on_disk(memories_dir, link, name, true);
    }
    for link in &removed {
        update_peer_links_on_disk(memories_dir, link, name, false);
    }

    // Recompute embedding if abstract changed
    #[cfg(feature = "embedding")]
    {
        let new_hash = crate::node::hash_abstract(&new_fm.abstract_text);
        if new_hash != old_meta.abstract_hash {
            if let Some(ref mut model) = state.embedding_model {
                match model.compute(&new_fm.abstract_text) {
                    Ok(embedding) => {
                        state.vector_index.insert(name, &embedding);
                    }
                    Err(e) => {
                        tracing::warn!("re-embedding failed for {}: {}", name, e);
                    }
                }
            }
        }
    }

    let _ = save_graph_idx(base_dir, &state.graph);
    let _ = state.wal.commit(&tx_id);

    Response::ok(ResponseBody::Message(format!(
        "updated: {} [links: +{} -{}]",
        name,
        added.len(),
        removed.len()
    )))
}

// ============================================================
// Patch
// ============================================================

fn handle_patch(
    state: &mut DaemonState,
    name: &str,
    patch_op: &PatchRequest,
    memories_dir: &Path,
) -> Response {
    if !state.name_index.contains(name) {
        return Response::user_error(format!("node '{}' not found", name));
    }

    let (frontmatter, body) = match node::read_node_from_dir(memories_dir, name) {
        Ok(parsed) => parsed,
        Err(e) => return Response::system_error(format!("read error: {}", e)),
    };

    let op = match patch_op {
        PatchRequest::Replace { old, new } => PatchOp::Replace {
            old: old.clone(),
            new: new.clone(),
        },
        PatchRequest::Append(text) => PatchOp::Append(text.clone()),
        PatchRequest::Prepend(text) => PatchOp::Prepend(text.clone()),
    };

    let new_body = match node::patch_body(&body, op) {
        Ok(b) => b,
        Err(e) => return Response::user_error(e.to_string()),
    };

    // Update timestamp (patch modifies content, so updated should reflect that)
    let mut frontmatter = frontmatter;
    frontmatter.updated = Utc::now();
    if let Some(meta) = state.node_metas.get_mut(name) {
        meta.updated = frontmatter.updated;
    }

    // No re-embedding needed — patch only touches body, not abstract
    if let Err(e) = node::write_node_to_dir(memories_dir, name, &frontmatter, &new_body) {
        return Response::system_error(format!("disk write error: {}", e));
    }

    let op_name = match patch_op {
        PatchRequest::Replace { .. } => "replace",
        PatchRequest::Append(_) => "append",
        PatchRequest::Prepend(_) => "prepend",
    };

    Response::ok(ResponseBody::Message(format!(
        "patched: {} [{}]",
        name, op_name
    )))
}

// ============================================================
// Delete
// ============================================================

fn handle_delete(
    state: &mut DaemonState,
    name: &str,
    memories_dir: &Path,
    base_dir: &Path,
) -> Response {
    if !state.name_index.contains(name) {
        return Response::user_error(format!("node '{}' not found", name));
    }

    let tx_id = match state.wal.begin(&WalOp::Delete(name.to_string())) {
        Ok(id) => id,
        Err(e) => return Response::system_error(format!("WAL error: {}", e)),
    };

    // Get neighbors before removing
    let neighbors = state.graph.remove_node(name);

    // Update peer in-memory state
    for neighbor in &neighbors {
        if let Some(peer) = state.node_metas.get_mut(neighbor) {
            peer.links.retain(|l| l != name);
        }
    }

    // Remove from indices
    state.name_index.remove(name);
    state.node_metas.remove(name);
    state.vector_index.remove(name);

    // Update peer .md files
    for neighbor in &neighbors {
        update_peer_links_on_disk(memories_dir, neighbor, name, false);
    }

    // Delete the file
    let _ = node::delete_node_from_dir(memories_dir, name);

    let _ = save_graph_idx(base_dir, &state.graph);
    let _ = state.wal.commit(&tx_id);

    Response::ok(ResponseBody::Message(format!(
        "deleted: {} [{} edges removed]",
        name,
        neighbors.len()
    )))
}

// ============================================================
// Rename
// ============================================================

fn handle_rename(
    state: &mut DaemonState,
    old: &str,
    new: &str,
    memories_dir: &Path,
    base_dir: &Path,
) -> Response {
    if !state.name_index.contains(old) {
        return Response::user_error(format!("node '{}' not found", old));
    }
    if let Err(e) = node::validate_name(new) {
        return Response::user_error(e.to_string());
    }
    if state.name_index.contains(new) {
        return Response::user_error(format!("node '{}' already exists", new));
    }

    let tx_id = match state.wal.begin(&WalOp::Rename(old.into(), new.into())) {
        Ok(id) => id,
        Err(e) => return Response::system_error(format!("WAL error: {}", e)),
    };

    // Get neighbors
    let neighbors: Vec<String> = state
        .graph
        .neighbors(old)
        .map(|n| n.iter().cloned().collect())
        .unwrap_or_default();

    // Update graph
    state.graph.rename_node(old, new);

    // Update name index
    state.name_index.remove(old);
    state.name_index.insert(new.to_string());

    // Update node meta
    if let Some(mut meta) = state.node_metas.remove(old) {
        meta.name = new.to_string();
        // Update links that reference old name in neighbors
        for neighbor in &neighbors {
            if let Some(peer) = state.node_metas.get_mut(neighbor) {
                for link in &mut peer.links {
                    if link == old {
                        *link = new.to_string();
                    }
                }
            }
        }
        state.node_metas.insert(new.to_string(), meta);
    }

    // Rename vector index entry
    state.vector_index.rename(old, new);

    // Rename file on disk
    let old_path = memories_dir.join(format!("{}.md", old));
    let new_path = memories_dir.join(format!("{}.md", new));
    if let Err(e) = std::fs::rename(&old_path, &new_path) {
        return Response::system_error(format!("file rename error: {}", e));
    }

    // Update peer .md files: replace old name with new in their links
    for neighbor in &neighbors {
        rename_in_peer_links(memories_dir, neighbor, old, new);
    }

    let _ = save_graph_idx(base_dir, &state.graph);
    let _ = state.wal.commit(&tx_id);

    Response::ok(ResponseBody::Message(format!(
        "renamed: {} -> {} [{} neighbor files updated]",
        old,
        new,
        neighbors.len()
    )))
}

// ============================================================
// Ls
// ============================================================

fn handle_ls(state: &DaemonState, sort: SortField) -> Response {
    let mut entries: Vec<NodeListEntry> = state
        .node_metas
        .values()
        .map(|meta| {
            let edge_count = state
                .graph
                .neighbors(&meta.name)
                .map(|n| n.len())
                .unwrap_or(0);

            let ago = format_time_ago(meta.last_accessed);

            NodeListEntry {
                name: meta.name.clone(),
                weight: meta.weight,
                edge_count,
                last_accessed: ago,
                pinned: meta.pinned,
            }
        })
        .collect();

    match sort {
        SortField::Name => entries.sort_by(|a, b| a.name.cmp(&b.name)),
        SortField::Weight => entries.sort_by(|a, b| {
            b.weight
                .partial_cmp(&a.weight)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.edge_count.cmp(&a.edge_count))
                .then_with(|| a.name.cmp(&b.name))
        }),
        SortField::Date => entries.sort_by(|a, b| a.last_accessed.cmp(&b.last_accessed)),
    }

    Response::ok(ResponseBody::NodeList(entries))
}

// ============================================================
// Link / Unlink
// ============================================================

fn handle_link(
    state: &mut DaemonState,
    a: &str,
    b: &str,
    memories_dir: &Path,
    base_dir: &Path,
) -> Response {
    if !state.name_index.contains(a) {
        return Response::user_error(format!("node '{}' not found", a));
    }
    if !state.name_index.contains(b) {
        return Response::user_error(format!("node '{}' not found", b));
    }
    if a == b {
        return Response::user_error("cannot link a node to itself".into());
    }

    let tx_id = match state.wal.begin(&WalOp::Link(a.into(), b.into())) {
        Ok(id) => id,
        Err(e) => return Response::system_error(format!("WAL error: {}", e)),
    };

    // Idempotent: add_edge returns false if already exists
    state.graph.add_edge(a, b);

    // Update in-memory links
    if let Some(meta_a) = state.node_metas.get_mut(a) {
        if !meta_a.links.contains(&b.to_string()) {
            meta_a.links.push(b.to_string());
        }
    }
    if let Some(meta_b) = state.node_metas.get_mut(b) {
        if !meta_b.links.contains(&a.to_string()) {
            meta_b.links.push(a.to_string());
        }
    }

    // Update .md files
    update_peer_links_on_disk(memories_dir, a, b, true);
    update_peer_links_on_disk(memories_dir, b, a, true);

    // Update timestamps
    update_timestamp_in_memory(state, a);
    update_timestamp_in_memory(state, b);

    let _ = save_graph_idx(base_dir, &state.graph);
    let _ = state.wal.commit(&tx_id);

    Response::ok(ResponseBody::Message(format!(
        "linked: {} <-> {}",
        a, b
    )))
}

fn handle_unlink(
    state: &mut DaemonState,
    a: &str,
    b: &str,
    memories_dir: &Path,
    base_dir: &Path,
) -> Response {
    if !state.name_index.contains(a) {
        return Response::user_error(format!("node '{}' not found", a));
    }
    if !state.name_index.contains(b) {
        return Response::user_error(format!("node '{}' not found", b));
    }

    let tx_id = match state.wal.begin(&WalOp::Unlink(a.into(), b.into())) {
        Ok(id) => id,
        Err(e) => return Response::system_error(format!("WAL error: {}", e)),
    };

    state.graph.remove_edge(a, b);

    if let Some(meta_a) = state.node_metas.get_mut(a) {
        meta_a.links.retain(|l| l != b);
    }
    if let Some(meta_b) = state.node_metas.get_mut(b) {
        meta_b.links.retain(|l| l != a);
    }

    update_peer_links_on_disk(memories_dir, a, b, false);
    update_peer_links_on_disk(memories_dir, b, a, false);

    update_timestamp_in_memory(state, a);
    update_timestamp_in_memory(state, b);

    let _ = save_graph_idx(base_dir, &state.graph);
    let _ = state.wal.commit(&tx_id);

    Response::ok(ResponseBody::Message(format!(
        "unlinked: {} <-> {}",
        a, b
    )))
}

// ============================================================
// Boost / Penalize
// ============================================================

fn handle_boost(
    state: &mut DaemonState,
    name: &str,
    memories_dir: &Path,
) -> Response {
    if !state.name_index.contains(name) {
        return Response::user_error(format!("node '{}' not found", name));
    }

    let old_weight = state.node_metas[name].weight;
    let new_weight = feedback::boost(old_weight, &state.config.weight);

    if (new_weight - old_weight).abs() < f32::EPSILON {
        return Response::ok(ResponseBody::Message(format!(
            "already at maximum weight ({:.2}): {}",
            old_weight, name
        )));
    }

    if let Some(meta) = state.node_metas.get_mut(name) {
        meta.weight = new_weight;
    }

    // Persist weight change (single-file, no WAL needed)
    update_weight_on_disk(memories_dir, name, new_weight);

    Response::ok(ResponseBody::Message(format!(
        "boosted: {} ({:.2} -> {:.2})",
        name, old_weight, new_weight
    )))
}

fn handle_penalize(
    state: &mut DaemonState,
    name: &str,
    memories_dir: &Path,
) -> Response {
    if !state.name_index.contains(name) {
        return Response::user_error(format!("node '{}' not found", name));
    }

    let old_weight = state.node_metas[name].weight;
    let new_weight = feedback::penalize(old_weight, &state.config.weight);

    if let Some(meta) = state.node_metas.get_mut(name) {
        meta.weight = new_weight;
    }

    update_weight_on_disk(memories_dir, name, new_weight);

    Response::ok(ResponseBody::Message(format!(
        "penalized: {} ({:.2} -> {:.2})",
        name, old_weight, new_weight
    )))
}

// ============================================================
// Pin / Unpin
// ============================================================

fn handle_pin(
    state: &mut DaemonState,
    name: &str,
    memories_dir: &Path,
) -> Response {
    if !state.name_index.contains(name) {
        return Response::user_error(format!("node '{}' not found", name));
    }

    if let Some(meta) = state.node_metas.get_mut(name) {
        meta.pinned = true;
    }

    update_pinned_on_disk(memories_dir, name, true);

    Response::ok(ResponseBody::Message(format!("pinned: {}", name)))
}

fn handle_unpin(
    state: &mut DaemonState,
    name: &str,
    memories_dir: &Path,
) -> Response {
    if !state.name_index.contains(name) {
        return Response::user_error(format!("node '{}' not found", name));
    }

    if let Some(meta) = state.node_metas.get_mut(name) {
        meta.pinned = false;
    }

    update_pinned_on_disk(memories_dir, name, false);

    Response::ok(ResponseBody::Message(format!("unpinned: {}", name)))
}

// ============================================================
// Neighbors
// ============================================================

fn handle_neighbors(
    state: &DaemonState,
    name: &str,
    depth: usize,
    limit: usize,
    offset: usize,
) -> Response {
    if !state.name_index.contains(name) {
        return Response::user_error(format!("node '{}' not found", name));
    }

    let all = state.graph.bfs(name, depth);
    let total = all.len();

    let entries: Vec<NeighborEntry> = all
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|(n, d)| {
            let weight = state
                .node_metas
                .get(&n)
                .map(|m| m.weight)
                .unwrap_or(0.0);
            NeighborEntry {
                name: n,
                depth: d,
                weight,
            }
        })
        .collect();

    Response::ok(ResponseBody::Neighbors {
        entries,
        total,
        depth,
    })
}

// ============================================================
// Search (pure vector similarity)
// ============================================================

fn handle_search(
    state: &mut DaemonState,
    query: &str,
    top_k: usize,
) -> Response {
    #[cfg(feature = "embedding")]
    {
        let model = match &mut state.embedding_model {
            Some(m) => m,
            None => return Response::system_error("embedding model not loaded".into()),
        };
        let query_embedding = match model.compute(query) {
            Ok(e) => e,
            Err(e) => return Response::system_error(format!("embedding error: {}", e)),
        };
        let hits = state.vector_index.search(&query_embedding, top_k);
        let results: Vec<SearchResultEntry> = hits
            .into_iter()
            .map(|hit| {
                let meta = state.node_metas.get(&hit.node_name);
                SearchResultEntry {
                    node_name: hit.node_name,
                    score: hit.similarity,
                    similarity: hit.similarity,
                    weight: meta.map(|m| m.weight).unwrap_or(0.0),
                    abstract_text: meta
                        .map(|m| m.abstract_text.clone())
                        .unwrap_or_default(),
                }
            })
            .collect();
        return Response::ok(ResponseBody::SearchResults(results));
    }

    #[cfg(not(feature = "embedding"))]
    {
        let _ = (state, query, top_k);
        Response::system_error("search requires embedding feature — rebuild with: cargo build --features embedding".into())
    }
}

// ============================================================
// Multi-term search
// ============================================================

fn handle_multi_search(
    state: &mut DaemonState,
    queries: &[String],
    top_k: usize,
) -> Response {
    #[cfg(feature = "embedding")]
    {
        let model = match &mut state.embedding_model {
            Some(m) => m,
            None => return Response::system_error("embedding model not loaded".into()),
        };

        // Compute all embeddings first (fail fast)
        let mut embeddings = Vec::with_capacity(queries.len());
        for q in queries {
            match model.compute(q) {
                Ok(e) => embeddings.push(e),
                Err(e) => return Response::system_error(format!("embedding error: {}", e)),
            }
        }

        let hits = crate::search::multi_vector_search(&state.vector_index, &embeddings, top_k);
        let results: Vec<SearchResultEntry> = hits
            .into_iter()
            .map(|hit| {
                let meta = state.node_metas.get(&hit.node_name);
                SearchResultEntry {
                    node_name: hit.node_name,
                    score: hit.similarity,
                    similarity: hit.similarity,
                    weight: meta.map(|m| m.weight).unwrap_or(0.0),
                    abstract_text: meta
                        .map(|m| m.abstract_text.clone())
                        .unwrap_or_default(),
                }
            })
            .collect();
        return Response::ok(ResponseBody::SearchResults(results));
    }

    #[cfg(not(feature = "embedding"))]
    {
        let _ = (state, queries, top_k);
        Response::system_error("multi-search requires embedding feature — rebuild with: cargo build --features embedding".into())
    }
}

// ============================================================
// Multi-term recall
// ============================================================

fn handle_multi_recall(
    state: &mut DaemonState,
    queries: &[String],
    top_k: usize,
    depth: usize,
) -> Response {
    #[cfg(feature = "embedding")]
    {
        let model = match &mut state.embedding_model {
            Some(m) => m,
            None => return Response::system_error("embedding model not loaded".into()),
        };

        // Compute all embeddings first (fail fast)
        let mut embeddings = Vec::with_capacity(queries.len());
        for q in queries {
            match model.compute(q) {
                Ok(e) => embeddings.push(e),
                Err(e) => return Response::system_error(format!("embedding error: {}", e)),
            }
        }

        let recall_results = crate::recall::multi_recall(
            &state.vector_index,
            &state.graph,
            &state.node_metas,
            &embeddings,
            &state.config.recall,
            top_k,
            depth,
        );
        let names: Vec<String> = recall_results.into_iter().map(|r| r.node_name).collect();
        return Response::ok(ResponseBody::NodeNames(names));
    }

    #[cfg(not(feature = "embedding"))]
    {
        let _ = (state, queries, top_k, depth);
        Response::system_error("multi-recall requires embedding feature — rebuild with: cargo build --features embedding".into())
    }
}

// ============================================================
// Recall (vector + graph + weight scoring)
// ============================================================

fn handle_recall(
    state: &mut DaemonState,
    query: &Option<String>,
    name_prefix: &Option<String>,
    top_k: usize,
    depth: usize,
) -> Response {
    // Name prefix mode
    if let Some(prefix) = name_prefix {
        let matches = state.name_index.prefix_search(prefix);
        let names: Vec<String> = matches.into_iter().take(top_k).map(|n| n.to_string()).collect();
        return Response::ok(ResponseBody::NodeNames(names));
    }

    // Query mode: vector search + graph expansion + scoring
    if let Some(query_text) = query {
        #[cfg(feature = "embedding")]
        {
            let model = match &mut state.embedding_model {
                Some(m) => m,
                None => return Response::system_error("embedding model not loaded".into()),
            };
            let query_embedding = match model.compute(query_text) {
                Ok(e) => e,
                Err(e) => return Response::system_error(format!("embedding error: {}", e)),
            };
            let recall_results = crate::recall::recall(
                &state.vector_index,
                &state.graph,
                &state.node_metas,
                &query_embedding,
                &state.config.recall,
                top_k,
                depth,
            );
            let names: Vec<String> = recall_results.into_iter().map(|r| r.node_name).collect();
            return Response::ok(ResponseBody::NodeNames(names));
        }

        #[cfg(not(feature = "embedding"))]
        {
            let _ = (query_text, depth);
            return Response::system_error("recall with query requires embedding feature — rebuild with: cargo build --features embedding".into());
        }
    }

    // Working memory mode (no query): pinned + high weight
    let mut names: Vec<String> = Vec::new();

    // Add pinned nodes
    for meta in state.node_metas.values() {
        if meta.pinned {
            names.push(meta.name.clone());
        }
    }

    // Add top-weight non-pinned (dedup against pinned)
    let mut non_pinned: Vec<&NodeMeta> = state
        .node_metas
        .values()
        .filter(|m| !m.pinned)
        .collect();
    non_pinned.sort_by(|a, b| {
        b.weight
            .partial_cmp(&a.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for meta in non_pinned.into_iter().take(top_k) {
        names.push(meta.name.clone());
    }

    names.truncate(top_k);
    Response::ok(ResponseBody::NodeNames(names))
}

// ============================================================
// Status
// ============================================================

fn handle_status(state: &DaemonState) -> Response {
    let uptime = Utc::now()
        .signed_duration_since(state.started_at)
        .num_seconds()
        .max(0) as u64;
    Response::ok(ResponseBody::Status(StatusData {
        pid: std::process::id(),
        port: state.config.daemon.port,
        node_count: state.name_index.len(),
        edge_count: state.graph.edge_count(),
        index_count: state.vector_index.node_count(),
        uptime_seconds: uptime,
    }))
}

// ============================================================
// Inspect (basic)
// ============================================================

fn handle_inspect_node(state: &DaemonState, name: &str, format: &OutputFormat) -> Response {
    // Check node exists
    if !state.node_metas.contains_key(name) {
        return Response::user_error(format!("node '{}' not found", name));
    }

    let edge_count = state
        .graph
        .neighbors(name)
        .map(|n| n.len())
        .unwrap_or(0);

    let mut warnings = Vec::new();

    let similar_nodes = match state.vector_index.get_embedding(name) {
        Some(emb) => {
            let results = state.vector_index.search(emb, 11); // top_k + 1 for self
            results
                .into_iter()
                .filter(|r| r.node_name != name)
                .take(10)
                .map(|r| SimilarNode {
                    name: r.node_name,
                    similarity: r.similarity,
                })
                .collect()
        }
        None => {
            warnings.push(format!("node '{}' has no embedding", name));
            Vec::new()
        }
    };

    let meta = state.node_metas.get(name);
    let data = NodeInspectData {
        name: name.to_string(),
        edge_count,
        weight: meta.map(|m| m.weight).unwrap_or(0.0),
        pinned: meta.map(|m| m.pinned).unwrap_or(false),
        links: meta.map(|m| m.links.clone()).unwrap_or_default(),
        abstract_text: meta.map(|m| m.abstract_text.clone()).unwrap_or_default(),
        similar_nodes,
        warnings,
    };

    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&data)
                .unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e));
            Response::ok(ResponseBody::Message(json))
        }
        OutputFormat::Human => Response::ok(ResponseBody::NodeInspectReport(data)),
    }
}

fn handle_inspect(state: &DaemonState, format: &OutputFormat, threshold: Option<f32>, cap: Option<usize>) -> Response {
    let node_count = state.name_index.len();
    let edge_count = state.graph.edge_count();
    let components = state.graph.connected_components();
    let cluster_count = components.len();

    let orphan_count = components.iter().filter(|c| c.len() == 1).count();
    let orphan_ratio = if node_count > 0 {
        orphan_count as f32 / node_count as f32
    } else {
        0.0
    };

    let graveyard_count = state
        .node_metas
        .values()
        .filter(|m| m.weight < state.config.weight.warn_threshold)
        .count();
    let graveyard_ratio = if node_count > 0 {
        graveyard_count as f32 / node_count as f32
    } else {
        0.0
    };

    let density = if node_count > 1 {
        edge_count as f32 / node_count as f32
    } else {
        0.0
    };

    // Simplified health (without redundancy and retrieval degradation)
    let density_imbalance = if node_count <= 1 {
        0.0 // Not meaningful for 0-1 nodes
    } else if density < 1.5 {
        (1.5 - density) / 1.5
    } else if density > 8.0 {
        (density - 8.0) / density
    } else {
        0.0
    };

    let health = 1.0
        - (0.20 * orphan_ratio + 0.20 * graveyard_ratio + 0.15 * density_imbalance);

    let orphans: Vec<String> = state
        .node_metas
        .values()
        .filter(|m| {
            state
                .graph
                .neighbors(&m.name)
                .map(|n| n.is_empty())
                .unwrap_or(true)
        })
        .map(|m| m.name.clone())
        .collect();

    let low_weight: Vec<String> = state
        .node_metas
        .values()
        .filter(|m| m.weight < state.config.weight.warn_threshold)
        .map(|m| m.name.clone())
        .collect();

    // Compute similar pairs via upper-triangle brute-force.
    // Uses a bounded min-heap to avoid O(N^2) memory for output,
    // and a HashSet<usize> (bounded by N) for redundancy tracking.
    let max_similar_pairs = cap.unwrap_or(50);
    let sim_threshold = threshold.unwrap_or(0.85);
    let mut names: Vec<&str> = state.vector_index.all_node_names();
    names.sort(); // deterministic ordering

    // Min-heap: keeps the top max_similar_pairs by similarity.
    // Stores (similarity, i, j) as indices into `names`.
    let mut top_pairs: std::collections::BinaryHeap<std::cmp::Reverse<SimPairEntry>> =
        std::collections::BinaryHeap::with_capacity(max_similar_pairs + 1);
    // Track ALL nodes involved in above-threshold pairs for redundancy metric.
    // Bounded by N (number of nodes), not N^2.
    let mut nodes_in_pairs: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut total_above_threshold: usize = 0;

    for i in 0..names.len() {
        let emb_a = match state.vector_index.get_embedding(names[i]) {
            Some(e) => e,
            None => continue,
        };
        for j in (i + 1)..names.len() {
            let emb_b = match state.vector_index.get_embedding(names[j]) {
                Some(e) => e,
                None => continue,
            };
            let sim = crate::util::cosine_similarity(emb_a, emb_b);
            if sim >= sim_threshold {
                total_above_threshold += 1;
                nodes_in_pairs.insert(i);
                nodes_in_pairs.insert(j);
                top_pairs.push(std::cmp::Reverse(SimPairEntry { similarity: sim, i, j }));
                if top_pairs.len() > max_similar_pairs {
                    top_pairs.pop(); // evict lowest similarity
                }
            }
        }
    }

    // Convert heap → sorted Vec<SimilarPair>, descending by similarity.
    // into_sorted_vec() on BinaryHeap<Reverse<T>> yields descending order
    // of the inner T's natural ordering (i.e. highest similarity first).
    let similar_pairs: Vec<SimilarPair> = top_pairs
        .into_sorted_vec()
        .into_iter()
        .map(|std::cmp::Reverse(entry)| SimilarPair {
            node_a: names[entry.i].to_string(),
            node_b: names[entry.j].to_string(),
            similarity: entry.similarity,
        })
        .collect();

    let redundancy = if node_count > 0 {
        Some(nodes_in_pairs.len() as f32 / node_count as f32)
    } else {
        Some(0.0)
    };

    let data = InspectData {
        node_count,
        edge_count,
        cluster_count,
        orphan_count,
        health_score: health.clamp(0.0, 1.0),
        redundancy,
        orphan_ratio,
        graveyard_ratio,
        density,
        similar_pairs,
        total_similar_pairs: total_above_threshold,
        orphans,
        low_weight,
    };

    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&data)
                .unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e));
            Response::ok(ResponseBody::Message(json))
        }
        OutputFormat::Human => Response::ok(ResponseBody::InspectReport(data)),
    }
}

// ============================================================
// GC
// ============================================================

fn handle_gc(
    state: &mut DaemonState,
    memories_dir: &Path,
    base_dir: &Path,
) -> Response {
    let mut cleaned = 0;

    // Collect nodes with dangling links
    let node_names: Vec<String> = state.node_metas.keys().cloned().collect();
    for name in &node_names {
        if let Some(meta) = state.node_metas.get_mut(name) {
            let before = meta.links.len();
            meta.links.retain(|link| state.name_index.contains(link));
            let after = meta.links.len();
            if before != after {
                cleaned += before - after;
                // Update the .md file
                if let Ok((mut fm, body)) = node::read_node_from_dir(memories_dir, name) {
                    fm.links = meta.links.clone();
                    let _ = node::write_node_to_dir(memories_dir, name, &fm, &body);
                }
            }
        }
    }

    // Rebuild graph from cleaned state
    let mut graph = crate::graph::Graph::new();
    for name in state.node_metas.keys() {
        graph.ensure_node(name);
    }
    for (name, meta) in &state.node_metas {
        for link in &meta.links {
            graph.add_edge(name, link);
        }
    }
    state.graph = graph;

    let _ = save_graph_idx(base_dir, &state.graph);

    Response::ok(ResponseBody::Message(format!(
        "gc: cleaned {} dangling references",
        cleaned
    )))
}

// ============================================================
// Reindex
// ============================================================

fn handle_reindex(
    state: &mut DaemonState,
    _memories_dir: &Path,
    base_dir: &Path,
) -> Response {
    #[cfg(feature = "embedding")]
    {
        let model = match &mut state.embedding_model {
            Some(m) => m,
            None => return Response::system_error("embedding model not loaded".into()),
        };

        let mut indexed = 0usize;
        let mut errors = 0usize;

        // Collect names + abstracts first to avoid borrow conflict
        let nodes: Vec<(String, String)> = state
            .node_metas
            .iter()
            .map(|(name, meta)| (name.clone(), meta.abstract_text.clone()))
            .collect();

        for (name, abstract_text) in &nodes {
            match model.compute(abstract_text) {
                Ok(embedding) => {
                    state.vector_index.insert(name, &embedding);
                    indexed += 1;
                }
                Err(e) => {
                    tracing::warn!("reindex: embedding failed for {}: {}", name, e);
                    errors += 1;
                }
            }
        }

        // Save updated index
        let index_dir = base_dir.join("index");
        let _ = state.vector_index.save_to_dir(&index_dir);

        return Response::ok(ResponseBody::Message(format!(
            "reindex: {} nodes indexed, {} errors",
            indexed, errors
        )));
    }

    #[cfg(not(feature = "embedding"))]
    {
        let _ = (state, _memories_dir, base_dir);
        Response::system_error("reindex requires embedding feature — rebuild with: cargo build --features embedding".into())
    }
}

// ============================================================
// Baseline
// ============================================================

fn handle_baseline(
    state: &mut DaemonState,
    base_dir: &Path,
) -> Response {
    #[cfg(feature = "embedding")]
    {
        if state.vector_index.node_count() < 2 {
            return Response::user_error("need at least 2 indexed nodes for baseline".into());
        }

        let names = state.vector_index.all_node_names();
        let mut similarities: Vec<f32> = Vec::new();

        for i in 0..names.len() {
            let emb_a = match state.vector_index.get_embedding(names[i]) {
                Some(e) => e,
                None => continue,
            };
            for j in (i + 1)..names.len() {
                let emb_b = match state.vector_index.get_embedding(names[j]) {
                    Some(e) => e,
                    None => continue,
                };
                similarities.push(crate::util::cosine_similarity(emb_a, emb_b));
            }
        }

        if similarities.is_empty() {
            return Response::user_error("no similarity pairs computed".into());
        }

        similarities.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let n = similarities.len();
        let mean = similarities.iter().sum::<f32>() / n as f32;
        let p25 = similarities[n / 4];
        let p50 = similarities[n / 2];
        let p75 = similarities[3 * n / 4];
        let p95_idx = (n as f64 * 0.95) as usize;
        let p95 = similarities[p95_idx.min(n - 1)];
        let min = similarities[0];
        let max = similarities[n - 1];

        // Persist to disk as mem-base-info.json
        let info = serde_json::json!({
            "pairs": n,
            "node_count": names.len(),
            "mean": mean,
            "min": min,
            "p25": p25,
            "p50": p50,
            "p75": p75,
            "p95": p95,
            "max": max,
            "computed_at": Utc::now().to_rfc3339(),
        });
        let info_path = base_dir.join("index").join("mem-base-info.json");
        let _ = std::fs::create_dir_all(base_dir.join("index"));
        match serde_json::to_string_pretty(&info) {
            Ok(json_str) => {
                if let Err(e) = std::fs::write(&info_path, &json_str) {
                    return Response::system_error(format!("failed to write mem-base-info.json: {}", e));
                }
            }
            Err(e) => {
                return Response::system_error(format!("failed to serialize baseline: {}", e));
            }
        }

        return Response::ok(ResponseBody::Message(format!(
            "baseline: {} pairs | mean={:.4} min={:.4} p25={:.4} p50={:.4} p75={:.4} p95={:.4} max={:.4}\nsaved to {}",
            n, mean, min, p25, p50, p75, p95, max, info_path.display()
        )));
    }

    #[cfg(not(feature = "embedding"))]
    {
        let _ = (state, base_dir);
        Response::system_error("baseline requires embedding feature — rebuild with: cargo build --features embedding".into())
    }
}

// ============================================================
// Similar pair helper for bounded min-heap
// ============================================================

/// Lightweight entry for tracking top-K similar pairs without String allocation.
/// Only indices into a `names` slice are stored; String conversion happens after
/// the heap is finalized.
#[derive(Clone, Copy, PartialEq)]
struct SimPairEntry {
    similarity: f32,
    i: usize,
    j: usize,
}

impl Eq for SimPairEntry {}

impl PartialOrd for SimPairEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SimPairEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.similarity
            .partial_cmp(&other.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

// ============================================================
// Disk update helpers
// ============================================================

/// Add or remove a link in a peer's .md file.
fn update_peer_links_on_disk(memories_dir: &Path, peer_name: &str, target: &str, add: bool) {
    let Ok((mut fm, body)) = node::read_node_from_dir(memories_dir, peer_name) else {
        return;
    };

    if add {
        if !fm.links.contains(&target.to_string()) {
            fm.links.push(target.to_string());
        }
    } else {
        fm.links.retain(|l| l != target);
    }

    let _ = node::write_node_to_dir(memories_dir, peer_name, &fm, &body);
}

/// Replace old_name with new_name in a peer's links.
fn rename_in_peer_links(memories_dir: &Path, peer_name: &str, old: &str, new: &str) {
    let Ok((mut fm, body)) = node::read_node_from_dir(memories_dir, peer_name) else {
        return;
    };

    for link in &mut fm.links {
        if link == old {
            *link = new.to_string();
        }
    }

    let _ = node::write_node_to_dir(memories_dir, peer_name, &fm, &body);
}

/// Update weight in a node's .md file.
fn update_weight_on_disk(memories_dir: &Path, name: &str, weight: f32) {
    let Ok((mut fm, body)) = node::read_node_from_dir(memories_dir, name) else {
        return;
    };
    fm.weight = weight;
    let _ = node::write_node_to_dir(memories_dir, name, &fm, &body);
}

/// Update pinned flag in a node's .md file.
fn update_pinned_on_disk(memories_dir: &Path, name: &str, pinned: bool) {
    let Ok((mut fm, body)) = node::read_node_from_dir(memories_dir, name) else {
        return;
    };
    fm.pinned = pinned;
    let _ = node::write_node_to_dir(memories_dir, name, &fm, &body);
}

/// Update in-memory timestamp
fn update_timestamp_in_memory(state: &mut DaemonState, name: &str) {
    if let Some(meta) = state.node_metas.get_mut(name) {
        meta.updated = Utc::now();
    }
}

fn format_time_ago(dt: chrono::DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(dt);
    let secs = duration.num_seconds();

    if secs < 60 {
        format!("{}s ago", secs)
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else if dt.year() == now.year() {
        dt.format("%b %d").to_string()
    } else {
        dt.format("%Y-%m-%d").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_format_time_ago_seconds() {
        let now = Utc::now();
        let dt = now - chrono::Duration::seconds(30);
        assert_eq!(format_time_ago(dt), "30s ago");
    }

    #[test]
    fn test_format_time_ago_minutes() {
        let now = Utc::now();
        let dt = now - chrono::Duration::minutes(5);
        assert_eq!(format_time_ago(dt), "5m ago");
    }

    #[test]
    fn test_format_time_ago_hours() {
        let now = Utc::now();
        let dt = now - chrono::Duration::hours(3);
        assert_eq!(format_time_ago(dt), "3h ago");
    }

    #[test]
    fn test_format_time_ago_same_year_absolute() {
        // Use a date definitely in the current year but >24h ago
        let now = Utc::now();
        let dt = now - chrono::Duration::days(3);
        let result = format_time_ago(dt);
        // Should be like "Feb 24" — NOT "3d ago"
        assert!(!result.contains("ago"), "expected absolute date, got: {}", result);
        assert!(result.len() >= 5, "expected 'Mon DD' format, got: {}", result);
    }

    #[test]
    fn test_format_time_ago_different_year() {
        let dt = Utc.with_ymd_and_hms(2023, 6, 15, 12, 0, 0).unwrap();
        let result = format_time_ago(dt);
        assert_eq!(result, "2023-06-15");
    }
}
