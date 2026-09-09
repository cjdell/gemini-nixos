/*
 * glprobe32.c — 32-bit Windows OpenGL present probe for the wine-wow64
 * stack (Gemini PDA, box64+wine-wow64). See pkgs/glprobe32.nix header.
 *
 * What it does:
 *   1. Create a 320x240 window, choose+set a double-buffered pixel
 *      format (24-bit color, 24-bit depth, 8-bit stencil), make a GL
 *      context current.
 *   2. 90 frames: glClearColor cycles red -> green -> blue -> white,
 *      glClear, draw a 2D yellow triangle (immediate mode), gdi32
 *      SwapBuffers, PeekMessage drain.
 *   3. Every 10th frame: glReadPixels of the BACK buffer (the buffer
 *      about to be presented) -> write BMP C:\probe32\frameNN.bmp.
 *   4. A framerate log line per frame in C:\probe32\frames.log.
 *
 * We read the back buffer because EGL surfaces commonly don't support
 * GL_FRONT reads; the back buffer is what SwapBuffers presents, so if
 * the BMP shows colors the presented content is colored and the
 * problem is the demo's own scene; if the BMP is uniformly the clear
 * color (or black) with no triangle, presentation/render of guest GL
 * is broken.
 *
 * Pure GL 1.1 + wgl — all builtin wine exports, no extensions.
 */

#include <windows.h>
#include <GL/gl.h>
#include <stdarg.h>
#include <stdio.h>
#include <string.h>

#define W 320
#define H 240

static FILE *g_log;

static void logf(const char *fmt, ...) {
    va_list ap;
    if (!g_log) return;
    va_start(ap, fmt);
    vfprintf(g_log, fmt, ap);
    va_end(ap);
    fflush(g_log);
}

/* Minimal 24bpp bottom-up BMP writer (no deps). */
static void write_bmp(const char *path, int w, int h, const unsigned char *rgb) {
    int row = (w * 3 + 3) & ~3;
    int img = row * h;
    unsigned char hdr[54];
    memset(hdr, 0, sizeof(hdr));
    hdr[0] = 'B'; hdr[1] = 'M';
    unsigned int fsz = 54 + img;
    hdr[2] = fsz & 0xff; hdr[3] = (fsz >> 8) & 0xff;
    hdr[4] = (fsz >> 16) & 0xff; hdr[5] = (fsz >> 24) & 0xff;
    hdr[10] = 54;
    hdr[14] = 40;
    unsigned int ww = w, hh = h;
    hdr[18] = ww & 0xff; hdr[19] = (ww >> 8) & 0xff; hdr[20] = (ww >> 16) & 0xff; hdr[21] = (ww >> 24) & 0xff;
    hdr[22] = hh & 0xff; hdr[23] = (hh >> 8) & 0xff; hdr[24] = (hh >> 16) & 0xff; hdr[25] = (hh >> 24) & 0xff;
    hdr[26] = 1;              /* planes */
    hdr[28] = 24;             /* bpp */
    FILE *f = fopen(path, "wb");
    if (!f) return;
    fwrite(hdr, 1, 54, f);
    /* bottom-up rows; rgb -> bgr */
    for (int y = h - 1; y >= 0; y--) {
        const unsigned char *src = rgb + (size_t)y * w * 3;
        unsigned char pad[3] = {0, 0, 0};
        int pp = 0;
        unsigned char *out = (unsigned char *)malloc(row);
        for (int x = 0; x < w; x++) {
            out[pp++] = src[x * 3 + 2]; /* B */
            out[pp++] = src[x * 3 + 1]; /* G */
            out[pp++] = src[x * 3 + 0]; /* R */
        }
        fwrite(out, 1, row, f);
        free(out);
        (void)pad;
    }
    fclose(f);
}

static LRESULT CALLBACK wndproc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
    if (msg == WM_DESTROY) PostQuitMessage(0);
    return DefWindowProcA(hwnd, msg, wp, lp);
}

