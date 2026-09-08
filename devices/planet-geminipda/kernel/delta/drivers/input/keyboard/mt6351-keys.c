// SPDX-License-Identifier: GPL-2.0
/*
 * MT6351 PMIC side-key driver for the Gemini PDA.
 *
 * The unit has two non-keyboard buttons, both wired to the MT6351 PMIC
 * (NOT the SoC KPD / AW9523 matrix — verified live 2026-09-06):
 *
 *   - the "Esc (On)" key at the keyboard's ESC position = the PMIC
 *     PWRKEY input:  TOPSTATUS 0x220 bit 1 (PWRKEY_DEB)
 *   - the silver "Voice Assist" side button (right edge) = the PMIC
 *     HOMEKEY input: TOPSTATUS 0x220 bit 2 (HOMEKEY_DEB)
 *
 * Both bits are debounced by the PMIC and ACTIVE-LOW (LK receipt:
 * lk/platform/mt6797/mt_pmic.c pmic_detect_powerkey() prints "Release"
 * for val==1 and "Press" for val==0). Mainline's mtk-pmic-keys does not
 * cover MT6351 (its of_match is mt6397/mt6323/mt6331/mt6357/mt6358 and
 * it needs the mt6397-MFD parent this platform lacks), so this driver
 * polls TOPSTATUS over the pwrap regmap and reports each key with the
 * keycode from its DT node (linux,keycodes). This unit maps:
 *
 *   esc-on  (PWRKEY_DEB, bit 1) -> KEY_ESC   (the keycap says ESC)
 *   silver  (HOMEKEY_DEB, bit 2) -> KEY_SLEEP (intended future sleep
 *          button; inert until suspend exists — logind drop-in sets
 *          HandleSuspendKey=ignore)
 *
 * Polling (not IRQ): the PWRKEY/HOMEKEY debounce bits have no Linux IRQ
 * path on MT6797 (the PMIC INT/EINT routing is unwired in mainline), and
 * a 25 ms poll is far faster than the key events users generate. The
 * 8-10 s PWRKEY hold is a PMIC-level hardware reset — software sees a
 * momentary key only (same as stock).
 *
 * DT: a "mt6351-keys" child of the pwrap "pmic: mt6351" node; each key
 * subnode carries "mediatek,keybit" (TOPSTATUS bit, 1 or 2) and
 * "linux,keycodes" (one keycode).
 */
#include <linux/delay.h>
#include <linux/input.h>
#include <linux/module.h>
#include <linux/of.h>
#include <linux/platform_device.h>
#include <linux/regmap.h>
#include <linux/workqueue.h>

#define MT6351_TOPSTATUS	0x0220	/* PMIC TOPSTATUS reg (16-bit) */
#define MT6351_POLL_MS		25

struct mt6351_key {
	u32 bit;		/* TOPSTATUS bit (1 = PWRKEY_DEB ...) */
	u32 keycode;		/* Linux key code from DT */
	bool pressed;		/* current reported state */
};

struct mt6351_keys {
	struct device *dev;
	struct regmap *regmap;
	struct input_dev *input;
	struct delayed_work poll_work;
	int nkeys;
	struct mt6351_key keys[4];	/* PWRKEY, HOMEKEY, ... */
};

static void mt6351_keys_poll(struct work_struct *work)
{
	struct mt6351_keys *d = container_of(work, struct mt6351_keys,
					     poll_work.work);
	u32 val = 0;
	int i;

	regmap_read(d->regmap, MT6351_TOPSTATUS, &val);
	for (i = 0; i < d->nkeys; i++) {
		struct mt6351_key *k = &d->keys[i];
		bool pressed = !(val & BIT(k->bit));	/* active-low */

		if (pressed != k->pressed) {
			input_report_key(d->input, k->keycode, pressed);
			k->pressed = pressed;
		}
	}
	input_sync(d->input);

	schedule_delayed_work(&d->poll_work, msecs_to_jiffies(MT6351_POLL_MS));
}

static int mt6351_keys_probe(struct platform_device *pdev)
{
	struct device *dev = &pdev->dev;
	struct device_node *np = dev->of_node;
	struct mt6351_keys *d;
	struct input_dev *input;
	struct device_node *child;
	u32 val;
	int i = 0, error;

	d = devm_kzalloc(dev, sizeof(*d), GFP_KERNEL);
	if (!d)
		return -ENOMEM;
	d->dev = dev;
	platform_set_drvdata(pdev, d);
	d->regmap = dev_get_regmap(dev->parent, NULL);
	if (!d->regmap) {
		dev_err(dev, "no pwrap regmap on parent %s\n",
			dev_name(dev->parent));
		return -ENODEV;
	}

	for_each_child_of_node(np, child) {
		if (i >= ARRAY_SIZE(d->keys)) {
			of_node_put(child);
			break;
		}
		if (of_property_read_u32(child, "mediatek,keybit",
					 &d->keys[i].bit) ||
		    of_property_read_u32(child, "linux,keycodes",
					 &d->keys[i].keycode)) {
			dev_warn(dev, "%pOF: needs mediatek,keybit + "
				 "linux,keycodes; skipping\n", child);
			continue;
		}
		i++;
	}
	if (i == 0) {
		dev_err(dev, "no usable key subnodes\n");
		return -EINVAL;
	}
	d->nkeys = i;

	/* settle to the current PMIC state so probe emits no phantom key */
	regmap_read(d->regmap, MT6351_TOPSTATUS, &val);
	for (i = 0; i < d->nkeys; i++)
		d->keys[i].pressed = !(val & BIT(d->keys[i].bit));

	input = devm_input_allocate_device(dev);
	if (!input)
		return -ENOMEM;
	d->input = input;
	input->name = "mt6351-keys";
	input->phys = "pwrap/mt6351/input0";
	input->id.bustype = BUS_HOST;
	input->dev.parent = dev;
	__set_bit(EV_KEY, input->evbit);
	for (i = 0; i < d->nkeys; i++) {
		__set_bit(d->keys[i].keycode, input->keybit);
		dev_info(dev, "key bit %u -> KEY_%u (%s)\n",
			 d->keys[i].bit, d->keys[i].keycode,
			 d->keys[i].bit == 1 ? "ESC/On" : "side");
	}
	error = input_register_device(input);
	if (error)
		return error;

	INIT_DELAYED_WORK(&d->poll_work, mt6351_keys_poll);
	schedule_delayed_work(&d->poll_work,
			      msecs_to_jiffies(MT6351_POLL_MS));
	dev_info(dev, "MT6351 side keys: %d key(s), polling every %d ms\n",
		 d->nkeys, MT6351_POLL_MS);
	return 0;
}

static int mt6351_keys_remove(struct platform_device *pdev)
{
	struct mt6351_keys *d = dev_get_drvdata(&pdev->dev);

	cancel_delayed_work_sync(&d->poll_work);
	return 0;
}

static const struct of_device_id mt6351_keys_of_match[] = {
	{ .compatible = "mediatek,mt6351-keys", },
	{ /* sentinel */ },
};
MODULE_DEVICE_TABLE(of, mt6351_keys_of_match);

static struct platform_driver mt6351_keys_driver = {
	.probe = mt6351_keys_probe,
	.remove = mt6351_keys_remove,
	.driver = {
		.name = "mt6351-keys",
		.of_match_table = mt6351_keys_of_match,
	},
};
module_platform_driver(mt6351_keys_driver);

MODULE_AUTHOR("Gemini PDA Linux port");
MODULE_DESCRIPTION("MT6351 PMIC side-key input driver (Gemini PDA)");
MODULE_LICENSE("GPL");
