// SPDX-License-Identifier: GPL-2.0-only
/*
 * MT6797 / MT6351 power-reset driver for the Gemini PDA.
 *
 * This registers two system power transitions that mainline Linux lacks
 * on this SoC:
 *
 *   - restart:  the TOPRGU watchdog external reset, exactly the
 *               sequence LK uses for every one of its reboots
 *               (gemini-lk lk/platform/mt6797/mtk_wdt.c:mtk_wdt_reset()).
 *               The mainline mtk_wdt restart handler only pokes
 *               WDT_SWRST without first restoring WDT_MODE; on this
 *               unit that leaves the SoC in a dead "limbo" (PMIC on,
 *               CPUs stopped, LK never re-runs; observed on glass
 *               2026-09-10). Replicating LK's full sequence fixes it.
 *
 *   - poweroff: the MT6351 RTC BBPU "pull PWRBB low" write, exactly
 *               the sequence LK (mt_rtc.c:rtc_bbpu_power_down()) and
 *               the vendor Android kernel (mt_power_off()) use. If USB
 *               power is attached the PMIC cannot drop the rails, so
 *               fall back to LK's off-mode-charging path (WDT reset
 *               without the "bypass power key" bit).
 *
 * Both paths are bring-up knowledge from the legacy GeminiPDA project;
 * see docs/power-states.md in the gemini-nixos repo for the full
 * source-traced receipts and the on-glass test plan.
 */

#include <linux/bitops.h>
#include <linux/delay.h>
#include <linux/io.h>
#include <linux/module.h>
#include <linux/notifier.h>
#include <linux/of.h>
#include <linux/of_address.h>
#include <linux/of_platform.h>
#include <linux/platform_device.h>
#include <linux/reboot.h>
#include <linux/regmap.h>

/* ---- TOPRGU / watchdog registers (0x10007000) ---- */
#define MTK_WDT_MODE			0x00
#define MTK_WDT_RESTART			0x08
#define MTK_WDT_SWRST			0x14

#define MTK_WDT_MODE_ENABLE		BIT(0)
#define MTK_WDT_MODE_EXTEN		BIT(2)
#define MTK_WDT_MODE_IRQ		BIT(3)
#define MTK_WDT_MODE_AUTO_RESTART	BIT(4)
#define MTK_WDT_MODE_DUAL_MODE		BIT(6)
#define MTK_WDT_MODE_KEY		0x22000000U
#define MTK_WDT_RESTART_KEY		0x1971
#define MTK_WDT_SWRST_KEY		0x1209

/* ---- MT6351 RTC space, reached over the pwrap 16-bit regmap ---- */
#define MT6351_RTC_BBPU			0x4000
#define MT6351_RTC_AL_SEC		0x4018
#define MT6351_RTC_PROT			0x4036
#define MT6351_RTC_WRTGR		0x403c

#define MT6351_RTC_BBPU_PWREN		BIT(0)
#define MT6351_RTC_BBPU_AUTO		BIT(3)
#define MT6351_RTC_BBPU_CBUSY		BIT(6)
#define MT6351_RTC_BBPU_AUTO_PDN_SEL	BIT(6)
#define MT6351_RTC_BBPU_2SEC_EN		BIT(8)
#define MT6351_RTC_BBPU_KEY		(0x43 << 8)

#define MT6351_RTC_PROT_UNLOCK1		0x586a
#define MT6351_RTC_PROT_UNLOCK2		0x9136

struct mt6797_power {
	struct device *dev;
	void __iomem *wdt_base;
	struct regmap *pmic;
	struct notifier_block restart_nb;
};

/* register_platform_power_off() takes a bare callback: singleton. */
static struct mt6797_power *mt6797_power_priv;

/*
 * LK's mtk_wdt_reset(): reload first, then set the hardware-reboot mode
 * (with the key) and finally trigger the external reset. bypass=true is
 * LK's mode 1 ("bypass power key"): the PMIC power-cycles the SoC and it
 * self-boots. bypass=false is mode 0, used for the off-mode-charging
 * fallback (LK re-runs, waits on the power key).
 */
