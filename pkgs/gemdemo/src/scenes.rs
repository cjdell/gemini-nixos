//! The scene acts. Everything is procedural: geometry is generated on the
//! CPU once, motion lives in the vertex shaders (the GPU does the work —
//! that's the stress), and colors come from the shared cosine palette.
//!
//!   act 0/6  SPACE  — fbm nebula + a starfield (birth = bar progress)
//!   act 1    TUNNEL — plasma cylinder + warp-starfield
//!   act 2    CORE   — raymarched crystal kaleidoscope (raymarch.rs)
//!   act 3    SHATTER— synthwave sun + scrolling wave-grid + stars
//!   act 4/5  VORTEX — 16k instanced particle spiral + core glow (+ logo)

use crate::font::{Font, Writer};
use crate::glutil;
use crate::raymarch::Raymarch;
use crate::shaders;
use crate::synth;

const N_STARS: usize = 8192;
const N_PARTS: usize = 16384;

// ---------------------------------------------------------------- mat4

/// Column-major 4x4 helpers (GLSL layout).
pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
    let f = 1.0 / (fov_y * 0.5).tan();
    let nf = 1.0 / (near - far);
    [
        f / aspect, 0.0, 0.0, 0.0,
        0.0, f, 0.0, 0.0,
        0.0, 0.0, (far + near) * nf, -1.0,
        0.0, 0.0, 2.0 * far * near * nf, 0.0,
    ]
}

pub fn translate(x: f32, y: f32, z: f32) -> [f32; 16] {
    [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, y, z, 1.0]
}

pub fn rot_y(a: f32) -> [f32; 16] {
    let (s, c) = (a.sin(), a.cos());
    [c, 0.0, -s, 0.0, 0.0, 1.0, 0.0, 0.0, s, 0.0, c, 0.0, 0.0, 0.0, 0.0, 1.0]
}

pub fn mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for c in 0..4 {
        for r in 0..4 {
            let mut v = 0.0;
            for k in 0..4 {
                v += a[k * 4 + r] * b[c * 4 + k];
            }
            out[c * 4 + r] = v;
        }
    }
    out
}

// ---------------------------------------------------------------- stars

struct Stars {
    prog: glutil::Program,
    vbo: glutil::Vbo,
    vao: u32,
}

