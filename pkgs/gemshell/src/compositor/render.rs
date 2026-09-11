//! GL rendering — EGL (Mesa, via libglvnd) on the gbm device, GLES.
//!
//! Immediate-mode quad drawing: one program (textured quad, y-down
//! pixel coords), every draw call renders one shape (a quad, or a fan
//! of quads for circles/arcs/lines). ~100 small draw calls per frame —
//! trivial for the T880.
//!
//! - solid shapes use a 4x4 white texture, straight-alpha blend
//! - window content: straight RGBA (converted from SHM XRGB/ARGB)
//! - glyph atlas: premultiplied white (r=g=b=a=coverage)
//! - icons: premultiplied RGBA
//!
//! The blend function is switched per category (premultiplied shapes
//! use (ONE, ONE_MINUS_SRC_ALPHA); straight uses (SRC_ALPHA,
//! ONE_MINUS_SRC_ALPHA)).

use std::collections::HashMap;
use std::ffi::{CString};
use std::os::raw::{c_char, c_int, c_void};

type EGLDisplay = *mut c_void;
type EGLContext = *mut c_void;
type EGLSurface = *mut c_void;
// EGLConfig is an opaque HANDLE (a pointer in Mesa), not the pointed-to
// object. Declaring it as `c_void` (1 byte) made `[EGLConfig; 64]` a
// 64-byte array while eglChooseConfig wrote 64 * 8 bytes of handles into
// it — stack corruption, the SEGV right after "config selected"
// (2026-09-11). It also made the eglCreateContext argument the address
// of the slot rather than the handle.
type EGLConfig = *mut c_void;

extern "C" {
    fn eglInitialize(d: EGLDisplay, major: *mut c_int, minor: *mut c_int) -> u32;
    fn eglBindAPI(api: u32) -> u32;
    fn eglChooseConfig(
        d: EGLDisplay,
        attrib_list: *const c_int,
        configs: *mut EGLConfig,
        config_count: c_int,
        num_configs: *mut c_int,
    ) -> u32;
    fn eglCreateContext(
        d: EGLDisplay,
        config: EGLConfig,
        share_context: EGLContext,
        attrib_list: *const c_int,
    ) -> EGLContext;
    fn eglMakeCurrent(d: EGLDisplay, draw: EGLSurface, read: EGLSurface, ctx: EGLContext) -> u32;
    fn eglCreatePbufferSurface(d: EGLDisplay, config: *const c_void, w: c_int, h: c_int) -> EGLSurface;
    fn eglQueryString(d: EGLDisplay, name: u32) -> *const c_char;
    fn eglSwapBuffers(d: EGLDisplay, surface: EGLSurface) -> u32;
    fn eglGetError() -> c_int;
    fn eglDestroySurface(d: EGLDisplay, s: EGLSurface) -> u32;
    fn eglDestroyContext(d: EGLDisplay, c: EGLContext) -> u32;
    fn eglTerminate(d: EGLDisplay) -> u32;
    fn eglGetProcAddress(name: *const c_char) -> *mut c_void;
}

// EGL enums — values from the Khronos EGL headers (verified against
// libglvnd 1.7.0 `include/EGL/egl.h`/`eglext.h`, 2026-09-11).
//
// These were ALL WRONG in the first cut (a hall of hallucinated
// constants: EGL_NONE = 0 instead of 0x3038, EGL_RENDERABLE_TYPE =
// 0x3095 instead of 0x3040, dma_buf attrs 0x32D5.. instead of
// 0x3270..). That single class of bug was the real cause of the
// on-glass crash loop: `eglChooseConfig` was handed the list [0],
// 0 is NOT EGL_NONE (0x3038), so the display parsed it as an unknown
// attribute and returned EGL_BAD_ATTRIBUTE (0x3004) with 0 configs —
// not a driver/ICD problem at all. The EGL_VENDOR_PATH/DRI_DRIVER_DIR
// work that chased it stays (they are genuinely needed on NixOS), but
// the config failure was here.
const EGL_PLATFORM_GBM_MESA: u32 = 0x31D7;
// EGL_MESA_platform_surfaceless — host/nested rendering with no DRM
// device (src/compositor/nested.rs + docs/gemshell.md "nested mode").
const EGL_PLATFORM_SURFACELESS_MESA: u32 = 0x31DD;
const EGL_SURFACE_TYPE: u32 = 0x3033;
const EGL_PBUFFER_BIT: u32 = 0x0001;
const EGL_WINDOW_BIT: u32 = 0x0004;
const EGL_OPENGL_ES_API: u32 = 0x30A0;
const EGL_OPENGL_ES3_BIT: c_int = 0x00000040;
const EGL_RENDERABLE_TYPE: c_int = 0x3040;
const EGL_RED_SIZE: c_int = 0x3024;
const EGL_WIDTH: c_int = 0x3057;
const EGL_HEIGHT: c_int = 0x3056;
const EGL_CONTEXT_CLIENT_VERSION: c_int = 0x3098;
const EGL_NONE: c_int = 0x3038;
const EGL_TRUE: u32 = 1;
const EGL_EXTENSIONS: u32 = 0x3055;
// EGL_EXT_image_dma_buf_import (resolved through eglGetProcAddress —
// libglvnd's libEGL exports no extension entry points, verified
// 2026-09-11; the mesa-geminipda ICD answers them).
const EGL_LINUX_DMA_BUF_EXT: u32 = 0x3270;
const EGL_LINUX_DRM_FOURCC_EXT: c_int = 0x3271;
const EGL_DMA_BUF_PLANE0_FD_EXT: c_int = 0x3272;
const EGL_DMA_BUF_PLANE0_OFFSET_EXT: c_int = 0x3273;
const EGL_DMA_BUF_PLANE0_PITCH_EXT: c_int = 0x3274;
// GL
const DRM_FORMAT_ABGR8888: u32 = 0x34324241; // 'ABGR'
const GL_FRAMEBUFFER: u32 = 0x8D40;
const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
const GL_TEXTURE0: u32 = 0x84C0;
const GL_TEXTURE1: u32 = 0x84C1;
const GL_WRITE_ONLY: u32 = 0x88B9;
// Values re-audited against the Khronos headers 2026-09-11 (these four
// were also hallucinated: GL_RGBA8 0x8D53 (is 0x8058), the framebuffer
// barrier 0x20 (is 0x400), compute shader 0x8DA2 (is 0x91B9) and
// GL_BLEND 0x0BE0 (is 0x0BE2 — 0xBE0 is GL_BLEND_DST, so glEnable
// silently failed and NOTHING alpha-blended).
const GL_RGBA8: u32 = 0x8058;
const GL_FRAMEBUFFER_BARRIER_BIT: u32 = 0x00000400;
const GL_COMPUTE_SHADER: u32 = 0x91B9;
// The LK framebuffer geometry (geminipda-fb.c receipts, verified on
// glass 2026-08-31 / gemwl 2026-09-01): 1080x2160 portrait, 1088-px
// (4352-byte) row pitch — the trailing 8 px/row are never scanned out.
const FB_W: u32 = 1080;
const FB_H: u32 = 2160;
const FB_PITCH: u32 = 1088 * 4;
// /dev/gemfb ioctl(GEMFB_IOC_EXPORT) -> dma-buf fd of the LK fb region
// (_IOW('G', 1, int); geminipda-fb.c).
const GEMFB_IOC_EXPORT: u64 = 0x4004_4701;

// GL enums
const GL_FLOAT: u32 = 0x1406;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_RGBA: u32 = 0x1908;
const GL_NEAREST: u32 = 0x2600;
const GL_LINEAR: u32 = 0x2601;
const GL_TEXTURE_MAG_FILTER: u32 = 0x2800;
const GL_TEXTURE_MIN_FILTER: u32 = 0x2801;
const GL_TEXTURE_WRAP_S: u32 = 0x2802;
const GL_TEXTURE_WRAP_T: u32 = 0x2803;
const GL_CLAMP_TO_EDGE: u32 = 0x812F;
const GL_BLEND: u32 = 0x0BE2;
const GL_ONE: u32 = 1;
const GL_ONE_MINUS_SRC_ALPHA: u32 = 0x0303;
const GL_SRC_ALPHA: u32 = 0x0302;
const GL_DEPTH_TEST: u32 = 0x0B71;
const GL_CULL_FACE: u32 = 0x0B44;
const GL_COLOR_BUFFER_BIT: u32 = 0x4000;
const GL_ARRAY_BUFFER: u32 = 0x8892;
const GL_DYNAMIC_DRAW: u32 = 0x88E8;
const GL_TRIANGLE_STRIP: u32 = 5;
const GL_VERTEX_SHADER: u32 = 0x8B31;
const GL_FRAGMENT_SHADER: u32 = 0x8B30;
const GL_COMPILE_STATUS: u32 = 0x8B81;
const GL_LINK_STATUS: u32 = 0x8B82;
const GL_VERSION: u32 = 0x1F02;
// egui mesh drawing (glDrawElements + scissor + premultiplied blend)
const GL_ELEMENT_ARRAY_BUFFER: u32 = 0x8893;
const GL_TRIANGLES: u32 = 0x0004;
const GL_UNSIGNED_INT: u32 = 0x1405;
const GL_SCISSOR_TEST: u32 = 0x0C11;
const GL_ONE_MINUS_DST_ALPHA: u32 = 0x0305;
const GL_BLEND_EQUATION: u32 = 0x8009;
const GL_FUNC_ADD: u32 = 0x8006;

