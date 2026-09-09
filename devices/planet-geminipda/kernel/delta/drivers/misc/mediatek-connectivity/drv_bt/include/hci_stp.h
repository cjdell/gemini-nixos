/* SPDX-License-Identifier: GPL-2.0 */
/*
 * hci_stp.h - MediaTek MT6630 CONSYS Bluetooth HCI-over-STP driver
 *
 * Gemini PDA (MT6797X) bring-up port (2026-09-10): the 3.18 vendor
 * driver (GeminiPDA/repos/gemini-linux-kernel-3.18/drivers/misc/
 * mediatek/connectivity/drv_bt/, CONFIG_MTK_COMBO_BT_HCI "MTK BT driver
 * for BlueZ") modernized for Linux 6.6. See docs/wifi-consys.md for the
 * CONSYS/WMT bring-up context (the delta tree's mtk_wcn module owns the
 * chip power-on + STP transport; this driver is the BT HCI client).
 *
 * Layout mirrors the vendor driver so byte-level behaviour stays
 * comparable; the 6.6 port removes the /data config-file IO (get_fs/
 * set_fs are gone) and lets the BD address come from the chip eFUSE
 * (vendor bt_get_bd_addr path) or a module parameter.
 */

#ifndef _HCI_STP_H
#define _HCI_STP_H

#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/init.h>
#include <linux/types.h>
#include <linux/interrupt.h>
#include <linux/slab.h>
#include <linux/errno.h>
#include <linux/string.h>
#include <linux/ioctl.h>
#include <linux/skbuff.h>
#include <linux/spinlock.h>
#include <linux/fs.h>
#include <linux/delay.h>
#include <linux/kthread.h>
#include <linux/workqueue.h>
#include <linux/wait.h>
#include <linux/version.h>

#include <net/bluetooth/bluetooth.h>
#include <net/bluetooth/hci_core.h>

/* Select the TX execution model: a kthread, exactly like the vendor
 * (HCI_STP_TX_THRD); MTK's stp_send_data can block on the BTIF FIFO.
 */
#define HCI_STP_TX_THRD		(1)
#define HCI_STP_TX		(HCI_STP_TX_THRD)

/* Maximum delay per init command, x20 safe-guard (vendor values). */
#define BT_CMD_DELAY_MS_COMM	(100)
#define BT_CMD_DELAY_MS_RESET	(600)
#define BT_CMD_DELAY_SAFE_GUARD	(20)

/* H4 receiver states */
#define H4_W4_PACKET_TYPE	(0)
#define H4_W4_EVENT_HDR		(1)
#define H4_W4_ACL_HDR		(2)
#define H4_W4_SCO_HDR		(3)
#define H4_W4_DATA		(4)

struct hci_stp_init_cmd {
	unsigned char *hci_cmd;
	unsigned int cmd_sz;
	unsigned char *hci_evt;
	unsigned int evt_sz;
	const char *str;
};

#define hci_stp_init_entry(c) \
	{ .hci_cmd = c, .cmd_sz = sizeof(c), .hci_evt = c##_evt, \
	  .evt_sz = sizeof(c##_evt), .str = #c }

struct hci_stp {
	struct hci_dev *hdev;

	struct sk_buff_head txq;

	struct work_struct init_work;
	struct completion *p_init_comp;
	wait_queue_head_t *p_init_evt_wq;
	spinlock_t init_lock;	/* protects init_evt_rx_flag + waitq */
	unsigned int init_cmd_idx;
	int init_evt_rx_flag;	/* 1 ok / 0 timeout / -1 size / -2 content */

	/* H4 rx parser state, persistent across STP channel indications
	 * (a single HCI frame may arrive split over several STP rx
	 * callbacks). Serialized by the STP core (one BT rx context).
	 */
	struct sk_buff *rx_skb;
	unsigned int rx_count;
	int rx_state;
};

/* Radio configuration data (mirrors the vendor btradio_conf_data; the
 * set_bd_addr/set_radio/set_sleep/... HCI commands are built from it).
 */
struct btradio_conf_data {
	unsigned char addr[6];
	unsigned char voice[2];		/* unused by the 6630 script */
	unsigned char codec[4];		/* unused */
	unsigned char radio[6];
	unsigned char sleep[7];
	unsigned char feature[2];	/* unused */
	unsigned char tx_pwr_offset[3];
};

#endif /* _HCI_STP_H */
