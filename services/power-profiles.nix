# Power modes on the Gemini PDA — the standard GNOME "Power Mode" selector
# (+ the Quick Settings power menu) wired to the big A72 cluster.
#
# WHAT THE STANDARD INTERFACE GIVES US
#   GNOME's Power Mode selector talks to power-profiles-daemon (PPD) over
#   org.freedesktop.UPower.PowerProfiles. On a device with no CPU/ACPI
#   platform_profile driver, PPD falls back to its generic *placeholder*
#   driver, which by upstream design advertises only power-saver and
#   balanced — so "Performance" never appears. We patch the placeholder to
#   advertise performance too (patches/
#   power-profiles-daemon-placeholder-performance.patch); that only
#   changes what the daemon advertises, it still drives no hardware.
#
# WHAT "PERFORMANCE" MEANS ON THIS DEVICE
#   The Gemini has no cpufreq/DVFS driver; the one real power lever is the
#   big A72 cluster (cpu8/cpu9), normally powered OFF (the cold-boot
#   state). Bringing it up is the vendor cold sequence in
#   gemcli/src/a72.rs (WDT-guarded, DA9214 BUCKB rail) and gives genuine
#   CPU headroom. gemini-power-profile watches PPD and applies:
#
#     performance            -> `gemcli a72 up both`
#     balanced / power-saver -> `gemcli a72 down both`
#
#   It is deliberately quiet while the silver-button light sleep owns the
#   device (sleep.rs powers the cluster down and restores it), and it waits
#   out a short (20 s) one-time boot settle before its first A72 action (a
#   persisted "performance" profile must not cold-start the cluster while
#   boot is still busy; a profile change during the settle applies at
#   once). A72 operations are flock-serialized so a sleep teardown and a
#   watcher bring-up can never overlap. See pkgs/gemshell/crates/gemdata-device/src/profile.rs.
#
# MANUAL USE
#   gemcli profile status                 # active profile + A72 state
#   gemcli profile set performance        # set PPD + apply now
#   gemcli a72 up|down                    # bypass PPD entirely
#
# The `gemini-sleepd` daemon (gemini-pda.nix) also powers the A72 down on
# sleep and restores it on wake when it was up.
{ config, lib, pkgs, ... }:

let
  gemcli = pkgs.callPackage ../pkgs/gemcli.nix { };

  # PPD with the placeholder driver advertising "performance" — the
  # patched daemon is otherwise stock (same version, same hardening).
  # The patch changes advertised behaviour, so PPD's own integration
  # suite (which asserts the unpatched placeholder: e.g.
  # test_amd_pstate_error expects the performance transition to FAIL on a
  # placeholder-only system) no longer applies → skip checks and configure
  # meson without tests (the test build needs umockdev, which stdenv only
  # pulls in for doCheck). The daemon itself is exercised on glass.
  ppd = pkgs.power-profiles-daemon.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [
      ../patches/power-profiles-daemon-placeholder-performance.patch
    ];
    doCheck = false;
    mesonFlags =
      builtins.filter (f: !(lib.hasPrefix "-Dtests=" f)) old.mesonFlags
      ++ [ "-Dtests=false" ];
  });
in
{
  services.power-profiles-daemon = {
    enable = true;
    package = ppd;
  };

  systemd.services.gemini-power-profile = {
    description = "Gemini PDA power-mode → A72 cluster bridge (gemcli profile watch)";
    # NB do NOT order this after power-profiles-daemon.service: upstream's
    # unit is `After=multi-user.target` + `WantedBy=graphical.target`
    # (because it must run once the boot transaction finishes), so a
    # multi-user unit ordered after it creates an ordering cycle —
    # `Job gemini-power-profile.service/start deleted to break ordering
    # cycle` (seen on glass 2026-09-10q; the unit was then inactive and
    # GNOME showed performance with the A72 still off). `wants` alone
    # pulls PPD in without ordering. The watcher does not need PPD running: it reads
    # the persisted state.ini, its boot settle (20 s) waits PPD out, and
    # powerprofilesctl dbus-activates PPD on demand. Do NOT add
    # `After=power-profiles-daemon.service` back — that recreates the
    # cycle (on glass 2026-09-10q the unit was silently skipped).
    wants = [ "power-profiles-daemon.service" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "simple";
      ExecStart = "${gemcli}/bin/gemcli profile watch";
      Restart = "on-failure";
      RestartSec = "5";
      # Runs as root: the A72 bring-up drives the DA9214 over i2c and the
      # WDT/PMIC registers via /dev/mem, and the speaker/amp path is not
      # involved here. No sandbox, matching gemini-sleepd.
    };
  };
}
