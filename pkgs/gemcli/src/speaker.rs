//! Built-in-speaker / headphone control — the `speaker` CLI of
//! services/scripts/speaker, natively via the kernel gpio chardev for
//! driving (no shell-out to gpioout); pad READS use spkamp's pinctrl
//! registers via /dev/mem (side-effect free — see the note below).
//!
//! The Gemini's flanking speakers are NOT wired to the codec's LOL
//! line-out: they ride the codec HPL/HPR (headphone) drivers through
//! external speaker amps whose enable = SoC pads 243+244 held HIGH
//! (verified audible 2026-09-06; stock-Android OpenSpeakerPath =
//! headphone_output + ext_speaker_output). Level is the ALSA 'Headphone
//! Volume' control (same amps as the jack).
//!
//! Default OFF at boot; with the amps on, the headphone jack plays the
//! speakers too (electrically in parallel) — no jack detection on this
//! mainline stack yet.
//!
//! NOTE: the deployed `audio-output` script still parses the C
//! spkamp tool's pinctrl output ("dout=") for live pad state; until
//! audio-output is migrated to gemcli it keeps using the packaged
//! spkamp helper — `gemcli speaker status` reads the SAME registers
//! (PARIS pinctrl, /dev/mem) and reports the same dout levels.
//!
//! [2026-09-10] status/`amps_on` deliberately read the pinctrl DOUT bit
//! via /dev/mem rather than the gpio chardev: the v1 linehandle API only
//! offers a *input* request to read a level, and requesting a pad as
//! input releases its output drive — i.e. reading the amp state would
//! stop driving the amp. The register read is side-effect free.

use crate::error::Res;
use crate::gpio;
use crate::util;

const PADS: [u32; 2] = [243, 244];

// --- PipeWire sink names (services/pipewire/*.conf) ------------------- //
// gemini_speakers is the L/R-correcting virtual sink
// (60-gemini-speakers.conf). Selecting it as the default output is what
// switches the amp ON; selecting anything else (the hardware sink, whose
// ALSA/ACP name is kept — 50-gemini-alsa-s16.conf only renames its
// description) means headphones/jack only. The `audio-output` script is
// what writes the default sink; this watcher only follows it.
pub const SINK_SPEAKERS: &str = "gemini_speakers";
/// The one PipeWire system session (services/audio.nix).
const AUDIO_RUNTIME_DIR: &str = "/run/gemwl-audio";

/// Resolve the chip that owns the speaker-amp pads: on this kernel the
/// gpiochip indexes follow probe order and are not guaranteed (the
/// bring-up used chip0; the lean/self-built kernel boots two chips) —
/// ask each chip for its line count instead of assuming. [fixed
/// 2026-09-08, on glass]
fn chip() -> Res<String> {
    gpio::chip_for_line(PADS[0]).ok_or_else(|| {
        crate::error::cmsg(format!("no gpiochip covers pad {} (chips: {:?})", PADS[0], gpio::list_chips()))
    })
}

pub fn on() -> Res<()> {
    gpio::drive(&chip()?, &PADS, &[1, 1])
}

pub fn off() -> Res<()> {
    gpio::drive(&chip()?, &PADS, &[0, 0])
}

// --- Side-effect-free pad reads (spkamp's pinctrl map, /dev/mem) -------
// MT6797 PARIS GPIO controller at 0x10005000:
//   dir  = 0x000 + (pad>>5)*0x10, bit pad&31
//   dout = 0x100 + (pad>>5)*0x10, bit pad&31
//   din  = 0x200 + (pad>>5)*0x10, bit pad&31
//   mode = 0x300 + (pad>>3)*4,   bits (pad&7)*4..+3
// (identical to pkgs/speaker-amp/spkamp.c, ported 2026-09-10).
const GPIO_PHYS: u64 = 0x1000_5000;

fn dir_reg(pad: u32) -> u64 {
    GPIO_PHYS + 0x000 + (((pad >> 5) as u64) << 4)
}
fn dout_reg(pad: u32) -> u64 {
    GPIO_PHYS + 0x100 + (((pad >> 5) as u64) << 4)
}
fn din_reg(pad: u32) -> u64 {
    GPIO_PHYS + 0x200 + (((pad >> 5) as u64) << 4)
}
fn mode_reg(pad: u32) -> u64 {
    GPIO_PHYS + 0x300 + (((pad >> 3) as u64) << 2)
}
fn bit(pad: u32) -> u32 {
    pad & 31
}
fn mode_shift(pad: u32) -> u32 {
    (pad & 7) << 2
}

