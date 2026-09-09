#![allow(dead_code)] // owned GL objects (vbo/quad fields) are retained
//! The direct rendering engine for "GEMINI: EXODUS" (gemdemo 0.2.0).
//!
//! PERF STRATEGY (the 60 fps plan, replacing the 0.1.0 multipass/raymarch
//! design that landed at single-digit FPS): every frame is drawn STRAIGHT
//! into the window surface as a back-to-front stack of layers, with no
//! intermediate FBOs at scale 1.0:
//!
//!   sky (gradient) → nebula blobs → stars → halo glows → [ring far] →
//!   planet → [ring near] → warp streaks → ship → particles → flash
//!
//! Cost control: the ONLY fullscreen fragment pass is the cheap sky
//! gradient. Nebula/stars/glows/particles are tiny textured quads
//! (procedural RGBA sprites baked once at startup), the expensive math
//! (planet sphere lighting, ring banding) lives on small screen regions,
//! and per-pixel overdraw averages ~2. Glows ARE the bloom — there is no
//! separate post stage. Vertex work is trivial: fields are CPU-updated
//! into DynVBOs (a few tens of thousands of floats per frame worst case —
//! nothing for an A72), matching the fork-panfrost constraints from the
//! 0.1.0 on-glass receipt: glGen* objects only (DSA glCreate* are stubs),
//! single interleaved buffer per VAO, no instancing, no DrawElements,
//! triangles only (all expanded).
//!
//! Coordinate space: normalized stage coords, origin top-left, (1,1) =
//! the render resolution (which --scale may shrink for headroom). Vertex
//! shaders map normalized → NDC via u_res. font.rs text uses the same
//! top-left pixel space, so titles align with layers.

use crate::glutil;
use crate::shaders;
use crate::show;

// ------------------------------------------------------------------ small

/// Procedural RGBA sprite textures baked once on the CPU (white RGB,
/// coverage in alpha — tinted at draw time).
pub struct Textures {
    pub soft: u32,  // wide radial falloff (glows, halos, particles)
    pub star: u32,  // tight core + faint cross (stars, sparkles)
    pub ring: u32,  // thin annulus (impact shockwaves)
    pub cloud: u32, // puffy fbm blob (nebula smudges)
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / 16777216.0
    }
}

fn hash_iv(ix: i64, iy: i64) -> f32 {
    let mut z = (ix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (iy as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    z ^= z >> 29;
    z = z.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z ^= z >> 32;
    (z >> 40) as f32 / 16777216.0
}

fn value_noise2(x: f32, y: f32) -> f32 {
    let (xi, yi) = (x.floor() as i64, y.floor() as i64);
    let (fx, fy) = (x - xi as f32, y - yi as f32);
    let u = fx * fx * (3.0 - 2.0 * fx);
    let v = fy * fy * (3.0 - 2.0 * fy);
    let (a, b, c, d) = (hash_iv(xi, yi), hash_iv(xi + 1, yi), hash_iv(xi, yi + 1), hash_iv(xi + 1, yi + 1));
    a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v
}

unsafe fn tex_from<F: FnMut(f32, f32) -> f32>(name: &str, w: usize, h: usize, mut f: F) -> u32 {
    let mut px = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let a = f(x as f32 / (w - 1) as f32, y as f32 / (h - 1) as f32).clamp(0.0, 1.0);
            let o = (y * w + x) * 4;
            let v = (a * 255.0) as u8;
            px[o] = 255;
            px[o + 1] = 255;
            px[o + 2] = 255;
            px[o + 3] = v;
        }
    }
    let mut tex = 0;
    gl::GenTextures(1, &mut tex);
    gl::BindTexture(gl::TEXTURE_2D, tex);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
    gl::TexImage2D(
        gl::TEXTURE_2D, 0, gl::RGBA8 as i32, w as i32, h as i32, 0,
        gl::RGBA, gl::UNSIGNED_BYTE, px.as_ptr() as *const _,
    );
    eprintln!("gfx: baked texture '{name}' {w}x{h}");
    tex
}

unsafe fn bake_textures() -> Textures {
    let soft = tex_from("soft", 64, 64, |x, y| {
        let d2 = (x - 0.5).powi(2) + (y - 0.5).powi(2);
        (-d2 * 14.0).exp()
    });
    let star = tex_from("star", 32, 32, |x, y| {
        let dx = (x - 0.5) * 4.0;
        let dy = (y - 0.5) * 90.0;
        let core = (-((x - 0.5).powi(2) + (y - 0.5).powi(2)) * 90.0).exp();
        let spill = (-dx * dx - dy * dy).exp() + (-dy * dy - dx * dx).exp();
        (core + spill * 0.18).min(1.0)
    });
    let ring = tex_from("ring", 64, 64, |x, y| {
        let r = ((x - 0.5) * 2.0).hypot((y - 0.5) * 2.0);
        let band = (-(((r - 0.72) / 0.06).powi(2))).exp();
        band
    });
    let cloud = tex_from("cloud", 96, 96, |x, y| {
        let mut n = 0.0;
        let mut amp = 0.55;
        let mut fx = x * 4.0;
        let mut fy = y * 4.0;
        for _ in 0..4 {
            n += amp * value_noise2(fx, fy);
            fx = fx * 2.13 + 11.7;
            fy = fy * 2.03 + 5.1;
            amp *= 0.5;
        }
        let d = (x - 0.5).powi(2) * 0.6 + (y - 0.5).powi(2);
        let blob = (-d * 5.0).exp();
        let body = ((n - 0.42) * 5.0).clamp(0.0, 1.0);
        let a = (body * 0.75 + blob * 0.55).clamp(0.0, 1.0);
        a * a * (3.0 - 2.0 * a)
    });
    Textures { soft, star, ring, cloud }
}

