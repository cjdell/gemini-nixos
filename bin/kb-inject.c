/* kb-inject.c — synthesize the Gemini keyboard's raw key events
 *
 * Why: the Fn layer (Fn = KEY_RIGHTALT = XKB ISO_Level3_Shift/Mod5)
 * means every Fn combo is a real modifier+key chord, and verifying that
 * GNOME actually fires the media-key keybindings over ssh otherwise
 * needs a human at the device.  Like bin/touch-inject.c, this writes raw
 * input_events straight into the keyboard's evdev node: the kernel
 * injects them as if the matrix had produced them (input_inject_event),
 * so libinput -> mutter -> xkb -> the shell sees exactly what a finger
 * press produces — including the Mod5 modifier state.
 *
 * Usage:
 *   kb-inject fn+c            press & hold Fn (RALT), tap C, release Fn
 *   kb-inject fn+v fn+b       several chords in sequence
 *   kb-inject a b c           plain keys (no modifiers)
 *   kb-inject --device NAME   pick a device by name substring
 *                             (default: the device named "keyboard")
 *
 * Key names: fn/ralt/rightalt -> KEY_RIGHTALT; lalt/alt, lctrl/ctrl,
 * lshift/shift, lmeta/super, plus a-z and 0-9.  A chord is
 * MOD[+MOD...]+KEY (e.g. shift+fn+t).  Unknown names are rejected, so a
 * typo never injects a random key into the running session.
 *
 * Build (device, aarch64 — same recipe as touch-inject.c):
 *   nix-build -E '(import <nixpkgs> {}).stdenv.mkDerivation {
 *     name = "kbinj"; src = builtins.toFile "kb-inject.c"
 *       (builtins.readFile /root/kb-inject.c);
 *     buildPhase = "cc -O2 -o $out $SRCDIR/kb-inject.c"; }'
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
 * verified 2026-09-10 in touch-inject.c).  The kernel replaces the
 * timestamp on injection, so tv=0 is fine. */
struct evt {
	long tv_sec, tv_usec;
	__u16 type, code;
	__s32 value;
};

struct keyname {
	const char *name;
	__u16 code;
};

/* The handful of keys the Gemini matrix can produce that matter for
 * desktop verification.  Letters follow the PC evdev numbering. */
static const struct keyname names[] = {
	{ "fn", KEY_RIGHTALT }, { "ralt", KEY_RIGHTALT },
	{ "rightalt", KEY_RIGHTALT },
	{ "lalt", KEY_LEFTALT }, { "alt", KEY_LEFTALT },
	{ "lctrl", KEY_LEFTCTRL }, { "ctrl", KEY_LEFTCTRL },
	{ "rctrl", KEY_RIGHTCTRL },
	{ "lshift", KEY_LEFTSHIFT }, { "shift", KEY_LEFTSHIFT },
	{ "rshift", KEY_RIGHTSHIFT },
	{ "lmeta", KEY_LEFTMETA }, { "super", KEY_LEFTMETA },
	{ "esc", KEY_ESC }, { "tab", KEY_TAB }, { "space", KEY_SPACE },
	{ "a", KEY_A }, { "b", KEY_B }, { "c", KEY_C }, { "d", KEY_D },
	{ "e", KEY_E }, { "f", KEY_F }, { "g", KEY_G }, { "h", KEY_H },
	{ "i", KEY_I }, { "j", KEY_J }, { "k", KEY_K }, { "l", KEY_L },
	{ "m", KEY_M }, { "n", KEY_N }, { "o", KEY_O }, { "p", KEY_P },
	{ "q", KEY_Q }, { "r", KEY_R }, { "s", KEY_S }, { "t", KEY_T },
	{ "u", KEY_U }, { "v", KEY_V }, { "w", KEY_W }, { "x", KEY_X },
	{ "y", KEY_Y }, { "z", KEY_Z },
	{ "1", KEY_1 }, { "2", KEY_2 }, { "3", KEY_3 }, { "4", KEY_4 },
	{ "5", KEY_5 }, { "6", KEY_6 }, { "7", KEY_7 }, { "8", KEY_8 },
	{ "9", KEY_9 }, { "0", KEY_0 },
	/* media keys (what a normal laptop emits at level 0; used to prove
	 * injection + the stock bindings work before testing the Fn/Mod5
	 * chords, which live at the layout's level 3). */
	{ "mute", KEY_MUTE },
	{ "voldown", KEY_VOLUMEDOWN }, { "volup", KEY_VOLUMEUP },
	{ "brdown", KEY_BRIGHTNESSDOWN },
	{ "brup", KEY_BRIGHTNESSUP },
};

