//! `gemcli status` aggregate + the on-glass `selfcheck` parity harness.

use crate::backlight;
use crate::battery;
use crate::boot;
use crate::charger;
use crate::gpu;
use crate::sysfs;
use crate::util;

/// One-shot aggregate of the device state (best-effort — every section
/// is independent; a failed probe never fails the whole command).
pub fn status_cmd() {
    println!("== gemcli status ({}) ==", util::stamp());

    // battery (battstat lines + rc)
    match battery::status_lines() {
        Ok((lines, rc)) => {
            println!("== battery ==");
            for l in lines {
                println!("{l}");
            }
            println!("battstat_rc={rc}");
        }
        Err(e) => println!("== battery ==\nerror: {}", e.msg),
    }

    // raw charger
    match charger::sample() {
        Ok(s) => println!("== charger raw ==\n{}", s.line()),
        Err(e) => println!("== charger raw ==\nerror: {}", e.msg),
    }

    // backlight
    match backlight::get() {
        Ok(pct) => println!("== backlight ==\nbrightness={pct}%"),
        Err(e) => println!("== backlight ==\nerror: {}", e.msg),
    }

    // cpu / a72
    match sysfs::cpu_online() {
        Ok(v) => println!(
            "== cpu ==\nonline: {}",
            v.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",")
        ),
        Err(e) => println!("== cpu ==\nerror: {}", e.msg),
    }
    match crate::a72::status() {
        Ok(s) => println!("== a72 regs ==\n{s}"),
        Err(e) => println!("== a72 regs ==\nerror: {}", e.msg),
    }

    // gpu (read-only status)
    match gpu::status() {
        Ok(s) => println!("== gpu ==\n{s}"),
        Err(e) => println!("== gpu ==\nerror: {}", e.msg),
    }

    // boot target
    match boot::status_text() {
        Ok(s) => println!("== boot target ==\n{s}"),
        Err(e) => println!("== boot target ==\nerror: {}", e.msg),
    }

    // guard state
    match util::read_str("/run/battery-guard/state") {
        Ok(s) => {
            println!("== guard state ==");
            for l in s.lines() {
                if let Some(v) = l.strip_prefix("guard=") {
                    println!("guard={v}");
                }
            }
        }
        Err(_) => println!("== guard state ==\nguard: (daemon not running)"),
    }
}

/// Read-only parity harness — run ON THE DEVICE after installing gemcli
/// and BEFORE flipping any unit away from its script. Every probe is
/// non-destructive. Exit 0 = all checks pass.
pub fn selfcheck() -> i32 {
    let mut ok = true;

    fn check(ok: &mut bool, name: &str, r: Result<(), String>) {
        match r {
            Ok(()) => println!("PASS  {name}"),
            Err(e) => {
                println!("FAIL  {name}: {e}");
                *ok = false;
            }
        }
    }

    // 1. devmem (SPM status regs — readable without powering anything)
    check(
        &mut ok,
        "devmem 0x10006180 (SPM PWR_STATUS)",
        devmem_readable(0x10006180),
    );
    check(
        &mut ok,
        "devmem 0x10007000 (WDT MODE)",
        devmem_readable(0x10007000),
    );

    // 2. charger power supply
    match battery::psy_dir() {
        Some(_) => println!("PASS  bq25890 power supply"),
        None => {
            println!("FAIL  bq25890 power supply (driver not probed)");
            ok = false;
        }
    }
    // 3. raw i2c charger read
    check(&mut ok, "bq25896 raw i2c read (i2c-0 @0x6b)", {
        match charger::sample() {
            Ok(s) => {
                println!("      {}", s.line());
                Ok(())
            }
            Err(e) => Err(e.msg),
        }
    });

    // 4. backlight (sysfs or devmem)
    check(&mut ok, "backlight read", {
        match backlight::get() {
            Ok(pct) => {
                println!("      brightness={pct}%");
                Ok(())
            }
            Err(e) => Err(e.msg),
        }
    });

    // 5. cpu online map
    check(&mut ok, "cpu online map", {
        match sysfs::cpu_online() {
            Ok(v) => {
                println!("      online={}", v.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(","));
                Ok(())
            }
            Err(e) => Err(e.msg),
        }
    });

    // 6. para partition present (no write)
    check(&mut ok, "para partition", {
        match boot::find_para() {
            Some(p) => {
                println!("      {}", p.display());
                Ok(())
            }
            None => Err("para partition not found".into()),
        }
    });

    // 7. gpio chardev: enumerate chips, then read pads 243/244 on the
    //    chip that owns them (read-only input request — the gpioout -g
    //    mode)
    let chips = crate::gpio::list_chips();
    if chips.is_empty() {
        println!("FAIL  gpio chardev: no /dev/gpiochipN found");
        ok = false;
    } else {
        for (path, name, label, ngpio) in &chips {
            println!("      chip {path}: name={name} label={label} ngpio={ngpio}");
        }
        check(&mut ok, "/dev/gpiochip + pads 243/244 (dout via pinctrl)", {
            match crate::gpio::chip_for_line(243) {
                Some(c) => match (crate::speaker::pad_dout(243), crate::speaker::pad_dout(244)) {
                    (Ok(a), Ok(b)) => {
                        // NB read via the pinctrl DOUT register, NOT the gpio
                        // chardev input request: the v1 linehandle API can only
                        // read a level by requesting the pad as input, which
                        // would release the amp's output drive (2026-09-10).
                        println!("      243={a} 244={b} (dout; gpiochip for pads = {c}, not used to read)");
                        Ok(())
                    }
                    (Err(e), _) | (_, Err(e)) => Err(e.msg),
                },
                None => Err("no gpiochip covers pad 243".into()),
            }
        });
    }

    // 8. gpu status regs
    check(&mut ok, "gpu status regs", {
        match gpu::status() {
            Ok(s) => {
                println!("      {}", s.lines().last().unwrap_or(""));
                Ok(())
            }
            Err(e) => Err(e.msg),
        }
    });

    if ok {
        println!("\nselfcheck: ALL PASS — gemcli is script-parity safe for read-only use");
        0
    } else {
        println!("\nselfcheck: FAILURES (fix before flipping any unit to gemcli)");
        1
    }
}

fn devmem_readable(addr: u64) -> Result<(), String> {
    crate::devmem::rd32(addr).map(|_| ()).map_err(|e| e.msg)
}
