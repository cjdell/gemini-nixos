# Gemini PDA audio (rootfs side).
#
# Ports the verified audio stack from the GeminiPDA project
# (build/rootfs-files/pipewire/) into the NixOS system:
#
#   PipeWire 1.6.x + WirePlumber + pipewire-pulse run as ONE system
#   session on XDG_RUNTIME_DIR=/run/gemwl-audio — the same layout as the
#   proven Debian units. [changed 2026-09-09] The session user is
#   cjdell (the device's default desktop user, config/gemini.nix
#   users.users.cjdell), NOT root: the desktop session (GNOME via GDM,
#   or the gemshell system service) runs as cjdell and must reach the
#   sound server's sockets
#   (/run/gemwl-audio, owned by cjdell via systemd RuntimeDirectory +
#   User= chown). /dev/snd* access comes from cjdell's `audio` group.
#   The only loss vs root: no realtime scheduling (no rtkit here —
#   PipeWire logs the RT refusal and runs SCHED_OTHER, as on any
#   rtkit-less desktop). gemini-audio-defaults (amixer + the speaker-
#   amp gpio pads, devmem) stays a ROOT service. /run/gemwl (the gemwl
#   compositor's runtime dir) is deliberately NOT used: systemd removes
#   it when the compositor restarts, which once vanished the audio
#   sockets mid-session (observed 2026-09-07).
#
#   The MT6351 analog path is S16-only in practice (the mt6797
#   AFE/ADDA driver never programs a data-width register — S32 plays as
#   white noise, S24 is refused; verified on hardware 2026-09-07).
#   /etc/asound.conf defines a plug PCM `gemini16` pinning the slave to
#   S16_LE, and the WirePlumber rule (50-gemini-alsa-s16.conf) opens the
#   ALSA card through it — alsa-lib then converts any incoming format
#   down to true S16 before it reaches the codec.
#
#   gemini-audio-defaults applies the DL1->ADDA->HPL/HPR route + the
#   persisted speaker/headphone output mode at boot (after alsa-restore,
#   which owns 'Headphone Volume' from /var/lib/alsa/asound.state).
#   The `speaker`/`audio-output` CLIs (gemini-pda-utils) drive the
#   speaker-amp pads (243/244); see the scripts' headers for why manual
#   output selection exists (no jack detection).
#
# Kernel prerequisite: SND_SOC_MT6797 (=y in the bring-up config — the
# MT6351 codec card 'mt6797mt6351' is card 0).
{ config, lib, pkgs, ... }:

let
  utils = pkgs.callPackage ./gemini-utils.nix { };

  # gemcli — used for the speaker-amp follow daemon (speaker watch).
  gemcli = pkgs.callPackage ../pkgs/gemcli.nix { };

  # Shared unit env: one session, its own runtime dir, the desktop
  # user's home (User=cjdell below — systemd also sets HOME from the
  # account; keep it explicit so wireplumber's state lands under
  # /home/cjdell, never /root).
  sessionEnv = [
    "XDG_RUNTIME_DIR=/run/gemwl-audio"
    "HOME=/home/cjdell"
  ];
