use std::collections::BTreeSet;
use std::os::fd::AsFd;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use calloop::channel::Sender;
use evdev::{Device, KeyCode};
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};

#[derive(Debug, Clone)]
pub enum GuiMessage {
    Show(BTreeSet<String>),
    Hide,
    ReloadConfig,
    FocusedOutput(String),
}

struct InputState {
    held_modifiers: BTreeSet<String>,
    overlay_visible: bool,
    generation: u64,
}

pub struct InputWatcher {
    threshold_ms: u64,
    tx: Sender<GuiMessage>,
}

struct ActiveKeyboard {
    path: PathBuf,
    device: Device,
}

fn scan_keyboards() -> Vec<ActiveKeyboard> {
    let mut keyboards = Vec::new();
    for (path, device) in evdev::enumerate() {
        let is_keyboard = device.supported_keys().is_some_and(|keys| {
            keys.contains(KeyCode::KEY_ENTER) && keys.contains(KeyCode::KEY_SPACE)
        });
        if is_keyboard {
            keyboards.push(ActiveKeyboard { path, device });
        }
    }
    keyboards
}

impl InputWatcher {
    pub fn new(threshold_ms: u64, tx: Sender<GuiMessage>) -> Self {
        Self { threshold_ms, tx }
    }

    pub fn start(self) {
        thread::spawn(move || {
            self.run_loop();
        });
    }

    fn run_loop(&self) {
        let state = Arc::new(Mutex::new(InputState {
            held_modifiers: BTreeSet::new(),
            overlay_visible: false,
            generation: 0,
        }));

        let mut keyboards = scan_keyboards();
        if keyboards.is_empty() {
            eprintln!("[niri-whichkey] No keyboard devices found or permission denied on /dev/input/event*.");
            eprintln!("[niri-whichkey] [HINT] Please ensure your user is in the 'input' group:");
            eprintln!("               sudo usermod -aG input $USER");
            eprintln!("               and log out / log back in (or run 'newgrp input').");
        } else {
            for kb in &keyboards {
                println!(
                    "[niri-whichkey] Monitoring keyboard: {:?} ({})",
                    kb.device.name(),
                    kb.path.display()
                );
            }
        }

        loop {
            // If no keyboards are currently available, wait and retry scanning
            if keyboards.is_empty() {
                thread::sleep(Duration::from_millis(1500));
                keyboards = scan_keyboards();
                if !keyboards.is_empty() {
                    println!(
                        "[niri-whichkey] Discovered {} keyboard(s).",
                        keyboards.len()
                    );
                    for kb in &keyboards {
                        println!(
                            "[niri-whichkey] Monitoring keyboard: {:?} ({})",
                            kb.device.name(),
                            kb.path.display()
                        );
                    }
                }
                continue;
            }

            let mut poll_fds: Vec<PollFd> = keyboards
                .iter()
                .map(|kb| PollFd::new(kb.device.as_fd(), PollFlags::POLLIN))
                .collect();

            // 1000ms timeout allows recovery from sleep/suspend without blocking forever
            let poll_res = poll(&mut poll_fds, PollTimeout::from(1000u16));
            if let Err(e) = poll_res {
                if e != nix::errno::Errno::EINTR {
                    thread::sleep(Duration::from_millis(100));
                }
                continue;
            }

            let mut dead_device_detected = false;

            let events_to_process: Vec<(usize, PollFlags)> = poll_fds
                .iter()
                .enumerate()
                .filter_map(|(idx, pfd)| pfd.revents().map(|r| (idx, r)))
                .collect();
            drop(poll_fds);

            for (idx, revents) in events_to_process {
                // Check for disconnection / error flags
                if revents.intersects(PollFlags::POLLHUP | PollFlags::POLLERR | PollFlags::POLLNVAL)
                {
                    dead_device_detected = true;
                    continue;
                }

                if revents.contains(PollFlags::POLLIN) {
                    let kb = &mut keyboards[idx];
                    match kb.device.fetch_events() {
                        Ok(events) => {
                            for ev in events {
                                if ev.event_type() == evdev::EventType::KEY {
                                    let key = KeyCode::new(ev.code());
                                    let val = ev.value(); // 0 = release, 1 = press, 2 = repeat

                                    if val == 2 {
                                        continue; // Ignore repeat
                                    }

                                    let mod_name = match key {
                                        KeyCode::KEY_LEFTMETA | KeyCode::KEY_RIGHTMETA => {
                                            Some("Mod")
                                        }
                                        KeyCode::KEY_LEFTALT | KeyCode::KEY_RIGHTALT => Some("Alt"),
                                        KeyCode::KEY_LEFTSHIFT | KeyCode::KEY_RIGHTSHIFT => {
                                            Some("Shift")
                                        }
                                        KeyCode::KEY_LEFTCTRL | KeyCode::KEY_RIGHTCTRL => {
                                            Some("Ctrl")
                                        }
                                        _ => None,
                                    };

                                    let mut s = state.lock().unwrap();

                                    if let Some(m) = mod_name {
                                        if val == 1 {
                                            s.held_modifiers.insert(m.to_string());
                                        } else if val == 0 {
                                            s.held_modifiers.remove(m);
                                        }

                                        let has_mod = s.held_modifiers.contains("Mod");

                                        if !has_mod {
                                            // Mod is not held: overlay MUST NOT be shown
                                            s.generation += 1;
                                            if s.overlay_visible {
                                                s.overlay_visible = false;
                                                let _ = self.tx.send(GuiMessage::Hide);
                                            }
                                        } else if s.overlay_visible {
                                            // Overlay is already visible and Mod is still held:
                                            // update chord immediately
                                            let _ = self
                                                .tx
                                                .send(GuiMessage::Show(s.held_modifiers.clone()));
                                        } else if val == 1 {
                                            // Mod is held, overlay not yet visible, and modifier was pressed:
                                            // start hold timer
                                            s.generation += 1;
                                            let current_gen = s.generation;
                                            let state_clone = Arc::clone(&state);
                                            let tx_clone = self.tx.clone();
                                            let threshold = self.threshold_ms;

                                            thread::spawn(move || {
                                                thread::sleep(Duration::from_millis(threshold));
                                                let mut st = state_clone.lock().unwrap();
                                                if st.generation == current_gen
                                                    && st.held_modifiers.contains("Mod")
                                                {
                                                    st.overlay_visible = true;
                                                    let _ = tx_clone.send(GuiMessage::Show(
                                                        st.held_modifiers.clone(),
                                                    ));
                                                }
                                            });
                                        }
                                    } else if val == 1 {
                                        // Non-modifier key pressed (e.g. shortcut executed or typing)
                                        s.generation += 1;
                                        if s.overlay_visible {
                                            s.overlay_visible = false;
                                            let _ = self.tx.send(GuiMessage::Hide);
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "[niri-whichkey] Error reading events on {}: {}",
                                kb.path.display(),
                                e
                            );
                            dead_device_detected = true;
                        }
                    }
                }
            }

            if dead_device_detected {
                println!(
                    "[niri-whichkey] Input device reset / disconnect detected. Re-scanning keyboards..."
                );
                {
                    let mut s = state.lock().unwrap();
                    s.held_modifiers.clear();
                    s.generation += 1;
                    if s.overlay_visible {
                        s.overlay_visible = false;
                        let _ = self.tx.send(GuiMessage::Hide);
                    }
                }
                keyboards = scan_keyboards();
                for kb in &keyboards {
                    println!(
                        "[niri-whichkey] Active keyboard: {:?} ({})",
                        kb.device.name(),
                        kb.path.display()
                    );
                }
            }
        }
    }
}
