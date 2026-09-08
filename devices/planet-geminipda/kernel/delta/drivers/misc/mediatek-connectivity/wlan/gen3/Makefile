# SPDX-License-Identifier: GPL-2.0
#
# MediaTek MT6797 Wi-Fi gen3 802.11 host stack - Gemini PDA port (B-21 Phase 2)
#
# Port of the vendor 3.18 wlan/gen3 driver onto mainline 6.6. Built as the
# wlan_gen3 module (CONFIG_MTK_WCN_GEN3), the same split as the vendor
# (wlan_drv_gen3.ko on top of wmt_drv.ko): this module registers the wiphy +
# the CONSYS wifi data-path driver ("mt-wifi" / mediatek,wifi @ 0x180f0000);
# the actual wlan bring-up (wlanProbe -> netdev) is triggered by the WMT core
# (mtk_wcn.ko) calling the registered wlan_probe_cb when the Wi-Fi function is
# turned on.
#
# The original vendor Makefile (CONFIG_MTK_COMBO_WIFI etc.) is kept as
# Makefile.vendor. This file mirrors its MT6797 (CONSYS_6797) build:
#   WLAN_CHIP_ID = MT6797 (ahb_sdioLike HIF, no _HIF_SDIO, no -D<chip> for
#                          MT6628/30/32/5931/6620)
# with the Android-only feature toggles set to their CONSYS_6797 values.
#
# See docs/wifi-consys.md (Host-driver port) for state and plan.

# ---------------------------------------------------------------
# Compile options (values from the vendor Makefile, MT6797 path)
# ---------------------------------------------------------------
ccflags-y += -U MT6620 -U MT6628 -U MT5931 -U MT6630 -U MT6632
ccflags-y += -D MT6797
ccflags-y += -D LINUX
ccflags-y += -D CFG_SUPPORT_DEBUG_FS=0
ccflags-y += -D WLAN_INCLUDE_PROC
ccflags-y += -D CFG_SUPPORT_AGPS_ASSIST=1
ccflags-y += -D CFG_SUPPORT_TSF_USING_BOOTTIME=1
ccflags-y += -D ARP_MONITER_ENABLE=1
ccflags-y += -D CFG_SUPPORT_CHNL_CONFLICT_REVISE=1
ccflags-y += -D CFG_SUPPORT_AEE=0
ccflags-y += -D BUILD_QA_DBG=1
ccflags-y += -D CFG_SUPPORT_PASSPOINT=0
ccflags-y += -D CFG_HS20_DEBUG=0
ccflags-y += -D CFG_ENABLE_GTK_FRAME_FILTER=0
ccflags-y += -D CFG_MET_PACKET_TRACE_SUPPORT=0
ccflags-y += -D DBG=0
ccflags-y += -D FW_CFG_SUPPORT
# MT6797 builds the ahb_sdioLike HIF; _HIF_SDIO must stay undefined.
# This module is standalone-loadable (like the vendor wlan_drv_gen3.ko), so
# MTK_WCN_BUILT_IN_DRIVER stays undefined: gl_init.c then compiles
# module_init(initWlan) instead of exporting mtk_wcn_wlan_gen3_init.

# ---------------------------------------------------------------
# Include paths
# ---------------------------------------------------------------
ccflags-y += -I$(src)/os
ccflags-y += -I$(src)/os/linux/include
ccflags-y += -I$(src)/os/linux/hif/ahb_sdioLike/include
ccflags-y += -I$(src)/include
ccflags-y += -I$(src)/include/nic
ccflags-y += -I$(src)/include/mgmt
# shim headers for 3.18 MediaTek SoC includes (mach/mt_*, mt-plat/...):
# shared with the mtk_wcn (WMT core) port
ccflags-y += -I$(src)/../../mt_plat
ccflags-y += -I$(src)/../../mt_plat/mach
ccflags-y += -I$(src)/../../inc_mt
ccflags-y += -I$(src)/../../inc_lin

# ---------------------------------------------------------------
# Objects (vendor Makefile list, MT6797 = ahb_sdioLike HIF)
# ---------------------------------------------------------------
wlan_gen3-objs := \
	common/dump.o \
	common/wlan_lib.o \
	common/wlan_oid.o \
	common/wlan_bow.o \
	common/debug.o \
	common/wlan_p2p.o \
	common/fwcfg.o \
	nic/nic.o \
	nic/nic_tx.o \
	nic/nic_rx.o \
	nic/nic_pwr_mgt.o \
	nic/nic_rate.o \
	nic/cmd_buf.o \
	nic/que_mgt.o \
	nic/nic_cmd_event.o \
	nic/p2p_nic.o \
	mgmt/ais_fsm.o \
	mgmt/aaa_fsm.o \
	mgmt/assoc.o \
	mgmt/auth.o \
	mgmt/bss.o \
	mgmt/cnm.o \
	mgmt/cnm_timer.o \
	mgmt/cnm_mem.o \
	mgmt/hem_mbox.o \
	mgmt/mib.o \
	mgmt/privacy.o \
	mgmt/rate.o \
	mgmt/rlm.o \
	mgmt/rlm_domain.o \
	mgmt/rlm_obss.o \
	mgmt/rlm_protection.o \
	mgmt/rsn.o \
	mgmt/saa_fsm.o \
	mgmt/scan.o \
	mgmt/scan_fsm.o \
	mgmt/swcr.o \
	mgmt/roaming_fsm.o \
	mgmt/tkip_mic.o \
	mgmt/hs20.o \
	mgmt/tdls.o \
	mgmt/stats.o \
	mgmt/wapi.o \
	mgmt/p2p_dev_fsm.o \
	mgmt/p2p_dev_state.o \
	mgmt/p2p_role_fsm.o \
	mgmt/p2p_role_state.o \
	mgmt/p2p_func.o \
	mgmt/p2p_scan.o \
	mgmt/p2p_ie.o \
	mgmt/p2p_rlm.o \
	mgmt/p2p_assoc.o \
	mgmt/p2p_bss.o \
	mgmt/p2p_rlm_obss.o \
	os/linux/gl_init.o \
	os/linux/gl_kal.o \
	os/linux/gl_bow.o \
	os/linux/gl_wext.o \
	os/linux/gl_wext_priv.o \
	os/linux/gl_rst.o \
	os/linux/gl_cfg80211.o \
	os/linux/gl_vendor.o \
	os/linux/platform.o \
	os/linux/gl_p2p.o \
	os/linux/gl_p2p_cfg80211.o \
	os/linux/gl_p2p_init.o \
	os/linux/gl_p2p_kal.o \
	os/linux/gl_proc.o \
	os/linux/hif/ahb_sdioLike/arm.o \
	os/linux/hif/ahb_sdioLike/ahb.o \
	os/linux/hif/ahb_sdioLike/ahb_pdma.o \
	os/linux/hif/ahb_sdioLike/sdio_bus_driver.o
# NOTE [port 6.6]: os/linux/plat/mt6797/plat_priv.o (kalBoostCpu ->
# mt_ppm_sysboost_*) is deliberately NOT built: MTK PPM is not in mainline
# and gl_kal.c's __weak kalBoostCpu() fallback (logs instead) takes over.

obj-$(CONFIG_MTK_WCN_GEN3) += wlan_gen3.o