fn corners6() -> [(f32, f32); 6] {
    [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
}

// ------------------------------------------------------------------ sky

struct Sky {
    prog: glutil::Program,
    vao: u32,
}

impl Sky {
    unsafe fn new(quad: glutil::Vbo) -> Self {
        let vs = shaders::FULLSCREEN_VS;
        let fs = shaders::frag(
            r#"
uniform vec3 u_top;
uniform vec3 u_mid;
uniform vec3 u_bot;
uniform float u_midy;
uniform vec2 u_gpos;   // glow centre, UV space (y up)
uniform vec3 u_gcol;
uniform float u_grad;
uniform float u_gain;
in vec2 uv;            // uv.y = 0 at the BOTTOM of the screen
void main() {
  // bottom (uv.y 0) wears u_bot (the warm horizon), top wears u_top
  vec3 col = mix(u_bot, u_mid, smoothstep(0.0, u_midy, uv.y));
  col = mix(col, u_top, smoothstep(u_midy, 1.0, uv.y));
  float d = distance(uv, u_gpos);
  col += u_gcol * exp(-(d * d) / max(u_grad * u_grad, 1e-6)) * u_gain;
  frag = vec4(col, 1.0);
}
"#,
        );
        let prog = glutil::build(
            "sky", vs, &fs,
            &["u_top", "u_mid", "u_bot", "u_midy", "u_gpos", "u_gcol", "u_grad", "u_gain"],
        );
        let vao = glutil::new_vao();
        gl::BindVertexArray(vao);
        gl::BindBuffer(gl::ARRAY_BUFFER, quad.id());
        gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 8, std::ptr::null());
        gl::EnableVertexAttribArray(0);
        gl::BindVertexArray(0);
        Sky { prog, vao }
    }

    unsafe fn draw(&self, s: &show::FrameState) {
        self.prog.use_();
        gl::Uniform3f(self.prog.uniform("u_top"), s.pal.sky_top.0, s.pal.sky_top.1, s.pal.sky_top.2);
        gl::Uniform3f(self.prog.uniform("u_mid"), s.pal.sky_mid.0, s.pal.sky_mid.1, s.pal.sky_mid.2);
        gl::Uniform3f(self.prog.uniform("u_bot"), s.pal.sky_bot.0, s.pal.sky_bot.1, s.pal.sky_bot.2);
        gl::Uniform1f(self.prog.uniform("u_midy"), 0.42);
        // sky_glow is in stage coords (y down); UV is y up → flip
        gl::Uniform2f(self.prog.uniform("u_gpos"), s.sky_glow.0, 1.0 - s.sky_glow.1);
        gl::Uniform3f(self.prog.uniform("u_gcol"), s.sky_glow_col.0, s.sky_glow_col.1, s.sky_glow_col.2);
        gl::Uniform1f(self.prog.uniform("u_grad"), s.sky_glow_r);
        gl::Uniform1f(self.prog.uniform("u_gain"), s.sky_glow_g);
        gl::BindVertexArray(self.vao);
        gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);
        gl::BindVertexArray(0);
    }
}

// ------------------------------------------------------------------ sprite
// ONE program + buffer layout for every textured-quad layer (nebula,
// stars, halos, particles, shocks): per-instance q(2) c(2) s(2) rgba(4),
// stride 40 — CPU expansion into a single dynamic interleaved buffer.

pub struct SpriteInst {
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl SpriteInst {
    pub fn new(cx: f32, cy: f32, w: f32, h: f32, col: (f32, f32, f32), a: f32) -> Self {
        SpriteInst { cx, cy, w, h, r: col.0, g: col.1, b: col.2, a }
    }
}

pub struct Sprites {
    pub prog: glutil::Program,
    vao: u32,
    buf: glutil::DynVbo,
    acc: Vec<f32>,
}

impl Sprites {
    pub unsafe fn new() -> Self {
        let vs = r#"
#version 300 es
layout(location=0) in vec2 q;
layout(location=1) in vec2 c;
layout(location=2) in vec2 s;
layout(location=3) in vec4 rgba;
uniform vec2 u_res;
out vec2 v_q;
out vec4 v_rgba;
void main() {
  vec2 px = (c + q * s * 0.5) * u_res;
  v_q = q;
  v_rgba = rgba;
  gl_Position = vec4(px.x / u_res.x * 2.0 - 1.0, 1.0 - px.y / u_res.y * 2.0, 0.0, 1.0);
}
"#;
        let fs = r#"#version 300 es
precision highp float;
uniform sampler2D u_tex;
uniform float u_gain;
in vec2 v_q;
in vec4 v_rgba;
out vec4 frag;
void main() {
  float a = texture(u_tex, v_q * 0.5 + 0.5).a;
  frag = vec4(v_rgba.rgb * (a * v_rgba.a * u_gain), 1.0);
}
"#;
        let prog = glutil::build("sprite", vs, fs, &["u_res", "u_tex", "u_gain"]);
        let buf = glutil::DynVbo::new(120_000);
        let vao = glutil::new_vao();
        gl::BindVertexArray(vao);
        gl::BindBuffer(gl::ARRAY_BUFFER, buf.id());
        gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 40, std::ptr::null());
        gl::VertexAttribPointer(1, 2, gl::FLOAT, gl::FALSE, 40, 8 as *const _);
        gl::VertexAttribPointer(2, 2, gl::FLOAT, gl::FALSE, 40, 16 as *const _);
        gl::VertexAttribPointer(3, 4, gl::FLOAT, gl::FALSE, 40, 24 as *const _);
        gl::EnableVertexAttribArray(0);
        gl::EnableVertexAttribArray(1);
        gl::EnableVertexAttribArray(2);
        gl::EnableVertexAttribArray(3);
        gl::BindVertexArray(0);
        Sprites { prog, vao, buf, acc: Vec::new() }
    }

