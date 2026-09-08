/* SPDX-License-Identifier: GPL-2.0 */
/*
 * mach/emi_mpu.h - EMI MPU shim for the MediaTek MT6797 Wi-Fi port (B-21)
 *
 * The vendor 3.18 mach/emi_mpu.h (drivers/misc/mediatek/include/mt-plat/
 * mt6797/include/mach/emi_mpu.h) declares the EMI MPU region API that the
 * wlan/gen3 stack calls around the CONSYS shared-EMI window (firmware
 * download). Mainline 6.6 has no MediaTek EMI MPU driver for MT6797; the
 * implementation lives in wcn_hw_glue.c (currently a no-op - LK leaves the
 * EMI regions open, so restriction writes are only needed once a firmware
 * load actually uses the window).
 *
 * Values copied from the vendor header (register offsets / DRAM enums are
 * not needed by the wlan driver call sites).
 */
#ifndef __MT_EMI_MPU_SHIM_H
#define __MT_EMI_MPU_SHIM_H

#define NO_PROTECTION 0
#define SEC_RW 1
#define SEC_RW_NSEC_R 2
#define SEC_RW_NSEC_W 3
#define SEC_R_NSEC_R 4
#define FORBIDDEN 5
#define SEC_R_NSEC_RW 6

#define SET_ACCESS_PERMISSON(d7, d6, d5, d4, d3, d2, d1, d0) \
(((d7) << 21) | ((d6) << 18) | ((d5) << 15) | \
((d4) << 12) | ((d3) << 9) | ((d2) << 6) | ((d1) << 3) | (d0))

extern int emi_mpu_set_region_protection(unsigned long long start_addr,
	unsigned long long end_addr, int region,
	unsigned int access_permission);

#endif /* __MT_EMI_MPU_SHIM_H */
