//! IPC for `checkpoint_ask`: the MCP sidecar (a child of a `claude -p` stage)
//! asks the running TUI to render a phase-transition question and blocks for the
//! user's choice. Transport is a single newline-delimited JSON request/response
//! over a per-workspace Unix domain socket at `<root>/.mcp.sock`.
//!
//! This module holds the shared protocol + the client (`ask`). The server side
//! (a listener thread that surfaces the question in the TUI) is wired into the
//! TUI separately. Unix-only.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Per-workspace socket the TUI binds and the sidecar connects to. The sidecar
/// runs with cwd at the workspace root, so it resolves the same path.
pub fn socket_path(root: &Path) -> PathBuf {
    root.join(".mcp.sock")
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CheckpointRequest {
    /// The phase-transition question.
    pub question: String,
    /// The agent's self-assessment / why it is asking now (shown to the user).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assessment: Option<String>,
    /// Options to choose from; the recommended one first.
    pub options: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CheckpointResponse {
    /// Index into `CheckpointRequest::options`.
    pub chosen: usize,
    /// The chosen option's label (echoed for convenience).
    pub label: String,
}

/// Client side (sidecar): connect, send the request, block for the response.
/// Errors if no TUI is listening on the socket.
pub fn ask(root: &Path, req: &CheckpointRequest) -> io::Result<CheckpointResponse> {
    let stream = UnixStream::connect(socket_path(root))?;
    let mut writer = stream.try_clone()?;
    let mut line = serde_json::to_string(req).map_err(invalid_data)?;
    line.push('\n');
    writer.write_all(line.as_bytes())?;
    writer.flush()?;

    let mut reader = BufReader::new(stream);
    let mut resp = String::new();
    reader.read_line(&mut resp)?;
    if resp.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "no checkpoint response",
        ));
    }
    serde_json::from_str(resp.trim()).map_err(invalid_data)
}

fn invalid_data<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::thread;

    #[test]
    fn ask_round_trips_over_socket() {
        let dir = std::env::temp_dir().join(format!("ros-ipc-{}", crate::ledger::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = socket_path(&dir);
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();

        // Stand-in server: read the request, choose option index 1, reply.
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let req: CheckpointRequest = serde_json::from_str(line.trim()).unwrap();
            let resp = CheckpointResponse {
                chosen: 1,
                label: req.options[1].clone(),
            };
            let mut w = stream;
            let mut out = serde_json::to_string(&resp).unwrap();
            out.push('\n');
            w.write_all(out.as_bytes()).unwrap();
            w.flush().unwrap();
        });

        let req = CheckpointRequest {
            question: "Advance to EXPERIMENT?".to_string(),
            assessment: Some("a hypothesis crystallized".to_string()),
            options: vec!["stay".to_string(), "advance".to_string()],
        };
        let r = ask(&dir, &req).unwrap();
        assert_eq!(r.chosen, 1);
        assert_eq!(r.label, "advance");
        server.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
