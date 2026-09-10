/* wl-keymap-dump.c — print the xkb keymap a Wayland compositor hands to
 * clients (verification tool).
 *
 * Why: the LAST hop of the keyboard chain is "what keymap does the
 * compositor actually compile and send?".  Env vars (XKB_CONFIG_ROOT,
 * XKB_DEFAULT_LAYOUT), gsettings input-sources and the xkb registry can
 * each look right while the compositor still ships a different layout —
 * GNOME Shell silently falls back to 'us' when the source id is not in
 * the xkb registry (see docs/session-log.md 2026-09-10m).  This client
 * binds wl_seat -> wl_keyboard and prints the keymap it receives, so the
 * real layout can be checked without typing on the glass.
 *
 * Usage (on the device, as the session user):
 *   WAYLAND_DISPLAY=wayland-0 XDG_RUNTIME_DIR=/run/user/1000 \
 *     wl-keymap-dump              > /tmp/km.xkb     # whole keymap
 *   ... wl-keymap-dump --grep 'key <AE01>'            # one key
 *   ... wl-keymap-dump --grep 'layout'                # group names
 *
 * Build (devshell/device, no meson):
 *   gcc -O0 -o /tmp/wl-keymap-dump bin/wl-keymap-dump.c \
 *       $(pkg-config --cflags --libs wayland-client)
 *
 * Receipts: used 2026-09-10 to prove gnome-shell's keymap was plain US
 * (AE01 = [1, !], no level3) before the registry fix and the gemini
 * layout (AE01 = [1, !, |, F1], RALT = ISO_Level3_Shift) after.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wayland-client.h>

static const char *grep = NULL;

static void
kb_keymap(void *data, struct wl_keyboard *kb, uint32_t format,
          int32_t fd, uint32_t size)
{
    char *buf = malloc(size + 1);
    ssize_t n;

    if (!buf)
        _exit(2);
    n = read(fd, buf, size);
    if (n < 0)
        n = 0;
    buf[n] = '\0';
    close(fd);

    fprintf(stderr, "KEYMAP format=%u size=%u\n", format, (unsigned)size);

    if (grep) {
        char *save = NULL, *line = strtok_r(buf, "\n", &save);

        while (line) {
            if (strstr(line, grep))
                printf("%s\n", line);
            line = strtok_r(NULL, "\n", &save);
        }
    } else {
        fputs(buf, stdout);
    }
    fflush(stdout);
    _exit(0);
}
static void
kb_enter(void *d, struct wl_keyboard *k, uint32_t s,
         struct wl_surface *sf, struct wl_array *a)
{
}
static void
kb_leave(void *d, struct wl_keyboard *k, uint32_t s, struct wl_surface *sf)
{
}
static void
kb_key(void *d, struct wl_keyboard *k, uint32_t s, uint32_t t,
       uint32_t key, uint32_t st)
{
}
static void
kb_modifiers(void *d, struct wl_keyboard *k, uint32_t s, uint32_t md,
             uint32_t ml, uint32_t mk, uint32_t g)
{
}
static void
kb_repeat(void *d, struct wl_keyboard *k, int32_t r, int32_t de)
{
}
static const struct wl_keyboard_listener kb_listener = {
    .keymap = kb_keymap,
    .enter = kb_enter,
    .leave = kb_leave,
    .key = kb_key,
    .modifiers = kb_modifiers,
    .repeat_info = kb_repeat,
};

static void
seat_caps(void *d, struct wl_seat *seat, uint32_t caps)
{
    static struct wl_keyboard *kb;

    if ((caps & WL_SEAT_CAPABILITY_KEYBOARD) && !kb) {
        kb = wl_seat_get_keyboard(seat);
        wl_keyboard_add_listener(kb, &kb_listener, NULL);
    }
}

static void
seat_name(void *d, struct wl_seat *s, const char *n)
{
}
static const struct wl_seat_listener seat_listener = {
    .capabilities = seat_caps,
    .name = seat_name,
};

static void
reg_global(void *d, struct wl_registry *reg, uint32_t name,
           const char *iface, uint32_t ver)
{
    if (strcmp(iface, wl_seat_interface.name) == 0) {
        struct wl_seat *seat =
            wl_registry_bind(reg, name, &wl_seat_interface, 4);
        wl_seat_add_listener(seat, &seat_listener, NULL);
    }
}

static void
reg_remove(void *d, struct wl_registry *r, uint32_t n)
{
}
static const struct wl_registry_listener reg_listener = {
    .global = reg_global,
    .global_remove = reg_remove,
};

int
main(int argc, char **argv)
{
    struct wl_display *display;
    struct wl_registry *registry;

    if (argc > 2 && strcmp(argv[1], "--grep") == 0)
        grep = argv[2];

    display = wl_display_connect(NULL);
    if (!display) {
        fprintf(stderr, "wl-keymap-dump: cannot connect to Wayland "
                        "(set WAYLAND_DISPLAY/XDG_RUNTIME_DIR)\n");
        return 1;
    }

    registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &reg_listener, NULL);
    wl_display_roundtrip(display);
    wl_display_roundtrip(display);
    wl_display_roundtrip(display);

    fprintf(stderr, "wl-keymap-dump: no keymap received\n");
    return 3;
}
