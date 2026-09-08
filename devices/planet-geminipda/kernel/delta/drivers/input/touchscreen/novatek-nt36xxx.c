// SPDX-License-Identifier: GPL-2.0-only
/*
 * Driver for the Novatek NT36772 touchscreen controller on the Planet
 * Computers Gemini PDA (i2c4 @ 0x62).
 *
 * Hardware identity (live-verified on this unit 2026-09-04 via raw i2c-dev
 * probing on the running kernel, see docs/session-log.md):
 *   - trim ID bytes 00 00 03 72 66 03  -> NT36772 (vendor trim table entry 8)
 *   - fw version 0x05, PID 0x0101, native portrait 1080x2160 (abs_x_max 1080,
 *     abs_y_max 2160), event map 0x11e00, 65-byte event buffer, 6-byte records
 *
 * Protocol (from the GPL vendor driver in the 3.18 tree,
 * drivers/input/touchscreen/mediatek/aeon_nt36xxx/, Planet MT6797 commit
 * c5b0be85): the chip answers at hardware address 0x62 while in its
 * bootloader; after the boot/reset sequence it moves to the logical target
 * address 0x01 (NOT a second i2c client - the target is copied into each
 * i2c_msg, exactly like the vendor CTP_I2C_READ/WRITE helpers).
 *
 * Unlike the pda-mainline draft (patches/v7.1.3/0075) this port has no
 * threaded-IRQ requirement: pinctrl-mt6797 cannot deliver the EINT on
 * CTP_INT (GPIO85) yet (docs blocker B-11), so the driver polls the event
 * buffer like the Gemini keyboard and SSD2092 touch drivers do. Reset is
 * I2C-only (vendor sequence below), so no reset GPIO is required either.
 *
 * The touchscreen DT properties (touchscreen-size-x/y + inverted/swap)
 * reproduce the vendor orientation transform: sensor native portrait
 * 1080x2160 -> the landscape fb/desktop 2160x1080 (X'=y, Y'=1080-x).
 */

#include <linux/delay.h>
#include <linux/gpio/consumer.h>
#include <linux/i2c.h>
#include <linux/input.h>
#include <linux/input/mt.h>
#include <linux/input/touchscreen.h>
#include <linux/module.h>
#include <linux/regulator/consumer.h>

#include <asm/unaligned.h>

#define NVT36XXX_HW_ADDR		0x62
#define NVT36XXX_FW_ADDR		0x01
#define NVT36XXX_XDATA_CMD		0xff
#define NVT36XXX_TRIM_CMD		0x4e
#define NVT36XXX_TRIM_LEN		6
#define NVT36XXX_EVENT_MAP		0x11e00
#define NVT36XXX_EVENT_DATA_LEN		65
#define NVT36XXX_FW_INFO_CMD		0x78
#define NVT36XXX_FW_INFO_LEN		16
#define NVT36XXX_RESET_STATE_CMD	0x60
#define NVT36XXX_RESET_STATE_LEN	5
#define NVT36XXX_PROJECT_ID_CMD		0x9a
#define NVT36XXX_MAX_TOUCHES		10
#define NVT36XXX_MAX_COORD		4096
#define NVT36XXX_MAX_PRESSURE		1000
#define NVT36XXX_MAX_WIDTH		255
#define NVT36XXX_RESET_RETRIES		5
#define NVT36XXX_RESET_STATE_INIT	0xa0

struct nvt36xxx_data {
	struct i2c_client *client;
	struct input_dev *input;
	struct gpio_desc *reset_gpio;
	struct regulator_bulk_data supplies[2];
	struct touchscreen_properties prop;
	u8 event[NVT36XXX_EVENT_DATA_LEN];
	u32 raw_x_max;
	u32 raw_y_max;
	u8 fw_version;
	u16 pid;
};

static int nvt36xxx_write(struct nvt36xxx_data *data, u16 target,
			  const u8 *buf, size_t len)
{
	struct i2c_msg msg = {
		.addr = target,
		.len = len,
		.buf = (u8 *)buf,
	};
	int ret;

	ret = i2c_transfer(data->client->adapter, &msg, 1);
	if (ret == 1)
		return 0;

	return ret < 0 ? ret : -EIO;
}

