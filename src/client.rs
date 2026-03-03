use std::io::Read;
use std::path::PathBuf;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::config;
use crate::protocol::{self, OutputFormat, PatchRequest, Request, Response, ResponseBody, SortField};
use crate::Commands;

/// Dispatch a CLI command to the daemon
pub async fn dispatch(command: Commands, quiet: bool) -> anyhow::Result<()> {
    match command {
        Commands::Init { dir } => {
            init(dir).await
        }
        Commands::Daemon => {
            unreachable!("daemon mode handled in main.rs")
        }
        other => {
            let request = build_request(other)?;
            let response = send_to_daemon(request).await?;
            let exit_code = response.exit_code;
            format_response(&response, quiet);
            if exit_code != 0 {
                std::process::exit(exit_code as i32);
            }
            Ok(())
        }
    }
}

/// Build a protocol Request from CLI Commands
fn build_request(command: Commands) -> anyhow::Result<Request> {
    match command {
        Commands::Create { name, f } => {
            let content = read_content(f)?;
            Ok(Request::Create { name, content })
        }
        Commands::Get { names } => {
            Ok(Request::Get { names })
        }
        Commands::Update { name, f } => {
            let content = read_content(f)?;
            Ok(Request::Update { name, content })
        }
        Commands::Patch {
            name,
            replace,
            append,
            prepend,
        } => {
            let op = if let Some(parts) = replace {
                if parts.len() != 2 {
                    anyhow::bail!("--replace requires exactly 2 arguments: old new");
                }
                PatchRequest::Replace {
                    old: parts[0].clone(),
                    new: parts[1].clone(),
                }
            } else if let Some(text) = append {
                PatchRequest::Append(text)
            } else if let Some(text) = prepend {
                PatchRequest::Prepend(text)
            } else {
                anyhow::bail!("patch requires --replace, --append, or --prepend");
            };
            Ok(Request::Patch { name, op })
        }
        Commands::Delete { name } => Ok(Request::Delete { name }),
        Commands::Rename { old, new } => Ok(Request::Rename { old, new }),
        Commands::Ls { sort } => {
            let sort = match sort.to_lowercase().as_str() {
                "name" => SortField::Name,
                "weight" => SortField::Weight,
                "date" => SortField::Date,
                other => anyhow::bail!("unknown sort field: {} (use name, weight, or date)", other),
            };
            Ok(Request::Ls { sort })
        }
        Commands::Inspect { node, format, threshold, cap } => {
            let format = match format.to_lowercase().as_str() {
                "human" => OutputFormat::Human,
                "json" => OutputFormat::Json,
                other => anyhow::bail!("unknown format: {} (use human or json)", other),
            };
            Ok(Request::Inspect { node, format, threshold, cap: Some(cap) })
        }
        Commands::Recall {
            query,
            top_k,
            depth,
            name,
        } => Ok(Request::Recall {
            query,
            name_prefix: name,
            top_k,
            depth,
        }),
        Commands::Search { query, top_k } => Ok(Request::Search { query, top_k }),
        Commands::MultiSearch { queries, top_k } => Ok(Request::MultiSearch { queries, top_k }),
        Commands::MultiRecall {
            queries,
            top_k,
            depth,
        } => Ok(Request::MultiRecall {
            queries,
            top_k,
            depth,
        }),
        Commands::Neighbors {
            name,
            depth,
            limit,
            offset,
        } => Ok(Request::Neighbors {
            name,
            depth,
            limit,
            offset,
        }),
        Commands::Link { a, b } => Ok(Request::Link { a, b }),
        Commands::Unlink { a, b } => Ok(Request::Unlink { a, b }),
        Commands::Boost { name } => Ok(Request::Boost { name }),
        Commands::Penalize { name } => Ok(Request::Penalize { name }),
        Commands::Pin { name } => Ok(Request::Pin { name }),
        Commands::Unpin { name } => Ok(Request::Unpin { name }),
        Commands::Status => Ok(Request::Status),
        Commands::Reindex => Ok(Request::Reindex),
        Commands::Gc => Ok(Request::Gc),
        Commands::Baseline => Ok(Request::Baseline),
        Commands::Stop => Ok(Request::Stop),
        Commands::Init { .. } | Commands::Daemon => unreachable!(),
    }
}

/// Read content from a file or stdin
fn read_content(file: Option<String>) -> anyhow::Result<String> {
    match file {
        Some(path) => {
            std::fs::read_to_string(&path)
                .map_err(|e| anyhow::anyhow!("cannot read file '{}': {}", path, e))
        }
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| anyhow::anyhow!("cannot read stdin: {}", e))?;
            Ok(buf)
        }
    }
}