impl Stars {
    unsafe fn new() -> Self {
        // Per-star data: dir(3) seed(1) | size(1) colormix(1) phase(1),
        // replicated over a 2-triangle quad (6 verts) per star, ONE
        // interleaved buffer (stride 36 B): q(2)@0 dir/seed(4)@8
        // size/cmix/phase(3)@24. NOT instanced and NOT multi-buffer: the
        // fork's panfrost u_vbuf segfaults on instancing and its
        // multi-buffer VAO path page-faults the GPU (on-glass 2026-09-08;
        // the wlroots-style single interleaved buffer is what the
        // desktop's GLES2 path exercises every frame without faulting).
        let mut verts = Vec::with_capacity(N_STARS * 6 * 9);
        let mut rng = 0x1234_5678_9abc_def0u64;
        let mut rnd = || -> f32 {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng >> 40) as f32 / 16777216.0
        };
        // unit-quad corners for the two triangles (FAN order)
        let corners: [(f32, f32); 6] = [
            (-1.0, -1.0),
            (1.0, -1.0),
            (1.0, 1.0),
            (-1.0, -1.0),
            (1.0, 1.0),
            (-1.0, 1.0),
        ];
        for _ in 0..N_STARS {
            // direction: forward hemisphere only (camera looks down -z)
            let (x, y, z) = (rnd() * 2.0 - 1.0, rnd() * 2.0 - 1.0, -(rnd() * 0.8 + 0.2));
            let l = (x * x + y * y + z * z).sqrt().max(1e-4);
            let (dx, dy, dz) = (x / l, y / l, z / l);
            let seed = rnd(); // depth phase
            let size = 0.6 + rnd() * 0.9;
            let cmix = rnd();
            let phase = rnd(); // twinkle phase
            for &(qx, qy) in &corners {
                verts.extend_from_slice(&[qx, qy, dx, dy, dz, seed, size, cmix, phase]);
            }
        }
        let vbo = glutil::Vbo::new(&verts);
        let vs = r#"
#version 300 es
layout(location=0) in vec2 q;
layout(location=1) in vec4 i0; // dir.xyz, seed
layout(location=2) in vec3 i1; // size, colormix, phase
uniform mat4 u_vp;
uniform float u_time;
uniform float u_speed;   // 0..~4 (warp)
uniform float u_birth;   // 0..1 intro
uniform float u_dim;     // global brightness
out vec2 v_q;
out vec3 v_col;
out float v_a;
void main() {
  float seed = i0.w;
  float z = mod(seed * 60.0 + u_time * (2.0 + 6.0 * u_speed), 60.0) + 0.8;
  vec3 p = i0.xyz * z;
  vec4 cp = u_vp * vec4(p, 1.0);
  vec2 scr = cp.xy / cp.w;
  vec2 rad = normalize(scr + vec2(1e-4));
  vec2 perp = vec2(-rad.y, rad.x);
  float sz = i1.x * 0.016 / z;
  float width = sz * (1.0 + 0.3 * u_speed);
  float streak = sz * (1.5 + 9.0 * u_speed);
  vec2 off = rad * (q.y * streak) + perp * (q.x * width);
  gl_Position = cp + vec4(off * cp.w, 0.0, 0.0);
  v_q = q;
  float tw = 0.72 + 0.28 * sin(u_time * 3.0 + i1.z * 6.2831);
  float birth = smoothstep(seed * 0.92, seed * 0.92 + 0.12, u_birth);
  float nearfade = smoothstep(0.8, 4.0, z);
  v_a = tw * birth * nearfade * u_dim;
  v_col = mix(vec3(0.55, 0.75, 1.0), vec3(1.0, 0.92, 0.75), i1.y);
}
"#;
        let fs = shaders::frag(
            r#"
in vec2 v_q;
in vec3 v_col;
in float v_a;
void main() {
  float d = length(v_q);
  float f = exp(-d * d * 7.0);
  frag = vec4(v_col * f * v_a, 1.0);
}
"#,
        );
        let prog = glutil::build("stars", vs, &fs, &["u_vp", "u_time", "u_speed", "u_birth", "u_dim"]);
        let vao = glutil::new_vao();
        gl::BindVertexArray(vao);
        gl::BindBuffer(gl::ARRAY_BUFFER, vbo.id());
        gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 36, std::ptr::null());
        gl::VertexAttribPointer(1, 4, gl::FLOAT, gl::FALSE, 36, 8 as *const _);
        gl::VertexAttribPointer(2, 3, gl::FLOAT, gl::FALSE, 36, 24 as *const _);
        gl::EnableVertexAttribArray(0);
        gl::EnableVertexAttribArray(1);
        gl::EnableVertexAttribArray(2);
        gl::BindVertexArray(0);
        Stars {
            prog,
            vbo,
            vao,
        }
    }

    unsafe fn draw(&self, vp: &[f32; 16], time: f32, speed: f32, birth: f32, dim: f32) {
        self.prog.use_();
        glutil::set_mat4(&self.prog, "u_vp", vp);
        gl::Uniform1f(self.prog.uniform("u_time"), time);
        gl::Uniform1f(self.prog.uniform("u_speed"), speed);
        gl::Uniform1f(self.prog.uniform("u_birth"), birth);
        gl::Uniform1f(self.prog.uniform("u_dim"), dim);
        gl::BindVertexArray(self.vao);
        gl::DrawArrays(gl::TRIANGLES, 0, (N_STARS * 6) as i32);
        gl::BindVertexArray(0);
    }
}

// ---------------------------------------------------------------- tunnel

struct Tunnel {
    prog: glutil::Program,
    vbo: glutil::Vbo,
    vao: u32,
    nverts: i32,
}

