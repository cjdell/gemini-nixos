// SPDX-License-Identifier: GPL-2.0
/*
 * sramldo-smc.c — issue the MTK BigiDVFS SRAM-LDO SMC from userspace.
 *
 * What/why: the Cortex-A72 (cluster2/cpu8-9) bring-up sequence requires
 * setting the big-cluster SRAM LDO to 1.1 V via the secure SMC
 * MTK_SIP_KERNEL_IDVFS_BIGIDVFSSRAMLDOSET = 0xC20003BF (BSP receipt:
 * gemini-linux-kernel-3.18 drivers/misc/mediatek/base/power/mt6797/
 * mt_idvfs.c BigiDVFSSRAMLDOSet() calls SEC_BIGIDVFSSRAMLDOSET(mVolts_x100),
 * called from cpu_psci_cpu_boot()'s cpu_power_on_buck()). EL0 cannot
 * issue SMC, so this module exposes a misc device:
 *
 *   echo 110000 > /dev/idvfs-sramldo    # arg = microvolts_x100 (1.1 V)
 *
 * 110000 (1.1 V) is the value the BSP uses for A72 bring-up.
 *
 * Usage: use before `echo 1 > /sys/devices/system/cpu/cpu8/online`.
 * See the GeminiPDA project's docs/session-log.md 2026-09-04 (9th/10th)
 * and build/a72-bringup/ (source of this file, verbatim).
 *
 * In this tree the module is built by the kernel derivation's
 * postInstall hook (../default.nix) against the same build tree, so it
 * is ABI-identical to the boot kernel; it lands in
 * $out/lib/modules/<ver>/extra/ and is loaded by the gemini-a72-up
 * systemd service (services/gemini-pda.nix).
 */

#include <linux/init.h>
#include <linux/module.h>
#include <linux/miscdevice.h>
#include <linux/fs.h>
#include <linux/uaccess.h>
#include <linux/arm-smccc.h>

#define MTK_SIP_BIGIDVFSSRAMLDOSET	0xC20003BFUL

static long sramldo_ioctl(struct file *file, unsigned int cmd, unsigned long arg)
{
	return -ENOTTY;
}

static ssize_t sramldo_write(struct file *file, const char __user *buf,
			     size_t count, loff_t *ppos)
{
	struct arm_smccc_res res;
	char kbuf[32];
	unsigned long val;
	int ret;

	if (count == 0 || count >= sizeof(kbuf))
		return -EINVAL;
	if (copy_from_user(kbuf, buf, count))
		return -EFAULT;
	kbuf[count] = '\0';

	ret = kstrtoul(kbuf, 0, &val);
	if (ret)
		return ret;
	if (val < 50000 || val > 120000)
		return -ERANGE;	/* BSP range check: 500 mV .. 1200 mV */

	arm_smccc_smc(MTK_SIP_BIGIDVFSSRAMLDOSET, val, 0, 0, 0, 0, 0, 0, &res);
	pr_info("idvfs-sramldo: SMC 0xC20003BF(%lu) -> a0=0x%lx err=%ld\n",
		val, res.a0, res.a0 ? (long)res.a0 : 0L);
	return res.a0 ? -EIO : count;
}

static const struct file_operations sramldo_fops = {
	.owner		= THIS_MODULE,
	.write		= sramldo_write,
	.unlocked_ioctl	= sramldo_ioctl,
};

static struct miscdevice sramldo_misc = {
	.minor	= MISC_DYNAMIC_MINOR,
	.name	= "idvfs-sramldo",
	.fops	= &sramldo_fops,
};

static int __init sramldo_init(void)
{
	return misc_register(&sramldo_misc);
}

static void __exit sramldo_exit(void)
{
	misc_deregister(&sramldo_misc);
}

module_init(sramldo_init);
module_exit(sramldo_exit);
MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("MT6797 BigiDVFS SRAM-LDO SMC helper (A72 cluster bring-up)");