static void die(const char *m) { perror(m); exit(1); }

static int lookup(const char *name, __u16 *code) {
	for (size_t i = 0; i < sizeof names / sizeof names[0]; i++) {
		if (strcmp(names[i].name, name) == 0) {
			*code = names[i].code;
			return 0;
		}
	}
	fprintf(stderr, "kb-inject: unknown key '%s'\n", name);
	return -1;
}

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

static int find_keyboard(char *path, size_t n, const char *want) {
	DIR *d = opendir("/sys/class/input");
	if (!d) die("opendir /sys/class/input");
	struct dirent *e;
	while ((e = readdir(d))) {
		if (strncmp(e->d_name, "input", 5) != 0)
			continue;
		char namep[512], name[256] = {0};
		snprintf(namep, sizeof namep,
			 "/sys/class/input/%s/name", e->d_name);
		int fd = open(namep, O_RDONLY);
		if (fd < 0)
			continue;
		ssize_t r = read(fd, name, sizeof name - 1);
		close(fd);
		if (r <= 0 || strstr(name, want) == NULL)
			continue;
		/* evdev node = the eventN entry in /sys/class/input/inputN/.
		 * (The inputN/dev attribute is empty on this kernel build.) */
		char basedir[512];
		snprintf(basedir, sizeof basedir,
			 "/sys/class/input/%s", e->d_name);
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
	closedir(d);
	fprintf(stderr, "kb-inject: no input device matching '%s'\n", want);
	return -1;
}

/* One chord: press each modifier, tap the key, release modifiers in
 * reverse.  Separators are '+' (e.g. "fn+c", "shift+fn+t"). */
static int tap_chord(int fd, char *chord) {
	__u16 mods[8];
	int nmods = 0;
	char *save = NULL;
	char *tok = strtok_r(chord, "+", &save);
	if (!tok) return -1;

	while (tok) {
		char *next = strtok_r(NULL, "+", &save);
		__u16 code;
		if (lookup(tok, &code) != 0)
			return -1;
		if (next) {
			if (nmods >= (int)(sizeof mods / sizeof mods[0])) {
				fprintf(stderr, "kb-inject: too many modifiers\n");
				return -1;
			}
			mods[nmods++] = code;
		} else {
			for (int i = 0; i < nmods; i++) {
				w(fd, EV_KEY, mods[i], 1);
				w(fd, EV_SYN, SYN_REPORT, 0);
			}
			w(fd, EV_KEY, code, 1);
			w(fd, EV_SYN, SYN_REPORT, 0);
			usleep(40 * 1000);
			w(fd, EV_KEY, code, 0);
			w(fd, EV_SYN, SYN_REPORT, 0);
			for (int i = nmods - 1; i >= 0; i--) {
				w(fd, EV_KEY, mods[i], 0);
				w(fd, EV_SYN, SYN_REPORT, 0);
			}
		}
		tok = next;
	}
	return 0;
}

int main(int argc, char **argv) {
	const char *device = "keyboard";
	int first = 1;

	if (argc >= 3 && strcmp(argv[1], "--device") == 0) {
		device = argv[2];
		first = 3;
	}
	if (first >= argc) {
		fprintf(stderr,
			"usage: %s [--device NAME] CHORD [CHORD ...]\n"
			"  e.g. %s fn+c fn+v fn+b fn+n\n", argv[0], argv[0]);
		return 2;
	}

	char node[256];
	if (find_keyboard(node, sizeof node, device) != 0)
		return 1;
	int fd = open(node, O_WRONLY);
	if (fd < 0)
		die("open keyboard evdev node");
	printf("kb-inject: %s on %s\n", argv[first], node);

	int rc = 0;
	for (int i = first; i < argc; i++) {
		/* tap_chord mutates its argument, so work on a copy. */
		char buf[128];
		snprintf(buf, sizeof buf, "%s", argv[i]);
		if (tap_chord(fd, buf) != 0)
			rc = 2;
		usleep(60 * 1000);
	}
	close(fd);
	return rc;
}
