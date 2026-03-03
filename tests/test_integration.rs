// Integration tests: end-to-end daemon ↔ client communication
//
// These tests exercise the full TCP protocol flow:
//   handshake → request framing → handler dispatch → response framing

use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

use memcore::config::Config;
use memcore::daemon_state::DaemonState;
use memcore::graph::Graph;
use memcore::handler;
use memcore::index::VectorIndex;
use memcore::name_index::NameIndex;
use memcore::protocol::{self, Request, Response, ResponseBody};
use memcore::wal::WalWriter;

/// Build valid node content with required YAML fields
fn make_content(abstract_text: &str, links: &[&str], body: &str) -> String {
    let links_yaml = if links.is_empty() {
        "links: []".to_string()
    } else {
        let items: Vec<String> = links.iter().map(|l| format!("- {}", l)).collect();
        format!("links:\n{}", items.join("\n"))
    };
    format!(
        "---\ncreated: '2025-01-01T00:00:00Z'\nupdated: '2025-01-01T00:00:00Z'\nweight: 1.0\nlast_accessed: '2025-01-01T00:00:00Z'\naccess_count: 0\npinned: false\n{}\nabstract: {}\n---\n\n{}",
        links_yaml, abstract_text, body
    )
}

/// Create a minimal DaemonState for testing
fn make_test_state(base_dir: &std::path::Path) -> DaemonState {
    std::fs::create_dir_all(base_dir.join("memories")).unwrap();
    let wal = WalWriter::at(base_dir.join("wal.log"));
    DaemonState {
        graph: Graph::new(),
        name_index: NameIndex::new(),
        vector_index: VectorIndex::new(),
        node_metas: std::collections::HashMap::new(),
        wal,
        config: Config::default(),
        started_at: chrono::Utc::now(),
        access_dirty: std::collections::HashSet::new(),
        #[cfg(feature = "embedding")]
        embedding_model: None,
    }
}

/// Spawn a mini daemon server on a random port, returns (port, nonce, state)
async fn spawn_test_daemon(
    base_dir: std::path::PathBuf,
) -> (u16, String, Arc<Mutex<DaemonState>>) {
    let state = make_test_state(&base_dir);
    let state = Arc::new(Mutex::new(state));
    let nonce = "test-nonce-12345".to_string();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let daemon_state = Arc::clone(&state);
    let daemon_nonce = nonce.clone();
    let daemon_base_dir = base_dir.clone();

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let state = Arc::clone(&daemon_state);
            let nonce = daemon_nonce.clone();
            let base_dir = daemon_base_dir.clone();

            tokio::spawn(async move {
                let _ =
                    handle_test_connection(stream, state, &nonce, &base_dir).await;
            });
        }
    });

    (port, nonce, state)
}

/// Mini daemon connection handler (mirrors daemon.rs logic)
async fn handle_test_connection(
    stream: tokio::net::TcpStream,
    state: Arc<Mutex<DaemonState>>,
    nonce: &str,
    base_dir: &std::path::Path,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    // Handshake
    let mut ping_line = String::new();
    reader.read_line(&mut ping_line).await?;

    let client_nonce = protocol::parse_ping(&ping_line)
        .ok_or_else(|| anyhow::anyhow!("bad ping"))?;

    if client_nonce != nonce {
        anyhow::bail!("nonce mismatch");
    }

    let pong = protocol::handshake_pong("0.1.0-test");
    writer.write_all(pong.as_bytes()).await?;
    writer.flush().await?;

    // Read request
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let msg_len = protocol::read_length(&len_buf) as usize;

    let mut msg_buf = vec![0u8; msg_len];
    reader.read_exact(&mut msg_buf).await?;

    let request: Request = protocol::decode_message(&msg_buf)?;

    // Handle request
    let response = {
        let mut state = state.lock().await;
        handler::handle_request(&mut state, &request, base_dir)
    };

    // Send response
    let resp_bytes = protocol::encode_message(&response)?;
    writer.write_all(&resp_bytes).await?;
    writer.flush().await?;

    Ok(())
}