/// Send a request to the daemon, auto-starting it if needed
async fn send_to_daemon(request: Request) -> anyhow::Result<Response> {
    let conn = connect_or_start_daemon().await?;
    conn.send_request(&request).await
}

/// Daemon connection info parsed from .daemon.pid
struct PidInfo {
    pid: u32,
    port: u16,
    nonce: String,
}

/// TCP connection to the daemon with framing
struct DaemonConnection {
    reader: BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: tokio::net::tcp::OwnedWriteHalf,
}

impl DaemonConnection {
    /// Connect and perform handshake
    async fn connect(port: u16, nonce: &str) -> anyhow::Result<Self> {
        let stream = TcpStream::connect(format!("127.0.0.1:{}", port)).await?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        // Send handshake ping
        let ping = protocol::handshake_ping(nonce);
        write_half.write_all(ping.as_bytes()).await?;
        write_half.flush().await?;

        // Read handshake pong
        let mut pong_line = String::new();
        reader.read_line(&mut pong_line).await?;

        match protocol::parse_pong(&pong_line) {
            Some(_version) => {}
            None => anyhow::bail!("invalid handshake response from daemon"),
        }

        Ok(Self {
            reader,
            writer: write_half,
        })
    }

    /// Send a request and receive the response
    async fn send_request(mut self, request: &Request) -> anyhow::Result<Response> {
        // Encode and send request
        let req_bytes = protocol::encode_message(request)
            .map_err(|e| anyhow::anyhow!("serialize error: {}", e))?;
        self.writer.write_all(&req_bytes).await?;
        self.writer.flush().await?;

        // Read response length
        let mut len_buf = [0u8; 4];
        self.reader.read_exact(&mut len_buf).await?;
        let msg_len = protocol::read_length(&len_buf) as usize;

        if msg_len > 10 * 1024 * 1024 {
            anyhow::bail!("response too large: {} bytes", msg_len);
        }

        // Read response body
        let mut msg_buf = vec![0u8; msg_len];
        self.reader.read_exact(&mut msg_buf).await?;

        let response: Response = protocol::decode_message(&msg_buf)
            .map_err(|e| anyhow::anyhow!("deserialize error: {}", e))?;

        Ok(response)
    }
}

/// Parse .daemon.pid file
fn read_pid_file() -> Option<PidInfo> {
    let path = config::memcore_dir().join(".daemon.pid");
    let content = std::fs::read_to_string(path).ok()?;

    let mut pid = None;
    let mut port = None;
    let mut nonce = None;

    for line in content.lines() {
        if let Some(val) = line.strip_prefix("PID=") {
            pid = val.parse().ok();
        } else if let Some(val) = line.strip_prefix("PORT=") {
            port = val.parse().ok();
        } else if let Some(val) = line.strip_prefix("NONCE=") {
            nonce = Some(val.to_string());
        }
    }

    Some(PidInfo {
        pid: pid?,
        port: port?,
        nonce: nonce?,
    })
}

/// Check if a process is still alive via /proc on Linux
fn process_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{}", pid)).exists()
}

/// Connect to existing daemon, or start a new one
async fn connect_or_start_daemon() -> anyhow::Result<DaemonConnection> {
    // Try connecting to existing daemon
    if let Some(info) = read_pid_file() {
        if process_alive(info.pid) {
            match DaemonConnection::connect(info.port, &info.nonce).await {
                Ok(conn) => return Ok(conn),
                Err(_) => {
                    // Stale pid file or connection refused, fall through to start new daemon
                }
            }
        }
    }

    // Start a new daemon
    start_daemon().await?;

    // Wait for daemon to be ready (pid file to appear)
    let max_wait = std::time::Duration::from_secs(10);
    let start = std::time::Instant::now();
    let poll_interval = std::time::Duration::from_millis(50);

    loop {
        if start.elapsed() > max_wait {
            anyhow::bail!("daemon failed to start within 10 seconds");
        }

        if let Some(info) = read_pid_file() {
            if process_alive(info.pid) {
                match DaemonConnection::connect(info.port, &info.nonce).await {
                    Ok(conn) => return Ok(conn),
                    Err(_) => {
                        // Not ready yet, keep waiting
                    }
                }
            }
        }

        tokio::time::sleep(poll_interval).await;
    }
}

/// Start daemon as a background process
async fn start_daemon() -> anyhow::Result<()> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("cannot determine executable path: {}", e))?;

    // Spawn the daemon as a detached child process
    let child = std::process::Command::new(exe)
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to start daemon: {}", e))?;

    // We don't wait for the child — it runs in the background
    // The PID file will be written by the daemon when it's ready
    drop(child);

    Ok(())
}