impl Tunnel {
    unsafe fn new() -> Self {
        const SEG: usize = 96;
        const RING: usize = 20;
        let mut verts = Vec::new();
        for r in 0..RING {
            let zr = |rr: usize| 0.49 - (rr as f32 / RING as f32) * 40.0;
            for s in 0..SEG {
                // quad (a,b,c,d); emit the two triangles a,c,b / b,c,d
                // (vertices expanded — no ELEMENT buffer: the fork's
                // DrawElements minmax scan crashes reading the index
                // buffer, on-glass 2026-09-08). One interleaved buffer,
                // stride 20: pos(3)@0 uv(2)@12.
                let step = SEG + 1;
                for &(i0, i1, i2) in &[
                    (r * step + s, (r + 1) * step + s, r * step + s + 1),
                    (r * step + s + 1, (r + 1) * step + s, (r + 1) * step + s + 1),
                ] {
                    for &i in &[i0, i1, i2] {
                        let rr = i / step;
                        let ss = i % step;
                        let a = ss as f32 / SEG as f32 * std::f32::consts::PI * 2.0;
                        verts.extend_from_slice(&[
                            a.cos(),
                            a.sin(),
                            zr(rr),
                            ss as f32 / SEG as f32,
                            rr as f32 / RING as f32,
                        ]);
                    }
                }
            }
        }
        let vbo = glutil::Vbo::new(&verts);
        let vs = r#"
#version 300 es
layout(location=0) in vec3 pos;
layout(location=1) in vec2 uv;
uniform mat4 u_vp;
out vec2 v_uv;
void main() {
  v_uv = uv;
  gl_Position = u_vp * vec4(pos, 1.0);
}
"#;
        let fs = shaders::frag(
            r#"
uniform float u_time;
uniform float u_scroll;
uniform float u_beat;
uniform float u_energy;
uniform vec3 u_fog;
in vec2 v_uv;
void main() {
  vec2 p = vec2(v_uv.x, v_uv.y + u_scroll);
  vec2 q = p * vec2(3.0, 1.8);
  float n = fbm(q + 1.6 * vec2(
      fbm(q * 1.7 + u_time * 0.21),
      fbm(q * 2.3 - u_time * 0.16)));
  vec3 col = pal(n * 0.9 + u_time * 0.02,
      vec3(0.5), vec3(0.5), vec3(1.0), vec3(0.0, 0.33, 0.67));
  col *= 0.30 + 0.95 * n;
  // beat rings racing toward the camera
  float ring = exp(-6.0 * abs(fract(p.y * 6.0 - u_beat) - 0.5));
  col += vec3(1.0, 0.85, 0.7) * ring * (0.2 + 0.8 * u_energy);
  // far end melts into the flash/fog
  col = mix(col, u_fog, smoothstep(0.15, 1.0, v_uv.y));
  frag = vec4(col, 1.0);
}
"#,
        );
        let prog = glutil::build(
            "tunnel", vs, &fs,
            &["u_vp", "u_time", "u_scroll", "u_beat", "u_energy", "u_fog"],
        );
        let vao = glutil::new_vao();
        gl::BindVertexArray(vao);
        gl::BindBuffer(gl::ARRAY_BUFFER, vbo.id());
        gl::VertexAttribPointer(0, 3, gl::FLOAT, gl::FALSE, 20, std::ptr::null());
        gl::VertexAttribPointer(1, 2, gl::FLOAT, gl::FALSE, 20, 12 as *const _);
        gl::EnableVertexAttribArray(0);
        gl::EnableVertexAttribArray(1);
        gl::BindVertexArray(0);
        Tunnel {
            prog,
            vbo,
            vao,
            nverts: (verts.len() / 5) as i32,
        }
    }

    unsafe fn draw(&self, vp: &[f32; 16], time: f32, beat: f64, energy: f32, fog: (f32, f32, f32)) {
        self.prog.use_();
        glutil::set_mat4(&self.prog, "u_vp", vp);
        gl::Uniform1f(self.prog.uniform("u_time"), time);
        gl::Uniform1f(self.prog.uniform("u_scroll"), (time * 0.12).rem_euclid(1.0));
        gl::Uniform1f(self.prog.uniform("u_beat"), beat as f32);
        gl::Uniform1f(self.prog.uniform("u_energy"), energy);
        gl::Uniform3f(self.prog.uniform("u_fog"), fog.0, fog.1, fog.2);
        gl::BindVertexArray(self.vao);
        gl::DrawArrays(gl::TRIANGLES, 0, self.nverts);
        gl::BindVertexArray(0);
    }
}

// ---------------------------------------------------------------- space (nebula bg)

struct Space {
    prog: glutil::Program,
    vao: u32,
}

impl Space {
    unsafe fn new(quad: glutil::Vbo) -> Self {
        let vs = crate::shaders::FULLSCREEN_VS;
        let fs = shaders::frag(
            r#"
uniform float u_time;
in vec2 uv;
void main() {
  vec2 p = uv * vec2(2.0, 1.2);
  float n = fbm(p * 1.4 + vec2(u_time * 0.008, -u_time * 0.005));
  n = fbm(p * 2.6 + n * 1.8 + u_time * 0.01);
  vec3 col = pal(n * 0.6 + u_time * 0.006,
      vec3(0.05, 0.04, 0.09), vec3(0.10, 0.07, 0.16),
      vec3(1.0), vec3(0.55, 0.75, 0.95));
  float vig = smoothstep(1.25, 0.35, distance(uv, vec2(0.5)));
  frag = vec4(col * vig, 1.0);
}
"#,
        );
        let prog = glutil::build("space", vs, &fs, &["u_time"]);
        let vao = fullscreen_vao(quad);
        Space { prog, vao }
    }

