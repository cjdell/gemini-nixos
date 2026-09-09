//! The post pipeline (all fullscreen passes, one shared quad):
//!
//!   scene FBO (ssaa res)
//!     → bright pass (half res)      → B1
//!     → blur H 9-tap (half res)     → B2
//!     → blur V 9-tap (half res)     → B1
//!     → composite: scene + bloom + feedback trail (previous composite,
//!       slightly zoomed/rotated — the demoscene warp trail) + chromatic
//!       aberration at the edges + vignette + scanlines + grain + flash
//!       → FINAL (full res)
//!     → resolve FINAL → window
//!     → copy FINAL → FB (half res)   (next frame's feedback source)
//!
//! The feedback trail is what makes the raymarch act smear into light
//! ribbons; in calmer acts the feedback level is near zero so it reads
//! as a subtle afterglow.

use crate::glutil;
use crate::shaders;

pub struct Post {
    bright: glutil::Program,
    blur: glutil::Program,
    composite: glutil::Program,
    fbcopy: glutil::Program,
    quad: glutil::Vbo,
    vao: u32,
    pub fbo_b1: glutil::Fbo, // half res
    pub fbo_b2: glutil::Fbo, // half res
    pub fbo_final: glutil::Fbo, // full res
    pub fbo_fb: glutil::Fbo, // half res (previous composite)
    w: u32,
    h: u32,
}

impl Post {
    pub unsafe fn new() -> Self {
        let quad = glutil::Vbo::new(&[-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0]);
        let vao = {
            let v = glutil::new_vao();
            gl::BindVertexArray(v);
            gl::BindBuffer(gl::ARRAY_BUFFER, quad.id());
            gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 8, std::ptr::null());
            gl::EnableVertexAttribArray(0);
            gl::BindVertexArray(0);
            v
        };

        let vs = crate::shaders::FULLSCREEN_VS;

        let bright = glutil::build(
            "bright",
            vs,
            &shaders::frag(
                r#"
uniform sampler2D u_scene;
in vec2 uv;
void main() {
  vec3 c = texture(u_scene, uv).rgb;
  float l = dot(c, vec3(0.299, 0.587, 0.114));
  frag = vec4(c * smoothstep(0.55, 0.95, l), 1.0);
}
"#,
            ),
            &["u_scene"],
        );

        let blur = glutil::build(
            "blur",
            vs,
            &shaders::frag(
                r#"
uniform sampler2D u_tex;
uniform vec2 u_dir; // px step
in vec2 uv;
void main() {
  vec3 c = texture(u_tex, uv).rgb * 0.227027;
  c += texture(u_tex, uv + u_dir * 1.3846).rgb * 0.316216;
  c += texture(u_tex, uv - u_dir * 1.3846).rgb * 0.316216;
  c += texture(u_tex, uv + u_dir * 3.2308).rgb * 0.070270;
  c += texture(u_tex, uv - u_dir * 3.2308).rgb * 0.070270;
  frag = vec4(c, 1.0);
}
"#,
            ),
            &["u_tex", "u_dir"],
        );

        let composite = glutil::build(
            "composite",
            vs,
            &shaders::frag(
                r#"
uniform sampler2D u_scene;
uniform sampler2D u_bloom;
uniform sampler2D u_fb;
uniform float u_bloomamt;
uniform float u_feedback;
uniform float u_flash;
uniform float u_kick;
uniform float u_time;
in vec2 uv;
void main() {
  // chromatic aberration grows toward the edges
  vec2 c = uv - 0.5;
  float r2 = dot(c, c);
  vec2 off = c * (0.0012 + 0.004 * r2) * (1.0 + u_kick * 1.5);
  vec3 col;
  col.r = texture(u_scene, uv + off).r;
  col.g = texture(u_scene, uv).g;
  col.b = texture(u_scene, uv - off).b;
  col += texture(u_bloom, uv).rgb * u_bloomamt * (0.75 + 0.5 * u_kick);

  // feedback trail: previous frame, zoomed + rotated a hair
  vec2 fuv = (uv - 0.5) * rot(0.0012 * (1.0 - u_feedback)) * (1.0 + 0.004);
  fuv += 0.5;
  vec3 fb = texture(u_fb, fuv).rgb;
  col += fb * u_feedback;

  // vignette
  float vig = smoothstep(1.35, 0.45, length(c) * 1.7);
  col *= mix(0.55, 1.0, vig);

  // scanlines (subtle) + film grain
  col *= 0.96 + 0.04 * sin(gl_FragCoord.y * 3.14159);
  col += (hash21(gl_FragCoord.xy + u_time * 61.7) - 0.5) * 0.035;

  // white flash (act cuts / F key)
  col = mix(col, vec3(1.0, 0.98, 0.95), clamp(u_flash, 0.0, 1.0));
  frag = vec4(col, 1.0);
}
"#,
            ),
            &[
                "u_scene",
                "u_bloom",
                "u_fb",
                "u_bloomamt",
                "u_feedback",
                "u_flash",
                "u_kick",
                "u_time",
            ],
        );