    /// Expand + draw `insts` sampling `tex` (blend = caller's choice).
    pub unsafe fn draw(&mut self, tex: u32, gain: f32, insts: &[SpriteInst]) {
        if insts.is_empty() {
            return;
        }
        self.acc.clear();
        for it in insts {
            for (qx, qy) in corners6() {
                self.acc.extend_from_slice(&[
                    qx, qy, it.cx, it.cy, it.w, it.h, it.r, it.g, it.b, it.a,
                ]);
            }
        }
        let n = (self.acc.len() / 10) as i32;
        self.buf.update(&self.acc, 10);
        self.prog.use_();
        gl::BindVertexArray(self.vao);
        gl::ActiveTexture(gl::TEXTURE0);
        gl::BindTexture(gl::TEXTURE_2D, tex);
        gl::Uniform1i(self.prog.uniform("u_tex"), 0);
        gl::Uniform1f(self.prog.uniform("u_gain"), gain);
        gl::DrawArrays(gl::TRIANGLES, 0, n);
        gl::BindVertexArray(0);
    }
}

// ------------------------------------------------------------------ warp

struct Warp {
    prog: glutil::Program,
    vao: u32,
    vbo: glutil::Vbo,
    nverts: i32,
}

impl Warp {
    unsafe fn new(n: usize) -> Self {
        // attr: q(2) sd(2) cm(3), stride 28
        let mut verts = Vec::with_capacity(n * 6 * 7);
        let mut rng = Rng(0x1234_5678_9abc_def0);
        for _ in 0..n {
            let (a, b, cm) = (rng.next(), rng.next(), rng.next());
            for (qx, qy) in corners6() {
                verts.extend_from_slice(&[qx, qy, a, b, cm, 0.0, 0.0]);
            }
        }
        let vbo = glutil::Vbo::new(&verts);
        let vs = r#"
#version 300 es
layout(location=0) in vec2 q;
layout(location=1) in vec2 sd;
layout(location=2) in vec3 cm;
uniform vec2 u_res;
uniform vec2 u_wc;
uniform float u_time;
uniform float u_warp;
uniform vec3 u_colA;
uniform vec3 u_colB;
out float v_a;
out vec3 v_col;
void main() {
  float ang = sd.x * 6.2831853;
  float rad0 = mix(0.015, 0.40, pow(sd.y, 0.55));
  float sp = mix(0.30, 1.0, fract(sd.x * 37.17) * fract(sd.y * 13.31));
  float z = fract(u_time * (0.05 + 0.45 * u_warp) * sp + fract(sd.y * 91.7) * 0.9);
  float r = mix(rad0, 0.9, z);
  vec2 dir = vec2(cos(ang), sin(ang));
  vec2 pos = u_wc + dir * r;
  float len = u_res.x * (0.004 + 0.028 * z) * (0.2 + u_warp) * (0.6 + 0.5 * sp);
  float wid = max(u_res.x * 0.0014 * (1.0 - 0.45 * z) * (0.3 + u_warp), 0.6);
  vec2 perp = vec2(-dir.y, dir.x);
  vec2 px = pos * u_res + dir * (q.x * len) + perp * (q.y * wid);
  v_a = smoothstep(0.0, 0.10, z) * smoothstep(1.0, 0.82, z) * (0.2 + 0.8 * u_warp);
  v_col = mix(u_colA, u_colB, cm.x);
  gl_Position = vec4(px.x / u_res.x * 2.0 - 1.0, 1.0 - px.y / u_res.y * 2.0, 0.0, 1.0);
}
"#;
        let fs = shaders::frag(
            r#"
in float v_a;
in vec3 v_col;
void main() {
  frag = vec4(v_col * v_a, 1.0);
}
"#,
        );
        let prog = glutil::build(
            "warp", vs, &fs,
            &["u_res", "u_wc", "u_time", "u_warp", "u_colA", "u_colB"],
        );
        let vao = glutil::new_vao();
        gl::BindVertexArray(vao);
        gl::BindBuffer(gl::ARRAY_BUFFER, vbo.id());
        gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 28, std::ptr::null());
        gl::VertexAttribPointer(1, 2, gl::FLOAT, gl::FALSE, 28, 8 as *const _);
        gl::VertexAttribPointer(2, 3, gl::FLOAT, gl::FALSE, 28, 16 as *const _);
        gl::EnableVertexAttribArray(0);
        gl::EnableVertexAttribArray(1);
        gl::EnableVertexAttribArray(2);
        gl::BindVertexArray(0);
        Warp { prog, vao, vbo, nverts: (verts.len() / 7) as i32 }
    }

    unsafe fn draw(&self, w: f32, h: f32, s: &show::FrameState) {
        self.prog.use_();
        gl::Uniform2f(self.prog.uniform("u_res"), w, h);
        gl::Uniform2f(self.prog.uniform("u_wc"), s.warp_c.0, s.warp_c.1);
        gl::Uniform1f(self.prog.uniform("u_time"), s.t);
        gl::Uniform1f(self.prog.uniform("u_warp"), s.warp);
        gl::Uniform3f(self.prog.uniform("u_colA"), s.warp_col.0, s.warp_col.1, s.warp_col.2);
        gl::Uniform3f(self.prog.uniform("u_colB"), s.warp_col2.0, s.warp_col2.1, s.warp_col2.2);
        gl::BindVertexArray(self.vao);
        gl::DrawArrays(gl::TRIANGLES, 0, self.nverts);
        gl::BindVertexArray(0);
    }
}

