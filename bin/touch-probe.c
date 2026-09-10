/* touch-probe.c — minimal wl_touch Wayland client (verification tool)
 *
 * STATUS: WIP (2026-09-10) — does NOT compile against the system's
 * wayland 1.26 yet: since wayland 1.23 the generated protocol headers
 * use the unified `struct wl_interface` (extern const), so the classic
 * per-interface listener structs (struct wl_seat_interface { ... })
 * no longer exist. Needs the new-API migration (see session log
 * 2026-09-10c for the details + the store arch gotcha).
 *
 * Why: proves the LAST hop of the Gemini PDA's touch chain — that the
 * nested compositor (phoc/labwc) actually delivers wl_touch
 * down/motion/up/frame to its clients, i.e. to phosh apps. Connects to
 * $WAYLAND_DISPLAY (phoc's display inside the nested session), binds
 * wl_seat, requests the touch capability, and logs every touch event
 * with coordinates.
 *
 * Usage: WAYLAND_DISPLAY=<phoc socket> touch-probe [seconds]
 *   (default: run until killed; give a duration to auto-exit)
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/epoll.h>
#include <poll.h>

#include <wayland-client.h>
#include <wayland-client-protocol.h>

static struct wl_display *display;
static struct wl_seat *seat;
static struct wl_touch *touch;
static struct wl_registry *registry;
static int duration;

static void seat_handle_capabilities(void *data, struct wl_seat *seat,
		uint32_t caps) {
	fprintf(stderr, "probe: seat capabilities: %s%s%s\n",
		(caps & WL_SEAT_CAPABILITY_POINTER) ? "pointer " : "",
		(caps & WL_SEAT_CAPABILITY_KEYBOARD) ? "keyboard " : "",
		(caps & WL_SEAT_CAPABILITY_TOUCH) ? "TOUCH" : "");
	if (caps & WL_SEAT_CAPABILITY_TOUCH && touch == NULL)
		touch = wl_seat_get_touch(seat);
}

static const struct wl_seat_interface seat_impl = {
	.capabilities = seat_handle_capabilities,
};

static void registry_handle_global(void *data, struct wl_registry *registry,
		uint32_t name, char const *interface, uint32_t version) {
	if (strcmp(interface, wl_seat_interface.name) == 0) {
		if (version < 5) {
			fprintf(stderr, "probe: seat v%u < 5, no get_touch\n", version);
			return;
		}
		seat = wl_registry_bind(registry, name, &wl_seat_interface, 5);
		wl_seat_add_listener(seat, &seat_impl, NULL);
	}
}

static void registry_handle_global_remove(void *data, struct wl_registry *registry,
		uint32_t name) {
}

static const struct wl_registry_interface registry_impl = {
	.global = registry_handle_global,
	.global_remove = registry_handle_global_remove,
};

static void touch_handle_down(void *data, struct wl_touch *touch,
		uint32_t time, struct wl_surface *surface, int32_t id,
		wl_fixed_t x, wl_fixed_t y) {
	fprintf(stderr, "probe: TOUCH DOWN  id=%d (%.1f,%.1f)\n",
		id, wl_fixed_to_double(x), wl_fixed_to_double(y));
}

static void touch_handle_up(void *data, struct wl_touch *touch,
		uint32_t time, int32_t id) {
	fprintf(stderr, "probe: TOUCH UP    id=%d\n", id);
}

static void touch_handle_motion(void *data, struct wl_touch *touch,
		int32_t id, wl_fixed_t x, wl_fixed_t y) {
	fprintf(stderr, "probe: TOUCH MOVE  id=%d (%.1f,%.1f)\n",
		id, wl_fixed_to_double(x), wl_fixed_to_double(y));
}

static void touch_handle_frame(void *data, struct wl_touch *touch) {
}

static void touch_handle_cancel(void *data, struct wl_touch *touch) {
	fprintf(stderr, "probe: TOUCH CANCEL\n");
}

static const struct wl_touch_interface touch_impl = {
	.down = touch_handle_down,
	.up = touch_handle_up,
	.motion = touch_handle_motion,
	.frame = touch_handle_frame,
	.cancel = touch_handle_cancel,
};

int main(int argc, char **argv) {
	duration = argc > 1 ? atoi(argv[1]) : 0;
	display = wl_display_connect(NULL);
	if (display == NULL) {
		fprintf(stderr, "probe: cannot connect to $WAYLAND_DISPLAY\n");
		return 1;
	}
	registry = wl_display_get_registry(display);
	wl_registry_add_listener(registry, &registry_impl, NULL);
	wl_display_dispatch(display); /* registry globals */
	if (seat == NULL) {
		fprintf(stderr, "probe: no wl_seat global\n");
		return 1;
	}
	wl_seat_add_listener(seat, &seat_impl, NULL);
	wl_display_dispatch(display); /* capabilities */
	if (touch == NULL) {
		fprintf(stderr, "probe: seat has NO touch capability\n");
		return 2;
	}
	wl_touch_add_listener(touch, &touch_impl, NULL);
	fprintf(stderr, "probe: watching touch on %s (ctrl-c or timeout to stop)\n",
		getenv("WAYLAND_DISPLAY") ? getenv("WAYLAND_DISPLAY") : "(null)");
	int ep = epoll_create1(0);
	struct epoll_event ev = { .events = EPOLLIN,
		.data.fd = wl_display_get_fd(display) };
	epoll_ctl(ep, EPOLL_CTL_ADD, wl_display_get_fd(display), &ev);
	struct pollfd pfd = { .fd = ep, .events = POLLIN };
	while (1) {
		int timeout = duration > 0 ? duration * 1000 : -1;
		int r = duration > 0 ? poll(&pfd, 1, timeout) :
			epoll_wait(ep, &ev, 1, -1);
		if (r < 0)
			break;
		if (r > 0 && (wl_display_dispatch(display) < 0))
			break;
		if (duration > 0 && r == 0) {
			fprintf(stderr, "probe: timeout\n");
			break;
		}
	}
	wl_display_disconnect(display);
	return 0;
}
