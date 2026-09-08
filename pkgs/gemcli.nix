# gemcli — the Gemini PDA device-control CLI, native Rust.
#
# Source (pkgs/gemcli/): a clap(derive)-based binary with one subcommand
# per device function — semantic ports of the verified bring-up shell
# scripts in services/scripts/ (see docs/gemcli.md for the map and the
# migration/test recipe):
#   backlight  <- backlight                  battery <- battstat
#   charger    <- bq25896-raw.sh             power   <- power
#   guard      <- battery-guard.sh (daemon)  a72     <- cl2-up.sh + cl2-down.sh
#   gpu        <- gemini-gpu-poweron.sh      wdt-reboot <- gemini-wdt-reboot
#   boot       <- gemini-boot-recovery/-debian
#   speaker    <- speaker (gpio chardev, no C helper)
# plus `status` (aggregate), `selfcheck` (read-only on-glass parity
# harness) and `version` (rule-0 identity banner).
#
# Runtime access model (no new kernel features): /dev/mem mmap for the
# SPM/DISP_PWM0/WDT registers (busybox-devmem equivalent), i2c-dev
# ioctls for the BQ25896/DA9214/RT5735 buses (i2c-tools equivalent,
# including the adapter-by-DT-base resolution and `-f` force semantics),
# the gpio chardev v1 linehandle API for the speaker-amp pads, and
# sysfs for the power supplies + cpu hotplug. Only two crate deps:
# clap + libc (both hydra-cached; the lockfile pins them — see
# Cargo.lock). Time is UTC (the device runs UTC); no chrono.
#
# Build = native aarch64 (this flake's canonical model): rustPlatform
# from eval.pkgs builds with the aarch64 rustc on the remote builder.
# cargo test runs in the sandbox (doCheck default; 6 unit tests, pure
# logic only — no /dev/mem needed).
#
# Migration status (2026-09-08): gemcli is added to the rootfs closure
# (services/gemini-pda.nix systemPackages) NEXT TO the scripts; NO
# systemd unit has been flipped. Flip order + the on-glass parity pass
# are in docs/gemcli.md.
{ lib, rustPlatform }:

rustPlatform.buildRustPackage rec {
  pname = "gemcli";
  version = "0.1.0";

  src = ./gemcli;

  # Deps pinned by the committed lockfile (clap 4.5 + libc 0.2 + the
  # clap_derive proc-macro set). Regenerate with `cargo generate-lockfile`
  # inside pkgs/gemcli and commit the result.
  cargoLock.lockFile = ./gemcli/Cargo.lock;

  meta = with lib; {
    description = "gemcli — Gemini PDA device control (backlight/battery/A72/WDT/boot/GPU/speaker)";
    longDescription = ''
      Native-Rust manager for the Gemini PDA device functions the bring-up
      shell scripts handle. Semantic port of services/scripts/* with
      script-compatible exit codes; ships alongside the scripts until the
      on-glass parity pass (docs/gemcli.md) — then the systemd units'
      ExecStart flip from script to gemcli one at a time.
    '';
    homepage = "https://github.com/planet-computers"; # upstream: gemini-nixos repo (local)
    license = licenses.mit;
    platforms = [ "aarch64-linux" ];
    mainProgram = "gemcli";
  };
}