// ------------------------------------------------------------------ planet

struct Planet {
    prog: glutil::Program,
    vao: u32,
}

impl Planet {
    unsafe fn new() -> Self {
        let vbo = glutil::Vbo::new(&[
            -1.0, -1.0, 1.0, -1.0, 1.0, 1.0,
            -1.0, -1.0, 1.0, 1.0, -1.0, 1.0,
        ]);
        let vs = r#"
#version 300 es
layout(location=0) in vec2 q;
uniform vec2 u_c;
uniform vec2 u_r;
uniform vec2 u_res;
out vec2 v_q;
void main() {
  v_q = q;
  vec2 px = u_c + q * u_r;
  gl_Position = vec4(px.x / u_res.x * 2.0 - 1.0, 1.0 - px.y / u_res.y * 2.0, 0.0, 1.0);
}
"#;
        let fs = shaders::frag(
            r#"
uniform float u_rot;
uniform vec3 u_ocean;
uniform vec3 u_land;
uniform vec3 u_desert;
uniform vec3 u_ice;
uniform vec3 u_cloud;
uniform vec3 u_atmos;
uniform float u_phase;
uniform float u_clouds;
in vec2 v_q;
void main() {
  float d = length(v_q);
  if (d > 1.0) discard;
  float z = sqrt(max(0.0, 1.0 - d * d));
  vec3 n = vec3(v_q.x, v_q.y, z);
  // spin: rotate the SURFACE coordinate around the view axis
  float ca = cos(u_rot * 0.5), sa = sin(u_rot * 0.5);
  vec3 m = vec3(n.x * ca + n.y * sa, -n.x * sa + n.y * ca, n.z);
  // light from the upper-left
  vec3 L = normalize(vec3(-0.55, -0.72, 0.42));
  float diff = clamp(dot(n, L), 0.0, 1.0);
  float night = 1.0 - diff;

  float t = u_phase + u_rot;
  float e1 = fbm(m.xy * 3.4 + vec2(t * 0.15, u_phase));
  float e2 = fbm(m.yz * 3.1 - vec2(u_phase * 3.0, t * 0.11));
  float elev = e1 * 0.62 + e2 * 0.38;
  float lat = abs(m.y);
  float ice = smoothstep(0.72, 0.60, lat);

  vec3 col = mix(u_ocean, u_land, smoothstep(0.44, 0.58, elev));
  col = mix(col, u_desert, smoothstep(0.68, 0.78, elev) * smoothstep(0.60, 0.38, abs(lat)));
  col = mix(col, u_ice, ice);

  float cl = fbm(m.xy * 5.2 + t * 0.22 + vec2(0.0, sin(t * 0.3) * 0.4));
  float cmask = smoothstep(0.60, 0.76, cl) * u_clouds * smoothstep(0.35, 0.75, diff);
  col = mix(col, u_cloud, cmask * 0.92);

  float lit = diff * (0.72 + 0.28 * smoothstep(0.42, 0.52, diff));
  col *= 0.05 + 1.1 * lit;
  col += vec3(1.0, 0.72, 0.42) * night * night * 0.15 * smoothstep(0.42, 0.6, elev);
  vec3 V = vec3(0.0, 0.0, 1.0);
  vec3 H = normalize(L + V);
  float spec = pow(clamp(dot(n, H), 0.0, 1.0), 60.0);
  col += vec3(1.0) * spec * (0.30 + 0.70 * cmask) * 0.45;
  float rim = pow(1.0 - d, 2.6);
  col += u_atmos * rim * (0.35 + 0.65 * diff);
  frag = vec4(col, 1.0);
}
"#,
        );
        let prog = glutil::build(
            "planet", vs, &fs,
            &[
                "u_c", "u_r", "u_res", "u_rot", "u_ocean", "u_land", "u_desert",
                "u_ice", "u_cloud", "u_atmos", "u_phase", "u_clouds",
            ],
        );
        let vao = glutil::new_vao();
        gl::BindVertexArray(vao);
        gl::BindBuffer(gl::ARRAY_BUFFER, vbo.id());
        gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 8, std::ptr::null());
        gl::EnableVertexAttribArray(0);
        gl::BindVertexArray(0);
        Planet { prog, vao }
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn draw(&self, w: f32, h: f32, cx: f32, cy: f32, r: f32, p: &show::PlanetPal, spin: f32) {
        self.prog.use_();
        gl::Uniform2f(self.prog.uniform("u_c"), cx, cy);
        gl::Uniform2f(self.prog.uniform("u_r"), r, r);
        gl::Uniform2f(self.prog.uniform("u_res"), w, h);
        gl::Uniform1f(self.prog.uniform("u_rot"), spin);
        gl::Uniform3f(self.prog.uniform("u_ocean"), p.ocean.0, p.ocean.1, p.ocean.2);
        gl::Uniform3f(self.prog.uniform("u_land"), p.land.0, p.land.1, p.land.2);
        gl::Uniform3f(self.prog.uniform("u_desert"), p.desert.0, p.desert.1, p.desert.2);
        gl::Uniform3f(self.prog.uniform("u_ice"), p.ice.0, p.ice.1, p.ice.2);
        gl::Uniform3f(self.prog.uniform("u_cloud"), p.cloud.0, p.cloud.1, p.cloud.2);
        gl::Uniform3f(self.prog.uniform("u_atmos"), p.atmos.0, p.atmos.1, p.atmos.2);
        gl::Uniform1f(self.prog.uniform("u_phase"), p.phase);
        gl::Uniform1f(self.prog.uniform("u_clouds"), p.clouds);
        gl::BindVertexArray(self.vao);
        gl::DrawArrays(gl::TRIANGLES, 0, 6);
        gl::BindVertexArray(0);
    }
}

