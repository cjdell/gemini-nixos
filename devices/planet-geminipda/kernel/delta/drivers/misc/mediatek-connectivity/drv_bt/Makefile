# MediaTek MT6797 Bluetooth HCI (hci_stp) - Gemini PDA port (2026-09-10)
#
# BlueZ HCI transport for the Bluetooth side of the on-die MT6630 CONSYS
# combo. H4-framed HCI over the STP BT channel (BT_TASK_INDX) of the WMT
# core; depends on the mtk_wcn module (chip power-on + STP transport).
#
# The -D flags must mirror drivers/misc/mediatek-connectivity/Makefile
# (ccflags-y): stp_exp.h/wmt_exp.h only expose the plain mtk_wcn_stp_* /
# mtk_wcn_wmt_* API (and the MTK_WCN_STP_IF_RX types) under
# MTK_WCN_WMT_STP_EXP_SYMBOL_ABSTRACT, which is how the exporting
# mtk_wcn module is built.
ccflags-y += -D MTK_WCN_BUILT_IN_DRIVER
ccflags-y += -D WMT_PLAT_ALPS=1
ccflags-y += -D WMT_UART_RX_MODE_WORK=0
ccflags-y += -D WMT_SDIO_MODE=1
ccflags-y += -D MTK_WCN_WMT_STP_EXP_SYMBOL_ABSTRACT
ccflags-y += -D WMT_IDC_SUPPORT=0
ccflags-y += -D MT6797
ccflags-y += -I$(src)/include

obj-$(CONFIG_MTK_WCN_BT_HCI) += hci_stp.o
hci_stp-objs := linux/hci_stp.o
