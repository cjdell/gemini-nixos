//! Library surface for GEMINI: EXODUS (Director's Cut).
//!
//! Only the pure, GL-free modules live here, so they can be unit-tested on
//! a host with a plain `cargo test --lib` — no EGL/ALSA/wayland `-L`
//! paths and no device. The renderer/audio/synth modules stay in the
//! binary (`src/main.rs`), because linking them needs the Gemini PDA's
//! graphics + codec stack.
//!
//! Currently that is the stress/benchmark model (`stress.rs`): the load
//! profiles and the frametime percentile math, which are exactly the
//! parts a benchmark must get right.

pub mod stress;
