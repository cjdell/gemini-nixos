/* SPDX-License-Identifier: GPL-2.0 */
/*
 * MT6797 CONSYS live WMT transport API (Gemini PDA, internal Wi-Fi B-21).
 *
 * The builtin mtk-consys-spike driver (drivers/soc/mediatek) brings the
 * CONSYS MCU up (power-on, ROM patch push, BTIF/DMA transport) and keeps
 * the WMT link alive after boot. This header defines the transport
 * surface the mtk_wcn module (drivers/misc/mediatek-connectivity) uses
 * to drive that same link: raw byte-stream TX, an RX byte-stream
 * callback (delivered from the spike's RX kthread in process context),
 * RX flush, and an MCU reset cycle.
 *
 * Ownership model: after the spike's gate G2b the link is owned by the
 * spike's live WMT client (/dev/consys_wmt). Calling
 * rx_cb_register(cb) with a non-NULL cb hands the link over to the WMT
 * core: the spike's RX kthread starts delivering bytes to cb, the live
 * client is disabled (-EBUSY), and TX is accepted from the WMT core.
 * rx_cb_register(NULL) hands the link back. Only one owner at a time.
 */
#ifndef _LINUX_CONSYS_WMT_H
#define _LINUX_CONSYS_WMT_H

#include <linux/types.h>

struct consys_wmt_ops {
	/* Send a raw byte stream (already STP-framed by the caller) to
	 * the CONSYS MCU. Returns 0 on success, -errno on failure. */
	int (*tx)(const u8 *buf, int len);
	/* Register the RX byte-stream callback (process context; the
	 * stream may contain partial frames - the receiver's parser must
	 * resync). cb == NULL releases the link back to the live client.
	 * Returns 0 on success, -ENETDOWN if the link is not up. */
	int (*rx_cb_register)(int (*cb)(const u8 *buf, unsigned int len));
	/* Discard all received bytes currently sitting in the RX FIFO. */
	void (*rx_flush)(void);
	/* MCU reset cycle: power-cycle the CONN scpsys domain (the only
	 * in-boot way back to a pre-patch MCU - AP_RGU SWSYSRST does not
	 * clear the MCU SRAM the ROM patch runs from, so post-patch
	 * software resets leave the MCU deaf to mand-mode WMT), re-init
	 * the BTIF AP side, and wait for the ROM to boot. The MCU comes
	 * back at boot #1 of a fresh power-on in the bare-ROM state
	 * (mand-mode STP) - the state the WMT core's init sequence
	 * expects. Returns 0 on success. */
	int (*mcu_reset)(void);
	/* Link is up (gate G2b passed, BTIF mapping alive). */
	bool (*ready)(void);
	/* Wake the CONSYS MCU over the BTIF WAK line (pulse ap_wakeup_consys
	 * low > one 32k period, then high). Required before any host TX
	 * when the MCU may have gone to its autonomous sleep (observed
	 * 2026-09-10: the WMT core's WAKEUP handshake times out and asserts
	 * a whole-chip reset when no wake reaches the MCU). Returns 0 on
	 * success. */
	int (*wake)(void);
};

/* NULL when the spike is not built / has not probed. */
extern const struct consys_wmt_ops *consys_wmt_transport;

/* B-30: toggle a VCN33 BT/WiFi sub-rail (type 0 = BT, 1 = WiFi; on
 * 1 = enable, 0 = disable). The WMT core's sw_init powers the RF LDOs
 * up for the post-push RF-calibration exchange (without them the
 * start-RF-cal command gets no response) and back down after. Returns
 * 0 on success, -ENODEV if the supply is not wired in the DT. */
extern int consys_paldo_ctrl(int type, int on);

/* B-32: base of the CONSYS-visible EMI window (the consys reserved-memory
 * region the spike programmed into the CONSYS EMI remap). The wlan/gen3
 * divided firmware download writes its EMI sections here. */
extern phys_addr_t g_emi_base;

#endif /* _LINUX_CONSYS_WMT_H */