    unsafe fn draw(&self, time: f32) {
        self.prog.use_();
        gl::Uniform1f(self.prog.uniform("u_time"), time);
        gl::BindVertexArray(self.vao);
        gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);
        gl::BindVertexArray(0);
    }
}

unsafe fn fullscreen_vao(quad: glutil::Vbo) -> u32 {
    let vao = glutil::new_vao();
    gl::BindVertexArray(vao);
    gl::BindBuffer(gl::ARRAY_BUFFER, quad.id());
    gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 8, std::ptr::null());
    gl::EnableVertexAttribArray(0);
    gl::BindVertexArray(0);
    vao
}

// ---------------------------------------------------------------- terrain

struct Terrain {
    prog: glutil::Program,
    vbo: glutil::Vbo,
    vao: u32,
    strips: usize,
    per_strip: i32,
}

impl Terrain {
    unsafe fn new() -> Self {
        const STRIPS: usize = 40;
        const SEGS: usize = 64;
        let mut verts = Vec::new();
        for i in 0..STRIPS {
            let z = -(i as f32 / STRIPS as f32) * 28.0;
            for s in 0..=SEGS {
                let x = -14.0 + (s as f32 / SEGS as f32) * 28.0;
                verts.extend_from_slice(&[x, z]);
            }
        }
        let vbo = glutil::Vbo::new(&verts);
        let vs = r#"
#version 300 es
layout(location=0) in vec2 gz;
uniform mat4 u_vp;
uniform float u_time;
uniform float u_speed;
out float v_h;
out float v_z;
void main() {
  float z = mod(gz.y + u_time * u_speed, 28.0);
  z = z - 28.0; // (-28, 0]: flows toward the camera
  float h = 0.9 * sin(gz.x * 0.55 + u_time * 1.3) * sin(z * 0.5 + u_time * 0.9)
          + 0.35 * sin((gz.x - z) * 0.9 - u_time * 1.9);
  v_h = h;
  v_z = z;
  gl_Position = u_vp * vec4(gz.x, h, z, 1.0);
}
"#;
        let fs = shaders::frag(
            r#"
uniform vec3 u_colA;
uniform vec3 u_colB;
in float v_h;
in float v_z;
void main() {
  float t = clamp(v_h * 0.5 + 0.5, 0.0, 1.0);
  vec3 col = mix(u_colA, u_colB, t);
  float fog = smoothstep(28.0, 6.0, -v_z);
  frag = vec4(col * fog, 1.0);
}
"#,
        );
        let prog = glutil::build(
            "terrain", vs, &fs,
            &["u_vp", "u_time", "u_speed", "u_colA", "u_colB"],
        );
        let vao = glutil::new_vao();
        gl::BindVertexArray(vao);
        gl::BindBuffer(gl::ARRAY_BUFFER, vbo.id());
        gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 8, std::ptr::null());
        gl::EnableVertexAttribArray(0);
        gl::BindVertexArray(0);
        Terrain { prog, vbo, vao, strips: STRIPS, per_strip: (SEGS + 1) as i32 }
    }

    unsafe fn draw(&self, vp: &[f32; 16], time: f32, speed: f32, colA: (f32, f32, f32), colB: (f32, f32, f32)) {
        self.prog.use_();
        glutil::set_mat4(&self.prog, "u_vp", vp);
        gl::Uniform1f(self.prog.uniform("u_time"), time);
        gl::Uniform1f(self.prog.uniform("u_speed"), speed);
        gl::Uniform3f(self.prog.uniform("u_colA"), colA.0, colA.1, colA.2);
        gl::Uniform3f(self.prog.uniform("u_colB"), colB.0, colB.1, colB.2);
        gl::BindVertexArray(self.vao);
        for i in 0..self.strips {
            gl::DrawArrays(gl::LINE_STRIP, (i * (self.per_strip as usize)) as i32, self.per_strip);
        }
        gl::BindVertexArray(0);
    }
}

