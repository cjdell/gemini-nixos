// SPDX-License-Identifier: GPL-2.0
//
// Minimal MT6351 PMIC regulator driver — CONSYS VCN rails only.
//
// The MT6351 never gained a mainline MFD/regulator driver (only its ASoC
// codec exists). This driver covers exactly the four VCN LDOs the MT6797
// CONSYS block needs (Gemini PDA bring-up); every other rail is left as
// the bootloader configured it. Register addresses and bit positions are
// taken from the vendor 3.18 header
// drivers/misc/mediatek/include/mt-plat/mt6797/include/mach/upmu_hw.h
// (MT6351_LDO_VCN*_CON* / MT6351_PMIC_RG_VCN*_EN_*).
//
// The device sits behind the MediaTek PMIC wrapper: this platform driver
// binds the pwrap's "mediatek,mt6351" child node and uses the pwrap's
// regmap, following the mt6380-regulator precedent.

#include <linux/module.h>
#include <linux/of.h>
#include <linux/platform_device.h>
#include <linux/regmap.h>
#include <linux/regulator/driver.h>
#include <linux/regulator/of_regulator.h>

/* LDO control registers (PMIC address space, 16-bit regs via pwrap) */
#define MT6351_LDO_VCN28_CON0		0x0a0c
#define MT6351_LDO_VCN18_CON0		0x0a52
#define MT6351_LDO_VCN33_CON3		0x0a98	/* BT enable bank */
#define MT6351_LDO_VCN33_CON4		0x0a9a	/* WIFI enable bank */

/* Common CON0/CON3/CON4 bit layout for these LDOs */
#define MT6351_LDO_EN			BIT(1)
#define MT6351_LDO_ON_CTRL		BIT(3)	/* 1 = hardware (SRCLKEN) mode */

struct mt6351_regulator_info {
	struct regulator_desc desc;
};

#define MT6351_LDO_FIXED(_name, _uV, _en_reg)				\
{									\
	.desc = {							\
		.name = #_name,						\
		.of_match = of_match_ptr(#_name),			\
		.regulators_node = of_match_ptr("regulators"),		\
		.ops = &mt6351_ldo_ops,					\
		.type = REGULATOR_VOLTAGE,				\
		.id = MT6351_ID_##_name,				\
		.owner = THIS_MODULE,					\
		.n_voltages = 1,					\
		.fixed_uV = (_uV),					\
		.enable_reg = (_en_reg),				\
		.enable_mask = MT6351_LDO_EN,				\
	},								\
}

enum {
	MT6351_ID_vcn18,
	MT6351_ID_vcn28,
	MT6351_ID_vcn33_bt,
	MT6351_ID_vcn33_wifi,
	MT6351_ID_max
};

/* Fixed-voltage LDOs: the core serves fixed_uV when n_voltages == 1 */
static const struct regulator_ops mt6351_ldo_ops = {
	.enable = regulator_enable_regmap,
	.disable = regulator_disable_regmap,
	.is_enabled = regulator_is_enabled_regmap,
};

static struct mt6351_regulator_info mt6351_regulators[] = {
	MT6351_LDO_FIXED(vcn18, 1800000, MT6351_LDO_VCN18_CON0),
	MT6351_LDO_FIXED(vcn28, 2800000, MT6351_LDO_VCN28_CON0),
	MT6351_LDO_FIXED(vcn33_bt, 3300000, MT6351_LDO_VCN33_CON3),
	MT6351_LDO_FIXED(vcn33_wifi, 3300000, MT6351_LDO_VCN33_CON4),
};

static int mt6351_regulator_probe(struct platform_device *pdev)
{
	struct regmap *regmap = dev_get_regmap(pdev->dev.parent, NULL);
	struct regulator_config config = {};
	struct regulator_dev *rdev;
	int i, ret;

	if (!regmap) {
		dev_err(&pdev->dev, "no pwrap regmap on parent device\n");
		return -ENODEV;
	}

	/*
	 * Vendor consys power-on switches VCN28 to hardware (SRCLKEN)
	 * control mode before enabling it (mtk_wcn_consys_hw.c: "switch
	 * VCN28 to HW control mode", RG_VCN28_ON_CTRL = 1, applied when
	 * co_clock_flag = 0, which is this device's WMT_SOC.cfg value).
	 */
	ret = regmap_update_bits(regmap, MT6351_LDO_VCN28_CON0,
				 MT6351_LDO_ON_CTRL, MT6351_LDO_ON_CTRL);
	if (ret) {
		dev_err(&pdev->dev, "VCN28 ON_CTRL setup failed: %d\n", ret);
		return ret;
	}

	for (i = 0; i < MT6351_ID_max; i++) {
		config.dev = &pdev->dev;
		config.regmap = regmap;
		rdev = devm_regulator_register(&pdev->dev,
					       &mt6351_regulators[i].desc,
					       &config);
		if (IS_ERR(rdev)) {
			dev_err(&pdev->dev, "failed to register %s: %pe\n",
				mt6351_regulators[i].desc.name, rdev);
			return PTR_ERR(rdev);
		}
	}

	dev_info(&pdev->dev, "MT6351 VCN regulators registered\n");
	return 0;
}

static const struct of_device_id mt6351_of_match[] = {
	{ .compatible = "mediatek,mt6351", },
	{ /* sentinel */ },
};
MODULE_DEVICE_TABLE(of, mt6351_of_match);

static struct platform_driver mt6351_regulator_driver = {
	.driver = {
		.name = "mt6351-regulator",
		.of_match_table = mt6351_of_match,
	},
	.probe = mt6351_regulator_probe,
};
module_platform_driver(mt6351_regulator_driver);

MODULE_AUTHOR("Gemini PDA Linux port");
MODULE_DESCRIPTION("MT6351 PMIC VCN regulator driver (CONSYS rails only)");
MODULE_LICENSE("GPL");
