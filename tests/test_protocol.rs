use memcore::protocol::*;

// ============================================================
// Framing: encode/decode roundtrip
// ============================================================

#[test]
fn test_encode_decode_request_create() {
    let req = Request::Create {
        name: "my-node".to_string(),
        content: "---\nabstract: test\n---\n\nHello world".to_string(),
    };
    let bytes = encode_message(&req).unwrap();
    // First 4 bytes are length
    let len = read_length(&bytes[..4].try_into().unwrap());
    assert_eq!(len as usize, bytes.len() - 4);
    let decoded: Request = decode_message(&bytes[4..]).unwrap();
    match decoded {
        Request::Create { name, content } => {
            assert_eq!(name, "my-node");
            assert!(content.contains("Hello world"));
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn test_encode_decode_request_get() {
    let req = Request::Get {
        names: vec!["a".into(), "b".into(), "c".into()],
    };
    let bytes = encode_message(&req).unwrap();
    let decoded: Request = decode_message(&bytes[4..]).unwrap();
    match decoded {
        Request::Get { names } => assert_eq!(names, vec!["a", "b", "c"]),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn test_encode_decode_request_patch_replace() {
    let req = Request::Patch {
        name: "node-a".into(),
        op: PatchRequest::Replace {
            old: "hello".into(),
            new: "goodbye".into(),
        },
    };
    let bytes = encode_message(&req).unwrap();
    let decoded: Request = decode_message(&bytes[4..]).unwrap();
    match decoded {
        Request::Patch { name, op } => {
            assert_eq!(name, "node-a");
            match op {
                PatchRequest::Replace { old, new } => {
                    assert_eq!(old, "hello");
                    assert_eq!(new, "goodbye");
                }
                _ => panic!("wrong patch variant"),
            }
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn test_encode_decode_request_recall() {
    let req = Request::Recall {
        query: Some("async python".into()),
        name_prefix: None,
        top_k: 5,
        depth: 2,
    };
    let bytes = encode_message(&req).unwrap();
    let decoded: Request = decode_message(&bytes[4..]).unwrap();
    match decoded {
        Request::Recall {
            query,
            name_prefix,
            top_k,
            depth,
        } => {
            assert_eq!(query.unwrap(), "async python");
            assert!(name_prefix.is_none());
            assert_eq!(top_k, 5);
            assert_eq!(depth, 2);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn test_encode_decode_all_simple_requests() {
    let requests = vec![
        Request::Delete {
            name: "old".into(),
        },
        Request::Rename {
            old: "a".into(),
            new: "b".into(),
        },
        Request::Ls {
            sort: SortField::Weight,
        },
        Request::Link {
            a: "x".into(),
            b: "y".into(),
        },
        Request::Unlink {
            a: "x".into(),
            b: "y".into(),
        },
        Request::Boost {
            name: "n".into(),
        },
        Request::Penalize {
            name: "n".into(),
        },
        Request::Pin {
            name: "n".into(),
        },
        Request::Unpin {
            name: "n".into(),
        },
        Request::Status,
        Request::Reindex,
        Request::Gc,
        Request::Baseline,
        Request::Stop,
    ];
    for req in &requests {
        let bytes = encode_message(req).unwrap();
        let _decoded: Request = decode_message(&bytes[4..]).unwrap();
    }
}

// ============================================================
// Response roundtrip
// ============================================================

#[test]
fn test_response_ok_message() {
    let resp = Response::ok(ResponseBody::Message("created: my-node [indexed]".into()));
    let bytes = encode_message(&resp).unwrap();
    let decoded: Response = decode_message(&bytes[4..]).unwrap();
    assert!(decoded.success);
    assert_eq!(decoded.exit_code, 0);
    match decoded.body {
        ResponseBody::Message(m) => assert!(m.contains("my-node")),
        _ => panic!("wrong body"),
    }
}

#[test]
fn test_response_user_error() {
    let resp = Response::user_error("node 'x' not found".into());
    let bytes = encode_message(&resp).unwrap();
    let decoded: Response = decode_message(&bytes[4..]).unwrap();
    assert!(!decoded.success);
    assert_eq!(decoded.exit_code, 1);
}

#[test]
fn test_response_system_error() {
    let resp = Response::system_error("I/O failure".into());
    assert_eq!(resp.exit_code, 2);
}

#[test]
fn test_response_connection_error() {
    let resp = Response::connection_error("timeout".into());
    assert_eq!(resp.exit_code, 3);
}

#[test]
fn test_response_node_list() {
    let resp = Response::ok(ResponseBody::NodeList(vec![
        NodeListEntry {
            name: "alpha".into(),
            weight: 0.9,
            edge_count: 3,
            last_accessed: "2h ago".into(),
            pinned: false,
        },
        NodeListEntry {
            name: "beta".into(),
            weight: 0.5,
            edge_count: 0,
            last_accessed: "1d ago".into(),
            pinned: true,
        },
    ]));
    let bytes = encode_message(&resp).unwrap();
    let decoded: Response = decode_message(&bytes[4..]).unwrap();
    match decoded.body {
        ResponseBody::NodeList(list) => {
            assert_eq!(list.len(), 2);
            assert_eq!(list[0].name, "alpha");
            assert!(list[1].pinned);
        }
        _ => panic!("wrong body"),
    }
}

#[test]
fn test_response_search_results() {
    let resp = Response::ok(ResponseBody::SearchResults(vec![SearchResultEntry {
        node_name: "user-profile".into(),
        score: 0.87,
        similarity: 0.94,
        weight: 0.95,
        abstract_text: "User preferences and settings".into(),
    }]));
    let bytes = encode_message(&resp).unwrap();
    let decoded: Response = decode_message(&bytes[4..]).unwrap();
    match decoded.body {
        ResponseBody::SearchResults(results) => {
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].node_name, "user-profile");
        }
        _ => panic!("wrong body"),
    }
}

#[test]
fn test_response_neighbors() {
    let resp = Response::ok(ResponseBody::Neighbors {
        entries: vec![NeighborEntry {
            name: "peer".into(),
            depth: 1,
            weight: 0.8,
        }],
        total: 47,
        depth: 2,
    });
    let bytes = encode_message(&resp).unwrap();
    let decoded: Response = decode_message(&bytes[4..]).unwrap();
    match decoded.body {
        ResponseBody::Neighbors {
            entries,
            total,
            depth,
        } => {
            assert_eq!(entries.len(), 1);
            assert_eq!(total, 47);
            assert_eq!(depth, 2);
        }
        _ => panic!("wrong body"),
    }
}

#[test]
fn test_response_status() {
    let resp = Response::ok(ResponseBody::Status(StatusData {
        pid: 12345,
        port: 9527,
        node_count: 42,
        edge_count: 87,
        index_count: 42,
        uptime_seconds: 3600,
    }));
    let bytes = encode_message(&resp).unwrap();
    let decoded: Response = decode_message(&bytes[4..]).unwrap();
    match decoded.body {
        ResponseBody::Status(s) => {
            assert_eq!(s.pid, 12345);
            assert_eq!(s.node_count, 42);
        }
        _ => panic!("wrong body"),
    }
}

// ============================================================
// Handshake
// ============================================================

#[test]
fn test_handshake_ping() {
    let line = handshake_ping("abc123");
    assert_eq!(line, "MEMCORE_PING abc123\n");
    let nonce = parse_ping(&line).unwrap();
    assert_eq!(nonce, "abc123");
}

#[test]
fn test_handshake_pong() {
    let line = handshake_pong("0.1.0");
    assert_eq!(line, "MEMCORE_PONG 0.1.0\n");
    let version = parse_pong(&line).unwrap();
    assert_eq!(version, "0.1.0");
}

#[test]
fn test_parse_ping_invalid() {
    assert!(parse_ping("HELLO world\n").is_none());
    assert!(parse_ping("").is_none());
}

#[test]
fn test_parse_pong_invalid() {
    assert!(parse_pong("HELLO world\n").is_none());
}

// ============================================================
// Sort field and output format
// ============================================================

#[test]
fn test_sort_field_roundtrip() {
    for field in &[SortField::Name, SortField::Weight, SortField::Date] {
        let json = serde_json::to_string(field).unwrap();
        let decoded: SortField = serde_json::from_str(&json).unwrap();
        // Verify same variant by matching
        match (field, &decoded) {
            (SortField::Name, SortField::Name) => {}
            (SortField::Weight, SortField::Weight) => {}
            (SortField::Date, SortField::Date) => {}
            _ => panic!("mismatch"),
        }
    }
}

#[test]
fn test_output_format_roundtrip() {
    for fmt in &[OutputFormat::Human, OutputFormat::Json] {
        let json = serde_json::to_string(fmt).unwrap();
        let _decoded: OutputFormat = serde_json::from_str(&json).unwrap();
    }
}
