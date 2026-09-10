/* touch-inject.c — synthesize NT36772 touchscreen events (Protocol B)
 *
 * Why: the Gemini PDA's touch input path (kernel evdev -> libinput in
 * gemwl -> wl_touch forwarding -> phoc's wayland-backend synthesized
 * wlr_touch -> phosh apps) needs an event source that a human finger
 * is NOT (remote verification over ssh, regression testing, gesture
 * checks without touching the glass). This program writes raw
 * input_events straight into the NT36772's evdev node — the kernel
 * treats them exactly like real sensor polls, so the whole stack sees
 * "real" multitouch.
 *
 * Usage: touch-inject <gesture>
 *   tap     one finger down+up at screen center
 *   swipe   one finger left->right across the middle
 *   pinch   two fingers converging at center (zoom-in style)
 *
 * Geometry: the driver already maps the sensor into landscape output
 * space, ABS_X 0..2160, ABS_Y 0..1080 (see novatek-nt36xxx.c).
 *
 * Build (device, aarch64):
 *   nix-build -E '(import <nixpkgs> {}).stdenv.mkDerivation
 *     { name = "touchinj"; src = builtins.toFile "touch-inject.c"
 *       (builtins.readFile /root/touch-inject.c);
 *       buildPhase = "cc -O2 -o $out $SRCDIR/touch-inject.c"; }'
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <dirent.h>
#include <linux/input.h>

/* aarch64 (64-bit) layout: 16-byte timeval FIRST — sizeof is 24, not the
 * 8 of the 32-bit world (evdev_write rejects short writes with EINVAL;
 * verified 2026-09-10). The kernel replaces the timestamp on injection,
 * so tv=0 is fine. */
struct evt {
	long tv_sec, tv_usec;
	__u16 type, code;
	__s32 value;
};

#define SW 2160
#define SH 1080

static int w(int fd, __u16 type, __u16 code, __s32 val) {
	struct evt e = {
		.tv_sec = 0, .tv_usec = 0,
		.type = type, .code = code, .value = val,
	};
	ssize_t r = write(fd, &e, sizeof e);
	if (r != (ssize_t)sizeof e) {
		perror("write evdev");
		exit(1);
	}
	return 0;
}

static void die(const char *m) { perror(m); exit(1); }

static int find_touch_node(char *path, size_t n) {
	DIR *d = opendir("/sys/class/input");
	if (!d) die("opdir /sys/class/input");
	struct dirent *e;
	while ((e = readdir(d))) {
		if (strncmp(e->d_name, "input", 5) != 0)
			continue;
		char namep[512], name[256] = {0};
		snprintf(namep, sizeof namep, "/sys/class/input/%s/name", e->d_name);
		int fd = open(namep, O_RDONLY);
		if (fd < 0)
			continue;
		ssize_t r = read(fd, name, sizeof name - 1);
		close(fd);
		if (r > 0 && strstr(name, "NT36772")) {
			/* evdev node = the eventN entry in /sys/class/input/inputN/.
			 * (The inputN/dev attribute is EMPTY on this kernel build —
			 * verified 2026-09-10 — so the dev-file trick does not work.)
			 */
			char basedir[512];
			snprintf(basedir, sizeof basedir, "/sys/class/input/%s", e->d_name);
			DIR *ed = opendir(basedir);
			if (ed) {
				struct dirent *ee;
				while ((ee = readdir(ed))) {
					if (strncmp(ee->d_name, "event", 5) == 0 &&
					    ee->d_name[5] >= '0' && ee->d_name[5] <= '9') {
						snprintf(path, n, "/dev/input/%s", ee->d_name);
						closedir(ed);
						closedir(d);
						return 0;
					}
				}
				closedir(ed);
			}
		}
	}
	closedir(d);
	fprintf(stderr, "no NT36772 input device found (driver bound?)\n");
	return -1;
}

/* Protocol B helpers */
static void finger_down(int fd, int slot, int id, int x, int y) {
	w(fd, EV_ABS, ABS_MT_SLOT, slot);
	w(fd, EV_ABS, ABS_MT_TRACKING_ID, id);
	w(fd, EV_ABS, ABS_MT_POSITION_X, x);
	w(fd, EV_ABS, ABS_MT_POSITION_Y, y);
	w(fd, EV_SYN, SYN_REPORT, 0);
}

static void finger_move(int fd, int slot, int x, int y) {
	w(fd, EV_ABS, ABS_MT_SLOT, slot);
	w(fd, EV_ABS, ABS_MT_POSITION_X, x);
	w(fd, EV_ABS, ABS_MT_POSITION_Y, y);
	w(fd, EV_SYN, SYN_REPORT, 0);
}

static void finger_up(int fd, int slot) {
	w(fd, EV_ABS, ABS_MT_SLOT, slot);
	w(fd, EV_ABS, ABS_MT_TRACKING_ID, -1);
	w(fd, EV_SYN, SYN_REPORT, 0);
}

int main(int argc, char **argv) {
	if (argc < 2) {
		fprintf(stderr, "usage: %s tap|swipe|pinch\n", argv[0]);
		return 2;
	}
	char node[256];
	if (find_touch_node(node, sizeof node) != 0)
		return 1;
	int fd = open(node, O_WRONLY);
	if (fd < 0)
		die("open evdev node");
	printf("touch-inject: %s on %s\n", argv[1], node);

	if (strcmp(argv[1], "tap") == 0) {
		finger_down(fd, 0, 1, SW / 2, SH / 2);
		usleep(80 * 1000);
		finger_up(fd, 0);
	} else if (strcmp(argv[1], "swipe") == 0) {
		finger_down(fd, 0, 1, SW * 1 / 4, SH / 2);
		for (int i = 1; i <= 12; i++) {
			finger_move(fd, 0, SW * 1 / 4 + i * (SW / 2) / 12, SH / 2);
			usleep(20 * 1000);
		}
		finger_up(fd, 0);
	} else if (strcmp(argv[1], "pinch") == 0) {
		/* two fingers 500px apart, converging to the center */
		finger_down(fd, 0, 1, SW / 2 - 250, SH / 2);
		finger_down(fd, 1, 2, SW / 2 + 250, SH / 2);
		for (int i = 1; i <= 12; i++) {
			int off = 250 * (12 - i) / 12;
			finger_move(fd, 0, SW / 2 - off, SH / 2);
			finger_move(fd, 1, SW / 2 + off, SH / 2);
			usleep(25 * 1000);
		}
		finger_up(fd, 0);
		finger_up(fd, 1);
	} else {
		fprintf(stderr, "unknown gesture '%s'\n", argv[1]);
		return 2;
	}
	close(fd);
	return 0;
}