// ---------------------------------------------------------------- sun

struct Sun {
    prog: glutil::Program,
    vao: u32,
}

impl Sun {
    unsafe fn new(quad: glutil::Vbo) -> Self {
        let vs = crate::shaders::FULLSCREEN_VS;
        let fs = shaders::frag(
            r#"
uniform float u_time;
uniform float u_aspect;
uniform vec3 u_fog;
in vec2 uv;
void main() {
  vec2 p = uv - vec2(0.5, 0.60);
  p.x *= u_aspect;
  float r = length(p);
  // background gradient
  vec3 col = mix(vec3(0.03, 0.01, 0.06), vec3(0.10, 0.02, 0.12), uv.y * 1.2);
  // sun disc with synthwave slices below the horizon
  float disc = smoothstep(0.285, 0.28, r);
  float slice = 0.25 + 0.75 * step(0.5, fract(p.y * 14.0 - u_time * 0.4));
  float below = smoothstep(0.02, -0.05, p.y);
  float sun = disc * mix(1.0, slice, below);
  vec3 suncol = mix(vec3(1.0, 0.55, 0.25), vec3(1.0, 0.2, 0.5), r * 2.2);
  col += suncol * sun * 1.2;
  col += suncol * exp(-r * 5.0) * 0.5;
  // faint horizon haze
  col = mix(col, u_fog, smoothstep(0.14, 0.0, abs(p.y)) * 0.5);
  frag = vec4(col, 1.0);
}
"#,
        );
        let prog = glutil::build("sun", vs, &fs, &["u_time", "u_aspect", "u_fog"]);
        let vao = fullscreen_vao(quad);
        Sun { prog, vao }
    }

    unsafe fn draw(&self, time: f32, aspect: f32, fog: (f32, f32, f32)) {
        self.prog.use_();
        gl::Uniform1f(self.prog.uniform("u_time"), time);
        gl::Uniform1f(self.prog.uniform("u_aspect"), aspect);
        gl::Uniform3f(self.prog.uniform("u_fog"), fog.0, fog.1, fog.2);
        gl::BindVertexArray(self.vao);
        gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);
        gl::BindVertexArray(0);
    }
}

// ---------------------------------------------------------------- vortex

struct Vortex {
    prog: glutil::Program,
    vbo: glutil::Vbo,
    vao: u32,
}

