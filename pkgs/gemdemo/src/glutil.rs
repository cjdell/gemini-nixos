#![allow(dead_code)] // helper surface shared with old modules — keep
//! Small GL helpers: program build with shader-log diagnostics, VBO/VAO/
//! FBO creation, and an error checker that names the last operation so a
//! panfrost/Mali-T880 failure is debuggable from the serial console.

use std::ffi::CString;

pub struct Program {
    pub id: u32,
    // named uniforms (looked up once at build time)
    pub u: Vec<(String, i32)>,
}

impl Program {
    pub fn uniform(&self, name: &str) -> i32 {
        self.u
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, l)| *l)
            .unwrap_or_else(|| panic!("uniform {name:?} not found in program"))
    }

    pub unsafe fn use_(&self) {
        gl::UseProgram(self.id);
    }
}

/// Compile + link a program. `uniforms` lists the names to cache (any
/// missing one is a build error — typos are the #1 demo killer).
pub unsafe fn build(
    name: &str,
    vs: &str,
    fs: &str,
    uniforms: &[&str],
) -> Program {
    let compile_stage = |src: &str, stage: u32| -> u32 {
        let csrc = CString::new(src).expect("shader has no NUL");
        let sh = gl::CreateShader(stage);
        gl::ShaderSource(sh, 1, &csrc.as_ptr(), std::ptr::null());
        gl::CompileShader(sh);
        log_if_failed(sh, gl::COMPILE_STATUS, name);
        sh
    };
    let vs_id = compile_stage(vs, gl::VERTEX_SHADER);
    let fs_id = compile_stage(fs, gl::FRAGMENT_SHADER);
    let prog = gl::CreateProgram();
    gl::AttachShader(prog, vs_id);
    gl::AttachShader(prog, fs_id);
    gl::LinkProgram(prog);
    log_if_failed_prog(prog, name);
    gl::DeleteShader(vs_id);
    gl::DeleteShader(fs_id);

    let u = uniforms
        .iter()
        .map(|n| {
            let loc = gl::GetUniformLocation(prog, CString::new(*n).unwrap().as_ptr());
            if loc < 0 {
                panic!("program {name}: uniform {n:?} missing");
            }
            (n.to_string(), loc)
        })
        .collect();
    Program { id: prog, u }
}

/// Render a GL info log (byte buffer — GLchar is i8 on x86_64, u8 on
/// aarch64; we hold raw bytes and cast at the FFI boundary).
fn gl_log_str(buf: &[u8]) -> String {
    String::from_utf8_lossy(buf).trim().to_string()
}

unsafe fn log_if_failed(sh: u32, pname: u32, name: &str) {
    let mut ok = 0;
    gl::GetShaderiv(sh, pname, &mut ok);
    if ok == 1 {
        return;
    }
    let mut len = 0;
    gl::GetShaderiv(sh, gl::INFO_LOG_LENGTH, &mut len);
    let mut buf = vec![0u8; len.max(1) as usize];
    gl::GetShaderInfoLog(sh, len, &mut len, buf.as_mut_ptr() as *mut _);
    let msg = gl_log_str(&buf);
    panic!("shader build failed ({name}): {msg}");
}

unsafe fn log_if_failed_prog(prog: u32, name: &str) {
    let mut ok = 0;
    gl::GetProgramiv(prog, gl::LINK_STATUS, &mut ok);
    if ok == 1 {
        return;
    }
    let mut len = 0;
    gl::GetProgramiv(prog, gl::INFO_LOG_LENGTH, &mut len);
    let mut buf = vec![0u8; len.max(1) as usize];
    gl::GetProgramInfoLog(prog, len, &mut len, buf.as_mut_ptr() as *mut _);
    let msg = gl_log_str(&buf);
    panic!("link failed ({name}): {msg}");
}

/// glError checker with a label — prints once per distinct error so a
/// misbehaving pass is visible without flooding the console.
static mut LAST_ERROR: u32 = 0;
pub unsafe fn check(op: &str) {
    let e = gl::GetError();
    if e != gl::NO_ERROR && e != LAST_ERROR {
        LAST_ERROR = e;
        eprintln!("gemdemo: GL error 0x{e:04x} after {op}");
    }
}