/// The pad's output-register bit (what we last drove) — side-effect free.
pub fn pad_dout(pad: u32) -> Res<u8> {
    let v = crate::devmem::rd32(dout_reg(pad))?;
    Ok(((v >> bit(pad)) & 1) as u8)
}

/// Padding direction bit (dir register bit: 1 = output).
fn pad_is_output(pad: u32) -> Res<bool> {
    Ok((crate::devmem::rd32(dir_reg(pad))? >> bit(pad)) & 1 == 1)
}

/// Live pad state, one spkamp-style line per pad — the same fields the C
/// tool prints ("pad 243 (gpio 755): mode=0 dir=out din=0 dout=1"), so
/// audio-output's `dout=` parse keeps working when it migrates to gemcli.
/// All reads are side-effect free (pinctrl registers via /dev/mem).
pub fn status() -> Res<String> {
    let mut lines = Vec::new();
    for &p in &PADS {
        let mode = (crate::devmem::rd32(mode_reg(p))? >> mode_shift(p)) & 0xf;
        let din = (crate::devmem::rd32(din_reg(p))? >> bit(p)) & 1;
        let dout = (crate::devmem::rd32(dout_reg(p))? >> bit(p)) & 1;
        lines.push(format!(
            "pad {p:3} (gpio {:3}): mode={mode} dir={} din={din} dout={dout}",
            p + 512,
            if pad_is_output(p)? { "out" } else { "in" }
        ));
    }
    Ok(lines.join("\n"))
}

/// True when the pads are currently driving the amps on (pad 243 high).
fn amps_on() -> Option<bool> {
    pad_dout(PADS[0]).ok().map(|v| v == 1)
}

// --- PipeWire default-sink following (GNOME output selector) -------------
//
// The GNU/Linux desktop way to choose output is the Sound menu's device
// list; GNOME Quick Settings exposes the same list. Selecting "Built-in
// speakers" picks the L/R-correcting loopback sink, selecting the
// hardware sink means headphones only. The amp GPIO is not part of that
// selection, so gemini-speakerd follows the default sink and drives the
// pads: speakers sink -> amps ON, hardware sink -> amps OFF.

fn with_audio_env(cmd: &mut std::process::Command) -> &mut std::process::Command {
    cmd.env("PIPEWIRE_RUNTIME_DIR", AUDIO_RUNTIME_DIR)
        .env("XDG_RUNTIME_DIR", AUDIO_RUNTIME_DIR)
}

/// The node.name of the current PipeWire default sink (None when
/// PipeWire/WirePlumber is not running — sleep stops it, and early boot).
pub fn default_sink() -> Option<String> {
    let mut cmd = std::process::Command::new(util::swbin("wpctl"));
    cmd.args(["inspect", "@DEFAULT_SINK@"]);
    let out = with_audio_env(&mut cmd).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // wpctl inspect prints one property per line, e.g.
    //   node.name = "gemini_speakers"
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("node.name = ") {
            let name = rest.trim().trim_matches('"').to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// One reconciliation pass: make the amp pads match the default sink.
/// Returns the sink name observed (None if PipeWire is down).
pub fn sync_amp() -> Option<String> {
    let sink = default_sink()?;
    let want_on = sink == SINK_SPEAKERS;
    if amps_on() != Some(want_on) {
        let r = if want_on { on() } else { off() };
        match r {
            Ok(()) => println!(
                "{} gemcli speaker: default sink {sink} -> amps {}",
                util::hms(),
                if want_on { "ON" } else { "OFF" }
            ),
            Err(e) => println!("gemcli speaker: WARNING amp drive failed: {}", e.msg),
        }
    }
    Some(sink)
}

/// `gemcli speaker watch` — the gemini-speakerd daemon (foreground).
pub fn watch() -> i32 {
    println!("gemcli speaker: watching the default sink (amp ON for {SINK_SPEAKERS})");
    loop {
        sync_amp();
        util::sleep(1.0);
    }
}