// ------------------------------------------------------------------ ring

struct Ring {
    prog: glutil::Program,
    vao: u32,
    vbo: glutil::Vbo,
    back_off: i32,
    back_n: i32,
    front_off: i32,
    front_n: i32,
}

impl Ring {
    unsafe fn new() -> Self {
        const SEG: usize = 160;
        const RI: f32 = 1.15;
        const RO: f32 = 2.05;
        let mut verts: Vec<f32> = Vec::new();
        let mut arcs = [(0usize, 0i32); 2]; // (vertex offset, count) [top, bottom]
        // top arc (sin >= 0) is drawn BEHIND the planet; bottom in front
        for (k, is_top) in [(0, true), (1, false)] {
            arcs[k].0 = verts.len() / 4;
            for a in 0..SEG {
                let a0 = a as f32 / SEG as f32 * std::f32::consts::TAU;
                let a1 = (a + 1) as f32 / SEG as f32 * std::f32::consts::TAU;
                let mid = (a0 + a1) * 0.5;
                if (mid.sin() >= 0.0) != is_top {
                    continue;
                }
                for &(aa, rr) in &[(a0, RI), (a1, RI), (a1, RO), (a0, RI), (a1, RO), (a0, RO)] {
                    let u = aa / std::f32::consts::TAU;
                    let v = (rr - RI) / (RO - RI);
                    verts.extend_from_slice(&[aa.cos() * rr, aa.sin() * rr, u, v]);
                }
            }
            arcs[k].1 = (verts.len() / 4 - arcs[k].0) as i32;
        }
        let vbo = glutil::Vbo::new(&verts);
        let vs = r#"
#version 300 es
layout(location=0) in vec2 pos;
layout(location=1) in vec2 uv;
uniform vec2 u_res;
uniform vec2 u_c;
uniform float u_r;
uniform float u_squash;
uniform float u_roll;
out vec2 v_uv;
void main() {
  v_uv = uv;
  float ca = cos(u_roll), sa = sin(u_roll);
  vec2 p = vec2(pos.x * ca - pos.y * sa, pos.x * sa + pos.y * ca);
  p.y *= u_squash;
  vec2 px = u_c + p * u_r;
  gl_Position = vec4(px.x / u_res.x * 2.0 - 1.0, 1.0 - px.y / u_res.y * 2.0, 0.0, 1.0);
}
"#;
        let fs = shaders::frag(
            r#"
uniform vec3 u_colA;
uniform vec3 u_colB;
uniform float u_alpha;
uniform float u_time;
in vec2 v_uv;
void main() {
  float band = 0.60 + 0.40 * sin(v_uv.y * 26.0 + v_uv.x * 18.0 + u_time * 0.5);
  band *= 0.70 + 0.30 * sin(v_uv.x * 97.0 - u_time * 0.3);
  vec3 col = mix(u_colB, u_colA, v_uv.y);
  float edge = smoothstep(0.0, 0.07, v_uv.y) * smoothstep(1.0, 0.90, v_uv.y);
  float gap = smoothstep(0.40, 0.46, abs(v_uv.y - 0.52));
  float a = u_alpha * band * gap * edge;
  if (a < 0.003) discard;
  frag = vec4(col * (0.35 + 0.9 * band), a);
}
"#,
        );
        let prog = glutil::build(
            "ring", vs, &fs,
            &["u_res", "u_c", "u_r", "u_squash", "u_roll", "u_colA", "u_colB", "u_alpha", "u_time"],
        );
        let vao = glutil::new_vao();
        gl::BindVertexArray(vao);
        gl::BindBuffer(gl::ARRAY_BUFFER, vbo.id());
        gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 16, std::ptr::null());
        gl::VertexAttribPointer(1, 2, gl::FLOAT, gl::FALSE, 16, 8 as *const _);
        gl::EnableVertexAttribArray(0);
        gl::EnableVertexAttribArray(1);
        gl::BindVertexArray(0);
        // back = TOP arc (sin >= 0, arcs[0]) drawn BEFORE the planet body,
        // front = BOTTOM arc (sin < 0, arcs[1]) drawn after it (classic
        // ring depth: seen from slightly above, the lower arc crosses in
        // front of the disc).
        Ring {
            prog,
            vao,
            vbo,
            back_off: arcs[0].0 as i32,
            back_n: arcs[0].1,
            front_off: arcs[1].0 as i32,
            front_n: arcs[1].1,
        }
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn draw_arc(&self, off: i32, n: i32, cx: f32, cy: f32, r: f32, s: &show::FrameState, squash: f32, roll: f32, col: (f32, f32, f32), alpha: f32, w: f32, h: f32) {
        if n <= 0 {
            return;
        }
        self.prog.use_();
        gl::Uniform2f(self.prog.uniform("u_c"), cx, cy);
        gl::Uniform1f(self.prog.uniform("u_r"), r);
        gl::Uniform1f(self.prog.uniform("u_squash"), squash);
        gl::Uniform1f(self.prog.uniform("u_roll"), roll);
        gl::Uniform2f(self.prog.uniform("u_res"), w, h);
        gl::Uniform3f(self.prog.uniform("u_colA"), col.0, col.1, col.2);
        gl::Uniform3f(self.prog.uniform("u_colB"), 0.85, 0.9, 1.0);
        gl::Uniform1f(self.prog.uniform("u_alpha"), alpha);
        gl::Uniform1f(self.prog.uniform("u_time"), s.t);
        gl::BindVertexArray(self.vao);
        gl::DrawArrays(gl::TRIANGLES, off, n);
        gl::BindVertexArray(0);
    }
}