/// Client helper: connect, handshake, send request, receive response
async fn client_roundtrip(
    port: u16,
    nonce: &str,
    request: &Request,
) -> anyhow::Result<Response> {
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await?;
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    // Handshake
    let ping = protocol::handshake_ping(nonce);
    writer.write_all(ping.as_bytes()).await?;
    writer.flush().await?;

    let mut pong_line = String::new();
    reader.read_line(&mut pong_line).await?;
    let _version = protocol::parse_pong(&pong_line)
        .ok_or_else(|| anyhow::anyhow!("bad pong"))?;

    // Send request
    let req_bytes = protocol::encode_message(request)?;
    writer.write_all(&req_bytes).await?;
    writer.flush().await?;

    // Read response
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let msg_len = protocol::read_length(&len_buf) as usize;

    let mut msg_buf = vec![0u8; msg_len];
    reader.read_exact(&mut msg_buf).await?;

    let response: Response = protocol::decode_message(&msg_buf)?;
    Ok(response)
}

// ============================================================
// Tests
// ============================================================

#[tokio::test]
async fn test_roundtrip_status() {
    let tmp = tempfile::tempdir().unwrap();
    let (port, nonce, _state) = spawn_test_daemon(tmp.path().to_path_buf()).await;

    let resp = client_roundtrip(port, &nonce, &Request::Status).await.unwrap();
    assert!(resp.success);
    assert_eq!(resp.exit_code, 0);
    match resp.body {
        ResponseBody::Status(s) => {
            assert_eq!(s.node_count, 0);
            assert_eq!(s.edge_count, 0);
        }
        other => panic!("expected Status, got {:?}", other),
    }
}

#[tokio::test]
async fn test_roundtrip_create_and_get() {
    let tmp = tempfile::tempdir().unwrap();
    let (port, nonce, _state) = spawn_test_daemon(tmp.path().to_path_buf()).await;

    // Create
    let content = make_content("test node", &[], "Hello world");
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Create {
            name: "test-node".into(),
            content: content.into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success, "create failed: {:?}", resp.body);

    // Get
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Get {
            names: vec!["test-node".into()],
        },
    )
    .await
    .unwrap();
    assert!(resp.success);
    match &resp.body {
        ResponseBody::NodeContent { name, content } => {
            assert_eq!(name, "test-node");
            assert!(content.contains("Hello world"));
        }
        other => panic!("expected NodeContent, got {:?}", other),
    }
}

#[tokio::test]
async fn test_roundtrip_create_delete() {
    let tmp = tempfile::tempdir().unwrap();
    let (port, nonce, _state) = spawn_test_daemon(tmp.path().to_path_buf()).await;

    let content = make_content("temp node", &[], "temp");
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Create {
            name: "temp-node".into(),
            content: content.into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);

    // Delete
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Delete {
            name: "temp-node".into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);
    match &resp.body {
        ResponseBody::Message(msg) => assert!(msg.contains("deleted")),
        other => panic!("expected Message, got {:?}", other),
    }

    // Get after delete should fail
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Get {
            names: vec!["temp-node".into()],
        },
    )
    .await
    .unwrap();
    assert!(!resp.success);
}

#[tokio::test]
async fn test_roundtrip_link_neighbors() {
    let tmp = tempfile::tempdir().unwrap();
    let (port, nonce, _state) = spawn_test_daemon(tmp.path().to_path_buf()).await;

    let content_a = make_content("node a", &[], "A");
    let content_b = make_content("node b", &[], "B");

    // Create two nodes
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Create {
            name: "node-a".into(),
            content: content_a.into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);

    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Create {
            name: "node-b".into(),
            content: content_b.into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);

    // Link
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Link {
            a: "node-a".into(),
            b: "node-b".into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);

    // Neighbors
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Neighbors {
            name: "node-a".into(),
            depth: 1,
            limit: 10,
            offset: 0,
        },
    )
    .await
    .unwrap();
    assert!(resp.success);
    match &resp.body {
        ResponseBody::Neighbors {
            entries, total, ..
        } => {
            assert_eq!(*total, 1);
            assert_eq!(entries[0].name, "node-b");
        }
        other => panic!("expected Neighbors, got {:?}", other),
    }
}

#[tokio::test]
async fn test_roundtrip_ls_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let (port, nonce, _state) = spawn_test_daemon(tmp.path().to_path_buf()).await;

    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Ls {
            sort: protocol::SortField::Name,
        },
    )
    .await
    .unwrap();
    assert!(resp.success);
    match &resp.body {
        ResponseBody::NodeList(entries) => assert!(entries.is_empty()),
        other => panic!("expected NodeList, got {:?}", other),
    }
}

