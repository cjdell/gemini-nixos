/*
 * spkamp.c — drive the Gemini PDA's external speaker-amp enable pads
 * (SoC GPIO243 + GPIO244 = linux gpio 755/756; BSP audgpio "hpdepop" /
 * "hpdepop_e2" slots) with the vendor AW8736-style enable pattern.
 *
 * What/why: Android's stock audio HAL (audio.primary.mt6797.so,
 * AudioALSAHardwareResourceManager::OpenSpeakerPath) plays the built-in
 * flanking speakers by (1) enabling the codec HPL/HPR headphone drivers
 * ("headphone_output" sequence) AND (2) pulsing the external speaker amps
 * ("ext_speaker_output" = kernel Ext_Speaker_Amp_Switch =
 * AudDrv_GPIO_EXTAMP/EXTAMP2_Select(true,3)) — the 2us low/high pattern
 * that AW8736-class amps use to latch an enable/mode. This tool applies
 * that pattern to pads 243/244 (and optionally 234/235) directly via the
 * mainline mt6797 pinctrl register map (PARIS layout, gpio base
 * 0x10005000):
 *   dir  reg = 0x000 + (pad>>5)*0x10, bit pad&31
 *   dout reg = 0x100 + (pad>>5)*0x10, bit pad&31
 *   din  reg = 0x200 + (pad>>5)*0x10, bit pad&31
 *   mode reg = 0x300 + (pad>>3)*4,    bits (pad&7)*4 .. +3
 * Run ON the device (needs root + /dev/mem). Compile with the device gcc:
 *   gcc -O2 -o spkamp spkamp.c
 *
 * Usage:
 *   spkamp status [pad ...]        # print mode/dir/din/dout for pads
 *                                  # (default: 243 244 234 235 147 148)
 *   spkamp set <pad> <0|1>         # hold output low/high
 *   spkamp pulse <pad> [cycles] [us]
 *                                  # vendor enable: LOW, then cycles x
 *                                  # (LOW us, HIGH us), ends HIGH
 *                                  # default cycles=3 us=2 (AW8736 mode 3)
 *   spkamp off <pad>               # LOW (disable), alias of set 0
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/mman.h>
#include <time.h>

#define GPIO_PHYS  0x10005000UL
#define GPIO_MAP   0x1000

#define DIR_BASE   0x000
#define DOUT_BASE  0x100
#define DIN_BASE   0x200
#define MODE_BASE  0x300

static volatile uint32_t *gpio;

static uint32_t rd(uint32_t off) { return gpio[off / 4]; }
static void wr(uint32_t off, uint32_t v) { gpio[off / 4] = v; }

static uint32_t dir_reg(int pad)  { return DIR_BASE + ((pad >> 5) << 4); }
static uint32_t dout_reg(int pad) { return DOUT_BASE + ((pad >> 5) << 4); }
static uint32_t din_reg(int pad)  { return DIN_BASE + ((pad >> 5) << 4); }
static uint32_t mode_reg(int pad) { return MODE_BASE + ((pad >> 3) << 2); }
static int dir_bit(int pad)  { return pad & 31; }
static int mode_bit(int pad) { return (pad & 7) << 2; }

static void busywait_us(unsigned long us)
{
    struct timespec ts, te;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    uint64_t start = (uint64_t)ts.tv_sec * 1000000000UL + ts.tv_nsec;
    uint64_t end = start + us * 1000UL;
    do {
        clock_gettime(CLOCK_MONOTONIC, &te);
    } while ((uint64_t)te.tv_sec * 1000000000UL + te.tv_nsec < end);
}

static void set_out(int pad, int val)
{
    uint32_t r = rd(dout_reg(pad));
    if (val)
        r |= 1u << dir_bit(pad);
    else
        r &= ~(1u << dir_bit(pad));
    wr(dout_reg(pad), r);
}

static int get_in(int pad)
{
    return (rd(din_reg(pad)) >> dir_bit(pad)) & 1;
}

static void set_dir_out(int pad)
{
    uint32_t r = rd(dir_reg(pad));
    r |= 1u << dir_bit(pad);
    wr(dir_reg(pad), r);
}

static void set_mode_gpio(int pad)
{
    uint32_t r = rd(mode_reg(pad));
    r &= ~(0xful << mode_bit(pad));      /* function 0 = GPIO */
    wr(mode_reg(pad), r);
}

static void show_pad(int pad)
{
    uint32_t m = rd(mode_reg(pad));
    uint32_t d = rd(dir_reg(pad));
    uint32_t din = rd(din_reg(pad));
    uint32_t dout = rd(dout_reg(pad));
    int mb = mode_bit(pad), b = dir_bit(pad);
    printf("pad %3d (gpio %3d): mode=%u dir=%s din=%d dout=%d\n",
           pad, pad + 512,
           (m >> mb) & 0xf,
           ((d >> b) & 1) ? "out" : "in",
           (din >> b) & 1, (dout >> b) & 1);
}

int main(int argc, char **argv)
{
    if (argc < 2) {
        fprintf(stderr, "usage: %s status|set|pulse|off ...\n", argv[0]);
        return 2;
    }
    int fd = open("/dev/mem", O_RDWR | O_SYNC);
    if (fd < 0) { perror("/dev/mem"); return 1; }
    void *map = mmap(NULL, GPIO_MAP, PROT_READ | PROT_WRITE, MAP_SHARED,
                     fd, GPIO_PHYS);
    if (map == MAP_FAILED) { perror("mmap"); return 1; }
    gpio = map;

    if (!strcmp(argv[1], "status")) {
        if (argc > 2)
            for (int i = 2; i < argc; i++) show_pad(atoi(argv[i]));
        else
            for (int i = 0; i < 6; i++)
                show_pad((int[]){243, 244, 234, 235, 147, 148}[i]);
        return 0;
    }

    if (argc < 4 && strcmp(argv[1], "off")) {
        fprintf(stderr, "need pad (and value/cycles)\n");
        return 2;
    }
    int pad = atoi(argv[2]);
    if (pad < 0 || pad > 261) { fprintf(stderr, "pad %d out of range\n", pad); return 2; }

    if (!strcmp(argv[1], "set")) {
        set_mode_gpio(pad);
        set_dir_out(pad);
        set_out(pad, atoi(argv[3]) ? 1 : 0);
        printf("pad %d -> %s\n", pad, atoi(argv[3]) ? "HIGH" : "LOW");
        return 0;
    }

    if (!strcmp(argv[1], "off")) {
        set_mode_gpio(pad);
        set_dir_out(pad);
        set_out(pad, 0);
        printf("pad %d -> LOW (off)\n", pad);
        return 0;
    }

    if (!strcmp(argv[1], "pulse")) {
        int cycles = argc > 3 ? atoi(argv[3]) : 3;
        int us = argc > 4 ? atoi(argv[4]) : 2;
        set_mode_gpio(pad);
        set_dir_out(pad);
        set_out(pad, 0);                 /* Select(false): drive LOW first */
        busywait_us(10000);              /* vendor: 1-20 ms settle */
        for (int i = 0; i < cycles * 2; i++) {   /* L H L H ... ends HIGH */
            busywait_us(us);
            set_out(pad, i & 1);
        }
        printf("pad %d pulsed %d cycles @ %d us, now HIGH\n", pad, cycles, us);
        return 0;
    }

    fprintf(stderr, "unknown cmd %s\n", argv[1]);
    return 2;
}