extern "C" {
    fn glCreateShader(t: u32) -> u32;
    fn glShaderSource(s: u32, n: c_int, strings: *const *const c_char, lengths: *const c_int);
    fn glCompileShader(s: u32);
    fn glGetShaderiv(s: u32, p: u32, v: *mut c_int);
    fn glGetShaderInfoLog(s: u32, max: c_int, len: *mut c_int, log: *mut c_void);
    fn glCreateProgram() -> u32;
    fn glAttachShader(p: u32, s: u32);
    fn glLinkProgram(p: u32);
    fn glGetProgramiv(p: u32, p2: u32, v: *mut c_int);
    fn glGetProgramInfoLog(p: u32, max: c_int, len: *mut c_int, log: *mut c_void);
    fn glUseProgram(p: u32);
    fn glGetUniformLocation(p: u32, name: *const c_char) -> c_int;
    fn glUniform2f(l: c_int, x: f32, y: f32);
    fn glUniform4f(l: c_int, r: f32, g: f32, b: f32, a: f32);
    fn glGenTextures(n: c_int, t: *mut u32);
    fn glDeleteTextures(n: c_int, t: *const u32);
    fn glBindTexture(t: u32, tex: u32);
    fn glTexImage2D(
        t: u32,
        level: c_int,
        internal: c_int,
        w: c_int,
        h: c_int,
        border: c_int,
        format: u32,
        ty: u32,
        data: *const c_void,
    );
    fn glTexSubImage2D(
        t: u32,
        level: c_int,
        x: c_int,
        y: c_int,
        w: c_int,
        h: c_int,
        format: u32,
        ty: u32,
        data: *const c_void,
    );
    fn glTexParameteri(t: u32, p: u32, v: c_int);
    fn glGenBuffers(n: c_int, b: *mut u32);
    fn glBindBuffer(t: u32, b: u32);
    fn glBufferData(t: u32, size: i64, data: *const c_void, usage: u32);
    fn glVertexAttribPointer(i: c_int, size: c_int, ty: u32, normalized: u32, stride: u32, offset: u32);
    fn glEnableVertexAttribArray(i: c_int);
    fn glGetAttribLocation(p: u32, name: *const c_char) -> c_int;
    fn glDeleteBuffers(n: c_int, b: *const u32);
    fn glDrawArrays(mode: u32, first: c_int, count: c_int);
    fn glClearColor(r: f32, g: f32, b: f32, a: f32);
    fn glClear(mask: u32);
    fn glEnable(cap: u32);
    fn glDisable(cap: u32);
    fn glBlendFunc(s: u32, d: u32);
    fn glViewport(x: c_int, y: c_int, w: c_int, h: c_int);
    fn glGetString(name: u32) -> *const c_char;
    // FBO + compute (the GPU-direct present path, gemwl-verified).
    fn glGenFramebuffers(n: c_int, f: *mut u32);
    fn glBindFramebuffer(t: u32, f: u32);
    fn glFramebufferTexture2D(t: u32, a: u32, tt: u32, tex: u32, level: c_int);
    fn glActiveTexture(t: u32);
    fn glBindImageTexture(unit: u32, tex: u32, level: c_int, layered: u8, layer: i32, access: u32, format: u32);
    fn glDispatchCompute(x: u32, y: u32, z: u32);
    fn glMemoryBarrier(bits: u32);
    fn glFinish();
    fn glUniform1i(l: c_int, v: c_int);
    fn glUniform2i(l: c_int, x: c_int, y: c_int);
    fn glDeleteFramebuffers(n: c_int, f: *const u32);
    fn glDeleteShader(s: u32);
    fn glDeleteProgram(p: u32);
    fn glReadPixels(x: c_int, y: c_int, w: c_int, h: c_int, format: u32, ty: u32, data: *mut c_void);
    // egui meshes: indexed triangles + clip rects
    fn glDrawElements(mode: u32, count: c_int, ty: u32, indices: *const c_void);
    fn glScissor(x: c_int, y: c_int, w: c_int, h: c_int);
    fn glBlendEquation(mode: u32);
    fn glBlendFuncSeparate(sr: u32, dr: u32, sa: u32, da: u32);
}

// EGL/GL extension entry points — NOT exported by libglvnd (verified
// 2026-09-11 on the store libs; the aarch64 link died on both), so they
// are resolved at runtime through eglGetProcAddress, which the libglvnd
// dispatch answers from the mesa-geminipda ICD (which must provide
// EGL_EXT_image_dma_buf_import + GL_OES_EGL_image — the gemwl ICD
// receipts). Resolved in Renderer::new, stored on the struct.
type EglImage = *mut c_void;
type EglCreateImageFn = unsafe extern "C" fn(
    EGLDisplay,
    EGLContext,
    u32,
    *const c_void,
    *const c_int,
) -> EglImage;
type GlEglImageTargetFn = unsafe extern "C" fn(u32, EglImage);

/// The GPU blit shader: scene FBO texture -> LK fb image (the gemwl
/// GEMFB_COPY_CS, verified on glass 2026-09-01). The `.bgra` swizzle is
/// load-bearing: the LK OVL scans the fb as a8r8g8b8 (byte0 = B), so the
/// imageStore must land B in byte0; texelFetch decodes the GL texture to
/// correct RGBA. (gemwl receipt: without it the whole desktop is R/B
/// swapped.) The COMPUTE path is also load-bearing: fragment draws/blits
/// into the 1088x2160 LINEAR fb target clip to ~1024x1024 on this
/// tiler (gemwl A/B, 2026-09-01).
/// egui mesh shader: pixel-space triangles -> NDC (Y flipped, scene is
/// top-left origin), vertex colour (egui's premultiplied sRGBA, 0-255
/// bytes normalized by the VS) times the sampled texture.
const EGUI_VERT: &str = r#"
attribute vec2 aPos;
attribute vec2 aUv;
attribute vec4 aColor;     // 0-255; GL normalizes when normalized=0
uniform vec2 uRes;         // scene size in pixels
varying vec2 vUv;
varying vec4 vColor;
void main() {
    vec2 ndc = (aPos / uRes) * 2.0 - 1.0;
    gl_Position = vec4(ndc.x, -ndc.y, 0.0, 1.0);
    vUv = aUv;
    vColor = aColor / 255.0;
}
"#;

const EGUI_FRAG: &str = r#"
precision mediump float;
varying vec2 vUv;
varying vec4 vColor;
uniform sampler2D uTex;
void main() {
    gl_FragColor = vColor * texture2D(uTex, vUv);
}
"#;

const COPY_CS: &str = r#"
#version 310 es
layout(local_size_x = 16, local_size_y = 8) in;
layout(rgba8, binding = 0) uniform highp writeonly image2D dst;
layout(binding = 1) uniform highp sampler2D src;
uniform ivec2 fbSize;   // destination (LK fb) size
uniform ivec2 srcSize;  // source (logical scene) size
uniform int rotation;   // 0/90/180/270 (counter-rotation of the panel)
void main() {
    ivec2 p = ivec2(gl_GlobalInvocationID.xy);
    if (p.x >= fbSize.x || p.y >= fbSize.y) return;
    // Map the fb pixel back to a scene texel (inverse of the panel
    // counter-rotation; see docs/gemshell.md "Orientation").
    ivec2 s;
    if (rotation == 90) {
        s = ivec2(p.y, fbSize.x - 1 - p.x);
    } else if (rotation == 180) {
        s = ivec2(srcSize.x - 1 - p.x, srcSize.y - 1 - p.y);
    } else if (rotation == 270) {
        s = ivec2(fbSize.y - 1 - p.y, p.x);
    } else {
        s = p;
    }
    if (s.x < 0 || s.y < 0 || s.x >= srcSize.x || s.y >= srcSize.y) return;
    // The scene FBO texture is stored BOTTOM-UP (a GL framebuffer's
    // origin is lower-left, so scene top row is the last texel row).
    // texelFetch indexes that storage directly, so flip the scene Y.
    // Omitting this turned the intended 90-degree rotation into a
    // transpose — the on-glass left-right mirror (2026-09-12).
    ivec2 t = ivec2(s.x, srcSize.y - 1 - s.y);
    imageStore(dst, p, texelFetch(src, t, 0).bgra);
}
"#;