/// Initialize memcore directory structure
async fn init(dir: Option<String>) -> anyhow::Result<()> {
    let base = dir
        .map(PathBuf::from)
        .unwrap_or_else(|| config::memcore_dir());

    std::fs::create_dir_all(base.join("memories"))?;
    std::fs::create_dir_all(base.join("index"))?;
    std::fs::create_dir_all(base.join("models"))?;

    // Write default config if not exists
    let config_path = base.join("memcore.toml");
    if !config_path.exists() {
        let default_config = config::Config::default();
        let toml_str = toml::to_string_pretty(&default_config)?;
        std::fs::write(&config_path, toml_str)?;
    }

    println!("initialized memcore at {}", base.display());
    Ok(())
}

// ============================================================
// Response formatting
// ============================================================

/// Format and print a response to stdout/stderr
fn format_response(response: &Response, quiet: bool) {
    match &response.body {
        ResponseBody::Message(msg) => {
            if !quiet {
                println!("{}", msg);
            }
        }

        ResponseBody::NodeContent { name: _, content } => {
            print!("{}", content);
        }

        ResponseBody::NodeBatch(entries) => {
            for (i, entry) in entries.iter().enumerate() {
                if i > 0 {
                    println!("\n=== {} ===\n", entry.name);
                }
                print!("{}", entry.content);
            }
        }

        ResponseBody::NodeList(entries) => {
            if entries.is_empty() {
                println!("(no nodes)");
                return;
            }
            // Calculate column widths, capped for readability
            let max_name = entries.iter().map(|e| e.name.len()).max().unwrap_or(4);
            let name_width = max_name.max(4).min(NAME_DISPLAY_CAP);

            println!(
                "{:<width$}  {:>6}  {:>5}  {:>3}  {}",
                "NAME",
                "WEIGHT",
                "EDGES",
                "PIN",
                "ACCESSED",
                width = name_width
            );
            for entry in entries {
                let display_name = truncate_name(&entry.name, NAME_DISPLAY_CAP);
                let pin_mark = if entry.pinned { "*" } else { "" };
                println!(
                    "{:<width$}  {:>6.2}  {:>5}  {:>3}  {}",
                    display_name,
                    entry.weight,
                    entry.edge_count,
                    pin_mark,
                    entry.last_accessed,
                    width = name_width
                );
            }
        }

        ResponseBody::SearchResults(results) => {
            if results.is_empty() {
                println!("(no results)");
                return;
            }
            let max_name = results.iter().map(|r| r.node_name.len()).max().unwrap_or(4);
            let name_width = max_name.max(4).min(NAME_DISPLAY_CAP);
            for result in results {
                let display_name = truncate_name(&result.node_name, NAME_DISPLAY_CAP);
                println!(
                    "{:.4}  {:<width$}  w={:.2}  {}",
                    result.score,
                    display_name,
                    result.weight,
                    truncate_str(&result.abstract_text, 60),
                    width = name_width
                );
            }
        }

        ResponseBody::NodeNames(names) => {
            if names.is_empty() {
                println!("(no results)");
                return;
            }
            for name in names {
                println!("{}", name);
            }
        }

        ResponseBody::Neighbors {
            entries,
            total,
            depth,
        } => {
            if entries.is_empty() {
                println!("(no neighbors)");
                return;
            }
            let max_name = entries.iter().map(|e| e.name.len()).max().unwrap_or(4);
            let name_width = max_name.max(4).min(NAME_DISPLAY_CAP);
            println!("depth={}, showing {}/{}", depth, entries.len(), total);
            for entry in entries {
                let display_name = truncate_name(&entry.name, NAME_DISPLAY_CAP);
                println!(
                    "  d={}  {:<width$}  w={:.2}",
                    entry.depth, display_name, entry.weight, width = name_width
                );
            }
        }

        ResponseBody::NodeInspectReport(data) => {
            println!(
                "inspect: {} ({} edges)",
                data.name, data.edge_count
            );
            println!("  weight:  {:.2}", data.weight);
            println!("  pinned:  {}", data.pinned);
            if !data.abstract_text.is_empty() {
                println!("  abstract: {}", truncate_str(&data.abstract_text, 80));
            }
            if !data.links.is_empty() {
                println!("  links:   {}", data.links.join(", "));
            }
            if !data.similar_nodes.is_empty() {
                println!("\n  similar nodes:");
                let max_name = data
                    .similar_nodes
                    .iter()
                    .map(|s| s.name.len())
                    .max()
                    .unwrap_or(4);
                for s in &data.similar_nodes {
                    println!("    {:<width$}  {:.2}", s.name, s.similarity, width = max_name);
                }
            }
            for w in &data.warnings {
                eprintln!("warning: {}", w);
            }
        }

        ResponseBody::InspectReport(data) => {
            println!("=== System Health ===");
            println!("health:     {:.0}%", data.health_score * 100.0);
            println!("nodes:      {}", data.node_count);
            println!("edges:      {}", data.edge_count);
            println!("clusters:   {}", data.cluster_count);
            println!("orphans:    {} ({:.0}%)", data.orphan_count, data.orphan_ratio * 100.0);
            println!("graveyard:  {:.0}%", data.graveyard_ratio * 100.0);
            println!("density:    {:.2}", data.density);
            if let Some(r) = data.redundancy {
                println!("redundancy: {:.2}", r);
            }
            if !data.similar_pairs.is_empty() {
                let shown = data.similar_pairs.len();
                let total = data.total_similar_pairs;
                if total > shown {
                    println!("\nSimilar pairs (showing {}/{}):", shown, total);
                } else {
                    println!("\nSimilar pairs ({} found):", shown);
                }
                let max_a = data.similar_pairs.iter().map(|p| p.node_a.len()).max().unwrap_or(4).min(NAME_DISPLAY_CAP);
                let max_b = data.similar_pairs.iter().map(|p| p.node_b.len()).max().unwrap_or(4).min(NAME_DISPLAY_CAP);
                for p in &data.similar_pairs {
                    let name_a = truncate_name(&p.node_a, NAME_DISPLAY_CAP);
                    let name_b = truncate_name(&p.node_b, NAME_DISPLAY_CAP);
                    println!(
                        "  {:<wa$}  \u{2194}  {:<wb$}  {:.2}",
                        name_a,
                        name_b,
                        p.similarity,
                        wa = max_a,
                        wb = max_b,
                    );
                }
                let not_shown = total.saturating_sub(shown);
                if not_shown > 0 {
                    println!("  ({} more pairs not shown)", not_shown);
                }
            }
            if !data.orphans.is_empty() {
                println!("\nOrphans: {}", data.orphans.join(", "));
            }
            if !data.low_weight.is_empty() {
                println!("Low weight: {}", data.low_weight.join(", "));
            }
        }

        ResponseBody::Status(status) => {
            println!("pid:        {}", status.pid);
            println!("port:       {}", status.port);
            println!("nodes:      {}", status.node_count);
            println!("edges:      {}", status.edge_count);
            println!("indexed:    {}", status.index_count);
            println!("uptime:     {}", format_duration(status.uptime_seconds));
        }

        ResponseBody::Error(msg) => {
            eprintln!("error: {}", msg);
        }

        ResponseBody::Empty => {}
    }
}

