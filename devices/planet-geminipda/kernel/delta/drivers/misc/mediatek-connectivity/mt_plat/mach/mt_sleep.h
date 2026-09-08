/* SPDX-License-Identifier: GPL-2.0 */
/*
 * mach/mt_sleep.h - SPM wake-reason shim for the MT6797 Wi-Fi port (B-21)
 *
 * The wlan/gen3 stack's wakeup-reason debug (gl_kal.c,
 * CFG_SUPPORT_WAKEUP_REASON_DEBUG) uses these types/constants from the 3.18
 * SPM header and provides its own __weak implementations of the functions
 * (SPM is not in mainline 6.6). Only the type/enum surface is needed here;
 * values copied from the 3.18 mt_spm.h / mt_sleep.h.
 */
#ifndef __MT_SLEEP_SHIM_H
#define __MT_SLEEP_SHIM_H

enum wake_src {
	WAKE_SRC_CONN2AP = (1U << 10),
	WAKE_SRC_SYSPWREQ = (1U << 24),
};

typedef enum {
	WR_NONE = 0,
	WR_UART_BUSY = 1,
	WR_PCM_ASSERT = 2,
	WR_PCM_TIMER = 3,
	WR_WAKE_SRC = 4,
	WR_UNKNOWN = 5,
} wake_reason_t;

#endif /* __MT_SLEEP_SHIM_H */