// ------------------------------------------------------------------ ship

pub struct ShipArt {
    // unit-shape triangles: x, y, r, g, b (nose at +y)
    parts: Vec<[f32; 5]>,
}

impl ShipArt {
    pub fn new() -> Self {
        let mut parts = Vec::new();
        let mut tri = |poly: &[(f32, f32)], base: (f32, f32, f32), lift: f32| {
            for i in 1..poly.len() - 1 {
                for &(x, y) in &[poly[0], poly[i], poly[i + 1]] {
                    let s = 1.0 + lift * (0.5 + y * 0.9);
                    parts.push([
                        x, y,
                        (base.0 * s).min(1.0), (base.1 * s).min(1.0), (base.2 * s).min(1.0),
                    ]);
                }
            }
        };
        let hull = vec![
            (0.0, 0.56), (0.20, 0.10), (0.30, -0.18), (0.16, -0.38),
            (0.0, -0.42), (-0.16, -0.38), (-0.30, -0.18), (-0.20, 0.10),
        ];
        tri(&hull, (0.66, 0.72, 0.84), 0.12);
        let nose = vec![(0.0, 0.56), (0.17, 0.16), (0.0, 0.20), (-0.17, 0.16)];
        tri(&nose, (0.30, 0.34, 0.46), 0.25);
        let tip = vec![(0.30, -0.18), (0.22, -0.30), (0.16, -0.38), (0.26, -0.22)];
        tri(&tip, (1.0, 0.55, 0.2), 0.55);
        let tip2 = vec![(-0.30, -0.18), (-0.26, -0.22), (-0.16, -0.38), (-0.22, -0.30)];
        tri(&tip2, (1.0, 0.55, 0.2), 0.55);
        let coc = vec![(0.0, 0.44), (0.07, 0.28), (0.0, 0.18), (-0.07, 0.28)];
        tri(&coc, (0.08, 0.12, 0.22), 0.18);
        let eng = vec![(0.13, -0.28), (0.13, -0.40), (-0.13, -0.40), (-0.13, -0.28)];
        tri(&eng, (0.15, 0.16, 0.20), 0.25);
        let spine = vec![(0.0, 0.16), (0.045, 0.0), (0.0, -0.30), (-0.045, 0.0)];
        tri(&spine, (0.30, 0.95, 1.0), 0.9);
        ShipArt { parts }
    }
}

struct Ship {
    prog: glutil::Program,
    vao: u32,
    buf: glutil::DynVbo,
    acc: Vec<f32>,
}

impl Ship {
    unsafe fn new() -> Self {
        let vs = r#"
#version 300 es
layout(location=0) in vec2 pos;
layout(location=1) in vec3 col;
uniform vec2 u_res;
out vec3 v_col;
void main() {
  v_col = col;
  gl_Position = vec4(pos.x / u_res.x * 2.0 - 1.0, 1.0 - pos.y / u_res.y * 2.0, 0.0, 1.0);
}
"#;
        let fs = r#"#version 300 es
precision highp float;
in vec3 v_col;
out vec4 frag;
void main() {
  frag = vec4(v_col, 1.0);
}
"#;
        let prog = glutil::build("ship", vs, fs, &["u_res"]);
        let buf = glutil::DynVbo::new(8192);
        let vao = glutil::new_vao();
        gl::BindVertexArray(vao);
        gl::BindBuffer(gl::ARRAY_BUFFER, buf.id());
        gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 20, std::ptr::null());
        gl::VertexAttribPointer(1, 3, gl::FLOAT, gl::FALSE, 20, 8 as *const _);
        gl::EnableVertexAttribArray(0);
        gl::EnableVertexAttribArray(1);
        gl::BindVertexArray(0);
        Ship { prog, vao, buf, acc: Vec::new() }
    }

    /// Transform the unit art to pixels; nose points along heading (0 =
    /// up on screen, positive = clockwise as seen by the viewer).
    unsafe fn draw(&mut self, w: f32, h: f32, s: &show::ShipPose, art: &ShipArt) {
        let scale = s.scale * w.min(h);
        let (sa, ca) = s.heading.sin_cos();
        self.acc.clear();
        for &[x, y, r, g, b] in art.parts.iter() {
            let rx = x * ca - y * sa;
            let ry = x * sa + y * ca;
            // shape space is +y up; stage space is +y down → flip
            self.acc.extend_from_slice(&[
                (s.cx + rx * scale) * w,
                (s.cy - ry * scale) * h,
                r, g, b,
            ]);
        }
        let n = (self.acc.len() / 5) as i32;
        self.buf.update(&self.acc, 5);
        self.prog.use_();
        gl::Uniform2f(self.prog.uniform("u_res"), w, h);
        gl::BindVertexArray(self.vao);
        gl::DrawArrays(gl::TRIANGLES, 0, n);
        gl::BindVertexArray(0);
    }

    /// Nozzle position (normalized stage coords) for engine flare/trail.
    fn nozzle(s: &show::ShipPose) -> (f32, f32) {
        let (sa, ca) = s.heading.sin_cos();
        let (tx, ty) = (0.0f32, -0.50f32); // tail in shape space
        let rx = tx * ca - ty * sa;
        let ry = tx * sa + ty * ca;
        (s.cx + rx * s.scale, s.cy - ry * s.scale)
    }
}

