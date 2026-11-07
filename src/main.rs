use std::path::PathBuf;
use std::process::exit;

mod config;
mod gui;
mod input;
mod niri_ipc;
mod parser;

use config::{expand_tilde, AppConfig};
use gui::run_gui;
use input::InputWatcher;
use niri_ipc::NiriIpcWatcher;

fn print_help() {
    println!("niri-whichkey - Modifier hint overlay for Niri compositor");
    println!();
    println!("USAGE:");
    println!("    niri-whichkey [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    -c, --config <PATH>       Path to YAML configuration file");
    println!("        --niri-config <PATH>  Path to niri config.kdl (default: ~/.config/niri/config.kdl)");
    println!(
        "        --threshold <MS>      Delay in milliseconds before showing overlay (default: 200)"
    );
    println!("        --margin-top <PX>     Margin from top screen edge in pixels (default: 24)");
    println!("        --margin-right <PX>   Margin from right screen edge in pixels (default: 24)");
    println!("        --font-size <PT>      Base font size in points (default: 13.0)");
    println!("        --columns <NUM>       Maximum columns for key list (1 or 2, default: 2)");
    println!("    -h, --help                Print this help message");
    println!("    -v, --version             Print version information");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // 1. First pass: find help, version, or custom config file path
    let mut custom_config_path: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_help();
                exit(0);
            }
            "-v" | "--version" => {
                println!("niri-whichkey {}", env!("CARGO_PKG_VERSION"));
                exit(0);
            }
            "-c" | "--config" if i + 1 < args.len() => {
                custom_config_path = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }

    // 2. Load configuration from YAML (or defaults)
    let (mut config, _) = AppConfig::load(custom_config_path.as_deref());

    // 3. Second pass: command-line overrides
    let mut j = 0;
    while j < args.len() {
        match args[j].as_str() {
            "-c" | "--config" => {
                j += 1; // Handled in first pass
            }
            "--niri-config" => {
                if j + 1 < args.len() {
                    config.niri_config_path = expand_tilde(&args[j + 1]);
                    j += 1;
                }
            }
            "--threshold" => {
                if j + 1 < args.len() {
                    if let Ok(val) = args[j + 1].parse() {
                        config.threshold_ms = val;
                    }
                    j += 1;
                }
            }
            "--margin-top" => {
                if j + 1 < args.len() {
                    if let Ok(val) = args[j + 1].parse() {
                        config.margin_top = val;
                    }
                    j += 1;
                }
            }
            "--margin-right" => {
                if j + 1 < args.len() {
                    if let Ok(val) = args[j + 1].parse() {
                        config.margin_right = val;
                    }
                    j += 1;
                }
            }
            "--font-size" => {
                if j + 1 < args.len() {
                    if let Ok(val) = args[j + 1].parse() {
                        config.font_size = val;
                    }
                    j += 1;
                }
            }
            "--columns" => {
                if j + 1 < args.len() {
                    if let Ok(val) = args[j + 1].parse() {
                        config.max_columns = val;
                    }
                    j += 1;
                }
            }
            unknown => {
                eprintln!("Unknown argument: {}", unknown);
                print_help();
                exit(1);
            }
        }
        j += 1;
    }

    println!(
        "[niri-whichkey] Starting niri-whichkey v{} (threshold: {}ms, margins: [top: {}, right: {}], font: {}pt, niri_config: {})",
        env!("CARGO_PKG_VERSION"),
        config.threshold_ms,
        config.margin_top,
        config.margin_right,
        config.font_size,
        config.niri_config_path.display()
    );

    let (tx, channel_source) = calloop::channel::channel();

    // Start evdev input watcher (with self-healing suspend/resume handling)
    let input_watcher = InputWatcher::new(config.threshold_ms, tx.clone());
    input_watcher.start();

    // Start niri IPC watcher (tracks focused output and config reloads)
    let ipc_watcher = NiriIpcWatcher::new(tx);
    ipc_watcher.start();

    // Run Wayland GUI loop on main thread
    run_gui(config, channel_source);
}