static void mt6797_wdt_reset(void __iomem *base, bool bypass)
{
	u32 mode;

	writel(MTK_WDT_RESTART_KEY, base + MTK_WDT_RESTART);

	mode = readl(base + MTK_WDT_MODE);
	mode &= ~(MTK_WDT_MODE_AUTO_RESTART | MTK_WDT_MODE_IRQ |
		  MTK_WDT_MODE_ENABLE | MTK_WDT_MODE_DUAL_MODE);
	mode |= MTK_WDT_MODE_KEY | MTK_WDT_MODE_EXTEN;
	if (bypass)
		mode |= MTK_WDT_MODE_AUTO_RESTART;
	writel(mode, base + MTK_WDT_MODE);

	udelay(100);
	writel(MTK_WDT_SWRST_KEY, base + MTK_WDT_SWRST);
}

static int mt6797_restart(struct notifier_block *nb, unsigned long action,
			  void *data)
{
	struct mt6797_power *p = container_of(nb, struct mt6797_power,
					      restart_nb);

	mt6797_wdt_reset(p->wdt_base, true);

	/* The reset should have fired; keep it alive if the SoC is slow. */
	while (1) {
		writel(MTK_WDT_SWRST_KEY, p->wdt_base + MTK_WDT_SWRST);
		mdelay(5);
	}

	return NOTIFY_DONE;
}

/* LK's rtc_write_trigger(): start the write and wait for !CBUSY. */
static void mt6351_rtc_trigger(struct regmap *pmic)
{
	unsigned int val, i;

	regmap_write(pmic, MT6351_RTC_WRTGR, 1);
	for (i = 0; i < 100; i++) {
		if (regmap_read(pmic, MT6351_RTC_BBPU, &val))
			break;
		if (!(val & MT6351_RTC_BBPU_CBUSY))
			break;
		udelay(10);
	}
}

static void mt6797_power_off(void)
{
	struct mt6797_power *p = mt6797_power_priv;
	unsigned int val;

	if (WARN_ON(!p))
		return;

	/* rtc_disable_2sec_reboot(): clear the 2-second reboot latch. */
	if (!regmap_read(p->pmic, MT6351_RTC_AL_SEC, &val)) {
		val &= ~(MT6351_RTC_BBPU_2SEC_EN |
			 MT6351_RTC_BBPU_AUTO_PDN_SEL);
		regmap_write(p->pmic, MT6351_RTC_AL_SEC, val);
		mt6351_rtc_trigger(p->pmic);
	}

	/* Unlock the RTC write interface. */
	regmap_write(p->pmic, MT6351_RTC_PROT, MT6351_RTC_PROT_UNLOCK1);
	mt6351_rtc_trigger(p->pmic);
	regmap_write(p->pmic, MT6351_RTC_PROT, MT6351_RTC_PROT_UNLOCK2);
	mt6351_rtc_trigger(p->pmic);

	/* Pull PWRBB low: KEY | AUTO | PWREN = 0x4309. */
	regmap_write(p->pmic, MT6351_RTC_BBPU,
		     MT6351_RTC_BBPU_KEY | MT6351_RTC_BBPU_AUTO |
		     MT6351_RTC_BBPU_PWREN);
	mt6351_rtc_trigger(p->pmic);

	/*
	 * If USB power holds the rails up we are still executing after a
	 * second; hand over to LK's off-mode charging (WDT reset without
	 * bypassing the power key). On battery the write above has
	 * already cut the AP and we never get here.
	 */
	mdelay(1000);
	mt6797_wdt_reset(p->wdt_base, false);

	while (1) {
		writel(MTK_WDT_SWRST_KEY, p->wdt_base + MTK_WDT_SWRST);
		mdelay(5);
	}
}