// ------------------------------------------------------------------ stage

struct Star {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    sp: f32,
    size: f32,
    bright: f32,
    tint: (f32, f32, f32),
    tw: f32,
}

struct Part {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
    max: f32,
    size: f32,
    col: (f32, f32, f32),
}

pub struct Stage {
    pub textures: Textures,
    sky: Sky,
    pub sprites: Sprites,
    warp: Warp,
    planet: Planet,
    ring: Ring,
    ship: Ship,
    pub ship_art: ShipArt,
    quad: glutil::Vbo,
    stars: Vec<Star>,
    parts: Vec<Part>,
    star_insts: Vec<SpriteInst>,
    glow_insts: Vec<SpriteInst>,
    part_insts: Vec<SpriteInst>,
    emit_t: f32,
}

impl Stage {
    pub unsafe fn new() -> Self {
        let textures = bake_textures();
        let quad = glutil::Vbo::new(&[-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0]);
        let mut stars = Vec::with_capacity(1800);
        let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
        for i in 0..1800 {
            let near = i % 6 == 0;
            let a = rng.next() * std::f32::consts::TAU;
            let spd = (0.02 + rng.next() * 0.06) * (if near { 2.2 } else { 0.5 });
            stars.push(Star {
                x: rng.next(),
                y: rng.next(),
                vx: a.cos() * spd,
                vy: a.sin() * spd * 0.6,
                sp: if near { 1.7 } else { 0.4 + rng.next() * 0.8 },
                size: if near { 0.0017 + rng.next() * 0.0012 } else { 0.0007 + rng.next() * 0.0008 },
                bright: if near { 0.7 + rng.next() * 0.3 } else { 0.3 + rng.next() * 0.4 },
                tint: (0.78 + rng.next() * 0.22, 0.82 + rng.next() * 0.18, 1.0),
                tw: rng.next() * 6.283,
            });
        }
        Stage {
            sky: Sky::new(quad),
            sprites: Sprites::new(),
            warp: Warp::new(2400),
            planet: Planet::new(),
            ring: Ring::new(),
            ship: Ship::new(),
            ship_art: ShipArt::new(),
            textures,
            quad,
            stars,
            parts: Vec::with_capacity(320),
            star_insts: Vec::new(),
            glow_insts: Vec::new(),
            part_insts: Vec::new(),
            emit_t: 0.0,
        }
    }

    /// Advance CPU state (stars drift, particles integrate, ship engine
    /// emission) by dt.
    pub fn update(&mut self, dt: f32, s: &show::FrameState) {
        for st in self.stars.iter_mut() {
            st.x = (st.x + st.vx * st.sp * dt).rem_euclid(1.0);
            st.y = (st.y + st.vy * st.sp * dt).rem_euclid(1.0);
            st.tw += dt * (0.8 + st.sp);
        }
        let mut i = 0;
        while i < self.parts.len() {
            let p = &mut self.parts[i];
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            let drag = (1.0 - dt * 0.5).max(0.0);
            p.vx *= drag;
            p.vy *= drag;
            p.life -= dt;
            if p.life <= 0.0 || p.x < -0.12 || p.x > 1.12 || p.y < -0.12 || p.y > 1.12 {
                self.parts.swap_remove(i);
            } else {
                i += 1;
            }
        }
        // continuous engine-trail emission while the ship is on screen
        if let Some(sp) = &s.ship {
            let (nx, ny) = Ship::nozzle(sp);
            let rate = 260.0 * sp.scale * 20.0; // per second
            self.emit_t += dt * rate;
            let mut rng = Rng(0x55AA_55AA_55AA_55AA);
            while self.emit_t >= 1.0 {
                self.emit_t -= 1.0;
                let a = rng.next() * std::f32::consts::TAU;
                let v = 0.02 + rng.next() * 0.05;
                self.parts.push(Part {
                    x: nx,
                    y: ny,
                    vx: a.cos() * v * 0.6,
                    vy: a.sin() * v * 0.6 + 0.03,
                    life: 0.35 + rng.next() * 0.4,
                    max: 0.6,
                    size: (0.004 + rng.next() * 0.008) * sp.scale * 4.0,
                    col: (0.45, 0.8, 1.0),
                });
            }
        }
    }

    /// A short-lived spark burst at normalized (x, y).
    pub fn burst(&mut self, x: f32, y: f32, n: usize, spread: f32, col: (f32, f32, f32)) {
        let mut rng = Rng(0x1234_5678 ^ ((x * 1e9) as u64) ^ ((y * 1e9) as u64));
        for _ in 0..n {
            let a = rng.next() * std::f32::consts::TAU;
            let v = rng.next() * spread;
            self.parts.push(Part {
                x,
                y,
                vx: a.cos() * v,
                vy: a.sin() * v * 0.7,
                life: 0.4 + rng.next() * 0.5,
                max: 0.9,
                size: 0.004 + rng.next() * 0.006,
                col,
            });
        }
    }