static int nvt36xxx_read(struct nvt36xxx_data *data, u16 target, u8 command,
			 u8 *buf, size_t len)
{
	u8 reg = command;
	struct i2c_msg msg[2] = {
		{
			.addr = target,
			.len = 1,
			.buf = &reg,
		}, {
			.addr = target,
			.flags = I2C_M_RD,
			.len = len,
			.buf = buf,
		},
	};
	int ret;

	/* Command byte + data MUST be one repeated-start transfer (the
	 * controller aborts the command on a STOP between them - observed
	 * live 2026-09-04). */
	ret = i2c_transfer(data->client->adapter, msg, ARRAY_SIZE(msg));
	if (ret == ARRAY_SIZE(msg))
		return 0;

	return ret < 0 ? ret : -EIO;
}

static int nvt36xxx_select_xdata(struct nvt36xxx_data *data, u32 address)
{
	u8 buf[3] = {
		NVT36XXX_XDATA_CMD,
		(address >> 16) & 0xff,
		(address >> 8) & 0xff,
	};

	return nvt36xxx_write(data, NVT36XXX_FW_ADDR, buf, sizeof(buf));
}

/* vendor nvt_bootloader_reset(): write 00 69 @0x62, 35 ms */
static int nvt36xxx_bootloader_reset(struct nvt36xxx_data *data)
{
	u8 buf[2] = { 0x00, 0x69 };
	int ret;

	ret = nvt36xxx_write(data, NVT36XXX_HW_ADDR, buf, sizeof(buf));
	if (ret)
		return ret;
	msleep(35);

	return 0;
}

/* vendor nvt_sw_reset_idle(): write 00 A5 @0x62, 15 ms */
static int nvt36xxx_sw_reset_idle(struct nvt36xxx_data *data)
{
	u8 buf[2] = { 0x00, 0xa5 };
	int ret;

	ret = nvt36xxx_write(data, NVT36XXX_HW_ADDR, buf, sizeof(buf));
	if (ret)
		return ret;
	msleep(15);

	return 0;
}

/* vendor nvt_ts_check_chip_ver_trim(): full boot sequence, returns 0 when
 * the controller identifies as NT36772 (trim bytes 72 66 03). */
static int nvt36xxx_identify(struct nvt36xxx_data *data)
{
	u8 buf[3];
	u8 trim[NVT36XXX_TRIM_LEN];
	int ret = -ENODEV;
	int attempt;

	for (attempt = 0; attempt < NVT36XXX_RESET_RETRIES; attempt++) {
		ret = nvt36xxx_bootloader_reset(data);
		if (ret)
			continue;
		ret = nvt36xxx_sw_reset_idle(data);
		if (ret)
			continue;

		buf[0] = 0x00;
		buf[1] = 0x35;
		ret = nvt36xxx_write(data, NVT36XXX_HW_ADDR, buf, 2);
		if (ret)
			continue;
		msleep(10);

		ret = nvt36xxx_select_xdata(data, 0x1f600);
		if (ret)
			continue;

		ret = nvt36xxx_read(data, NVT36XXX_FW_ADDR,
				    NVT36XXX_TRIM_CMD, trim, sizeof(trim));
		if (ret)
			continue;

		/* vendor trim table entry 8: id FF FF FF 72 66 03, mask 00 00
		 * 00 01 01 01 (bytes 3..5 compared). */
		if (trim[3] == 0x72 && trim[4] == 0x66 && trim[5] == 0x03)
			return 0;

		ret = -ENODEV;
		usleep_range(10000, 11000);
	}

	return ret;
}

/* vendor nvt_check_fw_reset_state(): poll cmd 0x60 until the reset-complete
 * byte is >= RESET_STATE_INIT (0xa0) and != 0xff. */
