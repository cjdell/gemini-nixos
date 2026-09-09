//! Audio output: one cpal stream on the ALSA backend (cpal 0.15 dropped
//! the Pulse backend; ALSA is the always-on Linux host). It opens the
//! `gemini16` ALSA plug — the S16_LE-pinning PCM defined by
//! services/audio.nix (/etc/asound.conf) in front of the MT6351 AFE — so
//! the engine's f32 output is converted to S16 by the ALSA plug layer,
//! exactly like the WirePlumber path. Falls back to the ALSA "default"
//! PCM if `gemini16` is not present.
//!
//! The demo must keep working when there is no usable audio: on any
//! failure main() drops to silent mode (system-time beat clock).
//!
//! ON-GLASS 2026-09-09: the `default` fallback is a NOISE trap on this
//! codec. cpal 0.15 enumerates PCMs via alsa name hints, and a custom
//! plug defined in asound.conf is NOT in that list unless it carries a
//! `hint { show on }` block — so before the hint was added (2026-09-09,
//! services/pipewire/asound.conf) `gemini16` was never found and the
//! fallback `default` (fromenv→sysdefault→hw:0) opened at F32, which the
//! alsa plug converts to S32_LE on the wire — S32 plays as square-wave/
//! white noise on the 16-bit-only MT6351 (driver never programs the
//! data-width register; live-verified with aplay -v: FLOAT_LE input
//! negotiates S32_LE, S16 input stays S16). The first on-glass symptom
//! was "no music, just a buzz at open + snap at close".

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

use crate::{synth, Clock};

