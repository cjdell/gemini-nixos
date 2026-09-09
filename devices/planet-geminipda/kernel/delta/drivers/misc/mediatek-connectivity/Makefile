# MediaTek MT6797 connectivity (Wi-Fi) - Gemini PDA port (B-21)
#
# Port of the vendor 3.18 connectivity tree onto mainline 6.6.
# Phase 1: WMT protocol core (mtk_stp_wmt_soc) - transport + command layer
#          to the CONSYS MCU. Phase 2: wlan/gen3 802.11 stack (wlan0).
#
# See docs/wifi-consys.md for bring-up state and plan.

ccflags-y += -I$(src)
ccflags-y += -D MTK_WCN_BUILT_IN_DRIVER
ccflags-y += -D WMT_PLAT_ALPS=1
ccflags-y += -D WMT_UART_RX_MODE_WORK=0
ccflags-y += -D WMT_SDIO_MODE=1
ccflags-y += -D MTK_WCN_WMT_STP_EXP_SYMBOL_ABSTRACT
ccflags-y += -D WMT_IDC_SUPPORT=0
ccflags-y += -D MT6797
ccflags-y += -D CFG_SPM_WORKAROUND_FOR_HOTSPOT=0

subdir-ccflags-y += \
	-I$(src)/common_main/linux/include \
	-I$(src)/common_main/core/include \
	-I$(src)/common_main/include \
	-I$(src)/common_main/mt6797/include \
	-I$(src)/common_main/mt6797 \
	-I$(src)/common_detect \
	-I$(src)/btif_inc/inc \
	-I$(src)/btif_inc/plat_inc \
	-I$(src)/mt_plat \
	-I$(src)/inc_mt \
	-I$(src)/inc_lin \
	-I$(src)/conn_md_inc

obj-$(CONFIG_MTK_WCN) += mtk_wcn.o

mtk_wcn-objs := \
	common_main/core/wmt_core.o \
	common_main/core/wmt_ctrl.o \
	common_main/core/wmt_func.o \
	common_main/core/wmt_ic_soc.o \
	common_main/core/wmt_ic_6620.o \
	common_main/core/wmt_ic_6628.o \
	common_main/core/wmt_ic_6630.o \
	common_main/core/wmt_ic_6632.o \
	common_main/core/wmt_lib.o \
	common_main/core/wmt_conf.o \
	common_main/core/wmt_exp.o \
	common_main/core/stp_core.o \
	common_main/core/stp_exp.o \
	common_main/core/psm_core.o \
	common_main/core/btm_core.o \
	common_main/linux/osal.o \
	common_main/linux/stp_btif.o \
	common_main/linux/stp_dbg.o \
	common_main/linux/stp_dbg_soc.o \
	common_main/linux/stp_dbg_combo.o \
	common_main/linux/wmt_dev.o \
	common_main/linux/bgw_desense.o \
	common_main/linux/wmt_dbg.o \
	common_detect/mtk_wcn_stub_alps.o \
	common_detect/wmt_detect.o \
	common_detect/wmt_detect_pwr.o \
	common_detect/wmt_gpio.o \
	wcn_hw_glue.o

# --- platform hardware glue (integration phase, B-21) ---------------------
# common_main/mt6797/{mtk_wcn_consys_hw.c,wmt_plat_alps.c} implement the
# wmt_plat_* / mtk_wcn_cmb_hw_* SoC power/EMI/clock/GPIO surface. They pull in
# MediaTek 3.18 SoC driver headers (mt_clkmgr, emi_mpu, upmu, mtk_hibernate_dpm,
# mt_clkbuf_ctl) whose implementations are not in mainline 6.6. The CONSYS
# bring-up (power-on, MCU reset, ROM patch push) is already done directly by
# drivers/soc/mediatek/mtk-consys-spike.c, so the glue (wcn_hw_glue.c) provides
# these symbols backed by the spike / direct register access. Re-enable the
# mt6797 objects once that layer is ported.
#
# 	common_main/mt6797/mtk_wcn_consys_hw.o \
# 	common_main/mt6797/wmt_plat_alps.o \

# --- wlan/gen3 802.11 host stack (Phase 2, B-21) ---------------------------
# Separate module wlan_gen3 (CONFIG_MTK_WCN_GEN3) mirroring the vendor's
# wlan_drv_gen3.ko-on-wmt_drv.ko split. Its own Makefile holds the gen3
# ccflags + object list (this dir's ccflags-y does NOT propagate down).
obj-$(CONFIG_MTK_WCN_GEN3) += wlan/gen3/

# --- Bluetooth HCI (BT bring-up, 2026-09-10) ------------------------------
# Separate module hci_stp (CONFIG_MTK_WCN_BT_HCI): the BlueZ HCI transport
# over the STP BT channel of the WMT core (the BT side of the same MT6630
# CONSYS combo). Ported from the vendor 3.18 drv_bt (CONFIG_MTK_COMBO_BT_HCI).
obj-$(CONFIG_MTK_WCN_BT_HCI) += drv_bt/