const VERT: &str = r#"
attribute vec2 aPos;
attribute vec2 aUv;
uniform vec2 uRes;
varying vec2 vUv;
void main() {
    vec2 ndc = (aPos / uRes) * 2.0 - 1.0;
    gl_Position = vec4(ndc.x, -ndc.y, 0.0, 1.0);
    vUv = aUv;
}
"#;

const FRAG: &str = r#"
precision mediump float;
varying vec2 vUv;
uniform sampler2D uTex;
uniform vec4 uColor;
void main() {
    gl_FragColor = texture2D(uTex, vUv) * uColor;
}
"#;

/// An rgba color, 0..=1.
#[derive(Clone, Copy, Debug)]
pub struct Color(pub [f32; 4]);

impl Color {
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Color([r, g, b, a])
    }
}

/// pos.xy + uv.xy per vertex, y-down pixel coordinates.
#[derive(Clone, Copy)]
struct Vert {
    x: f32,
    y: f32,
    u: f32,
    v: f32,
}

impl Vert {
    fn as_f32(&self) -> [f32; 4] {
        [self.x, self.y, self.u, self.v]
    }
}

/// A recorded draw operation (the compositor records a frame into a
/// Vec<Op> while immutably borrowing window state, then replays it).
#[derive(Clone)]
pub enum Op {
    Rect { x: f32, y: f32, w: f32, h: f32, c: Color },
    RectTex {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        u0: f32,
        v0: f32,
        u1: f32,
        v1: f32,
        tex: u32,
        c: Color,
        premult: bool,
    },
    Circle { cx: f32, cy: f32, r: f32, c: Color },
    Arc { cx: f32, cy: f32, r0: f32, r1: f32, a0: f32, a1: f32, c: Color },
    Line { x0: f32, y0: f32, x1: f32, y1: f32, t: f32, c: Color },
    Text { x: f32, baseline: f32, s: String, c: Color },
    TextCentered { x: f32, w: f32, baseline: f32, s: String, c: Color },
    TextClipped { x: f32, w: f32, baseline: f32, s: String, c: Color },
}

/// Resolve `eglGetPlatformDisplayEXT` through `eglGetProcAddress`
/// (libglvnd's libEGL exports no extension entry points — verified
/// 2026-09-11 — the vendor ICD answers it) and create a platform
/// display. `native` is the GBM device pointer for the GBM platform,
/// NULL for surfaceless.
unsafe fn platform_display(platform: u32, native: *mut c_void) -> EGLDisplay {
    type PlatformDisplayFn =
        unsafe extern "C" fn(u32, *mut c_void, *const c_int) -> EGLDisplay;
    let f: PlatformDisplayFn = std::mem::transmute(eglGetProcAddress(
        b"eglGetPlatformDisplayEXT\0".as_ptr() as *const c_char,
    ));
    f(platform, native, std::ptr::null())
}

pub struct Renderer {
    pub width: u32,
    pub height: u32,
    /// UI scale (1.0 / 1.5 / 2.0). The compositor lays out in LOGICAL
    /// units (`width/ui_scale` × `height/ui_scale`) and this shader maps
    /// them across the full physical viewport. See `Compositor::ui_scale`.
    pub ui_scale: f32,
    display: EGLDisplay,
    context: EGLContext,
    /// surfaceless (EGL_NO_SURFACE) — see the attrs comment above
    surface: EGLSurface,
    program: u32,
    a_pos: c_int,
    a_uv: c_int,
    u_res: c_int,
    u_color: c_int,
    vbo: u32,
    white_tex: u32,
    pub glyph_tex: u32,
    /// window id -> (texture, width, height)
    win_tex: HashMap<u32, (u32, u32, u32)>,
    verts: Vec<Vert>,
    colors: Vec<Color>,
    // GPU-direct present (gemwl chain): the scene renders into a
    // fullscreen FBO texture; a compute shader copies it into the LK
    // framebuffer (imported from /dev/gemfb as a dma-buf EGLImage).
    fbo: u32,
    fbo_tex: u32,
    fb_fd: c_int,      // LK fb dma-buf (GEMFB_IOC_EXPORT)
    fb_image: EglImage,
    fb_tex: u32,        // texture over fb_image (imageStore target)
    cprog: u32,
    c_src: c_int,
    c_size: c_int,
    c_srcsize: c_int,
    c_rot: c_int,
    /// Present rotation into the LK fb, degrees (0/90/180/270).
    /// The LK fb is a PORTRAIT 1080x2160 buffer while the product is a
    /// landscape clamshell; gemwl uses WL_OUTPUT_TRANSFORM_90 for the
    /// same reason (geminipda-drm defaults to panel-orientation LEFT_UP).
    /// Default 270 (changed from 90 on glass 2026-09-12: 90 rendered the
    /// scene 180° out). MUST match `GEMSHELL_TOUCH_ROTATE` in input.rs.
    /// Override with GEMSHELL_ROTATE for on-glass calibration.
    rotation: i32,
    eglCreateImageKHR: EglCreateImageFn,
    glEGLImageTargetTexture2DOES: GlEglImageTargetFn,
    // egui mesh drawing (the shell UI): one program + an element buffer;
    // texture deltas are uploaded into `egui_textures`.
    egui_prog: u32,
    egui_a_pos: c_int,
    egui_a_uv: c_int,
    egui_a_color: c_int,
    egui_u_res: c_int,
    egui_u_tex: c_int,
    egui_ebo: u32,
    egui_textures: HashMap<egui::TextureId, u32>,
}

impl Renderer {
    /// Device renderer: EGL on the GBM device (`renderD128`/panfrost),
    /// scene FBO, plus the LK-fb present chain. `gbm_device` is the
    /// pointer vended by `gbm::Gbm`.
    pub fn new(
        gbm_device: *mut c_void,
        width: u32,
        height: u32,
        glyph_pixels: &[u8],
        glyph_size: u32,
    ) -> Result<Self, String> {
        let display = unsafe { platform_display(EGL_PLATFORM_GBM_MESA, gbm_device) };
        if display.is_null() {
            return Err(format!(
                "eglGetPlatformDisplay(GBM) failed (error 0x{:x})",
                unsafe { eglGetError() }
            ));
        }
        Self::from_display(display, width, height, glyph_pixels, glyph_size, true)
    }

    /// Host/nested renderer: EGL GBM on the HOST render node (`gbm_device`
    /// from `Gbm::new("/dev/dri/renderD128")` — radeonsi on the dev box),
    /// scene FBO only — no `/dev/gemfb`, no compute blit. The nested
    /// client presents the scene over Wayland
    /// (src/compositor/nested.rs). x86_64 dev only.
    ///
    /// (EGL_PLATFORM_SURFACELESS_MESA returns a valid Mesa display but
    /// ZERO configs on this multi-GPU host, verified 2026-09-11 — GBM on
    /// an explicit render node is the reliable headless path, and is what
    /// the device path already uses.)
    pub fn new_host(
        gbm_device: *mut c_void,
        width: u32,
        height: u32,
        glyph_pixels: &[u8],
        glyph_size: u32,
    ) -> Result<Self, String> {
        let display = unsafe { platform_display(EGL_PLATFORM_GBM_MESA, gbm_device) };
        if display.is_null() {
            return Err(format!(
                "eglGetPlatformDisplay(host GBM) failed (error 0x{:x})",
                unsafe { eglGetError() }
            ));
        }
        Self::from_display(display, width, height, glyph_pixels, glyph_size, false)
    }