pub fn start(clock: Arc<Clock>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host(); // cpal 0.15: Host directly (ALSA on Linux)

    // Prefer the S16-pinning plug so the AFE path is the verified one.
    let mut devices = host.devices().map_err(|e| e.to_string())?;
    let device = devices
        .find(|d| d.name().map(|n| n.eq_ignore_ascii_case("gemini16")).unwrap_or(false))
        .or_else(|| host.default_output_device())
        .ok_or_else(|| {
            "no output device — is the MT6351 card present (asound: gemini16 / default)?"
                .to_string()
        })?;
    let devname = device.name().unwrap_or_else(|_| "?".into());

    let config = device.default_output_config().map_err(|e| e.to_string())?;

    // ON-GLASS 2026-09-09 (A/B, user-verified): the audible wire state on
    // this board is S16_LE @ 44100 Hz. aplay S16 44.1k = clean sine;
    // S16 @ 48k = white noise (AFE advertises 48k without programming it,
    // same family as the S32-width receipt). cpal's default config for
    // the gemini16 plug came back F32 — prefer an S16 config at 44100
    // (or the device's native rate) when the device offers one.
    let mut rate = config.sample_rate();
    let mut fmt = config.sample_format();
    if fmt != SampleFormat::I16 || rate.0 != 44_100 {
        if let Ok(mut it) = device.supported_output_configs() {
            let mut found = None;
            while let Some(range) = it.next() {
                // prefer S16 stereo at 44.1 k
                let want_lo = range.min_sample_rate().0 <= 44_100
                    && range.max_sample_rate().0 >= 44_100;
                if range.sample_format() == SampleFormat::I16 && want_lo {
                    let good = range.channels() >= 2;
                    if good || found.is_none() {
                        found = Some(range);
                        if good {
                            break;
                        }
                    }
                }
            }
            if let Some(range) = found {
                let r = 44_100;
                fmt = range.sample_format();
                rate = cpal::SampleRate(r);
                let cfg = range.with_sample_rate(rate).config();
                eprintln!(
                    "gemdemo: audio: forced S16 @ {r} Hz config ({} ch)",
                    cfg.channels
                );
            }
        }
    }
    let sr = rate.0;
    if !(30_000..=100_000).contains(&sr) {
        return Err(format!("unsupported device sample rate {sr} Hz"));
    }
    let channels = config.channels().min(2);
    let stream_config = StreamConfig {
        sample_rate: cpal::SampleRate(sr),
        channels,
        buffer_size: cpal::BufferSize::Default,
    };

    eprintln!(
        "gemdemo: audio device '{devname}' — {} Hz, {} ch, {:?}",
        sr, channels, fmt
    );

    let err_cb = |e: cpal::StreamError| eprintln!("gemdemo: audio stream error: {e}");
    // cpal 0.15: build_output_stream(config, data_callback, error_callback)

    // The plug usually reports I16; f32 also works (the ALSA plug layer
    // converts). Handle both. A cb/peak line is printed every ~64
    // callbacks so an on-glass "no music" report can be checked against
    // what the stream actually delivers (2026-09-09: audio ran with no
    // errors but was inaudible — the stream content was the open
    // question).
    let mut cb = 0u64;
    let mut pk: f32 = 0.0;
    // GEMDEMO_DUMP_WAV=/tmp/x.raw — write the first ~6 s of rendered
    // samples (interleaved f32 L R) so an inaudible run can be analyzed
    // off-device (2026-09-09: chime audible but "music silent" — is the
    // buffer music, clicks, or zeros?).
    struct Dump {
        f: std::fs::File,
        cap: usize,  // max interleaved samples to write
        done: usize,
    }
    let dump: Option<std::cell::RefCell<Dump>> =
        std::env::var("GEMDEMO_DUMP_WAV")
            .ok()
            .and_then(|p| std::fs::File::create(p).ok())
            .map(|f| {
                std::cell::RefCell::new(Dump {
                    f,
                    cap: 6usize * sr as usize * 2,
                    done: 0,
                })
            });
    // decide the branch from the format we ENDED on (fmt may have been
    // forced to I16 above even when the device default was F32)
    let use_i16 = fmt == SampleFormat::I16;
    if use_i16 {
        let mut engine = synth::Engine::new(sr as f32);
        let stream = device
            .build_output_stream(
                &stream_config,
                move |data: &mut [i16], _info: &cpal::OutputCallbackInfo| {
                    // data = interleaved frames*channels samples
                    let frames = data.len() / (channels.max(1) as usize);
                    let mut st = vec![0.0f32; frames * 2];
                    engine.render(&mut st, &clock);
                    if let Some(d) = &dump {
                        let mut d = d.borrow_mut();
                        let want = d.cap.saturating_sub(d.done).min(st.len());
                        if want > 0 {
                            use std::io::Write;
                            let mut buf = Vec::with_capacity(want * 4);
                            for v in &st[..want] {
                                buf.extend_from_slice(&v.to_le_bytes());
                            }
                            let _ = d.f.write_all(&buf);
                            d.done += want;
                        }
                    }
                    if channels >= 2 {
                        for i in 0..frames {
                            data[2 * i] = (st[2 * i].clamp(-1.0, 1.0) * 32767.0) as i16;
                            data[2 * i + 1] = (st[2 * i + 1].clamp(-1.0, 1.0) * 32767.0) as i16;
                        }
                    } else {
                        for i in 0..frames {
                            data[i] = (st[2 * i].clamp(-1.0, 1.0) * 32767.0) as i16;
                        }
                    }
                    cb += 1;
                    for v in data.iter() {
                        let a = v.abs() as f32 / 32767.0;
                        if a > pk {
                            pk = a;
                        }
                    }
                    if cb % 64 == 0 {
                        eprintln!("gemdemo: audio cb#{cb} peak {pk:.4}");
                        pk = 0.0;
                    }
                },
                err_cb,
                None,
            )
            .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        return Ok(stream);
    }
    let mut engine = synth::Engine::new(sr as f32);
    let stream = device
        .build_output_stream(
            &stream_config,
            move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                engine.render(data, &clock);
                if let Some(d) = &dump {
                    let mut d = d.borrow_mut();
                    let want = d.cap.saturating_sub(d.done).min(data.len());
                    if want > 0 {
                        use std::io::Write;
                        let mut buf = Vec::with_capacity(want * 4);
                        for v in &data[..want] {
                            buf.extend_from_slice(&v.to_le_bytes());
                        }
                        let _ = d.f.write_all(&buf);
                        d.done += want;
                    }
                }
                cb += 1;
                for v in data.iter() {
                    let a = v.abs();
                    if a > pk {
                        pk = a;
                    }
                }
                if cb % 64 == 0 {
                    eprintln!("gemdemo: audio cb#{cb} peak {pk:.4}");
                    pk = 0.0;
                }
            },
            err_cb,
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}