/// Max display width for node names in tabular output (ls, search, neighbors, inspect pairs).
const NAME_DISPLAY_CAP: usize = 50;

fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        return s;
    }
    let mut end = max_len;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Truncate a name to max_len chars, adding "..." suffix if truncated.
fn truncate_name(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    let t = truncate_str(s, max_len.saturating_sub(3));
    format!("{}...", t)
}

fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_str_ascii() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello", 5), "hello");
        assert_eq!(truncate_str("hello", 3), "hel");
        assert_eq!(truncate_str("hello", 0), "");
    }

    #[test]
    fn test_truncate_str_utf8_chinese() {
        // Each Chinese character is 3 bytes in UTF-8
        let s = "你好世界"; // 12 bytes total
        assert_eq!(truncate_str(s, 12), "你好世界");
        assert_eq!(truncate_str(s, 6), "你好");
        assert_eq!(truncate_str(s, 3), "你");
        // Cutting in the middle of a character should back up
        assert_eq!(truncate_str(s, 4), "你");
        assert_eq!(truncate_str(s, 5), "你");
        assert_eq!(truncate_str(s, 7), "你好");
    }

    #[test]
    fn test_truncate_str_mixed() {
        let s = "a你b好"; // 1 + 3 + 1 + 3 = 8 bytes
        assert_eq!(truncate_str(s, 8), "a你b好");
        assert_eq!(truncate_str(s, 4), "a你");
        assert_eq!(truncate_str(s, 1), "a");
        assert_eq!(truncate_str(s, 5), "a你b");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(59), "59s");
        assert_eq!(format_duration(60), "1m 0s");
        assert_eq!(format_duration(3661), "1h 1m");
    }
}