#[tokio::test]
async fn test_roundtrip_inspect() {
    let tmp = tempfile::tempdir().unwrap();
    let (port, nonce, _state) = spawn_test_daemon(tmp.path().to_path_buf()).await;

    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Inspect {
            node: None,
            format: protocol::OutputFormat::Human,
            threshold: None,
            cap: None,
        },
    )
    .await
    .unwrap();
    assert!(resp.success);
    match &resp.body {
        ResponseBody::InspectReport(data) => {
            assert_eq!(data.node_count, 0);
            assert!((data.health_score - 1.0).abs() < 0.01);
        }
        other => panic!("expected InspectReport, got {:?}", other),
    }
}

#[tokio::test]
async fn test_roundtrip_boost_penalize() {
    let tmp = tempfile::tempdir().unwrap();
    let (port, nonce, _state) = spawn_test_daemon(tmp.path().to_path_buf()).await;

    let content = make_content("test", &[], "body");
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Create {
            name: "test-node".into(),
            content: content.into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);

    // Boost at max weight → "already at maximum"
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Boost {
            name: "test-node".into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);
    match &resp.body {
        ResponseBody::Message(msg) => assert!(msg.contains("already at maximum"), "expected 'already at maximum', got: {}", msg),
        other => panic!("expected Message, got {:?}", other),
    }

    // Penalize to get below max
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Penalize {
            name: "test-node".into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);
    match &resp.body {
        ResponseBody::Message(msg) => assert!(msg.contains("penalized")),
        other => panic!("expected Message, got {:?}", other),
    }

    // Now boost works (below max)
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Boost {
            name: "test-node".into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);
    match &resp.body {
        ResponseBody::Message(msg) => assert!(msg.contains("boosted"), "expected 'boosted', got: {}", msg),
        other => panic!("expected Message, got {:?}", other),
    }
}

#[tokio::test]
async fn test_roundtrip_pin_unpin() {
    let tmp = tempfile::tempdir().unwrap();
    let (port, nonce, _state) = spawn_test_daemon(tmp.path().to_path_buf()).await;

    let content = make_content("pin test", &[], "body");
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Create {
            name: "pin-node".into(),
            content: content.into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);

    // Pin
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Pin {
            name: "pin-node".into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);

    // Unpin
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Unpin {
            name: "pin-node".into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);
}

#[tokio::test]
async fn test_roundtrip_rename() {
    let tmp = tempfile::tempdir().unwrap();
    let (port, nonce, _state) = spawn_test_daemon(tmp.path().to_path_buf()).await;

    let content = make_content("rename test", &[], "body");
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Create {
            name: "old-name".into(),
            content: content.into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);

    // Rename
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Rename {
            old: "old-name".into(),
            new: "new-name".into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);
    match &resp.body {
        ResponseBody::Message(msg) => {
            assert!(msg.contains("renamed"));
            assert!(msg.contains("old-name"));
            assert!(msg.contains("new-name"));
        }
        other => panic!("expected Message, got {:?}", other),
    }

    // Get old name should fail
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Get {
            names: vec!["old-name".into()],
        },
    )
    .await
    .unwrap();
    assert!(!resp.success);

    // Get new name should succeed
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Get {
            names: vec!["new-name".into()],
        },
    )
    .await
    .unwrap();
    assert!(resp.success);
}

#[tokio::test]
async fn test_roundtrip_patch() {
    let tmp = tempfile::tempdir().unwrap();
    let (port, nonce, _state) = spawn_test_daemon(tmp.path().to_path_buf()).await;

    let content = make_content("patch test", &[], "original body");
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Create {
            name: "patch-node".into(),
            content: content.into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);

    // Patch append
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Patch {
            name: "patch-node".into(),
            op: protocol::PatchRequest::Append("\nappended text".into()),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);

    // Verify
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Get {
            names: vec!["patch-node".into()],
        },
    )
    .await
    .unwrap();
    assert!(resp.success);
    match &resp.body {
        ResponseBody::NodeContent { content, .. } => {
            assert!(content.contains("original body"));
            assert!(content.contains("appended text"));
        }
        other => panic!("expected NodeContent, got {:?}", other),
    }
}