    /// Draw the full back-to-front stack at res (w, h) px.
    pub unsafe fn render(&mut self, w: u32, h: u32, s: &show::FrameState) {
        let (fw, fh) = (w as f32, h as f32);
        gl::Viewport(0, 0, w as i32, h as i32);

        // 1 sky (opaque)
        gl::Disable(gl::BLEND);
        self.sky.draw(s);

        // 2 nebula (additive cloud sprites)
        gl::Enable(gl::BLEND);
        gl::BlendFunc(gl::ONE, gl::ONE);
        self.star_insts.clear();
        for (i, nb) in s.nebula.iter().enumerate() {
            let ph = s.t * (0.02 + 0.008 * (i as f32 % 5.0));
            self.star_insts.push(SpriteInst {
                cx: nb.cx + 0.015 * ph.sin(),
                cy: nb.cy + 0.012 * (ph * 0.7 + 1.3).cos(),
                w: nb.w, h: nb.h,
                r: nb.col.0, g: nb.col.1, b: nb.col.2, a: nb.a,
            });
        }
        self.sprites.draw(self.textures.cloud, 0.55, &self.star_insts);

        // 3 stars
        self.star_insts.clear();
        for st in self.stars.iter() {
            let tw = 0.6 + 0.4 * (st.tw * 2.3).sin();
            let a = st.bright * tw * (1.0 - s.warp * 0.94).max(0.0) * s.star_dim;
            if a <= 0.004 {
                continue;
            }
            self.star_insts.push(SpriteInst {
                cx: st.x, cy: st.y,
                w: st.size, h: st.size,
                r: st.tint.0, g: st.tint.1, b: st.tint.2, a,
            });
        }
        self.sprites.draw(self.textures.star, 1.0, &self.star_insts);

        // 4 halo glows behind the planet / warp core (converted from the
        // show::SpriteGlow frame data)
        self.glow_insts.clear();
        for h in s.halos.iter() {
            self.glow_insts.push(SpriteInst {
                cx: h.cx,
                cy: h.cy,
                w: h.w,
                h: h.h,
                r: h.r,
                g: h.g,
                b: h.b,
                a: h.a,
            });
        }
        self.sprites.draw(self.textures.soft, 0.9, &self.glow_insts);

        // 5 ring far / planet / ring near
        if let Some(pl) = &s.planet {
            let cx = pl.cx * fw;
            let cy = pl.cy * fh;
            let r = pl.r * fw.min(fh) * (1.0 + 0.012 * s.kick);
            if pl.ring {
                gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
                self.ring.draw_arc(
                    self.ring.back_off, self.ring.back_n, cx, cy, r, s,
                    pl.ring_squash, pl.ring_roll, pl.ring_col, 0.45, fw, fh,
                );
            }
            gl::Disable(gl::BLEND);
            self.planet.draw(fw, fh, cx, cy, r, &pl.pal, pl.spin);
            if pl.ring {
                gl::Enable(gl::BLEND);
                gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
                self.ring.draw_arc(
                    self.ring.front_off, self.ring.front_n, cx, cy, r, s,
                    pl.ring_squash, pl.ring_roll, pl.ring_col, 0.95, fw, fh,
                );
            }
        }

        // 6 warp streaks
        if s.warp > 0.02 {
            gl::BlendFunc(gl::ONE, gl::ONE);
            self.warp.draw(fw, fh, s);
        }

        // 7 ship hull + engine flare
        if let Some(sp) = &s.ship {
            gl::Disable(gl::BLEND);
            self.ship.draw(fw, fh, sp, &self.ship_art);
            let (nx, ny) = Ship::nozzle(sp);
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::ONE, gl::ONE);
            let pulse = 0.8 + 0.4 * (s.t * 17.0).sin() + 0.3 * s.kick;
            let fl = 0.05 * sp.scale * pulse;
            self.glow_insts.clear();
            self.glow_insts.push(SpriteInst::new(nx, ny, fl, fl, (0.5, 0.85, 1.0), 0.9));
            self.glow_insts.push(SpriteInst::new(nx, ny, fl * 0.45, fl * 0.45, (1.0, 1.0, 1.0), 1.0));
            self.sprites.draw(self.textures.soft, 1.0, &self.glow_insts);
        }

        // 8 particles + shock rings
        self.part_insts.clear();
        for p in self.parts.iter() {
            let a = (p.life / p.max).clamp(0.0, 1.0);
            let sz = p.size * (0.5 + a);
            self.part_insts.push(SpriteInst {
                cx: p.x, cy: p.y, w: sz, h: sz,
                r: p.col.0, g: p.col.1, b: p.col.2, a: a * a,
            });
        }
        self.sprites.draw(self.textures.soft, 0.9, &self.part_insts);

        self.glow_insts.clear();
        for sh in s.shocks.iter() {
            let p = (sh.age / sh.max).clamp(0.0, 1.0);
            let r = sh.r0 + (sh.r1 - sh.r0) * p;
            let a = (1.0 - p) * sh.alpha;
            self.glow_insts.push(SpriteInst::new(sh.cx, sh.cy, r, r, sh.col, a));
        }
        if !self.glow_insts.is_empty() {
            gl::BlendFunc(gl::ONE, gl::ONE);
            self.sprites.draw(self.textures.ring, 1.0, &self.glow_insts);
        }
        gl::Disable(gl::BLEND);
    }
}
