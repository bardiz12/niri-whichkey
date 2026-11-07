use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use calloop::channel::Sender;
use serde_json::Value;

use crate::input::GuiMessage;

pub struct NiriIpcWatcher {
    tx: Sender<GuiMessage>,
}

impl NiriIpcWatcher {
    pub fn new(tx: Sender<GuiMessage>) -> Self {
        Self { tx }
    }

    pub fn start(self) {
        thread::spawn(move || {
            loop {
                if let Some(sock_path) = find_niri_socket() {
                    let mut workspace_outputs: HashMap<u64, String> = HashMap::new();
                    let mut current_focused: Option<String> = None;

                    // 1. Initial query to get currently focused output
                    if let Some(name) = query_focused_output(&sock_path) {
                        println!("[niri-whichkey] Initially focused output: {}", name);
                        current_focused = Some(name.clone());
                        let _ = self.tx.send(GuiMessage::FocusedOutput(name));
                    }

                    // 2. Start continuous EventStream
                    if let Ok(mut stream) = UnixStream::connect(&sock_path) {
                        println!(
                            "[niri-whichkey] Connected to Niri IPC socket: {}",
                            sock_path.display()
                        );
                        let req = "{\"EventStream\": null}\n";
                        if stream.write_all(req.as_bytes()).is_ok() && stream.flush().is_ok() {
                            let reader = BufReader::new(stream);
                            for line in reader.lines() {
                                match line {
                                    Ok(l) => {
                                        if let Ok(val) = serde_json::from_str::<Value>(&l) {
                                            // 1. WorkspacesChanged: update workspace_id -> output mapping
                                            if let Some(workspaces) = val
                                                .get("WorkspacesChanged")
                                                .and_then(|w| w.get("workspaces"))
                                                .and_then(|w| w.as_array())
                                            {
                                                for ws in workspaces {
                                                    if let (Some(id), Some(output)) = (
                                                        ws.get("id").and_then(|i| i.as_u64()),
                                                        ws.get("output").and_then(|o| o.as_str()),
                                                    ) {
                                                        workspace_outputs
                                                            .insert(id, output.to_string());
                                                        let is_focused = ws
                                                            .get("is_focused")
                                                            .and_then(|f| f.as_bool())
                                                            .unwrap_or(false);
                                                        if is_focused
                                                            && current_focused.as_deref()
                                                                != Some(output)
                                                        {
                                                            current_focused =
                                                                Some(output.to_string());
                                                            let _ = self.tx.send(
                                                                GuiMessage::FocusedOutput(
                                                                    output.to_string(),
                                                                ),
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                            // 2. WorkspaceActivated: focus moved to another workspace/monitor
                                            else if let Some(activated) =
                                                val.get("WorkspaceActivated")
                                            {
                                                let is_focused = activated
                                                    .get("focused")
                                                    .and_then(|f| f.as_bool())
                                                    .unwrap_or(false);
                                                if is_focused {
                                                    if let Some(ws_id) =
                                                        activated.get("id").and_then(|i| i.as_u64())
                                                    {
                                                        let target_output = workspace_outputs
                                                            .get(&ws_id)
                                                            .cloned()
                                                            .or_else(|| {
                                                                query_focused_output(&sock_path)
                                                            });
                                                        if let Some(out) = target_output {
                                                            if current_focused.as_deref()
                                                                != Some(&out)
                                                            {
                                                                current_focused = Some(out.clone());
                                                                let _ = self.tx.send(
                                                                    GuiMessage::FocusedOutput(out),
                                                                );
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            // 3. WindowFocusChanged: window focus changed across monitors
                                            else if val.get("WindowFocusChanged").is_some() {
                                                if let Some(out) = query_focused_output(&sock_path)
                                                {
                                                    if current_focused.as_deref() != Some(&out) {
                                                        current_focused = Some(out.clone());
                                                        let _ = self
                                                            .tx
                                                            .send(GuiMessage::FocusedOutput(out));
                                                    }
                                                }
                                            }
                                            // 4. ConfigLoaded: Niri config reloaded
                                            else if val.get("ConfigLoaded").is_some() {
                                                println!("[niri-whichkey] Niri config reload detected. Refreshing keybindings...");
                                                let _ = self.tx.send(GuiMessage::ReloadConfig);
                                            }
                                        }
                                    }
                                    Err(_) => break, // Socket disconnected
                                }
                            }
                        }
                    }
                }
                thread::sleep(Duration::from_secs(2));
            }
        });
    }
}

fn query_focused_output(sock_path: &Path) -> Option<String> {
    let mut stream = UnixStream::connect(sock_path).ok()?;
    let req = "{\"FocusedOutput\": null}\n";
    stream.write_all(req.as_bytes()).ok()?;
    stream.flush().ok()?;
    let mut line = String::new();
    let mut reader = BufReader::new(stream);
    reader.read_line(&mut line).ok()?;
    let val: Value = serde_json::from_str(&line).ok()?;
    val.get("Ok")
        .and_then(|ok| ok.get("FocusedOutput"))
        .and_then(|fo| fo.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
}

fn find_niri_socket() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("NIRI_SOCKET") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }

    // Check runtime dir
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let uid = nix::unistd::getuid();
            PathBuf::from(format!("/run/user/{}", uid))
        });

    if let Ok(entries) = std::fs::read_dir(&runtime_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("niri.") && name.ends_with(".sock") {
                return Some(entry.path());
            }
        }
    }

    None
}
