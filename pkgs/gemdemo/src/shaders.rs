//! Shared GLSL (ESSL 300 — the panfrost Mali-T880 serves an ES 3.1
//! context; 300-level source is the safe intersection) + the fullscreen
//! quad vertex shader. Everything is a `&str` assembled per program:
//! `frag(body)` prepends the precision + common helpers.

pub const FULLSCREEN_VS: &str = r#"
#version 300 es
layout(location=0) in vec2 pos;
out vec2 uv;
void main() {
  uv = pos * 0.5 + 0.5;
  gl_Position = vec4(pos, 0.0, 1.0);
}
"#;

/// Common fragment prelude: precision, output, hash/noise/fbm, the IQ
/// cosine palette, small math helpers.
pub fn frag(body: &str) -> String {
    format!(
        r#"#version 300 es
precision highp float;
out vec4 frag;

float hash11(float p) {{
  p = fract(p * 0.1031);
  p *= p + 33.33;
  p *= p + p;
  return fract(p);
}}
float hash21(vec2 p) {{
  vec3 p3 = fract(vec3(p.xyx) * 0.1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.x + p3.y) * p3.z);
}}
vec2 hash22(vec2 p) {{
  vec3 p3 = fract(vec3(p.xyx) * vec3(0.1031, 0.1030, 0.0973));
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.xx + p3.yz) * p3.zy);
}}
float vnoise(vec2 p) {{
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(
    mix(hash21(i), hash21(i + vec2(1.0, 0.0)), u.x),
    mix(hash21(i + vec2(0.0, 1.0)), hash21(i + vec2(1.0, 1.0)), u.x),
    u.y);
}}
float fbm(vec2 p) {{
  float a = 0.5;
  float r = 0.0;
  for (int i = 0; i < 5; i++) {{
    r += a * vnoise(p);
    p = p * 2.03 + vec2(11.7, 5.1);
    a *= 0.5;
  }}
  return r;
}}
vec3 pal(float t, vec3 a, vec3 b, vec3 c, vec3 d) {{
  return a + b * cos(6.28318 * (c * t + d));
}}
mat2 rot(float a) {{
  float s = sin(a), c = cos(a);
  return mat2(c, -s, s, c);
}}
float smin(float a, float b, float k) {{
  float h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
  return mix(b, a, h) - k * h * (1.0 - h);
}}
{body}
"#,
        body = body
    )
}