impl Vortex {
    unsafe fn new() -> Self {
        // Same per-attribute split as Stars/Tunnel (fork u_vbuf
        // nonzero-offset segfault, 2026-09-08): q(2f,stride 8) + seed
        // (1f) — single interleaved buffer, stride 12: q(2)@0 seed(1)@8
        // (wlroots-style single buffer; multi-buffer VAOs page-fault the
        // fork's panfrost — on-glass 2026-09-08).
        let mut verts = Vec::with_capacity(N_PARTS * 6 * 3);
        let mut rng = 0xfedc_ba98_7654_3210u64;
        let mut rnd = || -> f32 {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng >> 40) as f32 / 16777216.0
        };
        let corners: [(f32, f32); 6] = [
            (-1.0, -1.0),
            (1.0, -1.0),
            (1.0, 1.0),
            (-1.0, -1.0),
            (1.0, 1.0),
            (-1.0, 1.0),
        ];
        for _ in 0..N_PARTS {
            let seed = rnd();
            for &(qx, qy) in &corners {
                verts.extend_from_slice(&[qx, qy, seed]);
            }
        }
        let vbo = glutil::Vbo::new(&verts);
        let vs = r#"
#version 300 es
layout(location=0) in vec2 q;
layout(location=1) in float seed;
uniform mat4 u_vp;
uniform float u_time;
uniform float u_kick;
uniform float u_energy;
out vec2 v_q;
out vec3 v_col;
out float v_a;
// (pal lives in the frag prelude only — the vortex VS needs its own copy)
vec3 pal(float t, vec3 a, vec3 b, vec3 c, vec3 d) {
  return a + b * cos(6.28318 * (c * t + d));
}
void main() {
  float s = seed;
  float r0 = mix(0.5, 9.0, sqrt(s));
  float sp = mix(2.4, 0.22, sqrt(s));
  float th = s * 6.2831 * 23.0 + u_time * sp * (1.0 + 0.5 * u_kick);
  float y0 = s * 7.0 - 3.5 + 0.6 * sin(s * 40.0 + u_time * 0.9);
  float r = r0 * (1.0 - 0.22 * u_energy * u_kick);
  vec3 p = vec3(cos(th) * r, y0, sin(th) * r);
  // tilt the vortex (fixed orientation)
  p = mat3(0.938, 0.0, 0.346, 0.0, 1.0, 0.0, -0.346, 0.0, 0.938)
      * mat3(1.0, 0.0, 0.0, 0.0, 0.899, -0.437, 0.0, 0.437, 0.899) * p;
  vec4 cp = u_vp * vec4(p, 1.0);
  float sz = mix(0.06, 0.20, s) * (1.0 + 0.7 * u_kick);
  gl_Position = cp + vec4(q * sz * cp.w * 0.02, 0.0, 0.0);
  v_q = q;
  v_col = pal(s + u_time * 0.05,
      vec3(0.5), vec3(0.5), vec3(1.0), vec3(0.0, 0.15, 0.35));
  v_a = smoothstep(9.5, 2.0, r0) * 0.85;
}
"#;
        let fs = shaders::frag(
            r#"
in vec2 v_q;
in vec3 v_col;
in float v_a;
void main() {
  float d = length(v_q);
  float f = max(1.0 - d, 0.0);
  f = f * f * f;
  frag = vec4(v_col * f * v_a, 1.0);
}
"#,
        );
        let prog = glutil::build(
            "vortex", vs, &fs,
            &["u_vp", "u_time", "u_kick", "u_energy"],
        );
        let vao = glutil::new_vao();
        gl::BindVertexArray(vao);
        gl::BindBuffer(gl::ARRAY_BUFFER, vbo.id());
        gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 12, std::ptr::null());
        gl::VertexAttribPointer(1, 1, gl::FLOAT, gl::FALSE, 12, 8 as *const _);
        gl::EnableVertexAttribArray(0);
        gl::EnableVertexAttribArray(1);
        gl::BindVertexArray(0);
        Vortex {
            prog,
            vbo,
            vao,
        }
    }

    unsafe fn draw(&self, vp: &[f32; 16], time: f32, kick: f32, energy: f32) {
        self.prog.use_();
        glutil::set_mat4(&self.prog, "u_vp", vp);
        gl::Uniform1f(self.prog.uniform("u_time"), time);
        gl::Uniform1f(self.prog.uniform("u_kick"), kick);
        gl::Uniform1f(self.prog.uniform("u_energy"), energy);
        gl::BindVertexArray(self.vao);
        gl::DrawArrays(gl::TRIANGLES, 0, (N_PARTS * 6) as i32);
        gl::BindVertexArray(0);
    }
}

// ---------------------------------------------------------------- glow (core billboard)

struct Glow {
    prog: glutil::Program,
    vao: u32,
}

impl Glow {
    unsafe fn new(quad: glutil::Vbo) -> Self {
        let vs = r#"
#version 300 es
layout(location=0) in vec2 q;
uniform mat4 u_vp;
uniform float u_size;
out vec2 v_q;
void main() {
  vec4 cp = u_vp * vec4(0.0, 0.0, 0.0, 1.0);
  gl_Position = cp + vec4(q * u_size * cp.w, 0.0, 0.0);
  v_q = q;
}
"#;
        let fs = shaders::frag(
            r#"
uniform vec3 u_col;
uniform float u_gain;
in vec2 v_q;
void main() {
  float d = length(v_q);
  float f = exp(-d * d * 3.2);
  frag = vec4(u_col * f * u_gain, 1.0);
}
"#,
        );
        let prog = glutil::build("glow", vs, &fs, &["u_vp", "u_size", "u_col", "u_gain"]);
        let vao = fullscreen_vao(quad);
        Glow { prog, vao }
    }

    unsafe fn draw(&self, vp: &[f32; 16], size: f32, col: (f32, f32, f32), gain: f32) {
        self.prog.use_();
        glutil::set_mat4(&self.prog, "u_vp", vp);
        gl::Uniform1f(self.prog.uniform("u_size"), size);
        gl::Uniform3f(self.prog.uniform("u_col"), col.0, col.1, col.2);
        gl::Uniform1f(self.prog.uniform("u_gain"), gain);
        gl::BindVertexArray(self.vao);
        gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);
        gl::BindVertexArray(0);
    }
}

// ---------------------------------------------------------------- scene set

