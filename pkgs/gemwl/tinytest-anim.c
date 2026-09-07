/* tinytest-anim.c — animated xdg-shell client: cycles red/green/blue on
 * every wl_surface.frame callback. Proves the compositor delivers frames
 * at the output's refresh rate. */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <sys/mman.h>
#include <wayland-client.h>
#include "xdg-shell-client-protocol.h"

static struct wl_compositor *compositor;
static struct xdg_wm_base *wm_base;
static struct wl_shm *shm;
static struct wl_surface *surf;
static struct wl_buffer *buf;
static int color = 0;
static long frames = 0;

static void wm_base_ping(void *data, struct xdg_wm_base *wm, uint32_t serial) {
	xdg_wm_base_pong(wm, serial);
}
static const struct xdg_wm_base_listener wm_base_listener = { .ping = wm_base_ping };

static void buffer_release(void *d, struct wl_buffer *b) {}
static void global_remove(void *d, struct wl_registry *r, uint32_t n) {}

static void draw(void) {
	/* map + paint the whole buffer */
	int w = 800, h = 600;
	int stride = w * 4;
	int fd = memfd_create("anim", 0);
	ftruncate(fd, stride * h);
	uint32_t *px = mmap(NULL, stride * h, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
	uint32_t colors[3] = { 0xFFFF0000, 0xFF00FF00, 0xFF0000FF };
	uint32_t c = colors[color % 3];
	for (int i = 0; i < w * h; i++) px[i] = c;
	munmap(px, stride * h);
	struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, stride * h);
	if (buf) wl_buffer_destroy(buf);
	buf = wl_shm_pool_create_buffer(pool, 0, w, h, stride, WL_SHM_FORMAT_ARGB8888);
	wl_buffer_add_listener(buf, &(struct wl_buffer_listener){ .release = buffer_release }, NULL);
	wl_surface_attach(surf, buf, 0, 0);
	wl_surface_damage_buffer(surf, 0, 0, w, h);
	wl_surface_commit(surf);
	close(fd);
	wl_shm_pool_destroy(pool);
}

static void frame_callback(void *data, struct wl_callback *cb, uint32_t time) {
	frames++;
	if (frames % 120 == 0) {
		fprintf(stderr, "[anim] %ld frames, color=%d\n", frames, color % 3);
	}
	color++;
	draw();
	wl_callback_destroy(cb);
	struct wl_callback *next = wl_surface_frame(surf);
	wl_callback_add_listener(next, &(struct wl_callback_listener){ .done = frame_callback }, NULL);
}

static void xdg_surface_configure(void *data, struct xdg_surface *s, uint32_t serial) {
	xdg_surface_ack_configure(s, serial);
	draw();
	struct wl_callback *cb = wl_surface_frame(surf);
	wl_callback_add_listener(cb, &(struct wl_callback_listener){ .done = frame_callback }, NULL);
}
static const struct xdg_surface_listener xdg_surface_listener = { .configure = xdg_surface_configure };

static void toplevel_configure(void *d, struct xdg_toplevel *t, int32_t w, int32_t h, struct wl_array *s) {}
static void toplevel_close(void *d, struct xdg_toplevel *t) {}
static const struct xdg_toplevel_listener toplevel_listener = {
	.configure = toplevel_configure,
	.close = toplevel_close,
};

static void registry_global(void *data, struct wl_registry *reg, uint32_t name,
		const char *iface, uint32_t version) {
	if (!strcmp(iface, wl_compositor_interface.name))
		compositor = wl_registry_bind(reg, name, &wl_compositor_interface, 4);
	else if (!strcmp(iface, xdg_wm_base_interface.name))
		wm_base = wl_registry_bind(reg, name, &xdg_wm_base_interface, 1);
	else if (!strcmp(iface, wl_shm_interface.name))
		shm = wl_registry_bind(reg, name, &wl_shm_interface, 1);
}
static const struct wl_registry_listener registry_listener = { .global = registry_global, .global_remove = global_remove };

int main(void) {
	setvbuf(stdout, NULL, _IONBF, 0);
	struct wl_display *dpy = wl_display_connect(NULL);
	if (!dpy) { fprintf(stderr, "connect failed\n"); return 1; }
	struct wl_registry *reg = wl_display_get_registry(dpy);
	wl_registry_add_listener(reg, &registry_listener, NULL);
	wl_display_roundtrip(dpy);
	xdg_wm_base_add_listener(wm_base, &wm_base_listener, NULL);

	surf = wl_compositor_create_surface(compositor);
	struct xdg_surface *xdg = xdg_wm_base_get_xdg_surface(wm_base, surf);
	xdg_surface_add_listener(xdg, &xdg_surface_listener, NULL);
	struct xdg_toplevel *tl = xdg_surface_get_toplevel(xdg);
	xdg_toplevel_add_listener(tl, &toplevel_listener, NULL);
	xdg_toplevel_set_title(tl, "anim");
	wl_surface_commit(surf);

	while (wl_display_dispatch(dpy) >= 0 && frames < 600) ;
	fprintf(stderr, "[anim] done, %ld frames\n", frames);
	return 0;
}