#[tokio::test]
async fn test_roundtrip_gc() {
    let tmp = tempfile::tempdir().unwrap();
    let (port, nonce, _state) = spawn_test_daemon(tmp.path().to_path_buf()).await;

    let resp = client_roundtrip(port, &nonce, &Request::Gc).await.unwrap();
    assert!(resp.success);
    match &resp.body {
        ResponseBody::Message(msg) => assert!(msg.contains("gc")),
        other => panic!("expected Message, got {:?}", other),
    }
}

#[tokio::test]
async fn test_handshake_wrong_nonce_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let (port, _nonce, _state) = spawn_test_daemon(tmp.path().to_path_buf()).await;

    // Try connecting with wrong nonce — daemon should reject
    let result = client_roundtrip(port, "wrong-nonce", &Request::Status).await;
    // The connection should fail (daemon closes connection on nonce mismatch)
    assert!(result.is_err());
}

#[tokio::test]
async fn test_roundtrip_update() {
    let tmp = tempfile::tempdir().unwrap();
    let (port, nonce, _state) = spawn_test_daemon(tmp.path().to_path_buf()).await;

    // Create
    let content = make_content("original", &[], "original body");
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Create {
            name: "update-node".into(),
            content: content.into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);

    // Update
    let new_content = make_content("updated", &[], "new body");
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Update {
            name: "update-node".into(),
            content: new_content.into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);

    // Verify
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Get {
            names: vec!["update-node".into()],
        },
    )
    .await
    .unwrap();
    assert!(resp.success);
    match &resp.body {
        ResponseBody::NodeContent { content, .. } => {
            assert!(content.contains("new body"));
            assert!(content.contains("updated"));
        }
        other => panic!("expected NodeContent, got {:?}", other),
    }
}

#[tokio::test]
async fn test_roundtrip_unlink() {
    let tmp = tempfile::tempdir().unwrap();
    let (port, nonce, _state) = spawn_test_daemon(tmp.path().to_path_buf()).await;

    // Create two nodes and link
    let ca = make_content("a", &[], "A");
    let cb = make_content("b", &[], "B");

    client_roundtrip(
        port,
        &nonce,
        &Request::Create {
            name: "ua".into(),
            content: ca.into(),
        },
    )
    .await
    .unwrap();
    client_roundtrip(
        port,
        &nonce,
        &Request::Create {
            name: "ub".into(),
            content: cb.into(),
        },
    )
    .await
    .unwrap();
    client_roundtrip(
        port,
        &nonce,
        &Request::Link {
            a: "ua".into(),
            b: "ub".into(),
        },
    )
    .await
    .unwrap();

    // Unlink
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Unlink {
            a: "ua".into(),
            b: "ub".into(),
        },
    )
    .await
    .unwrap();
    assert!(resp.success);

    // Verify no neighbors
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Neighbors {
            name: "ua".into(),
            depth: 1,
            limit: 10,
            offset: 0,
        },
    )
    .await
    .unwrap();
    match &resp.body {
        ResponseBody::Neighbors { total, .. } => assert_eq!(*total, 0),
        other => panic!("expected Neighbors, got {:?}", other),
    }
}

#[tokio::test]
async fn test_multiple_sequential_requests() {
    let tmp = tempfile::tempdir().unwrap();
    let (port, nonce, _state) = spawn_test_daemon(tmp.path().to_path_buf()).await;

    // Create 5 nodes sequentially (each on a fresh connection)
    for i in 0..5 {
        let name = format!("node-{}", i);
        let content = make_content(&format!("node {}", i), &[], &format!("body {}", i));
        let resp = client_roundtrip(
            port,
            &nonce,
            &Request::Create {
                name,
                content,
            },
        )
        .await
        .unwrap();
        assert!(resp.success, "failed to create node-{}: {:?}", i, resp.body);
    }

    // List all
    let resp = client_roundtrip(
        port,
        &nonce,
        &Request::Ls {
            sort: protocol::SortField::Name,
        },
    )
    .await
    .unwrap();
    assert!(resp.success);
    match &resp.body {
        ResponseBody::NodeList(entries) => {
            assert_eq!(entries.len(), 5);
        }
        other => panic!("expected NodeList, got {:?}", other),
    }
}
