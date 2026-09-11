//! App icon lookup + PNG decode (XDG icon themes, the standard icon
//! interface — same trees hicolor-based themes use).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Decoded RGBA8 (un-premultiplied — the GL layer converts to
/// premultiplied on upload).
#[derive(Clone)]
pub struct Icon {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

pub fn decode_png(data: &[u8]) -> Option<Icon> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(data));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let mut rgba = vec![0u8; w * h * 4];
    match info.color_type {
        png::ColorType::Rgba => {
            rgba.copy_from_slice(&buf[..w * h * 4]);
        }
        png::ColorType::Rgb => {
            for p in 0..w * h {
                rgba[p * 4..p * 4 + 3].copy_from_slice(&buf[p * 3..p * 3 + 3]);
                rgba[p * 4 + 3] = 255;
            }
        }
        png::ColorType::Grayscale => {
            for p in 0..w * h {
                let g = buf[p];
                rgba[p * 4] = g;
                rgba[p * 4 + 1] = g;
                rgba[p * 4 + 2] = g;
                rgba[p * 4 + 3] = 255;
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for p in 0..w * h {
                let (g, a) = (buf[p * 2], buf[p * 2 + 1]);
                rgba[p * 4] = g;
                rgba[p * 4 + 1] = g;
                rgba[p * 4 + 2] = g;
                rgba[p * 4 + 3] = a;
            }
        }
        _ => return None,
    }
    Some(Icon { w: w as u32, h: h as u32, rgba })
}

pub struct IconSet {
    /// icon name (no extension) -> decoded icon
    pub map: HashMap<String, Icon>,
    /// the icon tree roots scanned
    pub roots: Vec<PathBuf>,
}

const SIZES: [u32; 4] = [128, 64, 48, 32];

fn icon_dirs(home: &str) -> Vec<PathBuf> {
    vec![
        PathBuf::from(format!("{home}/.local/share/icons")),
        PathBuf::from("/usr/local/share/icons"),
        PathBuf::from("/usr/share/icons"),
    ]
}

/// Find an icon by name: <root>/<theme>/hicolor or <theme>/<size>x<size>/
/// apps/<name>.{png,svg}. Only PNG is decoded (SVG would need a
/// renderer — letter-tile fallback instead). Bigger sizes win.
pub fn load(home: &str) -> IconSet {
    let roots = icon_dirs(home);
    let mut map: HashMap<String, Icon> = HashMap::new();
    for root in &roots {
        let Ok(themes) = std::fs::read_dir(root) else { continue };
        for theme in themes.flatten() {
            let theme = theme.path();
            if !theme.is_dir() {
                continue;
            }
            for size in SIZES.iter().rev() {
                let dir = theme.join(format!("{size}x{size}")).join("apps");
                let Ok(entries) = std::fs::read_dir(&dir) else { continue };
                for e in entries.flatten() {
                    let p = e.path();
                    if p.extension().and_then(|s| s.to_str()) != Some("png") {
                        continue;
                    }
                    let Some(name) = p.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    if map.contains_key(name) {
                        continue; // a larger size already won
                    }
                    if let Ok(data) = std::fs::read(&p) {
                        if let Some(icon) = decode_png(&data) {
                            map.insert(name.to_string(), icon);
                        }
                    }
                }
            }
            // also plain <theme>/apps (some themes skip the size dirs)
            let dir = theme.join("apps");
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().and_then(|s| s.to_str()) != Some("png") {
                    continue;
                }
                let Some(name) = p.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if map.contains_key(name) {
                    continue;
                }
                if let Ok(data) = std::fs::read(&p) {
                    if let Some(icon) = decode_png(&data) {
                        map.insert(name.to_string(), icon);
                    }
                }
            }
        }
    }
    IconSet { map, roots }
}

/// Resolve the icon for an App: by name, else by the file name stem
/// (a common .desktop Icon= convention).
pub fn for_app(set: &IconSet, icon_name: &str, desktop_path: &Path) -> Option<Icon> {
    let by_name = set.map.get(icon_name).cloned();
    if by_name.is_some() || icon_name.is_empty() {
        return by_name;
    }
    desktop_path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|stem| set.map.get(stem).cloned())
}