static int mt6797_power_probe(struct platform_device *pdev)
{
	struct mt6797_power *p;
	struct platform_device *pmic_pdev;
	struct device_node *np;
	struct resource *res;
	int ret;

	p = devm_kzalloc(&pdev->dev, sizeof(*p), GFP_KERNEL);
	if (!p)
		return -ENOMEM;

	p->dev = &pdev->dev;

	/*
	 * Do NOT use devm_platform_ioremap_resource(): the TOPRGU/WDT block
	 * at 0x10007000 is shared with the mainline mtk_wdt driver (bound
	 * through the watchdog@10007000 node). devm_ioremap_resource()
	 * calls devm_request_mem_region(), so whichever driver probes first
	 * would make the other fail with -EBUSY — and if mtk_wdt loses, the
	 * LK-armed watchdog is never kicked and the SoC resets a few
	 * seconds into every boot (observed on glass 2026-09-10). Map the
	 * resource without claiming it.
	 */
	res = platform_get_resource(pdev, IORESOURCE_MEM, 0);
	if (!res)
		return -ENODEV;
	p->wdt_base = devm_ioremap(&pdev->dev, res->start,
				   resource_size(res));
	if (!p->wdt_base)
		return -ENOMEM;

	np = of_parse_phandle(pdev->dev.of_node, "mediatek,pmic", 0);
	if (!np) {
		dev_err(&pdev->dev, "missing mediatek,pmic phandle\n");
		return -EINVAL;
	}
	pmic_pdev = of_find_device_by_node(np);
	of_node_put(np);
	if (!pmic_pdev)
		return -EPROBE_DEFER;

	p->pmic = dev_get_regmap(&pmic_pdev->dev, NULL);
	platform_device_put(pmic_pdev);
	if (!p->pmic)
		return -EPROBE_DEFER;

	/*
	 * Sanity-check that the RTC address window is reachable. This is
	 * informative only: never let a failed probe take the restart
	 * handler down with it.
	 */
	{
		unsigned int val;

		ret = regmap_read(p->pmic, MT6351_RTC_BBPU, &val);
		if (ret)
			dev_warn(&pdev->dev,
				 "cannot read MT6351 RTC space: %d (poweroff may not work)\n",
				 ret);
		else
			dev_info(&pdev->dev,
				 "MT6351 RTC_BBPU readback 0x%04x\n", val);
	}

	platform_set_drvdata(pdev, p);
	mt6797_power_priv = p;

	p->restart_nb.notifier_call = mt6797_restart;
	/* Above the mainline mtk_wdt handler's 128 (which is broken here). */
	p->restart_nb.priority = 200;
	ret = register_restart_handler(&p->restart_nb);
	if (ret) {
		dev_err(&pdev->dev, "cannot register restart handler: %d\n",
			ret);
		return ret;
	}

	ret = register_platform_power_off(mt6797_power_off);
	if (ret) {
		dev_err(&pdev->dev, "cannot register poweroff handler: %d\n",
			ret);
		unregister_restart_handler(&p->restart_nb);
		return ret;
	}

	dev_info(&pdev->dev,
		 "MT6797 restart + MT6351 poweroff handlers registered\n");
	return 0;
}

static int mt6797_power_remove(struct platform_device *pdev)
{
	struct mt6797_power *p = platform_get_drvdata(pdev);

	unregister_platform_power_off(mt6797_power_off);
	unregister_restart_handler(&p->restart_nb);
	mt6797_power_priv = NULL;

	return 0;
}

static const struct of_device_id mt6797_power_of_match[] = {
	{ .compatible = "mediatek,mt6797-power" },
	{ /* sentinel */ }
};
MODULE_DEVICE_TABLE(of, mt6797_power_of_match);

static struct platform_driver mt6797_power_driver = {
	.probe	= mt6797_power_probe,
	.remove	= mt6797_power_remove,
	.driver	= {
		.name		= "mt6797-power",
		.of_match_table	= mt6797_power_of_match,
	},
};
module_platform_driver(mt6797_power_driver);

MODULE_AUTHOR("Gemini PDA Linux port");
MODULE_DESCRIPTION("MT6797 restart + MT6351 poweroff (Gemini PDA)");
MODULE_LICENSE("GPL");
