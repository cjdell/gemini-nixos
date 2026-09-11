//! Font + glyph atlas — rasterized once at startup with ab_glyph 0.2.32,
//! then blitted (GL: one atlas texture; gemsettings: direct alpha
//! blend).
//!
//! Glyphs are stored PREMULTIPLIED WHITE (r=g=b=a=coverage) so both the
//! GL path (a textured quad * uColor) and the CPU path (a standard
//! alpha blend) get a plain color tint for free.

// `Font as _` — the local `pub struct Font` shadows the ab_glyph trait
// name; importing the trait anonymously keeps its methods in scope.
use ab_glyph::{Font as _, FontArc, Glyph, Point, PxScale, ScaleFont as _};
use std::collections::HashMap;

/// One placed glyph: atlas UVs + pixel metrics.
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    /// raster size in device pixels (0 = missing glyph)
    pub w: u32,
    pub h: u32,
    /// advance width in device pixels
    pub advance: f32,
    /// left bearing (px) — the raster's left edge sits at pen + x_off
    pub x_off: f32,
    /// raster top edge sits at baseline - y_top
    pub y_top: f32,
}

pub struct Font {
    /// atlas size (px)
    pub w: u32,
    pub h: u32,
    /// RGBA8 premultiplied-white glyph atlas
    pub pixels: Vec<u8>,
    glyphs: HashMap<char, Placed>,
    pub size: f32,
}

/// The charset the shell UIs need (ASCII + a few symbols present in
/// DejaVu Sans).
pub const CHARSET: &str =
    " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~£°•";

impl Font {
    pub fn load(path: &str, px: f32, atlas_size: u32) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("read font {path}: {e}"))?;
        let face = FontArc::try_from_vec(data).map_err(|e| format!("parse font {path}: {e}"))?;

        let mut out = vec![0u8; (atlas_size * atlas_size * 4) as usize];
        let mut glyphs: HashMap<char, Placed> = HashMap::new();
        let mut cx: u32 = 1;
        let mut cy: u32 = 1;
        let mut row_h: u32 = 0;

        let scale = PxScale::from(px);
        let scaled = face.as_scaled(scale);
        for c in CHARSET.chars() {
            let id = face.glyph_id(c);
            let glyph = Glyph {
                id,
                scale,
                position: Point {
                    x: 0.0,
                    y: 0.0,
                },
            };
            let Some(outline) = face.outline_glyph(glyph) else {
                // no outline (space / missing glyph) — still reserve the
                // advance so text layout works
                let advance = scaled.h_advance(id);
                glyphs.insert(
                    c,
                    Placed { u0: 0.0, v0: 0.0, u1: 0.0, v1: 0.0, w: 0, h: 0, advance, x_off: 0.0, y_top: 0.0 },
                );
                continue;
            };
            let bounds = outline.px_bounds();
            let w = (bounds.max.x - bounds.min.x).ceil().max(1.0) as u32;
            let h = (bounds.max.y - bounds.min.y).ceil().max(1.0) as u32;
            if w == 0 || h == 0 {
                continue;
            }
            if cx + w > atlas_size {
                cx = 1;
                cy += row_h + 2;
                row_h = 0;
                if cy + h > atlas_size {
                    log::warn!("font atlas full — dropping remaining glyphs");
                    break;
                }
            }
            // Rasterize into the atlas. `OutlinedGlyph::draw` yields
            // 0-based raster-local pixel coordinates (its bounds origin
            // is already subtracted internally), so do NOT subtract
            // bounds.min again: doing so pushed every glyph with a
            // negative min.y (i.e. everything above the baseline) out of
            // its own raster and produced an all-zero atlas — the
            // on-glass "no text" (2026-09-12).
            //
            // The ADVANCE had a second, independent bug: it used
            // `h_advance_unscaled` (raw font units, ~600) * px, giving
            // ~16 000 px per glyph. `Op::Text` then only ever showed the
            // first character and `Op::TextCentered` computed a pen
            // thousands of px off-screen (all launcher labels/titles
            // invisible). Use the SCALED advance; the units/em division
            // lives in ab_glyph (2026-09-11).
            let mut buf = vec![0f32; (w * h) as usize];
            outline.draw(|x, y, cv| {
                if x < w && y < h {
                    let o = (y * w + x) as usize;
                    if cv > buf[o] {
                        buf[o] = cv;
                    }
                }
            });
            for (i, &cv) in buf.iter().enumerate() {
                let a = (cv.clamp(0.0, 1.0) * 255.0) as u8;
                let o = (((cy + i as u32 / w) * atlas_size + (cx + i as u32 % w)) * 4) as usize;
                out[o] = a;
                out[o + 1] = a;
                out[o + 2] = a;
                out[o + 3] = a;
            }
            let advance = scaled.h_advance(id);
            let x_off = bounds.min.x; // raster left edge, relative to pen
            let y_top = -bounds.min.y; // raster top edge, above baseline
            glyphs.insert(
                c,
                Placed {
                    u0: cx as f32 / atlas_size as f32,
                    v0: cy as f32 / atlas_size as f32,
                    u1: (cx + w) as f32 / atlas_size as f32,
                    v1: (cy + h) as f32 / atlas_size as f32,
                    w,
                    h,
                    advance,
                    x_off,
                    y_top,
                },
            );
            cx += w + 2;
            row_h = row_h.max(h);
        }
        Ok(Font { w: atlas_size, h: atlas_size, pixels: out, glyphs, size: px })
    }

    pub fn glyph(&self, c: char) -> Option<&Placed> {
        self.glyphs.get(&c)
    }

    /// Width of `s` at this size (px), for centering/clipping.
    pub fn text_width(&self, s: &str) -> f32 {
        let mut x = 0f32;
        for c in s.chars() {
            if let Some(g) = self.glyph(c) {
                x += g.advance;
            } else {
                x += self.size * 0.6;
            }
        }
        x
    }
}