pub struct SceneSet {
    pub font: Font,
    pub hud_w: Writer,
    pub logo_w: Writer,
    stars: Stars,
    tunnel: Tunnel,
    space: Space,
    terrain: Terrain,
    sun: Sun,
    vortex: Vortex,
    glow: Glow,
    pub rm: Raymarch,
    quad: glutil::Vbo,
}

impl SceneSet {
    pub unsafe fn new() -> Self {
        let font = Font::new();
        let hud_w = Writer::new(4096);
        let logo_w = Writer::new(8192);
        let quad = glutil::Vbo::new(&[-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0]);
        SceneSet {
            font,
            hud_w,
            logo_w,
            stars: Stars::new(),
            tunnel: Tunnel::new(),
            space: Space::new(quad),
            terrain: Terrain::new(),
            sun: Sun::new(quad),
            vortex: Vortex::new(),
            glow: Glow::new(quad),
            rm: Raymarch::new(),
            quad,
        }
    }
}



/// Section → act id (0..5; 2 = raymarch, 3 = terrain, 4 = vortex).
pub fn section_act(section: i32) -> i32 {
    match section {
        0 => 0, // GENESIS  -> space/stars
        1 => 1, // DESCENT  -> tunnel
        2 => 2, // CORE     -> raymarch
        3 => 3, // SHATTER  -> terrain
        4 | 5 => 4, // SURGE / IGNITION -> vortex
        _ => 6, // AFTERGLOW -> space/stars
    }
}

/// Per-section energy (drives speed, bloom, camera push).
pub fn section_energy(section: i32, beat: f64) -> f32 {
    let bar_in = (beat / 4.0 - synth::section_start_beat(section as u32) / 4.0) as u32;
    let bars = synth::SECTION_BARS[section as usize] as u32;
    let p = (bar_in as f32 / bars.max(1) as f32).clamp(0.0, 1.0);
    match section {
        0 => 0.15,
        1 => 0.2 + 0.5 * p,
        2 => 1.0,
        3 => 0.35,
        4 => 0.5 + 0.4 * p,
        5 => 1.0,
        _ => 0.2,
    }
}

/// Draw the act into the current FBO.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_act(
    scene: &mut SceneSet,
    act: i32,
    w: u32,
    h: u32,
    time: f32,
    beat: f64,
    kick: f32,
    flash: f32,
    act_t: f32,
    section: i32,
) {
    let aspect = w as f32 / h.max(1) as f32;
    let energy = section_energy(section, beat);
    let bar = (beat / 4.0) as u32 % synth::TOTAL_BARS as u32;
    let bar_in = bar % synth::SECTION_BARS[section as usize];
    let bar_p = bar_in as f32 / synth::SECTION_BARS[section as usize].max(1) as f32;
    let fog = (0.9 * flash, 0.92 * flash, 1.0 * flash);

    match act {
        // ------------------------------------------------- SPACE (0 / 6)
        0 | 6 => {
            scene.space.draw(time);
            // stars: GENESIS births them with the bars; AFTERGLOW fully on
            let birth = if section == 0 { (bar_in as f32 / 4.0).clamp(0.0, 1.0) } else { 1.0 };
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::ONE, gl::ONE);
            scene.stars.draw(
                &mul(&perspective(1.1, aspect, 0.1, 100.0), &translate(0.0, 0.0, 0.5)),
                time,
                0.15 + 0.35 * energy,
                birth,
                0.7 + 0.3 * bar_p,
            );
            gl::Disable(gl::BLEND);
        }
        // ------------------------------------------------- TUNNEL (1)
        1 => {
            scene.tunnel.draw(
                &mul(&perspective(1.15, aspect, 0.1, 100.0), &translate(0.0, 0.0, 0.0)),
                time,
                beat,
                energy,
                fog,
            );
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::ONE, gl::ONE);
            scene.stars.draw(
                &mul(&perspective(1.1, aspect, 0.1, 100.0), &translate(0.0, 0.0, 0.5)),
                time,
                1.5 + 2.5 * energy + kick * 1.5,
                1.0,
                0.8,
            );
            gl::Disable(gl::BLEND);
        }
        // ------------------------------------------------- CORE (2)
        2 => {
            scene.rm.draw(w, h, time, beat, kick, 0, 1.0);
        }
        // ------------------------------------------------- SHATTER (3)
        3 => {
            scene.sun.draw(time, aspect, (0.35, 0.12, 0.4));
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::ONE, gl::ONE);
            scene.stars.draw(
                &mul(&perspective(1.1, aspect, 0.1, 100.0), &translate(0.0, 0.0, 0.5)),
                time,
                0.05,
                1.0,
                0.35,
            );
            scene.terrain.draw(
                &mul(&perspective(1.05, aspect, 0.1, 200.0), &translate(0.0, 2.4, 3.0)),
                time,
                3.0 + 2.0 * energy,
                (0.45, 0.10, 0.55),
                (0.10, 0.95, 0.95),
            );
            gl::Disable(gl::BLEND);
        }
        // ------------------------------------------------- VORTEX (4)
        _ => {
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::ONE, gl::ONE);
            scene.stars.draw(
                &mul(&perspective(1.1, aspect, 0.1, 100.0), &translate(0.0, 0.0, 0.5)),
                time,
                0.1,
                1.0,
                0.3,
            );
            let vp = mul(
                &perspective(1.0, aspect, 0.1, 200.0),
                &mul(&translate(0.0, 0.0, 11.0), &rot_y(time * 0.07)),
            );
            scene.vortex.draw(&vp, time, kick, energy);
            scene.glow.draw(&vp, 0.9 + 0.5 * kick, (1.0, 0.8, 0.6), 0.8 + 0.8 * kick);
            // the logo lives in IGNITION (section 5)
            if section == 5 {
                draw_logo(scene, w, h, act_t, time);
            }
            gl::Disable(gl::BLEND);
        }
    }
    let _ = bar_p;
    let _ = bar;
}

