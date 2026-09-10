//! The demo's own 5x7 bitmap font — baked into the binary, rendered to a
//! nearest-filtered atlas at startup. This is how the HUD, the act titles
//! and the "AETHER" logo are drawn: no font assets, demoscene style.
//!
//! `Font` = the shared program + atlas (built once). `Writer` = one
//! dynamic vertex buffer + VAO pair (the HUD, the logo, titles each own
//! one) that accumulates glyph/color quads in window-pixel coordinates
//! and draws them in one call.

use crate::glutil;

/// 5x7 glyphs, MSB = leftmost pixel, rows top to bottom.
const GLYPHS: &[(char, [u8; 7])] = &[
    ('!', [0x4, 0x4, 0x4, 0x4, 0x4, 0x0, 0x4]),
    ('%', [0x9, 0xA, 0x2, 0x4, 0x8, 0xB, 0x9]),
    ('(', [0x2, 0x4, 0x8, 0x8, 0x8, 0x4, 0x2]),
    (')', [0x8, 0x4, 0x2, 0x2, 0x2, 0x4, 0x8]),
    ('+', [0x0, 0x4, 0x4, 0xE, 0x4, 0x4, 0x0]),
    (',', [0x0, 0x0, 0x0, 0x0, 0x0, 0x6, 0x4]),
    ('-', [0x0, 0x0, 0x0, 0x7, 0x0, 0x0, 0x0]),
    ('.', [0x0, 0x0, 0x0, 0x0, 0x0, 0x6, 0x6]),
    ('/', [0x1, 0x1, 0x2, 0x4, 0x8, 0x8, 0x8]),
    ('0', [0x7, 0x9, 0xB, 0x5, 0x3, 0x9, 0x7]),
    ('1', [0x4, 0xC, 0x4, 0x4, 0x4, 0x4, 0x7]),
    ('2', [0x7, 0x9, 0x1, 0x2, 0x4, 0x8, 0xF]),
    ('3', [0xF, 0x2, 0x4, 0x2, 0x1, 0x9, 0x7]),
    ('4', [0x2, 0x6, 0xA, 0xA, 0xF, 0x2, 0x2]),
    ('5', [0xF, 0x8, 0xE, 0x1, 0x1, 0x9, 0x7]),
    ('6', [0x6, 0x8, 0x8, 0xE, 0x9, 0x9, 0x7]),
    ('7', [0xF, 0x1, 0x2, 0x4, 0x8, 0x8, 0x8]),
    ('8', [0x7, 0x9, 0x9, 0x7, 0x9, 0x9, 0x7]),
    ('9', [0x7, 0x9, 0x9, 0x7, 0x1, 0x2, 0x6]),
    (':', [0x0, 0x6, 0x6, 0x0, 0x6, 0x6, 0x0]),
    (';', [0x0, 0x6, 0x6, 0x0, 0x2, 0x6, 0x4]),
    ('=', [0x0, 0x0, 0xE, 0x0, 0xE, 0x0, 0x0]),
    ('?', [0x7, 0x9, 0x1, 0x2, 0x4, 0x0, 0x4]),
    ('A', [0x7, 0x9, 0x9, 0xF, 0x9, 0x9, 0x9]),
    ('B', [0xE, 0x9, 0x9, 0xE, 0x9, 0x9, 0xE]),
    ('C', [0x7, 0x9, 0x8, 0x8, 0x8, 0x9, 0x7]),
    ('D', [0xE, 0x9, 0x9, 0x9, 0x9, 0x9, 0xE]),
    ('E', [0xF, 0x8, 0x8, 0xE, 0x8, 0x8, 0xF]),
    ('F', [0xF, 0x8, 0x8, 0xE, 0x8, 0x8, 0x8]),
    ('G', [0x7, 0x9, 0x8, 0xB, 0x9, 0x9, 0x7]),
    ('H', [0x9, 0x9, 0x9, 0xF, 0x9, 0x9, 0x9]),
    ('I', [0x7, 0x2, 0x2, 0x2, 0x2, 0x2, 0x7]),
    ('J', [0x3, 0x1, 0x1, 0x1, 0x1, 0x9, 0x6]),
    ('K', [0x9, 0x9, 0xA, 0xC, 0xA, 0x9, 0x9]),
    ('L', [0x8, 0x8, 0x8, 0x8, 0x8, 0x8, 0xF]),
    ('M', [0x9, 0xD, 0xB, 0x9, 0x9, 0x9, 0x9]),
    ('N', [0x9, 0x9, 0xC, 0xA, 0x9, 0x9, 0x9]),
    ('O', [0x7, 0x9, 0x9, 0x9, 0x9, 0x9, 0x7]),
    ('P', [0xE, 0x9, 0x9, 0xE, 0x8, 0x8, 0x8]),
    ('Q', [0x7, 0x9, 0x9, 0x9, 0xB, 0x9, 0x7]),
    ('R', [0xE, 0x9, 0x9, 0xE, 0xA, 0x9, 0x9]),
    ('S', [0x7, 0x9, 0x8, 0x7, 0x1, 0x9, 0x7]),
    ('T', [0xF, 0x2, 0x2, 0x2, 0x2, 0x2, 0x2]),
    ('U', [0x9, 0x9, 0x9, 0x9, 0x9, 0x9, 0x7]),
    ('V', [0x9, 0x9, 0x9, 0x9, 0x9, 0x5, 0x2]),
    ('W', [0x9, 0x9, 0x9, 0x9, 0xB, 0xD, 0x9]),
    ('X', [0x9, 0x9, 0x5, 0x2, 0x5, 0x9, 0x9]),
    ('Y', [0x9, 0x9, 0x9, 0x5, 0x2, 0x2, 0x2]),
    ('Z', [0xF, 0x1, 0x2, 0x4, 0x8, 0x1, 0xF]),
    ('[', [0x7, 0x8, 0x8, 0x8, 0x8, 0x8, 0x7]),
    (']', [0x7, 0x2, 0x2, 0x2, 0x2, 0x2, 0x7]),
    ('_', [0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0xF]),
    ('a', [0x0, 0x0, 0x7, 0x1, 0x7, 0x9, 0x7]),
    ('b', [0x8, 0x8, 0xE, 0x9, 0x9, 0x9, 0xE]),
    ('c', [0x0, 0x0, 0x7, 0x8, 0x8, 0x9, 0x7]),
    ('d', [0x1, 0x1, 0x7, 0x9, 0x9, 0x9, 0x7]),
    ('e', [0x0, 0x0, 0x7, 0x9, 0xF, 0x8, 0x7]),
    ('f', [0x3, 0x4, 0x8, 0xE, 0x8, 0x8, 0x8]),
    ('g', [0x0, 0x7, 0x9, 0x7, 0x1, 0x6, 0x4]),
    ('h', [0x8, 0x8, 0xE, 0x9, 0x9, 0x9, 0x9]),
    ('i', [0x2, 0x0, 0x4, 0x2, 0x2, 0x2, 0x7]),
    ('j', [0x1, 0x0, 0x2, 0x1, 0x1, 0x9, 0x6]),
    ('k', [0x8, 0x8, 0x9, 0xA, 0xC, 0xA, 0x9]),
    ('l', [0x6, 0x2, 0x2, 0x2, 0x2, 0x2, 0x7]),
    ('m', [0x0, 0x0, 0xA, 0xB, 0x9, 0x9, 0x9]),
    ('n', [0x0, 0x0, 0xE, 0x9, 0x9, 0x9, 0x9]),
    ('o', [0x0, 0x0, 0x7, 0x9, 0x9, 0x9, 0x7]),
    ('p', [0x0, 0x0, 0xE, 0x9, 0xE, 0x8, 0x8]),
    ('q', [0x0, 0x0, 0x7, 0x9, 0x7, 0x1, 0x1]),
    ('r', [0x0, 0x0, 0xA, 0xC, 0x8, 0x8, 0x8]),
    ('s', [0x0, 0x0, 0x7, 0x8, 0x7, 0x1, 0xE]),
    ('t', [0x8, 0x8, 0xE, 0x8, 0x8, 0x9, 0x6]),
    ('u', [0x0, 0x0, 0x9, 0x9, 0x9, 0x9, 0x7]),
    ('v', [0x0, 0x0, 0x9, 0x9, 0x9, 0x5, 0x2]),
    ('w', [0x0, 0x0, 0x9, 0x9, 0xB, 0xB, 0x5]),
    ('x', [0x0, 0x0, 0x9, 0x5, 0x2, 0x5, 0x9]),
    ('y', [0x0, 0x0, 0x9, 0x9, 0x7, 0x1, 0x6]),
    ('z', [0x0, 0x0, 0xF, 0x2, 0x4, 0x8, 0xF]),
    ('·', [0x0, 0x0, 0x0, 0x4, 0x0, 0x0, 0x0]),
    ('×', [0x9, 0x9, 0x5, 0x2, 0x5, 0x9, 0x9]),
];

