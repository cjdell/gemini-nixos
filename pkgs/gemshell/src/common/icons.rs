//! App icon lookup + PNG/SVG decode (XDG icon themes, the standard icon
//! interface — same trees hicolor-based themes use).
//!
//! PNGs are decoded eagerly (there are only ~300 in a system profile);
//! SVGs are indexed by name and rendered lazily, on demand, only for the
//! icons an app actually uses. This matters because a NixOS profile
//! ships ~8000 SVGs (Adwaita/Pop/COSMIC ship app icons as SVG only —
//! decoding all of them would be minutes of work) while the launcher
//! needs at most ~100. Before SVG support the launcher showed blank
//! letter tiles for every GNOME/COSMIC app (2026-09-12).

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

/// Rasterize an SVG to straight-alpha RGBA8, scaled so its longest side
/// is `target` px. Uses resvg/usvg with text support disabled (app icons
/// are vector paths; no fonts are needed and it keeps the build small).
pub fn decode_svg(data: &[u8], target: u32) -> Option<Icon> {
    let tree = resvg::usvg::Tree::from_data(data, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    if size.width() <= 0.0 || size.height() <= 0.0 {
        return None;
    }
    let scale = target as f32 / size.width().max(size.height());
    let w = (size.width() * scale).round().max(1.0) as u32;
    let h = (size.height() * scale).round().max(1.0) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia stores premultiplied RGBA; `Icon` is straight alpha (the
    // GL uploader premultiplies once).
    let mut rgba = pixmap.take();
    for p in rgba.chunks_exact_mut(4) {
        let a = p[3];
        if a != 0 && a != 255 {
            let un = |c: u8| (((c as u16) * 255 + a as u16 / 2) / a as u16).min(255) as u8;
            p[0] = un(p[0]);
            p[1] = un(p[1]);
            p[2] = un(p[2]);
        }
    }
    Some(Icon { w, h, rgba })
}

pub struct IconSet {
    /// PNG icon name (no extension) -> decoded icon
    pub map: HashMap<String, Icon>,
    /// SVG icon name (no extension) -> file, rendered lazily by [`for_app`]
    pub svg: HashMap<String, PathBuf>,
    /// the icon tree roots scanned
    pub roots: Vec<PathBuf>,
}

const SIZES: [u32; 4] = [128, 64, 48, 32];

fn icon_dirs(home: &str) -> Vec<PathBuf> {
    crate::common::util::xdg_data_dirs(home)
        .into_iter()
        .map(|d| d.join("icons"))
        .collect()
}

/// The candidate `<theme>/<sub>/apps` directories, largest first, plus
/// the scalable/symbolic fallbacks. Shared by the PNG + SVG scans.
fn app_dirs(theme: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for size in SIZES {
        dirs.push(theme.join(format!("{size}x{size}")).join("apps"));
    }
    dirs.push(theme.join("scalable").join("apps"));
    dirs.push(theme.join("symbolic").join("apps"));
    dirs.push(theme.join("apps"));
    dirs
}

/// Find an icon by name: `<theme>/<size>x<size>/apps/<name>.{png,svg}`.
/// Bigger sizes win; a PNG beats an SVG of the same name.
pub fn load(home: &str) -> IconSet {
    let roots = icon_dirs(home);
    let mut map: HashMap<String, Icon> = HashMap::new();
    let mut svg: HashMap<String, PathBuf> = HashMap::new();
    for root in &roots {
        let Ok(themes) = std::fs::read_dir(root) else { continue };
        for theme in themes.flatten() {
            let theme = theme.path();
            if !theme.is_dir() {
                continue;
            }
            for dir in app_dirs(&theme) {
                let Ok(entries) = std::fs::read_dir(&dir) else { continue };
                for e in entries.flatten() {
                    let p = e.path();
                    let Some(name) = p.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    match p.extension().and_then(|s| s.to_str()) {
                        Some("png") => {
                            if map.contains_key(name) {
                                continue; // a larger size / earlier theme won
                            }
                            if let Ok(data) = std::fs::read(&p) {
                                if let Some(icon) = decode_png(&data) {
                                    map.insert(name.to_string(), icon);
                                }
                            }
                        }
                        Some("svg") => {
                            // First wins (largest dir first). SVG may be
                            // overridden by a PNG of the same name at
                            // lookup time.
                            svg.entry(name.to_string()).or_insert_with(|| p.clone());
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    IconSet { map, svg, roots }
}

/// Resolve the icon for an App: by name, else by the file name stem
/// (a common .desktop Icon= convention). SVGs are rendered on first use
/// and cached back into the set.
pub fn for_app(set: &mut IconSet, icon_name: &str, desktop_path: &Path) -> Option<Icon> {
    // An absolute Icon= path points straight at an image.
    if icon_name.starts_with('/') {
        let p = PathBuf::from(icon_name);
        return decode_file(&p, 96);
    }
    if let Some(icon) = set.map.get(icon_name) {
        return Some(icon.clone());
    }
    let names = [icon_name, desktop_path.file_stem().and_then(|s| s.to_str()).unwrap_or("")];
    for name in names {
        if name.is_empty() {
            continue;
        }
        if let Some(icon) = set.map.get(name) {
            return Some(icon.clone());
        }
        if let Some(path) = set.svg.get(name) {
            if let Ok(data) = std::fs::read(path) {
                if let Some(icon) = decode_svg(&data, 96) {
                    set.map.insert(name.to_string(), icon.clone());
                    return Some(icon);
                }
            }
        }
    }
    None
}

fn decode_file(p: &Path, target: u32) -> Option<Icon> {
    let data = std::fs::read(p).ok()?;
    match p.extension().and_then(|s| s.to_str()) {
        Some("png") => decode_png(&data),
        Some("svg") => decode_svg(&data, target),
        _ => None,
    }
}