/// The "AETHER" logo: per-glyph scale-in, a shine sweep, RGB-split
/// chromatic aberration (three additive passes).
unsafe fn draw_logo(scene: &mut SceneSet, w: u32, h: u32, act_t: f32, time: f32) {
    let title = "AETHER";
    let sub1 = "GEMINI PDA · NIXOS · MALI-T880";
    let sub2 = "RUST · PANFROST · OPENGL ES · 128 BPM";
    let scale = (h as f32 * 0.13).max(24.0);
    let adv = 6.0 * scale;
    let tw = title.len() as f32 * adv;
    let sx = (w as f32 - tw) * 0.5;
    let sy = h as f32 * 0.30;
    let sweep = ((time * 0.35).rem_euclid(1.0) * (tw + 240.0)) - 120.0;

    let passes: [(f32, (f32, f32, f32)); 3] = [
        (-1.6, (0.9, 0.12, 0.12)),
        (0.0, (0.12, 0.9, 0.18)),
        (1.6, (0.12, 0.16, 0.9)),
    ];
    for (dx, tint) in passes {
        scene.logo_w.clear();
        for (i, ch) in title.chars().enumerate() {
            // staggered elastic scale-in per glyph
            let t = act_t - 0.12 * i as f32 - 0.3;
            if t <= 0.0 {
                continue;
            }
            let k = t.min(1.2) / 1.2;
            let s = 1.0 + 1.9 * (1.0 - k) * k * (2.0 - k); // ease-out overshoot
            let gx = sx + i as f32 * adv + dx - 2.5 * scale * (s - 1.0);
            let gy = sy - 3.5 * scale * (s - 1.0);
            // shine band brightness
            let b = 1.0 + 2.2 * (-((gx - sweep) / (scale * 2.5)).powi(2)).exp();
            let c = (
                (tint.0 * b).min(1.0),
                (tint.1 * b).min(1.0),
                (tint.2 * b).min(1.0),
            );
            scene.logo_w.char_glyph(gx, gy, scale * s, ch, c);
        }
        scene.logo_w.draw(&scene.font, (w, h));
    }

    // subtitles (single pass, fade in after the title)
    let a = ((act_t - 2.2) / 1.2).clamp(0.0, 1.0);
    if a > 0.0 {
        scene.logo_w.clear();
        let s1 = scale * 0.32;
        let w1 = sub1.len() as f32 * 6.0 * s1;
        scene.logo_w.text(
            (w as f32 - w1) * 0.5,
            sy + 12.0 * scale,
            s1,
            sub1,
            (0.55 * a, 0.8 * a, 0.9 * a),
        );
        let s2 = scale * 0.32;
        let w2 = sub2.len() as f32 * 6.0 * s2;
        scene.logo_w.text(
            (w as f32 - w2) * 0.5,
            sy + 16.0 * scale,
            s2,
            sub2,
            (0.45 * a, 0.48 * a, 0.6 * a),
        );
        scene.logo_w.draw(&scene.font, (w, h));
    }
}