/// Solid-white sentinel cell (plain colored quads sample this).
const WHITE_CELL: usize = GLYPHS.len(); // 80 glyphs + 1 sentinel = 81 cells
const ATLAS_COLS: usize = 16;
const ATLAS_ROWS: usize = 6; // 96 cells — the table outgrew 16x4 (80 glyphs
                              // + sentinel > 64; first on-glass run 2026-09-08
                              // hit the OOB in the bake loop)
const CELL_W: usize = 6;
const CELL_H: usize = 8;
const ATLAS_W: usize = ATLAS_COLS * CELL_W; // 96
const ATLAS_H: usize = ATLAS_ROWS * CELL_H; // 48

/// Lookup: exact match, else the uppercase of the same char.
fn glyph_index(c: char) -> Option<usize> {
    if c == ' ' {
        return None;
    }
    GLYPHS.iter().position(|(g, _)| *g == c).or_else(|| {
        let up = c.to_ascii_uppercase();
        GLYPHS.iter().position(|(g, _)| *g == up)
    })
}

pub struct Font {
    pub prog: glutil::Program,
    pub atlas: u32,
}

impl Font {
    pub unsafe fn new() -> Font {
        assert!(
            GLYPHS.len() + 1 <= ATLAS_COLS * ATLAS_ROWS,
            "font atlas too small: {} glyphs + sentinel > {} cells",
            GLYPHS.len(),
            ATLAS_COLS * ATLAS_ROWS
        );
        // Bake the atlas: R = coverage, white on black.
        let mut px = vec![0u8; ATLAS_W * ATLAS_H * 4];
        let put = |i: usize, r: usize, b: usize, px: &mut [u8]| {
            let col = i % ATLAS_COLS;
            let row = i / ATLAS_COLS;
            let o = ((row * CELL_H + r) * ATLAS_W + (col * CELL_W + b)) * 4;
            px[o] = 255;
            px[o + 3] = 255;
        };
        for (i, &(_, rows)) in GLYPHS.iter().enumerate() {
            for r in 0..7 {
                for b in 0..5 {
                    if rows[r] & (1 << (4 - b)) != 0 {
                        put(i, r, b, &mut px);
                    }
                }
            }
        }
        for r in 0..7 {
            for b in 0..5 {
                put(WHITE_CELL, r, b, &mut px);
            }
        }

        let mut atlas = 0;
        gl::GenTextures(1, &mut atlas);
        gl::BindTexture(gl::TEXTURE_2D, atlas);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
        gl::TexImage2D(
            gl::TEXTURE_2D,
            0,
            gl::RGBA8 as i32,
            ATLAS_W as i32,
            ATLAS_H as i32,
            0,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            px.as_ptr() as *const _,
        );

        let vs = r#"
#version 300 es
layout(location=0) in vec2 px;   // window pixels, top-left origin
layout(location=1) in vec2 uv;
layout(location=2) in vec3 col;
uniform vec2 u_res;
out vec2 v_uv;
out vec3 v_col;
void main() {
  v_uv = uv;
  v_col = col;
  gl_Position = vec4(px.x / u_res.x * 2.0 - 1.0, 1.0 - px.y / u_res.y * 2.0, 0.0, 1.0);
}
"#;
        let fs = r#"
#version 300 es
precision highp float;
in vec2 v_uv;
in vec3 v_col;
uniform sampler2D u_atlas;
out vec4 frag;
void main() {
  float a = texture(u_atlas, v_uv).r;
  frag = vec4(v_col * a, a);
}
"#;
        let prog = glutil::build("font", vs, fs, &["u_res", "u_atlas"]);
        Font { prog, atlas }
    }