in
{
  # alsa-utils + alsa-restore.service (restores 'Headphone Volume' from
  # /var/lib/alsa/asound.state; gemini-audio-defaults runs after it).
  hardware.alsa.enable = true;

  # /etc/gemini holds the persisted manual output mode
  # (/etc/gemini/audio-output-mode). NixOS does not create plain /etc
  # subdirectories; without this rule audio-output's state write silently
  # fails on a fresh rootfs (found 2026-09-11: the file did not exist, so
  # every boot reverted to the built-in "speaker" default and a GNOME
  # headphone choice never survived a reboot). [added 2026-09-11]
  systemd.tmpfiles.rules = [ "d /etc/gemini 0755 root root -" ];

  environment.systemPackages = [
    pkgs.pipewire
    pkgs.wireplumber
    pkgs.alsa-utils
    utils # speaker, audio-output CLIs (+ gpioout/spkamp helpers)
  ];

  # /etc/asound.conf — the S16-pinning plug PCM (see header). hardware.alsa
  # generates this file from the config string (it owns the etc entry;
  # an environment.etc definition would conflict with it).
  hardware.alsa.config = builtins.readFile ./pipewire/asound.conf;

  # WirePlumber rule opening the card through pcm.gemini16.
  environment.etc."wireplumber/wireplumber.conf.d/50-gemini-alsa-s16.conf"
    .source = ./pipewire/50-gemini-alsa-s16.conf;

  # The L/R-correcting virtual "Built-in Speakers" sink (filter-chain
  # module — see the file header for the wiring and the amp coupling).
  environment.etc."pipewire/pipewire.conf.d/60-gemini-speakers.conf"
    .source = ./pipewire/60-gemini-speakers.conf;

  systemd.services.pipewire = {
    description = "PipeWire Multimedia Service (Gemini system session)";
    # No socket activation: no per-user logind sessions on this device,
    # and always-on audio matches how the rest of the stack boots.
    after = [ "systemd-udevd.service" ];
    before = [ "pipewire-pulse.service" "wireplumber.service" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "simple";
      # The desktop session user (config/gemini.nix users.users.cjdell):
      # systemd chowns RuntimeDirectory /run/gemwl-audio to the unit's
      # User=, so the sockets land cjdell-owned and the cjdell desktop
      # connects without privilege. [2026-09-09]
      User = "cjdell";
      RuntimeDirectory = "gemwl-audio";
      Environment = sessionEnv;
      ExecStart = "${pkgs.pipewire}/bin/pipewire";
      Restart = "on-failure";
      RestartSec = "2";
    };
  };

  systemd.services.wireplumber = {
    description = "WirePlumber Session Manager (Gemini system session)";
    # Owns the ALSA card (the MT6351 codec card 'mt6797mt6351'), creates
    # the sink, selects defaults. Runs fine without a D-Bus session bus
    # (headless); its dbus-dependent bits stay dormant.
    after = [ "pipewire.service" ];
    requires = [ "pipewire.service" ];
    before = [ "pipewire-pulse.service" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "simple";
      User = "cjdell"; # desktop session user (see pipewire.service)
      RuntimeDirectory = "gemwl-audio";
      Environment = sessionEnv;
      ExecStart = "${pkgs.wireplumber}/bin/wireplumber";
      Restart = "on-failure";
      RestartSec = "2";
    };
  };

  systemd.services."pipewire-pulse" = {
    description = "PipeWire PulseAudio compatibility (Gemini system session)";
    after = [ "pipewire.service" "wireplumber.service" ];
    requires = [ "pipewire.service" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "simple";
      User = "cjdell"; # desktop session user (see pipewire.service)
      RuntimeDirectory = "gemwl-audio";
      Environment = sessionEnv;
      ExecStart = "${pkgs.pipewire}/bin/pipewire-pulse";
      Restart = "on-failure";
      RestartSec = "2";
    };
  };

  # gemini-speakerd — follows the PipeWire default sink and drives the
  # speaker-amp pads (243/244): gemini_speakers -> amps ON, any other sink
  # (the hardware Headphones/Jack sink, or a USB output) -> OFF. This is
  # what makes selecting "Built-in Speakers" vs "Headphones" in GNOME's
  # Sound menu also switch the hardware amp. Root because the pads are the
  # kernel gpio chardev; it reaches the cjdell-owned PipeWire session
  # through PIPEWIRE_RUNTIME_DIR=/run/gemwl-audio (set inside gemcli —
  # pkgs/gemshell/crates/gemdata-device/src/speaker.rs). It leaves the amp alone when PipeWire is
  # down (early boot, sleep), so the boot-time amp state stays whatever
  # gemini-audio-defaults applied. Ordered AFTER gemini-audio-defaults so
  # the boot-time default-sink write is settled before the watcher reads
  # it (otherwise a transient hardware-sink default could be persisted as
  # the mode). [added 2026-09-10; ordering + mode persistence 2026-09-11]
  systemd.services.gemini-speakerd = {
    description = "Gemini PDA speaker-amp follows the PipeWire default sink";
    after = [ "pipewire.service" "wireplumber.service" "gemini-audio-defaults.service" ];
    wants = [ "pipewire.service" "gemini-audio-defaults.service" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "simple";
      ExecStart = "${gemcli}/bin/gemcli speaker watch";
      Restart = "on-failure";
      RestartSec = "5";
    };
  };

  systemd.services.gemini-audio-defaults = {
    description = "Gemini audio playback route + output mode defaults";
    # After alsa-restore (which owns 'Headphone Volume' from
    # /var/lib/alsa/asound.state) and before any desktop/audio client.
    # Also after PipeWire/WirePlumber so the persisted output mode can be
    # written as the PipeWire default sink (audio-output sync-default) —
    # the script retries until the metadata object exists.
    after = [ "alsa-restore.service" "sound.target" "pipewire.service" "wireplumber.service" ];
    wants = [ "alsa-restore.service" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = "yes";
      ExecStart = "${utils}/bin/audio-defaults.sh";
    };
    # R12: systemd 261 dropped the `Path=` unit key; use the module
    # option (packages list -> Environment PATH).
    path = [
      pkgs.bash
      pkgs.alsa-utils # amixer
      pkgs.pipewire # pw-metadata (default-sink sync)
      pkgs.util-linux # logger
      pkgs.coreutils
      utils # audio-output (-> speaker -> gpioout)
    ];
  };
}