    fn from_display(
        display: EGLDisplay,
        width: u32,
        height: u32,
        glyph_pixels: &[u8],
        glyph_size: u32,
        with_fb: bool,
    ) -> Result<Self, String> {
        // The config: ES3-renderable; the context is created surfaceless
        // (makeCurrent with EGL_NO_SURFACE, legal in EGL 1.5) because we
        // render into a GL FBO. At one point EGL_SURFACE_TYPE/
        // EGL_WINDOW_BIT were requested too; the GBM platform has no
        // window/pbuffer surface type and that was the 0x3004
        // (EGL_BAD_MATCH) from eglChooseConfig (on glass, 2026-09-11),
        // so only renderable_type is requested.
        let mut attrs = [
            EGL_RENDERABLE_TYPE,
            EGL_OPENGL_ES3_BIT,
            EGL_NONE,
        ];
        let (mut maj, mut min) = (0i32, 0i32);
        if unsafe { eglInitialize(display, &mut maj, &mut min) } != EGL_TRUE {
            return Err(format!("eglInitialize failed (error 0x{:x})", unsafe { eglGetError() }));
        }
        log::info!("EGL {maj}.{min}");
        // Vendor + API strings — distinguish the libglvnd STUB display
        // (no ICD loaded) from a real ICD display (diagnostic, 2026-09-11).
        let v = unsafe { eglQueryString(display, 0x3053) };
        let a = unsafe { eglQueryString(display, 0x3054) };
        let v = if v.is_null() { "<null>".to_string() } else { unsafe { std::ffi::CStr::from_ptr(v) }.to_string_lossy().into_owned() };
        let a = if a.is_null() { "<null>".to_string() } else { unsafe { std::ffi::CStr::from_ptr(a) }.to_string_lossy().into_owned() };
        log::info!("EGL vendor: {v} — api: {a}");
        let dev0 = unsafe { eglQueryString(display, EGL_EXTENSIONS) };
        let dev0 = if dev0.is_null() { "<null>".to_string() } else { unsafe { std::ffi::CStr::from_ptr(dev0) }.to_string_lossy().into_owned() };
        log::info!("EGL extensions: {}", if dev0.len() > 400 { format!("{}...", &dev0[..400]) } else { dev0 });
        if unsafe { eglBindAPI(EGL_OPENGL_ES_API) } != EGL_TRUE {
            return Err("eglBindAPI(ES) failed".into());
        }
        // Diagnostic (added 2026-09-11, the 0x3004 hunt): how many
        // configs does this display have AT ALL, and what do they say?
        // (eglGetConfigAttrib is NOT exported by libglvnd's libEGL —
        // runtime-resolved, like the other EGL ext entry points. Note the
        // real name is Attr, not Attribute.)
        type GetConfigAttrFn = Option<unsafe extern "C" fn(EGLDisplay, *const c_void, c_int, *mut c_int) -> u32>;
        let get_config_attr: GetConfigAttrFn = unsafe {
            std::mem::transmute(eglGetProcAddress(
                b"eglGetConfigAttrib\0".as_ptr() as *const c_char,
            ))
        };
        let mut all: [EGLConfig; 64] = unsafe { std::mem::zeroed() };
        let mut nall = 0i32;
        let none = [EGL_NONE];
        let ok_all = unsafe { eglChooseConfig(display, none.as_ptr(), all.as_mut_ptr(), 64, &mut nall) };
        log::info!("eglChooseConfig(EGL_NONE): {} (n={nall})", if ok_all == EGL_TRUE { "ok" } else { "FAIL" });
        if let Some(get_config_attr) = get_config_attr {
            if ok_all == EGL_TRUE {
                for (i, cfg) in all.iter().take(nall as usize).enumerate() {
                    let (mut rt, mut st, mut r) = (0i32, 0i32, 0i32);
                    unsafe { get_config_attr(display, *cfg, EGL_RENDERABLE_TYPE, &mut rt) };
                    unsafe { get_config_attr(display, *cfg, EGL_SURFACE_TYPE as c_int, &mut st) };
                    unsafe { get_config_attr(display, *cfg, EGL_RED_SIZE, &mut r) };
                    log::info!("  config[{i}]: renderable_type=0x{rt:x} surface_type=0x{st:x} red={r}");
                }
            }
        }
        log::info!("selecting config (ES3 renderable + window surface type)...");
        let mut config: EGLConfig = std::ptr::null_mut();
        let mut nconf = 0i32;
        if unsafe {
            eglChooseConfig(
                display,
                attrs.as_ptr(),
                &mut config as *mut EGLConfig,
                1,
                &mut nconf,
            )
        } != EGL_TRUE
            || nconf < 1
        {
            return Err(format!("eglChooseConfig failed (error 0x{:x})", unsafe {
                eglGetError()
            }));
        }
        log::info!("config selected (n={nconf})");
        let mut ctx_attrs = [EGL_CONTEXT_CLIENT_VERSION, 3, EGL_NONE];
        let context = unsafe {
            eglCreateContext(
                display,
                config,
                std::ptr::null_mut(),
                ctx_attrs.as_ptr(),
            )
        };
        if context.is_null() {
            return Err(format!("eglCreateContext failed (error 0x{:x})", unsafe {
                eglGetError()
            }));
        }
        log::info!("context created");
        // Surfaceless context (no pbuffer on the GBM platform): the FBO
        // is the render target; present() is the compute blit.
        if unsafe { eglMakeCurrent(display, std::ptr::null_mut(), std::ptr::null_mut(), context) } != EGL_TRUE {
            return Err(format!("eglMakeCurrent(surfaceless) failed (error 0x{:x})", unsafe {
                eglGetError()
            }));
        }
        log::info!("context current (surfaceless)");

        // --- GL ---
        gl::load_with(|name| unsafe {
            let c = CString::new(name).unwrap();
            eglGetProcAddress(c.as_ptr()) as *const c_void
        });
        log::info!("GL entry points loaded");

        let version = unsafe { glGetString(GL_VERSION) };
        let version = if version.is_null() {
            "unknown".to_string()
        } else {
            // GL owns this string until the next call — do NOT from_raw
            unsafe { std::ffi::CStr::from_ptr(version) }.to_string_lossy().into_owned()
        };
        log::info!("GL: {version}");

        let program = build_program(VERT, FRAG)?;
        log::info!("main program built");
        // ATTRIBUTES need glGetAttribLocation, uniforms glGetUniformLocation
        // — the first cut used the uniform query for aPos/aUv too, so both
        // came back -1, every glEnableVertexAttribArray(-1) raised
        // GL_INVALID_VALUE and NO geometry was drawn (the scene stayed the
        // clear colour; found via the GEMSHELL_SCREENSHOT readback
        // 2026-09-11).
        let attr = |name: &str| unsafe {
            glGetAttribLocation(program, CString::new(name).unwrap().as_ptr())
        };
        let loc = |name: &str| unsafe {
            glGetUniformLocation(program, CString::new(name).unwrap().as_ptr())
        };
        let a_pos = attr("aPos");
        let a_uv = attr("aUv");
        let u_res = loc("uRes");
        let u_color = loc("uColor");

        // egui program + element buffer (built here so a shader error
        // surfaces at startup, not on the first settings tap).
        let egui_prog = build_program(EGUI_VERT, EGUI_FRAG)?;
        let egui_attr = |name: &str| unsafe {
            glGetAttribLocation(egui_prog, CString::new(name).unwrap().as_ptr())
        };
        let egui_uniform = |name: &str| unsafe {
            glGetUniformLocation(egui_prog, CString::new(name).unwrap().as_ptr())
        };
        let egui_a_pos = egui_attr("aPos");
        let egui_a_uv = egui_attr("aUv");
        let egui_a_color = egui_attr("aColor");
        let egui_u_res = egui_uniform("uRes");
        let egui_u_tex = egui_uniform("uTex");
        let mut egui_ebo = 0u32;
        unsafe { glGenBuffers(1, &mut egui_ebo) };
        log::info!("egui program built (aPos={egui_a_pos} aUv={egui_a_uv} aColor={egui_a_color})");

        let mut vbo = 0u32;
        unsafe { glGenBuffers(1, &mut vbo) };

        // The extension entry points (runtime-resolved; see the type
        // block above). The ICD must have EGL_EXT_image_dma_buf_import —
        // without it the whole GPU-direct present path is dead.
        let exts = unsafe { eglQueryString(display, EGL_EXTENSIONS) };
        let exts = if exts.is_null() {
            String::new()
        } else {
            unsafe { std::ffi::CStr::from_ptr(exts) }
                .to_string_lossy()
                .into_owned()
        };
        if !exts.contains("EGL_EXT_image_dma_buf_import") {
            log::warn!(
                "ICD lacks EGL_EXT_image_dma_buf_import (EGL extensions: {exts}) — the GPU-direct present will fail"
            );
        }
        let eglCreateImageKHR: EglCreateImageFn = unsafe {
            std::mem::transmute(eglGetProcAddress(
                b"eglCreateImageKHR\0".as_ptr() as *const c_char,
            ))
        };
        let glEGLImageTargetTexture2DOES: GlEglImageTargetFn = unsafe {
            std::mem::transmute(eglGetProcAddress(
                b"glEGLImageTargetTexture2DOES\0".as_ptr() as *const c_char,
            ))
        };

        let mut r = Renderer {
            width,
            height,
            ui_scale: 1.0,
            display,
            context,
            surface: std::ptr::null_mut(),
            program,
            a_pos,
            a_uv,
            u_res,
            u_color,
            vbo,
            white_tex: 0,
            glyph_tex: 0,
            win_tex: HashMap::new(),
            verts: Vec::new(),
            colors: Vec::new(),
            fbo: 0,
            fbo_tex: 0,
            fb_fd: -1,
            fb_image: std::ptr::null_mut(),
            fb_tex: 0,
            cprog: 0,
            c_src: -1,
            c_size: -1,
            c_srcsize: -1,
            c_rot: -1,
            rotation: std::env::var("GEMSHELL_ROTATE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(270),
            eglCreateImageKHR,
            glEGLImageTargetTexture2DOES,
            egui_prog,
            egui_a_pos,
            egui_a_uv,
            egui_a_color,
            egui_u_res,
            egui_u_tex,
            egui_ebo,
            egui_textures: HashMap::new(),
        };

        let white = [255u8; 16 * 4];
        r.white_tex = r.make_texture(4, 4, &white)?;
        if !glyph_pixels.is_empty() {
            r.glyph_tex = r.make_texture(glyph_size, glyph_size, glyph_pixels)?;
        }
        log::info!("base textures created (glyph {glyph_size}px)");

        // The scene FBO is needed by EVERY backend (device + nested).
        r.init_scene_fbo()?;
        log::info!("scene FBO ready ({}x{})", r.width, r.height);

        if with_fb {
            // --- GPU-direct present target (the gemwl chain, on glass
            // 2026-09-01/09-10): /dev/gemfb LK-fb import + the compute
            // copy. ---
            let fb_fd = open_gemfb()?;
            log::info!("gemfb opened (fd={fb_fd})");
            let attrs = fb_image_attrs(fb_fd);
            let fb_image = unsafe {
                (r.eglCreateImageKHR)(
                    display,
                    std::ptr::null_mut(),
                    EGL_LINUX_DMA_BUF_EXT,
                    std::ptr::null(),
                    attrs.as_ptr(),
                )
            };
            if fb_image.is_null() {
                return Err(format!(
                    "eglCreateImageKHR(LK fb dma-buf) failed (error 0x{:x}) — is the ICD's EGL_EXT_image_dma_buf_import available?",
                    unsafe { eglGetError() }
                ));
            }
            log::info!("LK fb dma-buf imported as EGLImage");
            r.init_fb_present(fb_fd, fb_image)?;
            log::info!("GPU-direct present target ready");
        }
        Ok(r)
    }

    /// The scene FBO (fullscreen RGBA8 render target), shared by the
    /// device and nested backends (context must be current).
    fn init_scene_fbo(&mut self) -> Result<(), String> {
        // The scene FBO: a plain RGBA8 fullscreen texture. (gemwl's shadow
        // is a panfrost dma-buf BO because wlroots' swapchain needs a
        // wlr_buffer; a GL texture is the equivalent for our compute
        // texelFetch source.)
        let mut fbo_tex = 0u32;
        let mut fbo = 0u32;
        unsafe {
            glGenTextures(1, &mut fbo_tex);
            glBindTexture(GL_TEXTURE_2D, fbo_tex);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST as c_int);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST as c_int);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE as c_int);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE as c_int);
            glTexImage2D(
                GL_TEXTURE_2D,
                0,
                GL_RGBA as c_int,
                self.width as c_int,
                self.height as c_int,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                std::ptr::null(),
            );
            glGenFramebuffers(1, &mut fbo);
            glBindFramebuffer(GL_FRAMEBUFFER, fbo);
            glFramebufferTexture2D(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                fbo_tex,
                0,
            );
        }
        self.fbo_tex = fbo_tex;
        self.fbo = fbo;

        // T880 tiler first-batch bug (gemwl receipt, 2026-09-02): the
        // FIRST tiler draw into a fresh fullscreen-size target rasterizes
        // only ~1024x1024 — warm the FBO so the first real frame is
        // clean. (Harmless no-op on non-panfrost hosts.)
        tiler_warmup(self.fbo, self.width, self.height);
        Ok(())
    }

    /// The LK-fb image texture + the compute copy program (device only;
    /// context must be current).
    fn init_fb_present(&mut self, fb_fd: c_int, fb_image: EglImage) -> Result<(), String> {
        let mut fb_tex = 0u32;
        unsafe {
            glGenTextures(1, &mut fb_tex);
            glBindTexture(GL_TEXTURE_2D, fb_tex);
            (self.glEGLImageTargetTexture2DOES)(GL_TEXTURE_2D, fb_image);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST as c_int);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST as c_int);
        }
        log::info!("LK fb image bound to texture {fb_tex}");
        self.fb_tex = fb_tex;

        // The compute copy program.
        let cs = unsafe { glCreateShader(GL_COMPUTE_SHADER) };
        let src = CString::new(COPY_CS).unwrap();
        unsafe {
            // Array-of-pointers, same as build_program (see the note there).
            let sp = src.as_ptr();
            glShaderSource(cs, 1, &sp, std::ptr::null());
            glCompileShader(cs);
        }
        let mut ok = 0i32;
        unsafe { glGetShaderiv(cs, GL_COMPILE_STATUS, &mut ok) };
        if ok == 0 {
            let mut log = vec![0u8; 512];
            let mut l = 0i32;
            unsafe {
                glGetShaderInfoLog(cs, 512, &mut l, log.as_mut_ptr() as *mut c_void);
            }
            let msg = String::from_utf8_lossy(&log[..l.max(0) as usize]).into_owned();
            return Err(format!("copy compute shader: {msg}"));
        }
        let p = unsafe { glCreateProgram() };
        unsafe {
            glAttachShader(p, cs);
            glLinkProgram(p);
        }
        unsafe { glGetProgramiv(p, GL_LINK_STATUS, &mut ok) };
        if ok == 0 {
            let mut log = vec![0u8; 512];
            let mut l = 0i32;
            unsafe {
                glGetProgramInfoLog(p, 512, &mut l, log.as_mut_ptr() as *mut c_void);
            }
            let msg = String::from_utf8_lossy(&log[..l.max(0) as usize]).into_owned();
            return Err(format!("copy compute program: {msg}"));
        }
        unsafe { glDeleteShader(cs) };
        let loc = |name: &str| unsafe {
            glGetUniformLocation(p, CString::new(name).unwrap().as_ptr())
        };
        self.cprog = p;
        self.c_src = loc("src");
        self.c_size = loc("fbSize");
        self.c_srcsize = loc("srcSize");
        self.c_rot = loc("rotation");
        self.fb_fd = fb_fd;
        self.fb_image = fb_image;
        Ok(())
    }

    /// Read the scene FBO back as top-down RGBA8 (the nested present;
    /// same pixels `screenshot` writes, without the PNG).
    pub fn read_scene_rgba(&self) -> Vec<u8> {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut buf = vec![0u8; w * h * 4];
        unsafe {
            glBindFramebuffer(GL_FRAMEBUFFER, self.fbo);
            glReadPixels(
                0,
                0,
                self.width as c_int,
                self.height as c_int,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                buf.as_mut_ptr() as *mut c_void,
            );
        }
        // glReadPixels returns rows bottom-up; flip to top-down.
        let mut flipped = vec![0u8; buf.len()];
        for y in 0..h {
            let src = (h - 1 - y) * w * 4;
            flipped[y * w * 4..(y + 1) * w * 4].copy_from_slice(&buf[src..src + w * 4]);
        }
        flipped
    }

    fn make_texture(&mut self, w: u32, h: u32, rgba8: &[u8]) -> Result<u32, String> {
        let mut tex = 0u32;
        unsafe {
            glGenTextures(1, &mut tex);
            glBindTexture(GL_TEXTURE_2D, tex);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST as c_int);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST as c_int);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE as c_int);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE as c_int);
            glTexImage2D(
                GL_TEXTURE_2D,
                0,
                GL_RGBA as c_int,
                w as c_int,
                h as c_int,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                rgba8.as_ptr() as *const c_void,
            );
        }
        Ok(tex)
    }

    /// (Re)create a window content texture from an SHM buffer
    /// (XRGB8888 or ARGB8888, little-endian byte order) and return the
    /// texture id.
    pub fn window_texture(
        &mut self,
        id: u32,
        w: u32,
        h: u32,
        stride: u32,
        format: u32,
        data: &[u8],
    ) -> u32 {
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let s = (y * stride + x * 4) as usize;
                let d = ((y * w + x) * 4) as usize;
                // wl_shm XRGB8888/ARGB8888 memory layout (little-endian)
                // is [B, G, R, X/A] — build a standard [R, G, B, A]
                let b = data[s];
                let g = data[s + 1];
                let r = data[s + 2];
                let a = if format == 0x34325258 { 255 } else { data[s + 3] }; // XRGB8888
                rgba[d] = r;
                rgba[d + 1] = g;
                rgba[d + 2] = b;
                rgba[d + 3] = a;
            }
        }
        match self.win_tex.get(&id) {
            Some(&(tex, tw, th)) if tw == w && th == h => {
                unsafe {
                    glBindTexture(GL_TEXTURE_2D, tex);
                    glTexSubImage2D(
                        GL_TEXTURE_2D,
                        0,
                        0,
                        0,
                        w as c_int,
                        h as c_int,
                        GL_RGBA,
                        GL_UNSIGNED_BYTE,
                        rgba.as_ptr() as *const c_void,
                    );
                }
                tex
            }
            _ => {
                if let Some(&(tex, _, _)) = self.win_tex.get(&id) {
                    unsafe { glDeleteTextures(1, &tex) };
                }
                let tex = match self.make_texture(w, h, &rgba) {
                    Ok(t) => t,
                    Err(e) => {
                        log::error!("window texture: {e}");
                        return self.white_tex;
                    }
                };
                self.win_tex.insert(id, (tex, w, h));
                tex
            }
        }
    }

    pub fn drop_window_texture(&mut self, id: u32) {
        if let Some((tex, _, _)) = self.win_tex.remove(&id) {
            unsafe { glDeleteTextures(1, &tex) };
        }
    }

    // ---------- immediate drawing ----------

    fn frame_setup(&mut self) {
        unsafe {
            glViewport(0, 0, self.width as c_int, self.height as c_int);
            glBindFramebuffer(GL_FRAMEBUFFER, self.fbo);
            glClearColor(0.055, 0.07, 0.085, 1.0);
            glClear(GL_COLOR_BUFFER_BIT);
            glDisable(GL_DEPTH_TEST);
            glDisable(GL_CULL_FACE);
            glEnable(GL_BLEND);
            glUseProgram(self.program);
            let s = if self.ui_scale > 0.01 { self.ui_scale } else { 1.0 };
            glUniform2f(
                self.u_res,
                self.width as f32 / s,
                self.height as f32 / s,
            );
            glBindBuffer(GL_ARRAY_BUFFER, self.vbo);
            glEnableVertexAttribArray(self.a_pos);
            glEnableVertexAttribArray(self.a_uv);
            glVertexAttribPointer(self.a_pos, 2, GL_FLOAT, 0, 16, 0);
            glVertexAttribPointer(self.a_uv, 2, GL_FLOAT, 0, 16, 8);
        }
    }

    fn draw(&mut self, tex: u32, c: Color, premult: bool) {
        if self.verts.is_empty() {
            return;
        }
        unsafe {
            glBlendFunc(
                if premult { GL_ONE } else { GL_SRC_ALPHA },
                GL_ONE_MINUS_SRC_ALPHA,
            );
            glBindTexture(GL_TEXTURE_2D, tex);
            glUniform4f(self.u_color, c.0[0], c.0[1], c.0[2], c.0[3]);
            let bytes: Vec<f32> = self
                .verts
                .iter()
                .flat_map(|v: &Vert| v.as_f32())
                .collect();
            glBufferData(
                GL_ARRAY_BUFFER,
                (bytes.len() * 4) as i64,
                bytes.as_ptr() as *const c_void,
                GL_DYNAMIC_DRAW,
            );
            glDrawArrays(GL_TRIANGLE_STRIP, 0, self.verts.len() as c_int);
        }
        self.verts.clear();
    }

    fn quad(&mut self, x: f32, y: f32, w: f32, h: f32, u0: f32, v0: f32, u1: f32, v1: f32) {
        self.verts.extend([
            Vert { x, y, u: u0, v: v0 },
            Vert { x: x + w, y, u: u1, v: v0 },
            Vert { x, y: y + h, u: u0, v: v1 },
            Vert { x: x + w, y: y + h, u: u1, v: v1 },
        ]);
    }

    /// Solid rectangle (straight alpha).
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Color) {
        self.quad(x, y, w, h, 0.0, 0.0, 1.0, 1.0);
        self.draw(self.white_tex, c, false);
    }

    /// Textured rectangle (glyphs: premultiplied; window content:
    /// straight). `premult` selects the blend mode.
    pub fn rect_tex(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        u0: f32,
        v0: f32,
        u1: f32,
        v1: f32,
        tex: u32,
        c: Color,
        premult: bool,
    ) {
        self.quad(x, y, w, h, u0, v0, u1, v1);
        self.draw(tex, c, premult);
    }

    /// Filled circle (fan of quads, straight alpha).
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, c: Color) {
        const SEG: usize = 24;
        for i in 0..SEG {
            let a0 = i as f32 / SEG as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / SEG as f32 * std::f32::consts::TAU;
            let p0 = (cx + r * a0.cos(), cy + r * a0.sin());
            let p1 = (cx + r * a1.cos(), cy + r * a1.sin());
            self.verts.extend([
                Vert { x: cx, y: cy, u: 0.0, v: 0.0 },
                Vert { x: p0.0, y: p0.1, u: 0.0, v: 0.0 },
                Vert { x: cx, y: cy, u: 0.0, v: 0.0 },
                Vert { x: p1.0, y: p1.1, u: 0.0, v: 0.0 },
            ]);
        }
        self.draw(self.white_tex, c, false);
    }

    /// Annular arc (ring segment), straight alpha. Angles in radians
    /// (y-down: 0 = right, PI/2 = down).
    pub fn arc(&mut self, cx: f32, cy: f32, r0: f32, r1: f32, a0: f32, a1: f32, c: Color) {
        const SEG: usize = 10;
        for i in 0..SEG {
            let t0 = a0 + (a1 - a0) * i as f32 / SEG as f32;
            let t1 = a0 + (a1 - a0) * (i + 1) as f32 / SEG as f32;
            let p = |a: f32, r: f32| (cx + r * a.cos(), cy + r * a.sin());
            let (q0o, q1o, q1i, q0i) = (p(t0, r0), p(t1, r0), p(t1, r1), p(t0, r1));
            self.verts.extend([
                Vert { x: q0o.0, y: q0o.1, u: 0.0, v: 0.0 },
                Vert { x: q1o.0, y: q1o.1, u: 0.0, v: 0.0 },
                Vert { x: q0i.0, y: q0i.1, u: 0.0, v: 0.0 },
                Vert { x: q1i.0, y: q1i.1, u: 0.0, v: 0.0 },
            ]);
        }
        self.draw(self.white_tex, c, false);
    }

    /// Thick line (a quad along the segment), straight alpha.
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, t: f32, c: Color) {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let nx = -dy / len * t * 0.5;
        let ny = dx / len * t * 0.5;
        self.verts.extend([
            Vert { x: x0 + nx, y: y0 + ny, u: 0.0, v: 0.0 },
            Vert { x: x1 + nx, y: y1 + ny, u: 0.0, v: 0.0 },
            Vert { x: x0 - nx, y: y0 - ny, u: 0.0, v: 0.0 },
            Vert { x: x1 - nx, y: y1 - ny, u: 0.0, v: 0.0 },
        ]);
        self.draw(self.white_tex, c, false);
    }

    /// Draw `s` at pen (x = left, baseline = y) in color c.
    pub fn text(&mut self, font: &crate::common::font::Font, x: f32, baseline: f32, s: &str, c: Color) {
        let mut pen = x;
        for ch in s.chars() {
            if let Some(g) = font.glyph(ch) {
                let top = baseline - g.y_top;
                self.rect_tex(
                    pen + g.x_off,
                    top,
                    g.w as f32 / crate::common::font::SUP,
                    g.h as f32 / crate::common::font::SUP,
                    g.u0,
                    g.v0,
                    g.u1,
                    g.v1,
                    self.glyph_tex,
                    c,
                    true,
                );
            }
            pen += font
                .glyph(ch)
                .map(|g| g.advance)
                .unwrap_or_else(|| font.size * 0.6);
        }
    }

    /// Centered text inside [x, x+w], baseline at y.
    pub fn text_centered(
        &mut self,
        font: &crate::common::font::Font,
        x: f32,
        w: f32,
        baseline: f32,
        s: &str,
        c: Color,
    ) {
        let tw = font.text_width(s);
        self.text(font, x + (w - tw) / 2.0, baseline, s, c);
    }

    /// Text clipped to [x, x+w] (left aligned) — for window titles.
    pub fn text_clipped(
        &mut self,
        font: &crate::common::font::Font,
        x: f32,
        w: f32,
        baseline: f32,
        s: &str,
        c: Color,
    ) {
        let mut pen = x;
        for ch in s.chars() {
            let Some(g) = font.glyph(ch) else { break };
            let gx = pen + g.x_off;
            if gx >= x + w {
                break;
            }
            let gw = (g.w as f32 / crate::common::font::SUP).min(x + w - gx);
            let top = baseline - g.y_top;
            self.rect_tex(
                gx,
                top,
                gw,
                g.h as f32 / crate::common::font::SUP,
                g.u0,
                g.v0,
                g.u0 + (g.u1 - g.u0) * gw / (g.w as f32 / crate::common::font::SUP),
                g.v1,
                self.glyph_tex,
                c,
                true,
            );
            pen += g.advance;
            if pen > x + w {
                break;
            }
        }
    }

    /// Begin the frame (bind the scene FBO, viewport + clear).
    pub fn begin_frame(&mut self) {
        unsafe { glBindFramebuffer(GL_FRAMEBUFFER, self.fbo) };
        self.frame_setup();
    }

    /// Replay recorded ops.
    pub fn replay(&mut self, font: &crate::common::font::Font, ops: &[Op]) {
        for op in ops {
            match op {
                Op::Rect { x, y, w, h, c } => self.rect(*x, *y, *w, *h, *c),
                Op::RectTex {
                    x, y, w, h, u0, v0, u1, v1, tex, c, premult,
                } => self.rect_tex(*x, *y, *w, *h, *u0, *v0, *u1, *v1, *tex, *c, *premult),
                Op::Circle { cx, cy, r, c } => self.circle(*cx, *cy, *r, *c),
                Op::Arc {
                    cx, cy, r0, r1, a0, a1, c,
                } => self.arc(*cx, *cy, *r0, *r1, *a0, *a1, *c),
                Op::Line { x0, y0, x1, y1, t, c } => self.line(*x0, *y0, *x1, *y1, *t, *c),
                Op::Text { x, baseline, s, c } => self.text(font, *x, *baseline, s, *c),
                Op::TextCentered { x, w, baseline, s, c } => {
                    self.text_centered(font, *x, *w, *baseline, s, *c)
                }
                Op::TextClipped { x, w, baseline, s, c } => {
                    self.text_clipped(font, *x, *w, *baseline, s, *c)
                }
            }
        }
    }

    /// Draw the egui shell UI (settings panel) on top of the scene.
    ///
    /// egui emits premultiplied-sRGBA triangle meshes + a font-atlas
    /// texture delta; this uploads the atlas/textures and draws the
    /// meshes with the indexed `EGUI_VERT` shader. All GPU-side: no CPU
    /// rasterization (2026-09-12). Clip rects become GL scissor rects
    /// (note GL's bottom-left origin vs egui's top-left).
    pub fn draw_egui(
        &mut self,
        primitives: &[egui::ClippedPrimitive],
        textures: &egui::TexturesDelta,
        pixels_per_point: f32,
    ) {
        if self.egui_prog == 0 {
            return;
        }
        // egui vertices are in POINTS; the scene is physical pixels.
        let ppp = if pixels_per_point > 0.0 { pixels_per_point } else { 1.0 };
        for id in &textures.free {
            if let Some(tex) = self.egui_textures.remove(id) {
                unsafe { glDeleteTextures(1, &tex) };
            }
        }
        for (id, delta) in &textures.set {
            if let Err(e) = self.egui_upload_texture(*id, delta) {
                log::warn!("egui texture {id:?}: {e}");
            }
        }

        unsafe {
            glUseProgram(self.egui_prog);
            glUniform2f(
                self.egui_u_res,
                self.width as f32 / ppp,
                self.height as f32 / ppp,
            );
            glUniform1i(self.egui_u_tex, 0);
            glActiveTexture(GL_TEXTURE0);
            glDisable(GL_DEPTH_TEST);
            glDisable(GL_CULL_FACE);
            glEnable(GL_BLEND);
            glBlendEquation(GL_FUNC_ADD);
            glBlendFuncSeparate(GL_ONE, GL_ONE_MINUS_SRC_ALPHA, GL_ONE_MINUS_DST_ALPHA, GL_ONE);
            glEnable(GL_SCISSOR_TEST);
            glBindBuffer(GL_ARRAY_BUFFER, self.vbo);
            glBindBuffer(GL_ELEMENT_ARRAY_BUFFER, self.egui_ebo);
            glEnableVertexAttribArray(self.egui_a_pos);
            glVertexAttribPointer(self.egui_a_pos, 2, GL_FLOAT, 0, 20, 0);
            glEnableVertexAttribArray(self.egui_a_uv);
            glVertexAttribPointer(self.egui_a_uv, 2, GL_FLOAT, 0, 20, 8);
            glEnableVertexAttribArray(self.egui_a_color);
            glVertexAttribPointer(self.egui_a_color, 4, GL_UNSIGNED_BYTE, 0, 20, 16);
        }

        for prim in primitives {
            let egui::epaint::Primitive::Mesh(mesh) = &prim.primitive else {
                continue;
            };
            if mesh.indices.is_empty() || mesh.vertices.is_empty() {
                continue;
            }
            let r = prim.clip_rect;
            let x = (r.min.x * ppp).max(0.0) as i32;
            let y = (r.min.y * ppp).max(0.0) as i32;
            let x1 = (r.max.x * ppp).min(self.width as f32).max(0.0) as i32;
            let y1 = (r.max.y * ppp).min(self.height as f32).max(0.0) as i32;
            let (cw, ch) = ((x1 - x).max(0), (y1 - y).max(0));
            if cw == 0 || ch == 0 {
                continue;
            }
            unsafe {
                // GL scissor origin is bottom-left; egui's is top-left.
                glScissor(x, self.height as i32 - (y + ch), cw, ch);
                let tex = self
                    .egui_textures
                    .get(&mesh.texture_id)
                    .copied()
                    .unwrap_or(self.white_tex);
                glBindTexture(GL_TEXTURE_2D, tex);
                glBufferData(
                    GL_ARRAY_BUFFER,
                    (mesh.vertices.len() * std::mem::size_of::<egui::epaint::Vertex>()) as i64,
                    mesh.vertices.as_ptr() as *const c_void,
                    GL_DYNAMIC_DRAW,
                );
                glBufferData(
                    GL_ELEMENT_ARRAY_BUFFER,
                    (mesh.indices.len() * std::mem::size_of::<u32>()) as i64,
                    mesh.indices.as_ptr() as *const c_void,
                    GL_DYNAMIC_DRAW,
                );
                glDrawElements(
                    GL_TRIANGLES,
                    mesh.indices.len() as c_int,
                    GL_UNSIGNED_INT,
                    std::ptr::null(),
                );
            }
        }
        unsafe {
            glDisable(GL_SCISSOR_TEST);
        }
    }

    fn egui_upload_texture(
        &mut self,
        id: egui::TextureId,
        delta: &egui::epaint::ImageDelta,
    ) -> Result<(), String> {
        let [w, h] = delta.image.size();
        // egui only ever emits premultiplied sRGBA; Color32 is [u8;4].
        let mut rgba: Vec<u8> = Vec::with_capacity(w * h * 4);
        match &delta.image {
            egui::epaint::ImageData::Color(img) => {
                for p in &img.pixels {
                    rgba.extend_from_slice(&p.to_array());
                }
            }
            egui::epaint::ImageData::Font(img) => {
                for p in img.srgba_pixels(None) {
                    rgba.extend_from_slice(&p.to_array());
                }
            }
        }
        let ptr = rgba.as_ptr() as *const c_void;
        match delta.pos {
            None => {
                let tex = match self.egui_textures.get(&id) {
                    Some(&t) => {
                        unsafe {
                            glBindTexture(GL_TEXTURE_2D, t);
                            glTexImage2D(
                                GL_TEXTURE_2D,
                                0,
                                GL_RGBA as c_int,
                                w as c_int,
                                h as c_int,
                                0,
                                GL_RGBA,
                                GL_UNSIGNED_BYTE,
                                ptr,
                            );
                        }
                        t
                    }
                    None => self.egui_alloc_texture(w, h, ptr)?,
                };
                self.egui_textures.insert(id, tex);
            }
            Some([x, y]) => {
                let Some(&tex) = self.egui_textures.get(&id) else {
                    return Err("partial update for an unknown texture".into());
                };
                unsafe {
                    glBindTexture(GL_TEXTURE_2D, tex);
                    glTexSubImage2D(
                        GL_TEXTURE_2D,
                        0,
                        x as c_int,
                        y as c_int,
                        w as c_int,
                        h as c_int,
                        GL_RGBA,
                        GL_UNSIGNED_BYTE,
                        ptr,
                    );
                }
            }
        }
        Ok(())
    }

    /// A LINEAR-filtered texture (egui wants smooth font scaling).
    fn egui_alloc_texture(&mut self, w: usize, h: usize, data: *const c_void) -> Result<u32, String> {
        let mut tex = 0u32;
        unsafe {
            glGenTextures(1, &mut tex);
            glBindTexture(GL_TEXTURE_2D, tex);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR as c_int);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR as c_int);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE as c_int);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE as c_int);
            glTexImage2D(
                GL_TEXTURE_2D,
                0,
                GL_RGBA as c_int,
                w as c_int,
                h as c_int,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                data,
            );
        }
        Ok(tex)
    }

    /// Public texture creation (app icons).
    pub fn make_texture_pub(&mut self, w: u32, h: u32, rgba8: &[u8]) -> Result<u32, String> {
        self.make_texture(w, h, rgba8)
    }

    /// The window content texture id (for switcher thumbnails).
    pub fn window_texture_id(&self, id: u32) -> Option<u32> {
        self.win_tex.get(&id).map(|t| t.0)
    }

    /// Read the scene FBO back and write it as a PNG (debug/
    /// verification — `GEMSHELL_SCREENSHOT=/path`, see the compositor).
    /// glReadPixels returns rows bottom-up, so flip to top-down for PNG.
    pub fn screenshot(&self, path: &str) -> Result<(), String> {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut buf = vec![0u8; w * h * 4];
        unsafe {
            glBindFramebuffer(GL_FRAMEBUFFER, self.fbo);
            glReadPixels(
                0,
                0,
                self.width as c_int,
                self.height as c_int,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                buf.as_mut_ptr() as *mut c_void,
            );
        }
        let mut flipped = vec![0u8; buf.len()];
        for y in 0..h {
            let src = (h - 1 - y) * w * 4;
            flipped[y * w * 4..(y + 1) * w * 4].copy_from_slice(&buf[src..src + w * 4]);
        }
        write_png(path, self.width, self.height, &flipped)
    }

    /// Present the frame: compute-blit the scene FBO texture into the
    /// LK framebuffer (the gemwl chain — zero CPU pixel movement, no
    /// KMS, no page flip; the panel scans the LK OVL memory directly).
    /// Ends with glFinish, so on return the panel has the frame.
    pub fn present(&mut self) -> Result<(), String> {
        // Nested/host renderer: no LK fb — the nested client reads the
        // scene FBO back and presents it over Wayland.
        if self.cprog == 0 {
            return Ok(());
        }
        unsafe {
            glUseProgram(self.cprog);
            glUniform1i(self.c_src, 1);
            glUniform2i(self.c_size, FB_W as c_int, FB_H as c_int);
            glUniform2i(self.c_srcsize, self.width as c_int, self.height as c_int);
            glUniform1i(self.c_rot, self.rotation);
            glActiveTexture(GL_TEXTURE1);
            glBindTexture(GL_TEXTURE_2D, self.fbo_tex);
            glBindImageTexture(
                0,
                self.fb_tex,
                0,
                0,
                0,
                GL_WRITE_ONLY,
                GL_RGBA8,
            );
            glDispatchCompute((FB_W + 15) / 16, (FB_H + 7) / 8, 1);
            glMemoryBarrier(GL_FRAMEBUFFER_BARRIER_BIT);
            glFinish();
            glBindImageTexture(0, 0, 0, 0, 0, GL_WRITE_ONLY, GL_RGBA8);
            glActiveTexture(GL_TEXTURE0);
        }
        Ok(())
    }
}