static int nvt36xxx_wait_reset(struct nvt36xxx_data *data)
{
	u8 state[NVT36XXX_RESET_STATE_LEN];
	int ret;
	int i;

	for (i = 0; i < 100; i++) {
		ret = nvt36xxx_select_xdata(data, NVT36XXX_EVENT_MAP);
		if (ret)
			return ret;

		ret = nvt36xxx_read(data, NVT36XXX_FW_ADDR,
				    NVT36XXX_RESET_STATE_CMD, state,
				    sizeof(state));
		if (ret)
			return ret;
		if (state[0] >= NVT36XXX_RESET_STATE_INIT && state[0] != 0xff)
			return 0;

		usleep_range(10000, 11000);
	}

	return -ETIMEDOUT;
}

/* vendor nvt_get_fw_info(): abs_x_max at bytes 4..5, abs_y_max at 6..7 of
 * the 16-byte block (fw_ver = byte 0, ~fw_ver = byte 1). */
static int nvt36xxx_read_fw_info(struct nvt36xxx_data *data)
{
	u8 info[NVT36XXX_FW_INFO_LEN];
	int ret;

	ret = nvt36xxx_select_xdata(data, NVT36XXX_EVENT_MAP);
	if (ret)
		return ret;

	ret = nvt36xxx_read(data, NVT36XXX_FW_ADDR,
			    NVT36XXX_FW_INFO_CMD, info, sizeof(info));
	if (ret)
		return ret;

	if ((u8)(info[0] + info[1]) != 0xff)
		return -EIO;

	data->fw_version = info[0];
	data->raw_x_max = get_unaligned_be16(&info[4]);
	data->raw_y_max = get_unaligned_be16(&info[6]);
	if (!data->raw_x_max || !data->raw_y_max ||
	    data->raw_x_max > NVT36XXX_MAX_COORD ||
	    data->raw_y_max > NVT36XXX_MAX_COORD)
		return -EINVAL;

	return 0;
}

/* vendor nvt_read_pid(): pid = (data[1] << 8) | data[0] */
static void nvt36xxx_read_pid(struct nvt36xxx_data *data)
{
	u8 buf[2];
	int ret;

	ret = nvt36xxx_select_xdata(data, NVT36XXX_EVENT_MAP);
	if (ret)
		return;

	ret = nvt36xxx_read(data, NVT36XXX_FW_ADDR,
			    NVT36XXX_PROJECT_ID_CMD, buf, sizeof(buf));
	if (ret)
		return;

	data->pid = (buf[1] << 8) | buf[0];
}

static void nvt36xxx_report_points(struct nvt36xxx_data *data, const u8 *ev)
{
	struct input_dev *input = data->input;
	unsigned int i;
	unsigned int active = 0;

	for (i = 0; i < NVT36XXX_MAX_TOUCHES; i++) {
		const u8 *touch = &ev[i * 6];
		unsigned int slot = touch[0] >> 3;
		unsigned int status = touch[0] & 0x07;
		unsigned int x, y, width, pressure;

		if (!slot || slot > NVT36XXX_MAX_TOUCHES ||
		    (status != 0x01 && status != 0x02))
			continue;

		x = (touch[1] << 4) | (touch[3] >> 4);
		y = (touch[2] << 4) | (touch[3] & 0x0f);
		if (x >= data->raw_x_max || y >= data->raw_y_max)
			continue;

		width = touch[4] ? touch[4] : 1;
		width = min_t(unsigned int, width, NVT36XXX_MAX_WIDTH);
		if (i < 2)
			pressure = touch[5] | (data->event[62 + i] << 8);
		else
			pressure = touch[5];
		pressure = clamp_val(pressure, 1U, NVT36XXX_MAX_PRESSURE);

		input_mt_slot(input, slot - 1);
		input_mt_report_slot_state(input, MT_TOOL_FINGER, true);
		touchscreen_report_pos(input, &data->prop, x, y, true);
		input_report_abs(input, ABS_MT_TOUCH_MAJOR, width);
		input_report_abs(input, ABS_MT_PRESSURE, pressure);
		active++;
	}

	input_mt_sync_frame(input);
	input_report_key(input, BTN_TOUCH, active != 0);
	input_sync(input);
}