    /// Cell UV rect for glyph index `i`.
    pub fn cell_uv(i: usize) -> (f32, f32, f32, f32) {
        let col = i % ATLAS_COLS;
        let row = i / ATLAS_COLS;
        let u0 = col as f32 * CELL_W as f32 / ATLAS_W as f32;
        let u1 = (col * CELL_W + 5) as f32 / ATLAS_W as f32;
        let v0 = 1.0 - (row * CELL_H + 7) as f32 / ATLAS_H as f32;
        let v1 = 1.0 - row as f32 * CELL_H as f32 / ATLAS_H as f32;
        (u0, v0, u1, v1)
    }
}

/// One dynamic glyph/color-quad buffer on top of a shared Font.
/// Three separate offset-0 attribute buffers (pos/uv/col) — the fork's
/// u_vbuf segfaults on interleaved nonzero offsets (on-glass 2026-09-08).
/// NOTE: no stored Font pointer — SceneSet is moved after construction, so
/// a *const Font captured in new() dangles (SIGSEGV on later draw,
/// on-glass 2026-09-08). The Font is borrowed at draw() time instead.
pub struct Writer {
    vao: u32,
    vbo: glutil::DynVbo,
    verts: Vec<f32>, // interleaved accumulator, uploaded at draw()
}

impl Writer {
    /// `cap` = max vertices (4 per quad).
    pub unsafe fn new(cap_verts: usize) -> Writer {
        let vbo = glutil::DynVbo::new(cap_verts * 7);
        let vao = glutil::new_vao();
        gl::BindVertexArray(vao);
        gl::BindBuffer(gl::ARRAY_BUFFER, vbo.id());
        // single interleaved buffer, stride 28: pos(2)@0 uv(2)@8 col(3)@16
        // (wlroots-style single buffer; multi-buffer VAOs page-fault the
        // fork's panfrost — on-glass 2026-09-08)
        gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 28, std::ptr::null());
        gl::VertexAttribPointer(1, 2, gl::FLOAT, gl::FALSE, 28, 8 as *const _);
        gl::VertexAttribPointer(2, 3, gl::FLOAT, gl::FALSE, 28, 16 as *const _);
        gl::EnableVertexAttribArray(0);
        gl::EnableVertexAttribArray(1);
        gl::EnableVertexAttribArray(2);
        gl::BindVertexArray(0);
        Writer {
            vao,
            vbo,
            verts: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        self.verts.clear();
    }