/// Encode an RGBA8 pixel buffer as a PNG.
fn write_png(path: &str, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    Ok(())
}

/// Open /dev/gemfb and export the LK framebuffer as a dma-buf fd
/// (GEMFB_IOC_EXPORT; geminipda-fb.c — the gemwl access path).
fn open_gemfb() -> Result<c_int, String> {
    let path = CString::new("/dev/gemfb").unwrap();
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(format!("open /dev/gemfb: {}", std::io::Error::last_os_error()));
    }
    // The driver RETURNS the dma-buf fd as the ioctl result (it ignores
    // the arg; geminipda-fb.c gemfb_ioctl + gemwl.c:1500
    // `int dma_fd = ioctl(gfd, GEMFB_IOC_EXPORT)`). The old code expected
    // the fd in the pointer arg, so it treated the positive return as an
    // error and reported a stale errno (ENOENT from the render-node
    // fallback opens).
    let dma_fd = unsafe { libc::ioctl(fd, GEMFB_IOC_EXPORT as _) };
    unsafe { libc::close(fd) };
    if dma_fd < 0 {
        return Err(format!(
            "GEMFB_IOC_EXPORT: {}",
            std::io::Error::last_os_error()
        ));
    }
    log::info!("gemfb: LK fb dma-buf exported (fd {dma_fd})");
    Ok(dma_fd)
}

