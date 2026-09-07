/* wlegltst.c — Wayland client using the EGL wayland platform (wl_egl):
 * creates a window surface, clears it red, swaps. This is exactly the
 * buffer path KWin/Qt use as Wayland clients. */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wayland-client.h>
#include <wayland-egl.h>
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES2/gl2.h>
#include "xdg-shell-client-protocol.h"

static struct wl_compositor *compositor;
static struct xdg_wm_base *wm_base;
static struct wl_display *dpy;
static struct wl_egl_window *egl_window;
static EGLDisplay egldpy;
static EGLSurface egl_surf;
static int frames = 0;

static void wm_base_ping(void *d, struct xdg_wm_base *w, uint32_t s) { xdg_wm_base_pong(w, s); }
static const struct xdg_wm_base_listener wm_base_listener = { .ping = wm_base_ping };

static int configured = 0;
static void xdg_surface_configure(void *d, struct xdg_surface *s, uint32_t serial) {
    xdg_surface_ack_configure(s, serial);
    configured = 1;
    printf("[wlegl] configure serial=%u\n", serial);
}
static const struct xdg_surface_listener xdg_surface_listener = { .configure = xdg_surface_configure };

static void toplevel_configure(void *d, struct xdg_toplevel *t, int32_t w, int32_t h, struct wl_array *s) {
    printf("[wlegl] toplevel configure %dx%d\n", w, h);
}
static void toplevel_close(void *d, struct xdg_toplevel *t) { printf("[wlegl] close\n"); }
static const struct xdg_toplevel_listener toplevel_listener = { .configure = toplevel_configure, .close = toplevel_close };

static void global_remove(void *d, struct wl_registry *r, uint32_t n) {}

static void registry_global(void *data, struct wl_registry *reg, uint32_t name,
        const char *iface, uint32_t version) {
    if (!strcmp(iface, wl_compositor_interface.name))
        compositor = wl_registry_bind(reg, name, &wl_compositor_interface, 4);
    else if (!strcmp(iface, xdg_wm_base_interface.name))
        wm_base = wl_registry_bind(reg, name, &xdg_wm_base_interface, 1);
    else if (!strcmp(iface, "wl_drm"))
        printf("[wlegl] wl_drm present (v%u)\n", version);
    else if (!strcmp(iface, "zwp_linux_dmabuf_v1"))
        printf("[wlegl] linux_dmabuf present (v%u)\n", version);
}
static const struct wl_registry_listener registry_listener = { .global = registry_global, .global_remove = global_remove };

int main(void) {
    setvbuf(stdout, NULL, _IONBF, 0);
    dpy = wl_display_connect(NULL);
    if (!dpy) { fprintf(stderr, "connect failed\n"); return 1; }
    struct wl_registry *reg = wl_display_get_registry(dpy);
    wl_registry_add_listener(reg, &registry_listener, NULL);
    wl_display_roundtrip(dpy);
    xdg_wm_base_add_listener(wm_base, &wm_base_listener, NULL);

    /* EGL on the wayland platform */
    egldpy = eglGetPlatformDisplay(EGL_PLATFORM_WAYLAND_EXT, dpy, NULL);
    printf("[wlegl] eglGetPlatformDisplay(wayland): %s\n", egldpy ? "ok" : "FAIL");
    EGLint maj, min;
    if (!eglInitialize(egldpy, &maj, &min)) { printf("eglInit failed 0x%x\n", (unsigned)eglGetError()); return 1; }
    const char *exts = eglQueryString(egldpy, EGL_EXTENSIONS);
    printf("[wlegl] EGL %d.%d; has bind_wayland: %d\n", maj, min,
           exts && strstr(exts, "EGL_WL_bind_wayland_display"));

    const EGLint ca[] = { EGL_SURFACE_TYPE, EGL_WINDOW_BIT,
                          EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT,
                          EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8,
                          EGL_BLUE_SIZE, 8, EGL_ALPHA_SIZE, 8, EGL_NONE };
    EGLConfig cfg; EGLint nc = 0;
    eglChooseConfig(egldpy, ca, &cfg, 1, &nc);
    printf("[wlegl] window ES2 8888 configs: %d\n", nc);
    if (nc == 0) { printf("[wlegl] FAIL: no config\n"); return 1; }

    struct wl_surface *surf = wl_compositor_create_surface(compositor);
    struct xdg_surface *xdg = xdg_wm_base_get_xdg_surface(wm_base, surf);
    xdg_surface_add_listener(xdg, &xdg_surface_listener, NULL);
    struct xdg_toplevel *tl = xdg_surface_get_toplevel(xdg);
    xdg_toplevel_add_listener(tl, &toplevel_listener, NULL);
    xdg_toplevel_set_title(tl, "wlegl");
    wl_surface_commit(surf);

    /* xdg-shell: wait for the configure, ack it, THEN attach buffers */
    for (int i = 0; i < 50 && !configured; i++) {
        wl_display_dispatch(dpy);
        usleep(20000);
    }
    if (!configured) { printf("[wlegl] never configured\n"); return 1; }

    egl_window = wl_egl_window_create(surf, 800, 600);
    if (!egl_window) { printf("[wlegl] wl_egl_window_create FAILED\n"); return 1; }
    EGLContext ctx = eglCreateContext(egldpy, cfg, EGL_NO_CONTEXT,
        (const EGLint[]){ EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE });
    egl_surf = eglCreateWindowSurface(egldpy, cfg, egl_window, NULL);
    if (egl_surf == EGL_NO_SURFACE) { printf("[wlegl] eglCreateWindowSurface FAILED 0x%x\n", (unsigned)eglGetError()); return 1; }
    eglMakeCurrent(egldpy, egl_surf, egl_surf, ctx);
    printf("[wlegl] GL: %s | %s\n", glGetString(GL_VERSION), glGetString(GL_RENDERER));

    glClearColor(1, 0, 0, 1);
    for (int i = 0; i < 900; i++) {
        glClear(GL_COLOR_BUFFER_BIT);
        eglSwapBuffers(egldpy, egl_surf);
        frames++;
        if (frames % 30 == 0) fprintf(stderr, "[wlegl] %d swaps\n", frames);
        wl_display_dispatch(dpy);
        usleep(16000);
    }
    printf("[wlegl] done, %d swaps\n", frames);
    return 0;
}