    /// A 5x7 glyph quad at window-pixel (x, y) top-left, scale s, tint col.
    /// Two triangles (a,b,d)(b,e,d) — the whole buffer draws as TRIANGLES.
    pub fn glyph(&mut self, x: f32, y: f32, s: f32, glyph: usize, col: (f32, f32, f32)) {
        let (u0, v0, u1, v1) = Font::cell_uv(glyph);
        let w = 5.0 * s;
        let h = 7.0 * s;
        self.push_tri(x, y, w, h, (u0, v1, u1, v0), col);
    }

    /// A plain colored rect (samples the white sentinel cell).
    pub fn quad(&mut self, x: f32, y: f32, w: f32, h: f32, col: (f32, f32, f32)) {
        let (u0, v0, u1, v1) = Font::cell_uv(WHITE_CELL);
        self.push_tri(x, y, w, h, (u0, v1, u1, v0), col);
    }

    fn push_tri(&mut self, x: f32, y: f32, w: f32, h: f32, uv: (f32, f32, f32, f32), col: (f32, f32, f32)) {
        // a=top-left b=top-right d=bottom-left e=bottom-right
        let a = (x, y, uv.0, uv.1);
        let b = (x + w, y, uv.2, uv.1);
        let d = (x, y + h, uv.0, uv.3);
        let e = (x + w, y + h, uv.2, uv.3);
        for v in [a, b, d, b, e, d] {
            self.verts.extend_from_slice(&[v.0, v.1, v.2, v.3, col.0, col.1, col.2]);
        }
    }

    /// A single character at (x, y) top-left (the logo's per-glyph
    /// animation); unknown chars are skipped.
    pub fn char_glyph(&mut self, x: f32, y: f32, s: f32, c: char, col: (f32, f32, f32)) {
        if let Some(g) = glyph_index(c) {
            self.glyph(x, y, s, g, col);
        }
    }

    /// A string at (x, y) top-left, scale s, tint col. Returns the advance
    /// width in pixels. Uppercases on the fly; unknown chars are skipped.
    pub fn text(&mut self, x: f32, y: f32, s: f32, str: &str, col: (f32, f32, f32)) -> f32 {
        let mut cx = x;
        for c in str.chars() {
            match c {
                ' ' => cx += 4.0 * s,
                other => match glyph_index(other) {
                    Some(g) => {
                        self.glyph(cx, y, s, g, col);
                        cx += 6.0 * s;
                    }
                    None => {}
                },
            }
        }
        cx - x
    }

    /// Upload + draw everything accumulated. `res` = window pixels.
    pub unsafe fn draw(&mut self, font: &Font, res: (u32, u32)) {
        if self.verts.is_empty() {
            return;
        }
        let n = (self.verts.len() / 7) as i32;
        self.vbo.update(&self.verts, 7);
        self.verts.clear();
        font.prog.use_();
        gl::Uniform2f(font.prog.uniform("u_res"), res.0 as f32, res.1 as f32);
        gl::ActiveTexture(gl::TEXTURE0);
        gl::BindTexture(gl::TEXTURE_2D, font.atlas);
        gl::Uniform1i(font.prog.uniform("u_atlas"), 0);
        gl::BindVertexArray(self.vao);
        gl::DrawArrays(gl::TRIANGLES, 0, n);
        gl::BindVertexArray(0);
    }
}