/// Create one vertex array object (ES 3.0+ API — glGenVertexArrays, NOT
/// the 4.5 DSA glCreateVertexArrays: the DSA entrypoints are "unsupported
/// function" stubs on the GLES 3.1 panfrost context and silently create
/// nothing (on-glass 2026-09-08 — every VAO/buffer/FBO was a ghost).
pub unsafe fn new_vao() -> u32 {
    let mut v: u32 = 0;
    gl::GenVertexArrays(1, &mut v);
    v
}

/// Upload a 4x4 column-major matrix uniform.
pub unsafe fn set_mat4(prog: &Program, name: &str, m: &[f32; 16]) {
    gl::UniformMatrix4fv(prog.uniform(name), 1, gl::FALSE, m.as_ptr());
}

/// A color render target (RGBA8). Recreates texture+fb on resize.
pub struct Fbo {
    id: u32,
    tex: u32,
    w: u32,
    h: u32,
}

impl Fbo {
    pub unsafe fn new(w: u32, h: u32) -> Self {
        let mut id = 0;
        let mut tex = 0;
        gl::GenFramebuffers(1, &mut id);
        gl::GenTextures(1, &mut tex);
        let f = Self { id, tex, w, h };
        f.alloc();
        f
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if (w, h) == (self.w, self.h) {
            return;
        }
        self.w = w;
        self.h = h;
        unsafe {
            self.alloc();
        }
    }

    unsafe fn alloc(&self) {
        gl::BindTexture(gl::TEXTURE_2D, self.tex);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
        gl::TexImage2D(
            gl::TEXTURE_2D,
            0,
            gl::RGBA8 as i32,
            self.w as i32,
            self.h as i32,
            0,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            std::ptr::null(),
        );
        gl::BindFramebuffer(gl::FRAMEBUFFER, self.id);
        gl::FramebufferTexture2D(gl::FRAMEBUFFER, gl::COLOR_ATTACHMENT0, gl::TEXTURE_2D, self.tex, 0);
        if gl::CheckFramebufferStatus(gl::FRAMEBUFFER) != gl::FRAMEBUFFER_COMPLETE {
            panic!("FBO {}x{} incomplete", self.w, self.h);
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }
    pub fn tex(&self) -> u32 {
        self.tex
    }
    pub fn size(&self) -> (u32, u32) {
        (self.w, self.h)
    }
}

/// A static VBO. `data` in floats.
#[derive(Clone, Copy)]
pub struct Vbo {
    id: u32,
}

impl Vbo {
    pub unsafe fn new(data: &[f32]) -> Self {
        let mut id = 0;
        gl::GenBuffers(1, &mut id);
        gl::BindBuffer(gl::ARRAY_BUFFER, id);
        gl::BufferData(
            gl::ARRAY_BUFFER,
            (data.len() * 4) as isize,
            data.as_ptr() as *const _,
            gl::STATIC_DRAW,
        );
        Vbo { id }
    }

    pub unsafe fn sub(&self, data: &[f32]) {
        gl::BindBuffer(gl::ARRAY_BUFFER, self.id);
        gl::BufferSubData(
            gl::ARRAY_BUFFER,
            0,
            (data.len() * 4) as isize,
            data.as_ptr() as *const _,
        );
    }

    pub fn id(&self) -> u32 {
        self.id
    }
}

/// A dynamic VBO (preallocated to `cap` floats, updated with sub()).
pub struct DynVbo {
    id: u32,
    cap: usize,
    verts: usize, // vertex count of the last update
}

impl DynVbo {
    pub unsafe fn new(cap: usize) -> Self {
        let mut id = 0;
        gl::GenBuffers(1, &mut id);
        gl::BindBuffer(gl::ARRAY_BUFFER, id);
        gl::BufferData(gl::ARRAY_BUFFER, (cap * 4) as isize, std::ptr::null(), gl::DYNAMIC_DRAW);
        DynVbo { id, cap, verts: 0 }
    }

    /// Upload `data` (floats). `verts_per_update` floats per vertex.
    pub unsafe fn update(&mut self, data: &[f32], floats_per_vert: usize) {
        assert!(data.len() <= self.cap, "DynVbo overflow");
        self.verts = data.len() / floats_per_vert;
        gl::BindBuffer(gl::ARRAY_BUFFER, self.id);
        gl::BufferSubData(
            gl::ARRAY_BUFFER,
            0,
            (data.len() * 4) as isize,
            data.as_ptr() as *const _,
        );
    }

    pub fn verts(&self) -> i32 {
        self.verts as i32
    }

    pub fn id(&self) -> u32 {
        self.id
    }
}
