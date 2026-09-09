//! The CORE act: a raymarched crystal kaleidoscope — the GPU's headline
//! workload. Two levels of domain-repeated SDF (a fractal-ish shell),
//! 96 raymarch steps per fragment, cosine-palette "iridescence", 4-tap
//! AO, fresnel rim, fog-to-flash. The same pass is what `--stress`
//! re-renders N extra times per frame with the output discarded.

use crate::glutil;
use crate::shaders;

pub struct Raymarch {
    prog: glutil::Program,
    vao: u32,
}

impl Raymarch {
    pub unsafe fn new() -> Self {
        let vs = crate::shaders::FULLSCREEN_VS;
        let fs = shaders::frag(
            r#"
uniform float u_time;
uniform float u_kick;
uniform float u_seed;
uniform float u_fogamt;
uniform vec2 u_res;

vec2 rot2(float a) {
  float s = sin(a), c = cos(a);
  return vec2(c, s);
}

// 6-fold kaleidoscope in xz
vec3 kaleido(vec3 p, float t) {
  vec2 q = p.xz;
  float seg = 3.14159265 / 3.0;
  float a = mod(atan(q.y, q.x) + t * 0.15, seg) - seg * 0.5;
  float r = length(q);
  q = vec2(cos(a), sin(a)) * r;
  return vec3(p.y, q.x, q.y);
}

float map(vec3 p) {
  float br = 1.0 + 0.10 * u_kick;      // breathes with the kick
  vec3 q = p * br;
  // slow counter-rotations
  float a1 = 0.35 * sin(u_time * 0.21 + u_seed);
  q.yz = q.yz * rot2(a1);
  float a2 = 0.25 * cos(u_time * 0.17 + u_seed * 2.0);
  q.xy = q.xy * rot2(a2);

  // level 1: diamond shell
  vec3 k = kaleido(q, u_time * 0.5 + u_seed);
  float d1 = length(abs(k) - vec3(0.42, 0.30, 0.42)) - 0.10;
  // level 2: inner counter-rotated shell (fractal-ish nesting)
  vec3 k2 = kaleido(k * 0.52 - 0.30, -u_time * 0.7 + u_seed * 3.0);
  float d2 = length(abs(k2) - vec3(0.30, 0.34, 0.30)) - 0.08;
  return smin(d1, d2, 0.16);
}

vec3 calcNormal(vec3 p) {
  const float e = 0.0025;
  vec2 h = vec2(1.0, -1.0) * 0.5773;
  return normalize(
      h.xyy * map(p + h.xyy * e) + h.yyx * map(p + h.yyx * e) +
      h.yxy * map(p + h.yxy * e) + h.xxx * map(p + h.xxx * e));
}

float calcAO(vec3 p, vec3 n) {
  float occ = 0.0;
  float sca = 1.0;
  for (int i = 0; i < 4; i++) {
    float hgt = 0.02 + 0.11 * float(i);
    float d = map(p + n * hgt);
    occ += (hgt - d) * sca;
    sca *= 0.7;
  }
  return clamp(1.0 - 2.0 * occ, 0.0, 1.0);
}

void main() {
  vec2 uv = (gl_FragCoord.xy - 0.5 * u_res) / u_res.y;
  float t = u_time * 0.3 + u_seed;
  // orbiting camera with a kick dolly
  float ang = u_time * 0.13 + u_kick * 0.04;
  vec3 ro = vec3(cos(ang), 0.9 + 0.25 * sin(u_time * 0.2), sin(ang)) * (3.4 - 0.25 * u_kick);
  vec3 ta = vec3(0.0);
  vec3 fw = normalize(ta - ro);
  vec3 rt = normalize(cross(vec3(0.0, 1.0, 0.0), fw));
  vec3 up = cross(fw, rt);
  vec3 rd = normalize(fw * 1.6 + rt * uv.x + up * uv.y);

  float tdist = 0.0;
  float glow = 0.0;
  vec3 col = vec3(0.0);
  bool hit = false;
  vec3 p;
  for (int i = 0; i < 96; i++) {
    p = ro + rd * tdist;
    float d = map(p);
    glow += exp(-3.5 * abs(d)) * 0.012;
    if (d < 0.0015 || tdist > 14.0) {
      if (d < 0.0015) hit = true;
      break;
    }
    tdist += d * 0.8;
  }

  // background: deep space with the same palette family
  vec3 bg = pal(uv.x * 0.3 + uv.y * 0.2 + u_time * 0.02,
      vec3(0.02, 0.02, 0.05), vec3(0.05, 0.04, 0.09),
      vec3(1.0), vec3(0.0, 0.33, 0.67));
  col = mix(bg, vec3(0.9, 0.92, 1.0) * glow * 3.0, clamp(glow * 2.0, 0.0, 1.0));

  if (hit) {
    vec3 n = calcNormal(p);
    float ao = calcAO(p, n);
    float fres = pow(1.0 - clamp(dot(n, -rd), 0.0, 1.0), 3.0);
    // iridescent palette driven by the normal + time
    vec3 albedo = pal(n.x * 0.5 + n.y * 0.4 + n.z * 0.3 + u_time * 0.05,
        vec3(0.5), vec3(0.5), vec3(1.0), vec3(0.0, 0.33, 0.67));
    vec3 light = normalize(vec3(0.6, 0.8, -0.4));
    float dif = clamp(dot(n, light), 0.0, 1.0);
    float spe = pow(clamp(dot(reflect(-light, n), -rd), 0.0, 1.0), 24.0);
    col = albedo * (0.25 + 0.75 * dif) * ao;
    col += spe * 0.8 * ao;
    col += fres * albedo * 1.4;
    // core glow bleeds through thin parts
    float core = exp(-2.2 * length(p));
    col += vec3(1.0, 0.85, 0.6) * core * (0.3 + 0.9 * u_kick);
    // distance fog into the flash color
    col = mix(col, vec3(1.0), u_fogamt * smoothstep(4.0, 14.0, tdist));
  }
  frag = vec4(col, 1.0);
}
"#,
        );
        let prog = glutil::build(
            "raymarch",
            vs,
            &fs,
            &["u_time", "u_kick", "u_seed", "u_fogamt", "u_res"],
        );
        // fullscreen quad
        let quad = glutil::Vbo::new(&[-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0]);
        let vao = glutil::new_vao();
        gl::BindVertexArray(vao);
        gl::BindBuffer(gl::ARRAY_BUFFER, quad.id());
        gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 8, std::ptr::null());
        gl::EnableVertexAttribArray(0);
        gl::BindVertexArray(0);
        Raymarch { prog, vao }
    }

    /// Render one raymarch pass into the CURRENTLY BOUND FBO.
    /// `seed` varies the motion (stress passes get different seeds).
    pub unsafe fn draw(
        &self,
        w: u32,
        h: u32,
        time: f32,
        beat: f64,
        kick: f32,
        seed: u32,
        fogamt: f32,
    ) {
        self.prog.use_();
        gl::Uniform1f(self.prog.uniform("u_time"), time);
        // the 16th-note clock gives a subtle pulse under the kick transient
        let pulse = (beat * 0.25).fract().abs() as f32 * 0.12;
        gl::Uniform1f(self.prog.uniform("u_kick"), kick + pulse);
        gl::Uniform1f(self.prog.uniform("u_seed"), seed as f32 * 7.31);
        gl::Uniform1f(self.prog.uniform("u_fogamt"), fogamt);
        gl::Uniform2f(self.prog.uniform("u_res"), w as f32, h as f32);
        gl::BindVertexArray(self.vao);
        gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);
        gl::BindVertexArray(0);
    }
}