static void nvt36xxx_poll(struct input_dev *input)
{
	struct nvt36xxx_data *data = input_get_drvdata(input);
	int ret;

	ret = nvt36xxx_select_xdata(data, NVT36XXX_EVENT_MAP);
	if (ret)
		return;

	ret = nvt36xxx_read(data, NVT36XXX_FW_ADDR, 0x00,
			    data->event, sizeof(data->event));
	if (ret)
		return;

	/* Idle event buffer reads back all-0xff (observed live). Still close
	 * the frame: INPUT_MT_DROP_UNUSED (input_mt_sync_frame) releases every
	 * slot that was active in an earlier frame but is not used in this
	 * one, so the input core emits TRACKING_ID -1 (and BTN_TOUCH 0) and
	 * libinput/compositors see the finger(s) lift. Without this empty
	 * frame the last finger's UP is never delivered upstream — a tap
	 * reads as an endless press/drag and synthesized clicks never
	 * release. [fixed 2026-09-04]
	 */
	if (data->event[0] == 0xff) {
		input_mt_sync_frame(input);
		input_sync(input);
		return;
	}

	nvt36xxx_report_points(data, data->event);
}

static void nvt36xxx_disable_regulators(void *arg)
{
	struct nvt36xxx_data *data = arg;

	regulator_bulk_disable(ARRAY_SIZE(data->supplies), data->supplies);
}

