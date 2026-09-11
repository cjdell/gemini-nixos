//! .desktop file scanning for the launcher (XDG Desktop Entry
//! syntax, the standard app-launch interface — same files GNOME uses).
//!
//! Searched dirs (first match of a given file name wins, in this
//! order): ~/.local/share/applications, /usr/local/share/applications,
//! /usr/share/applications.

use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct App {
    pub name: String,
    pub exec: String,
    pub icon: String, // may be empty
    pub path: PathBuf,
}

fn parse_entry(path: &PathBuf) -> Option<App> {
    let text = crate::common::util::read_to_string(path)?;
    let mut in_section = false;
    let mut kv: HashMap<String, String> = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_section = line.trim_start_matches('[').trim_end_matches(']') == "Desktop Entry";
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            kv.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    let name = kv.get("Name")?.clone();
    let exec = kv.get("Exec")?.clone();
    if kv.get("NoDisplay").map(|s| s == "true").unwrap_or(false) {
        return None;
    }
    // Terminal=true apps are unusable in a compositor with no terminal
    // emulator of our own — skip (same rule as most mobile shells).
    if kv.get("Terminal").map(|s| s == "true").unwrap_or(false) {
        return None;
    }
    if name.is_empty() || exec.is_empty() {
        return None;
    }
    Some(App {
        name,
        exec,
        icon: kv.get("Icon").cloned().unwrap_or_default(),
        path: path.clone(),
    })
}

/// Scan the standard dirs; returns apps sorted by name (case-insens.).
pub fn scan(home: &str) -> Vec<App> {
    let dirs = [
        format!("{home}/.local/share/applications"),
        "/usr/local/share/applications".to_string(),
        "/usr/share/applications".to_string(),
    ];
    let mut seen: HashMap<String, ()> = HashMap::new();
    let mut out = Vec::new();
    // later dirs are more general — scan general first so user/desktop
    // files override by name (first-wins below)
    for dir in dirs.iter().rev() {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("desktop") {
                continue;
            }
            let Some(app) = parse_entry(&p) else { continue };
            let key = app.name.to_lowercase();
            if seen.contains_key(&key) {
                continue;
            }
            seen.insert(key, ());
            out.push(app);
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// Turn an `Exec=` line into argv: strip field codes (%f %u %U %F %U %d
/// %D %n %c %k ...) — we launch plain (no file open in v1).
pub fn exec_argv(exec: &str) -> Option<Vec<String>> {
    let cleaned: String = exec
        .split_whitespace()
        .filter(|t| !t.starts_with('%'))
        .collect::<Vec<_>>()
        .join(" ");
    let mut it = cleaned.split_whitespace();
    let program = it.next()?.to_string();
    Some(std::iter::once(program).chain(it.map(|s| s.to_string())).collect())
}
