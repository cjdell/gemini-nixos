/* SPDX-License-Identifier: GPL-2.0 */
/*
 * 6.6 port compat shims for the vendor 3.18 MediaTek connectivity tree
 * (Gemini PDA internal Wi-Fi, B-21). Included explicitly (after the file's
 * own includes) by the vendor .c files that rely on 3.18-era time idioms.
 */
#ifndef _WCN_COMPAT_H
#define _WCN_COMPAT_H

#include <linux/types.h>
#include <linux/time.h>
#include <linux/ktime.h>
#include <linux/timekeeping.h>

/* 6.6 y2038 cleanup: the legacy 32-bit timeval is now
 * struct __kernel_old_timeval (uapi, in the userspace-only guard block), so
 * bare `struct timeval` is not defined in kernel code. Provide the vendor's
 * name with the identical layout so the vendor time-stamping code compiles. */
struct timeval {
	__kernel_long_t	tv_sec;
	__kernel_long_t	tv_usec;
};

/* do_gettimeofday() was removed from the kernel; provide it on top of
 * ktime_get_real_ts64(). */
static inline int wcn_do_gettimeofday(struct timeval *tv)
{
	struct timespec64 ts;

	ktime_get_real_ts64(&ts);
	tv->tv_sec = ts.tv_sec;
	tv->tv_usec = ts.tv_nsec / 1000;
	return 0;
}
#ifndef do_gettimeofday
#define do_gettimeofday(tv) wcn_do_gettimeofday(tv)
#endif

#endif /* _WCN_COMPAT_H */