static int nvt36xxx_probe(struct i2c_client *client)
{
	struct device *dev = &client->dev;
	struct nvt36xxx_data *data;
	struct input_dev *input;
	int ret;

	if (client->addr != NVT36XXX_HW_ADDR)
		return -EINVAL;

	data = devm_kzalloc(dev, sizeof(*data), GFP_KERNEL);
	if (!data)
		return -ENOMEM;

	data->client = client;
	i2c_set_clientdata(client, data);

	/* The touch rails (MT6351 VLDO28 2.8 V + 1.8 V I/O) are left on by
	 * LK; the DTS models them as always-on fixed regulators. */
	data->supplies[0].supply = "vcc";
	data->supplies[1].supply = "iovcc";
	ret = devm_regulator_bulk_get(dev, ARRAY_SIZE(data->supplies),
				      data->supplies);
	if (ret)
		return ret;

	ret = regulator_bulk_enable(ARRAY_SIZE(data->supplies), data->supplies);
	if (ret)
		return ret;
	ret = devm_add_action_or_reset(dev, nvt36xxx_disable_regulators, data);
	if (ret)
		return ret;

	/* Reset is I2C-driven (vendor boot sequence), so the GPIO only needs
	 * to be deasserted (run state = physical high; reset-gpios is declared
	 * active-low, so OUT_LOW) if the board wires one at all. */
	data->reset_gpio = devm_gpiod_get_optional(dev, "reset",
						    GPIOD_OUT_LOW);
	if (IS_ERR(data->reset_gpio))
		return PTR_ERR(data->reset_gpio);

	dev_info(dev, "probing NT36772 at 0x%02x\n", client->addr);

	/*
	 * [2026-09-04] Display protection (default ON): the NT36xxx is a
	 * TDDI (touch + display in one IC) whose display side is initialized
	 * ONLY by LK on this board (Linux has no working panel driver). The
	 * vendor boot sequence below issues a full-chip bootloader reset,
	 * which can leave the LK-initialized display un-reinitialized. So
	 * probe CHECK-FIRST: if the chip is already running (fw info readable
	 * at the 0x01 logical target without any reset), attach without
	 * touching the chip. The boot sequence runs only when the board opts
	 * in via the "novatek,force-boot" DT property (display-aware
	 * bring-up); otherwise a not-running chip fails the probe cleanly.
	 */
	ret = nvt36xxx_read_fw_info(data);
	if (!ret) {
		dev_info(dev, "chip already running (fw 0x%02x %ux%u) - "
			 "no reset issued, display untouched\n",
			 data->fw_version, data->raw_x_max, data->raw_y_max);
	} else if (device_property_read_bool(dev, "novatek,force-boot")) {
		dev_info(dev, "chip not running (%d) but force-boot set - "
			 "running the vendor boot sequence\n", ret);
		ret = nvt36xxx_identify(data);
		if (ret)
			return dev_err_probe(dev, ret,
					     "NT36772 trim identity failed\n");
		/* vendor re-enters bootloader reset before FW-info reads */
		ret = nvt36xxx_bootloader_reset(data);
		if (ret)
			return dev_err_probe(dev, ret,
					     "bootloader reset failed\n");
		ret = nvt36xxx_wait_reset(data);
		if (ret)
			return dev_err_probe(dev, ret,
					     "controller did not leave reset\n");
		ret = nvt36xxx_read_fw_info(data);
		if (ret)
			return dev_err_probe(dev, ret,
					     "invalid firmware information\n");
	} else {
		dev_warn(dev, "chip not in normal mode (%d) - skipping the "
			 "bootloader reset to protect the LK-initialized "
			 "display; touch stays off (set novatek,force-boot to "
			 "override)\n", ret);
		return dev_err_probe(dev, -ENODEV,
				     "no touch without TDDI boot\n");
	}

	/* best effort: pid is for the record only */
	nvt36xxx_read_pid(data);

	input = devm_input_allocate_device(dev);
	if (!input)
		return -ENOMEM;

	input->name = "Novatek NT36772 Touchscreen";
	input->id.bustype = BUS_I2C;
	input_set_drvdata(input, data);

	/* raw sensor space (native portrait 1080x2160); the DT
	 * touchscreen-* properties map it to the landscape desktop. */
	input_set_abs_params(input, ABS_MT_POSITION_X, 0,
			     data->raw_x_max - 1, 0, 0);
	input_set_abs_params(input, ABS_MT_POSITION_Y, 0,
			     data->raw_y_max - 1, 0, 0);
	input_set_abs_params(input, ABS_MT_TOUCH_MAJOR, 0,
			     NVT36XXX_MAX_WIDTH, 0, 0);
	input_set_abs_params(input, ABS_MT_PRESSURE, 0,
			     NVT36XXX_MAX_PRESSURE, 0, 0);

	touchscreen_parse_properties(input, true, &data->prop);
	if (!data->prop.max_x)
		data->prop.max_x = data->raw_x_max - 1;
	if (!data->prop.max_y)
		data->prop.max_y = data->raw_y_max - 1;

	ret = input_mt_init_slots(input, NVT36XXX_MAX_TOUCHES,
				  INPUT_MT_DIRECT | INPUT_MT_DROP_UNUSED);
	if (ret)
		return ret;

	data->input = input;

	ret = input_setup_polling(input, nvt36xxx_poll);
	if (ret)
		return dev_err_probe(dev, ret, "polling setup failed\n");
	input_set_poll_interval(input, 15);

	ret = input_register_device(input);
	if (ret)
		return dev_err_probe(dev, ret, "input registration failed\n");

	/* Dual-boot handoff: we leave the chip running (firmware live, events
	 * latched at 0x01). The Android vendor driver reboots it at probe. */
	dev_info(dev, "NT36772 ready: fw 0x%02x pid 0x%04x, %ux%u raw, "
		 "polling 15ms\n",
		 data->fw_version, data->pid, data->raw_x_max, data->raw_y_max);
	return 0;
}

static const struct of_device_id nvt36xxx_of_match[] = {
	{ .compatible = "novatek,nt36772-ts" },
	{ }
};
MODULE_DEVICE_TABLE(of, nvt36xxx_of_match);

static const struct i2c_device_id nvt36xxx_i2c_id[] = {
	{ "nt36772-ts" },
	{ }
};
MODULE_DEVICE_TABLE(i2c, nvt36xxx_i2c_id);

static struct i2c_driver nvt36xxx_driver = {
	.driver = {
		.name = "novatek-nt36xxx",
		.of_match_table = nvt36xxx_of_match,
	},
	.probe = nvt36xxx_probe,
	.id_table = nvt36xxx_i2c_id,
};
module_i2c_driver(nvt36xxx_driver);

MODULE_DESCRIPTION("Novatek NT36772 touchscreen driver (Gemini PDA, polled)");
MODULE_LICENSE("GPL");