int WINAPI WinMain(HINSTANCE inst, HINSTANCE prev, LPSTR cmd, int show) {
    (void)prev; (void)cmd; (void)show;
    CreateDirectoryA("C:\\probe32", NULL);
    g_log = fopen("C:\\probe32\\frames.log", "w");

    /* ---- window ---- */
    WNDCLASSA wc;
    memset(&wc, 0, sizeof(wc));
    wc.lpfnWndProc = wndproc;
    wc.hInstance = inst;
    wc.lpszClassName = "glprobe32";
    wc.hbrBackground = NULL;
    wc.style = CS_OWNDC;
    RegisterClassA(&wc);
    HWND hwnd = CreateWindowExA(0, "glprobe32", "glprobe32", WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                                CW_USEDEFAULT, CW_USEDEFAULT, W, H, NULL, NULL, inst, NULL);
    ShowWindow(hwnd, SW_SHOW);
    UpdateWindow(hwnd);
    logf("window hwnd=%p\n", hwnd);

    /* ---- pixel format + context ---- */
    HDC hdc = GetDC(hwnd);
    PIXELFORMATDESCRIPTOR pfd;
    memset(&pfd, 0, sizeof(pfd));
    pfd.nSize = sizeof(pfd);
    pfd.nVersion = 1;
    pfd.dwFlags = PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER;
    pfd.iPixelType = PFD_TYPE_RGBA;
    pfd.cColorBits = 24;
    pfd.cDepthBits = 24;
    pfd.cStencilBits = 8;
    int pf = ChoosePixelFormat(hdc, &pfd);
    logf("pixel format %d chosen, desc %d/%d/%d/%d\n",
         pf, pfd.cColorBits, pfd.cDepthBits, pfd.cStencilBits, pfd.iPixelType);
    SetPixelFormat(hdc, pf, &pfd);
    /* demo-faithful context dance: TWO contexts, lists shared the way
       dsd_lm.exe does it (its trace shows ctx B created first, then ctx
       A re-created sharing B's lists, A made current on the window). */
    HGLRC rcB = wglCreateContext(hdc);
    logf("ctxB %p current(on B for list build) %d\n", rcB, wglMakeCurrent(hdc, rcB));
    HGLRC rcA = wglCreateContext(hdc);
    logf("ctxA %p shareLists(B) %d\n", rcA, wglShareLists(rcA, rcB));
    logf("ctxA current %d\n", wglMakeCurrent(hdc, rcA));
    logf("renderer: %s\n", (const char *)glGetString(GL_RENDERER));
    logf("vendor: %s\n", (const char *)glGetString(GL_VENDOR));
    logf("version: %s\n", (const char *)glGetString(GL_VERSION));

    glViewport(0, 0, W, H);

    /* ---- display lists: build on B, execute on A (shared) ----
       The demo's whole scene is immediate-mode geometry captured into
       display lists (its imports: glNewList/glEndList/glCallList/glCallLists
       + glListBase); if list capture/execution breaks under wow64+box64,
       the demo shows only clear colors — white intro, then black. */
    wglMakeCurrent(hdc, rcB);
    GLuint list = glGenLists(1);
    glNewList(list, GL_COMPILE);
    glColor3f(1, 1, 0);          /* yellow triangle */
    glBegin(GL_TRIANGLES);
    glVertex2f(-0.8f, -0.8f);
    glVertex2f(0.8f, -0.8f);
    glVertex2f(0.0f, 0.8f);
    glEnd();
    glEndList();
    logf("list %u built on B, err=0x%x\n", list, glGetError());
    wglMakeCurrent(hdc, rcA);

    /* ---- frame loop ---- */
    static const float colors[4][3] = {
        {1, 0, 0}, {0, 1, 0}, {0, 0, 1}, {1, 1, 1}
    };
    for (int f = 0; f < 90; f++) {
        /* drain messages (WM_PAINT etc.) */
        MSG msg;
        while (PeekMessageA(&msg, NULL, 0, 0, PM_REMOVE)) {
            TranslateMessage(&msg);
            DispatchMessageA(&msg);
        }
        const float *c = colors[f & 3];
        glClearColor(c[0], c[1], c[2], 1.0f);
        glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
        /* the triangle, via the shared display list (from ctx B) */
        glCallList(list);
        GLenum err = glGetError();
        SwapBuffers(hdc);
        if ((f % 10) == 0) {
            /* read back the buffer we just presented (GL_BACK is the
               read buffer for double-buffered pixel formats) */
            unsigned char *px = (unsigned char *)malloc(W * H * 3);
            glReadBuffer(GL_BACK);
            glReadPixels(0, 0, W, H, GL_RGB, GL_UNSIGNED_BYTE, px);
            GLenum rerr = glGetError();
            char path[64];
            sprintf(path, "C:\\probe32\\frame%02d.bmp", f / 10);
            write_bmp(path, W, H, px);
            free(px);
            logf("frame %d clear=%g,%g,%g swap ok err=0x%x read err=0x%x wrote %s\n",
                 f, c[0], c[1], c[2], err, rerr, path);
        } else {
            logf("frame %d clear=%g,%g,%g err=0x%x\n", f, c[0], c[1], c[2], err);
        }
        Sleep(50); /* ~20 fps — llvmpipe trivial load */
    }

    logf("done\n");
    fclose(g_log);
    return 0;
}
