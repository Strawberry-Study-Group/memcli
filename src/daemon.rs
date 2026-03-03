use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

use crate::config;
use crate::daemon_state::{self, DaemonState};
use crate::handler;
use crate::protocol::{self, Request, ResponseBody};

/// The daemon process
pub struct Daemon {
    pub state: Arc<Mutex<DaemonState>>,
    pub nonce: String,
    pub start_time: std::time::Instant,
}

/// Main daemon entry point
pub async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let base_dir = config::memcore_dir();

    // Ensure base dir exists
    std::fs::create_dir_all(&base_dir)?;

    // Load state from disk (includes WAL recovery + consistency check)
    let state = daemon_state::load_state_from_dir(&base_dir)?;
    let bind_host = state.config.daemon.bind_host.clone();
    let port = state.config.daemon.port;
    let idle_timeout_mins = state.config.daemon.idle_timeout_minutes;

    let nonce = generate_nonce();

    let daemon = Arc::new(Daemon {
        state: Arc::new(Mutex::new(state)),
        nonce: nonce.clone(),
        start_time: std::time::Instant::now(),
    });

    // Save graph.idx after loading
    {
        let state = daemon.state.lock().await;
        let _ = daemon_state::save_graph_idx(&base_dir, &state.graph);
    }

    let addr = format!("{}:{}", bind_host, port);
    tracing::info!("daemon starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let local_port = listener.local_addr()?.port();

    // Write pid file
    write_pid_file(local_port, &nonce)?;

    tracing::info!("daemon listening on {}:{}", bind_host, local_port);

    let last_activity = Arc::new(Mutex::new(std::time::Instant::now()));

    // Idle timeout task
    let idle_last = Arc::clone(&last_activity);
    tokio::spawn(async move {
        let timeout = std::time::Duration::from_secs(idle_timeout_mins * 60);
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            let last = *idle_last.lock().await;
            if last.elapsed() > timeout {
                tracing::info!("idle timeout reached, shutting down");
                cleanup_pid_file();
                std::process::exit(0);
            }
        }
    });

    // Periodic access metadata flush (every 60s)
    let flush_state = Arc::clone(&daemon.state);
    let flush_dir = base_dir.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            let mut state = flush_state.lock().await;
            daemon_state::flush_access_metadata(&mut state, &flush_dir);
        }
    });

    // Accept loop
    loop {
        let (stream, _addr) = listener.accept().await?;
        let daemon = Arc::clone(&daemon);
        let base_dir = base_dir.clone();
        let last_activity = Arc::clone(&last_activity);

        tokio::spawn(async move {
            *last_activity.lock().await = std::time::Instant::now();
            if let Err(e) = handle_connection(stream, daemon, &base_dir).await {
                tracing::error!("connection error: {}", e);
            }
        });
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    daemon: Arc<Daemon>,
    base_dir: &std::path::Path,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    // Step 1: Handshake
    let mut ping_line = String::new();
    reader.read_line(&mut ping_line).await?;

    let client_nonce = match protocol::parse_ping(&ping_line) {
        Some(n) => n.to_string(),
        None => {
            anyhow::bail!("invalid handshake: {}", ping_line.trim());
        }
    };

    if client_nonce != daemon.nonce {
        anyhow::bail!("nonce mismatch");
    }

    let pong = protocol::handshake_pong(env!("CARGO_PKG_VERSION"));
    writer.write_all(pong.as_bytes()).await?;
    writer.flush().await?;

    // Step 2: Read request (length-prefixed JSON)
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let msg_len = protocol::read_length(&len_buf) as usize;

    if msg_len > 10 * 1024 * 1024 {
        // 10MB limit
        anyhow::bail!("request too large: {} bytes", msg_len);
    }

    let mut msg_buf = vec![0u8; msg_len];
    reader.read_exact(&mut msg_buf).await?;

    let request: Request = protocol::decode_message(&msg_buf)?;

    // Step 3: Check for stop command
    let is_stop = matches!(&request, Request::Stop);

    // Step 4: Process request
    let response = {
        let mut state = daemon.state.lock().await;

        // Inject uptime for status requests
        let resp = handler::handle_request(&mut state, &request, base_dir);
        match &resp.body {
            ResponseBody::Status(_s) => {
                // Re-create with actual uptime
                let uptime = daemon.start_time.elapsed().as_secs();
                let mut state_resp = handler::handle_request(&mut state, &request, base_dir);
                if let ResponseBody::Status(ref mut s) = state_resp.body {
                    s.uptime_seconds = uptime;
                }
                state_resp
            }
            _ => resp,
        }
    };

    // Step 5: Send response (length-prefixed JSON)
    let resp_bytes = protocol::encode_message(&response)?;
    writer.write_all(&resp_bytes).await?;
    writer.flush().await?;

    // Step 6: Handle stop — flush dirty access metadata before exit
    if is_stop {
        tracing::info!("stop command received, flushing and shutting down");
        {
            let mut state = daemon.state.lock().await;
            daemon_state::flush_access_metadata(&mut state, base_dir);
        }
        cleanup_pid_file();
        // Give the response time to be sent
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        std::process::exit(0);
    }

    Ok(())
}

fn generate_nonce() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:x}", t)
}

fn write_pid_file(port: u16, nonce: &str) -> std::io::Result<()> {
    let pid = std::process::id();
    let content = format!("PID={}\nPORT={}\nNONCE={}\n", pid, port, nonce);
    let path = config::memcore_dir().join(".daemon.pid");
    std::fs::write(path, content)
}

fn cleanup_pid_file() {
    let path = config::memcore_dir().join(".daemon.pid");
    let _ = std::fs::remove_file(path);
}