/// The EGL_EXT_image_dma_buf_import attribute list for the LK fb
/// (ABGR8888 at the 1088-px pitch — the gemwl import attrs).
fn fb_image_attrs(fb_fd: c_int) -> [c_int; 13] {
    [
        EGL_WIDTH,
        FB_W as c_int,
        EGL_HEIGHT,
        FB_H as c_int,
        EGL_LINUX_DRM_FOURCC_EXT,
        DRM_FORMAT_ABGR8888 as c_int,
        EGL_DMA_BUF_PLANE0_FD_EXT,
        fb_fd,
        EGL_DMA_BUF_PLANE0_OFFSET_EXT,
        0,
        EGL_DMA_BUF_PLANE0_PITCH_EXT,
        FB_PITCH as c_int,
        EGL_NONE,
    ]
}

/// T880 tiler warmup (gemwl receipt 2026-09-02): the first tiler batch
/// into a fresh fullscreen-size target clips to ~1024x1024; a
/// throwaway full-frame draw into the FBO makes the first real frame
/// clean.
fn tiler_warmup(fbo: u32, width: u32, height: u32) {
    let wvs = CString::new("attribute vec2 pos; void main(){ gl_Position=vec4(pos,0.0,1.0); }").unwrap();
    let wfs = CString::new("precision mediump float; void main(){ gl_FragColor=vec4(0,0,0,1); }").unwrap();
    unsafe {
        let vs = glCreateShader(GL_VERTEX_SHADER);
        // Array-of-pointers (see build_program).
        let vsp = wvs.as_ptr();
        glShaderSource(vs, 1, &vsp, std::ptr::null());
        glCompileShader(vs);
        let fs = glCreateShader(GL_FRAGMENT_SHADER);
        let fsp = wfs.as_ptr();
        glShaderSource(fs, 1, &fsp, std::ptr::null());
        glCompileShader(fs);
        let prog = glCreateProgram();
        glAttachShader(prog, vs);
        glAttachShader(prog, fs);
        glLinkProgram(prog);
        glDeleteShader(vs);
        glDeleteShader(fs);
        // The attribute NAME, not the shader source (the old code passed
        // wvs.as_ptr() — the whole source string — so the lookup returned
        // -1 and the draw used attrib -1).
        let attr_name = CString::new("pos").unwrap();
        let a_pos = glGetAttribLocation(prog, attr_name.as_ptr());
        // Warm the REAL scene FBO (the exact target the first frame
        // uses) — its attachment stays in place.
        glBindFramebuffer(GL_FRAMEBUFFER, fbo);
        glViewport(0, 0, width as c_int, height as c_int);
        glClearColor(0.0, 0.0, 0.0, 1.0);
        glClear(GL_COLOR_BUFFER_BIT);
        glUseProgram(prog);
        let q: [f32; 8] = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
        let mut vbo = 0u32;
        glGenBuffers(1, &mut vbo);
        glBindBuffer(GL_ARRAY_BUFFER, vbo);
        glBufferData(GL_ARRAY_BUFFER, (q.len() * 4) as i64, q.as_ptr() as *const c_void, GL_DYNAMIC_DRAW);
        glVertexAttribPointer(a_pos, 2, GL_FLOAT, 0, 0, 0);
        glEnableVertexAttribArray(a_pos);
        glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
        glFinish();
        glDeleteBuffers(1, &vbo);
        glDeleteProgram(prog);
        log::info!("tiler warmup: full-frame draw into the scene FBO done");
    }
}

