/*
 * gpioout.c — drive GPIO lines via the kernel gpio chardev (gpiochip0 =
 * mt6797 pio; the Gemini speaker-amp enables are SoC pads 243/244 = chip
 * lines 243/244). Replaces the /dev/mem pinctrl writes of spkamp.c for
 * the deployed `speaker` control: gpiolib owns the pad (proper request/
 * release, no raw register access).
 *
 * Run ON the device (root or gpio group). Compile with the device gcc:
 *   gcc -O2 -o gpioout gpioout.c
 *
 * Usage:
 *   gpioout <line=val>...              # e.g. gpioout 243=1 244=1
 *   gpioout -c <chip> <line=val>...    # chip path (default /dev/gpiochip0)
 *   gpioout -g <line>...               # read lines, print "line=value"
 *
 * Notes: lines stay at their last value after exit (gpiolib does not
 * reset outputs on release). Outputs requested push-pull, not
 * active-low (pads boot dir=out LOW via the DTS pinctrl default).
 */
#include <fcntl.h>
#include <linux/gpio.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <unistd.h>

#define DEFAULT_CHIP "/dev/gpiochip0"

int main(int argc, char **argv)
{
    const char *chip = DEFAULT_CHIP;
    int get_mode = 0, i;
    unsigned lines[32], vals[32];
    int n = 0;

    for (i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "-c") && i + 1 < argc) { chip = argv[++i]; }
        else if (!strcmp(argv[i], "-g")) { get_mode = 1; }
        else if (!get_mode) {
            char *eq = strchr(argv[i], '=');
            if (!eq) { fprintf(stderr, "bad arg %s\n", argv[i]); return 2; }
            char num[16];
            size_t l = eq - argv[i];
            if (l == 0 || l >= sizeof num) { fprintf(stderr, "bad line %s\n", argv[i]); return 2; }
            memcpy(num, argv[i], l); num[l] = 0;
            lines[n] = strtoul(num, NULL, 0);
            vals[n] = strtoul(eq + 1, NULL, 0);
            n++;
        } else { lines[n++] = strtoul(argv[i], NULL, 0); }
    }
    if (n == 0) { fprintf(stderr, "usage: %s [-c <chip>] <line=val>... | %s -g <line>...\n", argv[0], argv[0]); return 2; }

    int fd = open(chip, O_RDONLY);
    if (fd < 0) { perror(chip); return 1; }

    struct gpiohandle_request req;
    memset(&req, 0, sizeof req);
    for (i = 0; i < n; i++) { req.lineoffsets[i] = lines[i]; }
    req.lines = n;
    req.flags = get_mode ? GPIOHANDLE_REQUEST_INPUT : GPIOHANDLE_REQUEST_OUTPUT;
    if (!get_mode)
        for (i = 0; i < n; i++) { req.default_values[i] = vals[i]; }
    strcpy(req.consumer_label, "speaker-amp");
    if (ioctl(fd, GPIO_GET_LINEHANDLE_IOCTL, &req) < 0) { perror("GPIO_GET_LINEHANDLE_IOCTL"); return 1; }
    close(fd);

    if (!get_mode) {
        struct gpiohandle_data data;
        memset(&data, 0, sizeof data);
        for (i = 0; i < n; i++) { data.values[i] = vals[i]; }
        if (ioctl(req.fd, GPIOHANDLE_SET_LINE_VALUES_IOCTL, &data) < 0) { perror("SET_LINE_VALUES"); return 1; }
    } else {
        struct gpiohandle_data data;
        memset(&data, 0, sizeof data);
        if (ioctl(req.fd, GPIOHANDLE_GET_LINE_VALUES_IOCTL, &data) < 0) { perror("GET_LINE_VALUES"); return 1; }
        for (i = 0; i < n; i++) { printf("%u=%u\n", lines[i], data.values[i]); }
    }
    close(req.fd);
    return 0;
}
