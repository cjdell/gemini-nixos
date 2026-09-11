# gemcli — the Gemini PDA device-control CLI, native Rust.
#
# 2026-09-12 refactor: the device functions were MOVED out of this
# package into the shared data layer (`pkgs/gemshell/crates/
# gemdata-device/`), so gemshell and gemcli share ONE implementation
# (no duplication). gemcli is now a thin clap frontend over
# `gemdata-device`; its crate lives in the gemshell cargo workspace at
# `pkgs/gemshell/crates/gemcli`.
#
# This derivation therefore builds the workspace member: `src` is the
# gemshell workspace and `buildAndTestSubdir` selects the gemcli crate.
# The nix cargo hook points CARGO_TARGET_DIR at the workspace root, so
# only gemcli's binary lands in $out/bin (see the hook's comment: "ensure
# the output doesn't end up in the subdirectory").
#
# Subcommand map / parity recipe / migration status: docs/gemcli.md.
# Runtime access model (unchanged): /dev/mem mmap, i2c-dev ioctls, the
# gpio chardev v1 linehandle API, sysfs. Only crate deps: clap + libc.
{ lib, rustPlatform }:

rustPlatform.buildRustPackage rec {
  pname = "gemcli";
  version = "0.1.0";

  # The shared workspace (gemdata / gemdata-device / gemcli / gemshell).
  # `target/` is excluded (a local cargo tree is 500 MB+).
  src = lib.cleanSourceWith {
    src = ./gemshell;
    filter = path: _type: baseNameOf (toString path) != "target";
  };
  cargoLock.lockFile = ./gemshell/Cargo.lock;
  # Build (and test) the gemcli member only.
  buildAndTestSubdir = "crates/gemcli";

  # The unit tests live with the implementation in gemdata-device (run
  # via that crate's derivation / the host cargo loop), not the CLI.
  doCheck = false;

  meta = with lib; {
    description = "gemcli — Gemini PDA device control (backlight/battery/A72/WDT/boot/GPU/speaker)";
    longDescription = ''
      Thin clap frontend over the shared gemdata-device implementation:
      backlight, battery/charger, the safety guard, A72 bring-up, WDT
      reboot, boot-target selection, GPU power, speaker amps, sleep,
      power profile and session selection. Runs alongside the bring-up
      shell scripts until the on-glass parity pass flips each unit
      (docs/gemcli.md).
    '';
    homepage = "https://github.com/planet-computers"; # upstream: gemini-nixos repo (local)
    license = licenses.mit;
    platforms = [ "aarch64-linux" ];
    mainProgram = "gemcli";
  };
}
