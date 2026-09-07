/* tinytest.c — minimal xdg-shell client that prints every event it gets,
 * then commits a red shm buffer on configure. For debugging gemwl. */
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

static void buffer_release(void *d, struct wl_buffer *b) {
	printf("[client] buffer released\n");
}

/* See tinytest-anim.c: libwayland stores the listener vtable POINTER, so
 * it must outlive the wl_buffer — a function-scope compound literal here
 * dangled and the async release event segfaulted one dispatch later.
 * [fixed 2026-09-07] */
static const struct wl_buffer_listener buffer_listener = {
	.release = buffer_release,
};

static void toplevel_close(void *d, struct xdg_toplevel *t) {
	printf("[client] toplevel.close\n");
}

static void global_remove(void *d, struct wl_registry *r, uint32_t n) {}

static void wm_base_ping(void *data, struct xdg_wm_base *wm, uint32_t serial) {
	printf("[client] got wm_base ping %u, ponging\n", serial);
	xdg_wm_base_pong(wm, serial);
}

static const struct xdg_wm_base_listener wm_base_listener = {
	.ping = wm_base_ping,
};

static void xdg_surface_configure(void *data, struct xdg_surface *s, uint32_t serial) {
	struct wl_surface *surf = data;
	printf("[client] xdg_surface.configure serial=%u\n", serial);
	xdg_surface_ack_configure(s, serial);

	int w = 400, h = 300;
	int stride = w * 4;
	int fd = memfd_create("buf", 0);
	ftruncate(fd, stride * h);
	uint32_t *px = mmap(NULL, stride * h, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
	for (int i = 0; i < w * h; i++) px[i] = 0xFFFF0000; /* ARGB red */
	munmap(px, stride * h);

	struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, stride * h);
	struct wl_buffer *buf = wl_shm_pool_create_buffer(pool, 0, w, h, stride,
		WL_SHM_FORMAT_ARGB8888);
	wl_buffer_add_listener(buf, &buffer_listener, NULL);

	wl_surface_attach(surf, buf, 0, 0);
	wl_surface_damage(surf, 0, 0, w, h);
	wl_surface_commit(surf);
	printf("[client] committed red %dx%d buffer\n", w, h);
}

static const struct xdg_surface_listener xdg_surface_listener = {
	.configure = xdg_surface_configure,
};

static void toplevel_configure(void *data, struct xdg_toplevel *t,
		int32_t w, int32_t h, struct wl_array *states) {
	printf("[client] toplevel.configure %dx%d\n", w, h);
}

static const struct xdg_toplevel_listener toplevel_listener = {
	.configure = toplevel_configure,
	.close = toplevel_close,
};

static void registry_global(void *data, struct wl_registry *reg, uint32_t name,
		const char *iface, uint32_t version) {
	printf("[client] registry: %s v%u\n", iface, version);
	if (!strcmp(iface, wl_compositor_interface.name))
		compositor = wl_registry_bind(reg, name, &wl_compositor_interface, 4);
	else if (!strcmp(iface, xdg_wm_base_interface.name))
		wm_base = wl_registry_bind(reg, name, &xdg_wm_base_interface, 1);
	else if (!strcmp(iface, wl_shm_interface.name))
		shm = wl_registry_bind(reg, name, &wl_shm_interface, 1);
}

static const struct wl_registry_listener registry_listener = {
	.global = registry_global,
	.global_remove = global_remove,
};

int main(void) {
	struct wl_display *dpy = wl_display_connect(NULL);
	if (!dpy) { fprintf(stderr, "connect failed\n"); return 1; }
	printf("[client] connected\n");

	struct wl_registry *reg = wl_display_get_registry(dpy);
	wl_registry_add_listener(reg, &registry_listener, NULL);
	wl_display_roundtrip(dpy);

	if (!compositor || !wm_base || !shm) { fprintf(stderr, "missing globals\n"); return 1; }
	xdg_wm_base_add_listener(wm_base, &wm_base_listener, NULL);

	struct wl_surface *surf = wl_compositor_create_surface(compositor);
	struct xdg_surface *xdg = xdg_wm_base_get_xdg_surface(wm_base, surf);
	xdg_surface_add_listener(xdg, &xdg_surface_listener, surf);
	struct xdg_toplevel *tl = xdg_surface_get_toplevel(xdg);
	xdg_toplevel_add_listener(tl, &toplevel_listener, NULL);
	xdg_toplevel_set_title(tl, "tinytest");
	wl_surface_commit(surf);
	printf("[client] created surface + committed initial state\n");

	for (int i = 0; i < 30; i++) {
		if (wl_display_dispatch(dpy) < 0) break;
	}
	printf("[client] done\n");
	return 0;
}