        // plain copy (resolve + feedback downsample share it)
        let fbcopy = glutil::build(
            "fbcopy",
            vs,
            &shaders::frag(
                r#"
uniform sampler2D u_tex;
in vec2 uv;
void main() {
  frag = texture(u_tex, uv);
}
"#,
            ),
            &["u_tex"],
        );

        let p = Post {
            bright,
            blur,
            composite,
            fbcopy,
            quad,
            vao,
            fbo_b1: glutil::Fbo::new(1, 1),
            fbo_b2: glutil::Fbo::new(1, 1),
            fbo_final: glutil::Fbo::new(1, 1),
            fbo_fb: glutil::Fbo::new(1, 1),
            w: 1,
            h: 1,
        };
        p
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if (w, h) == (self.w, self.h) {
            return;
        }
        self.w = w;
        self.h = h;
        unsafe {
            self.fbo_b1.resize((w / 2).max(1), (h / 2).max(1));
            self.fbo_b2.resize((w / 2).max(1), (h / 2).max(1));
            self.fbo_final.resize(w, h);
            self.fbo_fb.resize((w / 2).max(1), (h / 2).max(1));
        }
    }

    unsafe fn draw_pass(&self, prog: &glutil::Program, w: u32, h: u32) {
        prog.use_();
        gl::Viewport(0, 0, w as i32, h as i32);
        gl::BindVertexArray(self.vao);
        gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);
        gl::BindVertexArray(0);
    }

    pub unsafe fn render(
        &self,
        scene: &glutil::Fbo,
        bloom: f32,
        feedback: f32,
        flash: f32,
        kick: f32,
        time: f32,
        w: u32,
        h: u32,
    ) {
        let hw = (w / 2).max(1);
        let hh = (h / 2).max(1);
        gl::Disable(gl::BLEND);

        // 1) bright (half res)
        gl::BindFramebuffer(gl::FRAMEBUFFER, self.fbo_b1.id());
        self.bright.use_();
        gl::Uniform1i(self.bright.uniform("u_scene"), 0);
        gl::ActiveTexture(gl::TEXTURE0);
        gl::BindTexture(gl::TEXTURE_2D, scene.tex());
        self.draw_pass(&self.bright, hw, hh);

        // 2) blur H → B2, blur V → B1
        gl::BindFramebuffer(gl::FRAMEBUFFER, self.fbo_b2.id());
        self.blur.use_();
        gl::Uniform1i(self.blur.uniform("u_tex"), 0);
        gl::BindTexture(gl::TEXTURE_2D, self.fbo_b1.tex());
        gl::Uniform2f(self.blur.uniform("u_dir"), 1.0 / hw as f32, 0.0);
        self.draw_pass(&self.blur, hw, hh);

        gl::BindFramebuffer(gl::FRAMEBUFFER, self.fbo_b1.id());
        gl::BindTexture(gl::TEXTURE_2D, self.fbo_b2.tex());
        gl::Uniform2f(self.blur.uniform("u_dir"), 0.0, 1.0 / hh as f32);
        self.draw_pass(&self.blur, hw, hh);

        // 3) composite → FINAL
        gl::BindFramebuffer(gl::FRAMEBUFFER, self.fbo_final.id());
        self.composite.use_();
        gl::Uniform1i(self.composite.uniform("u_scene"), 0);
        gl::Uniform1i(self.composite.uniform("u_bloom"), 1);
        gl::Uniform1i(self.composite.uniform("u_fb"), 2);
        gl::ActiveTexture(gl::TEXTURE0);
        gl::BindTexture(gl::TEXTURE_2D, scene.tex());
        gl::ActiveTexture(gl::TEXTURE1);
        gl::BindTexture(gl::TEXTURE_2D, self.fbo_b1.tex());
        gl::ActiveTexture(gl::TEXTURE2);
        gl::BindTexture(gl::TEXTURE_2D, self.fbo_fb.tex());
        gl::Uniform1f(self.composite.uniform("u_bloomamt"), bloom);
        gl::Uniform1f(self.composite.uniform("u_feedback"), feedback);
        gl::Uniform1f(self.composite.uniform("u_flash"), flash);
        gl::Uniform1f(self.composite.uniform("u_kick"), kick);
        gl::Uniform1f(self.composite.uniform("u_time"), time);
        self.draw_pass(&self.composite, w, h);

        // 4) resolve FINAL → window
        gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
        self.fbcopy.use_();
        gl::Uniform1i(self.fbcopy.uniform("u_tex"), 0);
        gl::ActiveTexture(gl::TEXTURE0);
        gl::BindTexture(gl::TEXTURE_2D, self.fbo_final.tex());
        self.draw_pass(&self.fbcopy, w, h);

        // 5) feedback: FINAL → FB (half res, straight copy; the warp is
        //    applied when the composite samples it)
        gl::BindFramebuffer(gl::FRAMEBUFFER, self.fbo_fb.id());
        gl::BindTexture(gl::TEXTURE_2D, self.fbo_final.tex());
        self.draw_pass(&self.fbcopy, hw, hh);
    }
}
