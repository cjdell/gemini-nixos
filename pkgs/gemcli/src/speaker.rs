//! Built-in-speaker / headphone control — the `speaker` CLI of
//! services/scripts/speaker, natively via the kernel gpio chardev (no
//! /dev/mem, no shell-out to gpioout).
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
//! spkamp helper — `gemcli speaker status` reads the SAME lines
//! through gpiolib and reports the same levels.

use crate::error::Res;
use crate::gpio;

const PADS: [u32; 2] = [243, 244];

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

/// Live pad levels ("243=1 244=0" — same info gpioout -g prints).
pub fn status() -> Res<String> {
    let vals = gpio::read_levels(&chip()?, &PADS)?;
    let s = PADS
        .iter()
        .zip(vals.iter())
        .map(|(p, v)| format!("{p}={v}"))
        .collect::<Vec<_>>()
        .join(" ");
    Ok(s)
}
