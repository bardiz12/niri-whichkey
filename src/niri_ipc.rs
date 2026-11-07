use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
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
                    // 1. Initial query to get currently focused output
                    if let Ok(mut query_stream) = UnixStream::connect(&sock_path) {
                        let req = "{\"FocusedOutput\": null}\n";
                        if query_stream.write_all(req.as_bytes()).is_ok()
                            && query_stream.flush().is_ok()
                        {
                            let mut line = String::new();
                            let mut reader = BufReader::new(query_stream);
                            if reader.read_line(&mut line).is_ok() {
                                if let Ok(val) = serde_json::from_str::<Value>(&line) {
                                    if let Some(name) = val
                                        .get("Ok")
                                        .and_then(|ok| ok.get("FocusedOutput"))
                                        .and_then(|fo| fo.get("name"))
                                        .and_then(|n| n.as_str())
                                    {
                                        println!(
                                            "[niri-whichkey] Initially focused output: {}",
                                            name
                                        );
                                        let _ = self
                                            .tx
                                            .send(GuiMessage::FocusedOutput(name.to_string()));
                                    }
                                }
                            }
                        }
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
                                            // Handle workspace & output focus changes
                                            if let Some(workspaces) = val
                                                .get("WorkspacesChanged")
                                                .and_then(|w| w.get("workspaces"))
                                                .and_then(|w| w.as_array())
                                            {
                                                for ws in workspaces {
                                                    let is_focused = ws
                                                        .get("is_focused")
                                                        .and_then(|f| f.as_bool())
                                                        .unwrap_or(false);
                                                    if is_focused {
                                                        if let Some(output) = ws
                                                            .get("output")
                                                            .and_then(|o| o.as_str())
                                                        {
                                                            let _ = self.tx.send(
                                                                GuiMessage::FocusedOutput(
                                                                    output.to_string(),
                                                                ),
                                                            );
                                                        }
                                                    }
                                                }
                                            } else if val.get("ConfigLoaded").is_some() {
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
