use kdl::KdlDocument;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct KeyBind {
    #[allow(dead_code)]
    pub chord: String,
    pub modifiers: BTreeSet<String>,
    pub key: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct SubChordHint {
    pub next_modifier: String,
    pub count: usize,
    #[allow(dead_code)]
    pub sample_titles: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct KeybindRegistry {
    pub binds: Vec<KeyBind>,
}

impl KeybindRegistry {
    pub fn load_from(path: &Path) -> Self {
        let mut visited = BTreeSet::new();
        let binds = parse_niri_file(path, &mut visited);
        Self { binds }
    }

    pub fn lookup(&self, active_mods: &BTreeSet<String>) -> (Vec<KeyBind>, Vec<SubChordHint>) {
        let mut direct = Vec::new();
        let mut sub_chords_map: BTreeMap<String, Vec<&KeyBind>> = BTreeMap::new();

        for b in &self.binds {
            if &b.modifiers == active_mods {
                direct.push(b.clone());
            } else if active_mods.is_subset(&b.modifiers) && b.modifiers.len() > active_mods.len() {
                // Find the difference (additional modifiers)
                let diff: Vec<String> = b.modifiers.difference(active_mods).cloned().collect();
                if let Some(first_diff) = diff.first() {
                    let sub_key = if diff.len() == 1 {
                        first_diff.clone()
                    } else {
                        diff.join("+")
                    };
                    sub_chords_map.entry(sub_key).or_default().push(b);
                }
            }
        }

        let mut sub_hints = Vec::new();
        for (next_mod, list) in sub_chords_map {
            let samples: Vec<String> = list.iter().take(2).map(|k| k.title.clone()).collect();
            sub_hints.push(SubChordHint {
                next_modifier: next_mod,
                count: list.len(),
                sample_titles: samples,
            });
        }

        let direct = collapse_numeric_ranges(direct);

        (direct, sub_hints)
    }
}

fn get_num_and_base_title(bind: &KeyBind) -> (Option<u32>, String) {
    if let Ok(num) = bind.key.parse::<u32>() {
        let suffix = format!(" {}", num);
        let base_title = if bind.title.ends_with(&suffix) {
            bind.title[..bind.title.len() - suffix.len()]
                .trim()
                .to_string()
        } else {
            bind.title.trim().to_string()
        };
        (Some(num), base_title)
    } else {
        (None, bind.title.clone())
    }
}

fn flush_run(result: &mut Vec<KeyBind>, run: Vec<(u32, KeyBind)>, base_title: &str) {
    if run.len() >= 3 {
        let first = run.first().unwrap();
        let last = run.last().unwrap();
        result.push(KeyBind {
            chord: format!("{}..{}", first.1.chord, last.1.chord),
            modifiers: first.1.modifiers.clone(),
            key: format!("{}..{}", first.0, last.0),
            title: base_title.to_string(),
        });
    } else {
        for (_, b) in run {
            result.push(b);
        }
    }
}

fn collapse_numeric_runs(mut items: Vec<(u32, KeyBind)>, base_title: &str) -> Vec<KeyBind> {
    items.sort_by_key(|(n, _)| *n);
    items.dedup_by_key(|(n, _)| *n);

    let mut result = Vec::new();
    let mut current_run: Vec<(u32, KeyBind)> = Vec::new();

    for (num, bind) in items {
        if let Some((prev_num, _)) = current_run.last() {
            if num == prev_num + 1 {
                current_run.push((num, bind));
                continue;
            } else {
                flush_run(&mut result, std::mem::take(&mut current_run), base_title);
                current_run.push((num, bind));
            }
        } else {
            current_run.push((num, bind));
        }
    }

    if !current_run.is_empty() {
        flush_run(&mut result, current_run, base_title);
    }

    result
}

pub fn collapse_numeric_ranges(binds: Vec<KeyBind>) -> Vec<KeyBind> {
    let mut non_numeric = Vec::new();
    let mut numeric_groups: BTreeMap<String, Vec<(u32, KeyBind)>> = BTreeMap::new();

    for b in binds {
        let (num_opt, base_title) = get_num_and_base_title(&b);
        if let Some(n) = num_opt {
            numeric_groups.entry(base_title).or_default().push((n, b));
        } else {
            non_numeric.push(b);
        }
    }

    let mut collapsed = Vec::new();
    for (base_title, num_list) in numeric_groups {
        collapsed.extend(collapse_numeric_runs(num_list, &base_title));
    }

    collapsed.extend(non_numeric);
    collapsed.sort_by_key(|a| a.key.to_lowercase());
    collapsed
}

pub fn normalize_mod(m: &str) -> String {
    match m.to_lowercase().as_str() {
        "mod" | "super" | "logo" | "win" => "Mod".to_string(),
        "alt" | "meta" => "Alt".to_string(),
        "ctrl" | "control" => "Ctrl".to_string(),
        "shift" => "Shift".to_string(),
        other => other.to_string(),
    }
}

pub fn prettify_key(k: &str) -> String {
    match k.to_lowercase().as_str() {
        // Punctuation & symbols
        "apostrophe" | "singlequote" => "'".to_string(),
        "quotedbl" | "doublequote" => "\"".to_string(),
        "grave" => "`".to_string(),
        "minus" => "-".to_string(),
        "equal" => "=".to_string(),
        "plus" => "+".to_string(),
        "slash" => "/".to_string(),
        "backslash" => "\\".to_string(),
        "bracketleft" => "[".to_string(),
        "bracketright" => "]".to_string(),
        "braceleft" => "{".to_string(),
        "braceright" => "}".to_string(),
        "semicolon" => ";".to_string(),
        "colon" => ":".to_string(),
        "comma" => ",".to_string(),
        "period" => ".".to_string(),
        "exclam" => "!".to_string(),
        "at" => "@".to_string(),
        "numbersign" | "hash" => "#".to_string(),
        "dollar" => "$".to_string(),
        "percent" => "%".to_string(),
        "asciicircum" | "caret" => "^".to_string(),
        "ampersand" => "&".to_string(),
        "asterisk" => "*".to_string(),
        "parenleft" => "(".to_string(),
        "parenright" => ")".to_string(),
        "underscore" => "_".to_string(),
        "asciitilde" | "tilde" => "~".to_string(),
        "bar" | "pipe" => "|".to_string(),
        "question" => "?".to_string(),
        "less" => "<".to_string(),
        "greater" => ">".to_string(),

        // Directional & arrow keys
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),

        // Navigation
        "page_up" | "pageup" => "PgUp".to_string(),
        "page_down" | "pagedown" => "PgDn".to_string(),
        "home" => "Home".to_string(),
        "end" => "End".to_string(),

        // Special keys
        "return" | "enter" => "⏎".to_string(),
        "backspace" => "⌫".to_string(),
        "escape" | "esc" => "Esc".to_string(),
        "tab" => "Tab".to_string(),
        "space" => "Space".to_string(),

        // Mouse & Touchpad scroll
        "wheelscrollup" => "Wheel ↑".to_string(),
        "wheelscrolldown" => "Wheel ↓".to_string(),
        "wheelscrollleft" => "Wheel ←".to_string(),
        "wheelscrollright" => "Wheel →".to_string(),
        "touchpadscrollup" => "Touchpad ↑".to_string(),
        "touchpadscrolldown" => "Touchpad ↓".to_string(),

        _ => k.to_string(),
    }
}

pub fn parse_chord(s: &str) -> (BTreeSet<String>, String) {
    let parts: Vec<&str> = s.split('+').map(|p| p.trim()).collect();
    let mut mods = BTreeSet::new();
    if parts.len() <= 1 {
        let key = parts.first().unwrap_or(&"");
        return (mods, prettify_key(key));
    }
    for m in &parts[..parts.len() - 1] {
        mods.insert(normalize_mod(m));
    }
    let key = parts.last().unwrap_or(&"");
    (mods, prettify_key(key))
}

fn prettify_action(name: &str) -> String {
    name.split('-')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_niri_file(path: &Path, visited: &mut BTreeSet<PathBuf>) -> Vec<KeyBind> {
    let mut binds = Vec::new();
    let canonical = match path.canonicalize() {
        Ok(c) => c,
        Err(_) => path.to_path_buf(),
    };

    if visited.contains(&canonical) {
        return binds;
    }
    visited.insert(canonical);

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[niri-whichkey] Failed to read {}: {}", path.display(), e);
            return binds;
        }
    };

    let doc: KdlDocument = match content.parse() {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "[niri-whichkey] Failed to parse KDL {}: {:?}",
                path.display(),
                e
            );
            return binds;
        }
    };

    let parent_dir = path.parent().unwrap_or_else(|| Path::new("."));

    for node in doc.nodes() {
        let node_name = node.name().value();
        if node_name == "include" {
            if let Some(entry) = node.entries().first() {
                if let Some(inc_file) = entry.value().as_string() {
                    let inc_path = parent_dir.join(inc_file);
                    binds.extend(parse_niri_file(&inc_path, visited));
                }
            }
        } else if node_name == "binds" {
            if let Some(children) = node.children() {
                for bind_node in children.nodes() {
                    let chord_str = bind_node.name().value();
                    if chord_str.is_empty() {
                        continue;
                    }

                    // Check for hotkey-overlay-title
                    let mut title = None;
                    for entry in bind_node.entries() {
                        if let Some(name) = entry.name() {
                            if name.value() == "hotkey-overlay-title" {
                                if entry.value().is_null() {
                                    title = Some(None); // explicitly suppressed
                                } else if let Some(t) = entry.value().as_string() {
                                    title = Some(Some(t.to_string()));
                                }
                            }
                        }
                    }

                    if title == Some(None) {
                        continue; // Hidden by config
                    }

                    let final_title = if let Some(Some(t)) = title {
                        t
                    } else if let Some(action_doc) = bind_node.children() {
                        if let Some(action_node) = action_doc.nodes().first() {
                            let act_name = action_node.name().value();
                            if act_name == "spawn" || act_name == "spawn-sh" {
                                let cmd = action_node
                                    .entries()
                                    .iter()
                                    .filter_map(|e| e.value().as_string())
                                    .collect::<Vec<_>>()
                                    .join(" ");
                                if cmd.starts_with("noctalia msg panel-toggle") {
                                    let panel =
                                        cmd.trim_start_matches("noctalia msg panel-toggle").trim();
                                    format!("Panel: {}", panel)
                                } else {
                                    format!("Run: {}", cmd)
                                }
                            } else {
                                prettify_action(act_name)
                            }
                        } else {
                            chord_str.to_string()
                        }
                    } else {
                        chord_str.to_string()
                    };

                    let (mods, key) = parse_chord(chord_str);
                    binds.push(KeyBind {
                        chord: chord_str.to_string(),
                        modifiers: mods,
                        key,
                        title: final_title,
                    });
                }
            }
        }
    }

    binds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prettify_key() {
        assert_eq!(prettify_key("Apostrophe"), "'");
        assert_eq!(prettify_key("apostrophe"), "'");
        assert_eq!(prettify_key("Left"), "←");
        assert_eq!(prettify_key("Right"), "→");
        assert_eq!(prettify_key("Up"), "↑");
        assert_eq!(prettify_key("Down"), "↓");
        assert_eq!(prettify_key("BracketLeft"), "[");
        assert_eq!(prettify_key("BracketRight"), "]");
        assert_eq!(prettify_key("Minus"), "-");
        assert_eq!(prettify_key("Equal"), "=");
        assert_eq!(prettify_key("Slash"), "/");
        assert_eq!(prettify_key("Return"), "⏎");
        assert_eq!(prettify_key("Backspace"), "⌫");
        assert_eq!(prettify_key("WheelScrollDown"), "Wheel ↓");
    }

    #[test]
    fn test_parse_chord_with_prettify() {
        let (mods, key) = parse_chord("Mod+Left");
        assert!(mods.contains("Mod"));
        assert_eq!(key, "←");

        let (mods2, key2) = parse_chord("Mod+Shift+Apostrophe");
        assert!(mods2.contains("Mod"));
        assert!(mods2.contains("Shift"));
        assert_eq!(key2, "'");

        let (_, key3) = parse_chord("Mod+BracketLeft");
        assert_eq!(key3, "[");

        let (_, key4) = parse_chord("Mod+BracketRight");
        assert_eq!(key4, "]");

        let (_, key5) = parse_chord("Mod+Minus");
        assert_eq!(key5, "-");

        let (_, key6) = parse_chord("Mod+Equal");
        assert_eq!(key6, "=");

        let (_, key7) = parse_chord("Mod+WheelScrollDown");
        assert_eq!(key7, "Wheel ↓");
    }

    #[test]
    fn test_collapse_numeric_ranges() {
        let mut mods = BTreeSet::new();
        mods.insert("Mod".to_string());

        let mut binds = Vec::new();
        for i in 1..=9 {
            binds.push(KeyBind {
                chord: format!("Mod+{}", i),
                modifiers: mods.clone(),
                key: i.to_string(),
                title: "Focus Workspace".to_string(),
            });
        }
        binds.push(KeyBind {
            chord: "Mod+T".to_string(),
            modifiers: mods.clone(),
            key: "T".to_string(),
            title: "Terminal".to_string(),
        });

        let collapsed = collapse_numeric_ranges(binds);
        assert_eq!(collapsed.len(), 2);

        let ws_bind = collapsed
            .iter()
            .find(|b| b.title == "Focus Workspace")
            .unwrap();
        assert_eq!(ws_bind.key, "1..9");

        let term_bind = collapsed.iter().find(|b| b.title == "Terminal").unwrap();
        assert_eq!(term_bind.key, "T");
    }

    #[test]
    fn test_collapse_numeric_ranges_with_numbered_titles() {
        let mut mods = BTreeSet::new();
        mods.insert("Mod".to_string());

        let mut binds = Vec::new();
        for i in 1..=5 {
            binds.push(KeyBind {
                chord: format!("Mod+{}", i),
                modifiers: mods.clone(),
                key: i.to_string(),
                title: format!("Workspace {}", i),
            });
        }

        let collapsed = collapse_numeric_ranges(binds);
        assert_eq!(collapsed.len(), 1);
        assert_eq!(collapsed[0].key, "1..5");
        assert_eq!(collapsed[0].title, "Workspace");
    }
}