fn build_program(vert: &str, frag: &str) -> Result<u32, String> {
    unsafe {
        let make = |src: &str, ty: u32| -> Result<u32, String> {
            let s = glCreateShader(ty);
            let c = CString::new(src).unwrap();
            let len = [c.as_bytes().len() as c_int];
            // glShaderSource wants an ARRAY OF POINTERS to the strings
            // (`const GLchar *const*`), not the string pointer itself.
            // Passing `c.as_ptr()` directly made Mesa read the first 8
            // bytes of the shader source as a pointer and dereference it
            // — the on-glass SEGV right after "GL: ..." (2026-09-11).
            let sp = c.as_ptr();
            glShaderSource(s, 1, &sp, len.as_ptr());
            glCompileShader(s);
            let mut ok = 0i32;
            glGetShaderiv(s, GL_COMPILE_STATUS, &mut ok);
            if ok == 0 {
                let mut log = vec![0u8; 512];
                let mut l = 0i32;
                glGetShaderInfoLog(s, 512, &mut l, log.as_mut_ptr() as *mut c_void);
                let msg = String::from_utf8_lossy(&log[..l.max(0) as usize]).into_owned();
                return Err(format!("shader compile: {msg}"));
            }
            Ok(s)
        };
        // NOTE: use the `vert` PARAMETER. A copy-paste made this build
        // every program from the global VERT, so the egui program linked
        // VERT with EGUI_FRAG and failed "fragment input `vColor' has no
        // matching output" (found by the nested run 2026-09-12).
        let vs = make(vert, GL_VERTEX_SHADER)?;
        let fs = make(frag, GL_FRAGMENT_SHADER)?;
        let p = glCreateProgram();
        glAttachShader(p, vs);
        glAttachShader(p, fs);
        glLinkProgram(p);
        let mut ok = 0i32;
        glGetProgramiv(p, GL_LINK_STATUS, &mut ok);
        if ok == 0 {
            let mut log = vec![0u8; 512];
            let mut l = 0i32;
            glGetProgramInfoLog(p, 512, &mut l, log.as_mut_ptr() as *mut c_void);
            return Err(format!(
                "program link: {}",
                String::from_utf8_lossy(&log[..l.max(0) as usize])
            ));
        }
        Ok(p)
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            eglMakeCurrent(self.display, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
            if !self.surface.is_null() {
                eglDestroySurface(self.display, self.surface);
            }
            eglDestroyContext(self.display, self.context);
            eglTerminate(self.display);
        }
    }
}
