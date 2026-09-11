//! Raw BQ25896 charger reads — port of services/scripts/bq25896-raw.sh.
//!
//! Why raw: the mainline bq25890 driver only re-triggers the chip's ADC
//! conversion when !online/hiz (bq25890_update_state), so while charging
//! sysfs current_now stays 0 even though the chip is regulating. This
//! triggers a fresh conversion (REG02 bit 7, auto-clears on completion)
//! and reads the results directly over i2c-0 @ 0x6b with the `-f`
//! (I2C_SLAVE_FORCE) flag — the kernel driver claims 0x6b.
//!
//! Register map (driver bq25890_charger.c, BQ25896 datasheet):
//!   REG00 bits 5-0  IINLIM  = 100 + N*50 mA
//!   REG0B bits 7-5  VBUS_STAT, bits 4-3 CHG_STAT (0=none 1=pre 2=fast 3=done)
//!   REG0E bits 6-0  BATV    = 2304 + N*20 mV
//!   REG0F bits 6-0  SYSV    = 2304 + N*20 mV
//!   REG11 bits 6-0  VBUSV   = 2600 + N*100 mV
//!   REG12 bits 6-0  ICHGR   = N*50 mA   (measured charge current)
//!   REG13 bit 7     VDPM_STAT, bit 6 IDPM_STAT
//!
//! Line format (the `power` script parses these tokens):
//!   "HH:MM:SS chg=fast vbus_stat=1 vbat=3904mV vsys=3904mV ichgr=150mA
//!    vbus=5000mV iinlim=500mA vdpm=0 idpm=0"

use crate::error::{cmsg, Res};
use crate::i2c;
use crate::util;

const BUS: i32 = 0;
const ADDR: u8 = 0x6b;
const FORCE: bool = true;

pub struct Raw {
    pub hms: String,
    pub chg: &'static str,
    pub vbus_stat: u8,
    pub vbat_mv: u32,
    pub vsys_mv: u32,
    pub ichgr_ma: u32,
    pub vbus_mv: u32,
    pub iinlim_ma: u32,
    pub vdpm: u8,
    pub idpm: u8,
}

impl Raw {
    /// The bq25896-raw.sh single-sample line.
    pub fn line(&self) -> String {
        format!(
            "{} chg={} vbus_stat={} vbat={}mV vsys={}mV ichgr={}mA vbus={}mV iinlim={}mA vdpm={} idpm={}",
            self.hms,
            self.chg,
            self.vbus_stat,
            self.vbat_mv,
            self.vsys_mv,
            self.ichgr_ma,
            self.vbus_mv,
            self.iinlim_ma,
            self.vdpm,
            self.idpm
        )
    }
}

fn conv_and_read() -> Res<Raw> {
    let r2 = i2c::rd_reg(BUS, ADDR, 0x02, FORCE)?;
    i2c::wr_reg(BUS, ADDR, 0x02, r2 | 0x80, FORCE)?;
    // poll REG02 bit7 until the conversion auto-clears it (40 x 0.05 s)
    let mut done = false;
    for _ in 0..40 {
        util::sleep(0.05);
        if let Ok(v) = i2c::rd_reg(BUS, ADDR, 0x02, FORCE) {
            if v & 0x80 == 0 {
                done = true;
                break;
            }
        }
    }
    if !done {
        return Err(cmsg("ADC conversion did not complete"));
    }
    let r0 = i2c::rd_reg(BUS, ADDR, 0x00, FORCE)?;
    let rb = i2c::rd_reg(BUS, ADDR, 0x0B, FORCE)?;
    let re = i2c::rd_reg(BUS, ADDR, 0x0E, FORCE)?;
    let rf = i2c::rd_reg(BUS, ADDR, 0x0F, FORCE)?;
    let r11 = i2c::rd_reg(BUS, ADDR, 0x11, FORCE)?;
    let r12 = i2c::rd_reg(BUS, ADDR, 0x12, FORCE)?;
    let r13 = i2c::rd_reg(BUS, ADDR, 0x13, FORCE)?;
    let chg = match (rb >> 3) & 3 {
        0 => "none",
        1 => "pre",
        2 => "fast",
        _ => "done",
    };
    Ok(Raw {
        hms: util::hms(),
        chg,
        vbus_stat: (rb >> 5) & 7,
        vbat_mv: 2304 + (re as u32 & 0x7f) * 20,
        vsys_mv: 2304 + (rf as u32 & 0x7f) * 20,
        ichgr_ma: (r12 as u32 & 0x7f) * 50,
        vbus_mv: 2600 + (r11 as u32 & 0x7f) * 100,
        iinlim_ma: 100 + (r0 as u32 & 0x3f) * 50,
        vdpm: (r13 >> 7) & 1,
        idpm: (r13 >> 6) & 1,
    })
}

/// One fresh sample.
pub fn sample() -> Res<Raw> {
    conv_and_read()
}

/// `charger raw [samples] [interval_s]` (default 1 sample).
pub fn raw_cmd(samples: usize, interval: f64) -> Res<()> {
    let mut rc = 0;
    for i in 0..samples {
        match sample() {
            Ok(s) => println!("{}", s.line()),
            Err(e) => {
                eprintln!("gemcli charger: {}", e.msg);
                rc = 1;
                break;
            }
        }
        if i + 1 < samples {
            util::sleep(interval);
        }
    }
    if rc != 0 {
        Err(cmsg("i2c read failed"))
    } else {
        Ok(())
    }
}

/// Extract the signed integer of a `key=NNNmA` token from a raw line
/// (the `power` script's raw_field). Returns None if absent/unparseable.
pub fn ichgr_from_line(line: &str) -> Option<i64> {
    for tok in line.split_whitespace() {
        if let Some(v) = tok.strip_prefix("ichgr=") {
            let v = v.strip_suffix("mA").unwrap_or(v);
            return v.parse::<i64>().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn ichgr_parse() {
        let l = "12:00:00 chg=fast vbus_stat=1 vbat=3904mV vsys=3904mV ichgr=150mA vbus=5000mV iinlim=500mA vdpm=0 idpm=0";
        assert_eq!(super::ichgr_from_line(l), Some(150));
        assert_eq!(super::ichgr_from_line("nothing here"), None);
    }
}
