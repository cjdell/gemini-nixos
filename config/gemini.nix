# System configuration for the Gemini PDA (stage-2).
#
# Phase 0/1/3 scope: a headless NixOS that boots from the `linux`
# partition and is reachable over the g_ether USB network (10.15.19.82).
# Phase 4 (preview): the GPU desktop is now in-tree too — gemwl (custom
# wlroots 0.18 compositor, pkgs/gemwl.nix + pkgs/wlroots-geminipda.nix)
# is wired as services/desktop.nix and auto-starts at boot (console-
# less fb desktop; serial console unaffected). Nested KWin/Plasma or
# labwc/LXQt sessions are still phase-4 follow-up work (see README R2).
{ config, lib, pkgs, ... }:
let
  # Mesa 25.0.7 + geminipda panfrost fork (see pkgs/mesa-geminipda.nix
  # for why this is a standalone derivation, not nixpkgs' mesa).
  mesaGeminipda = pkgs.callPackage ../pkgs/mesa-geminipda.nix { };
in
{
  imports = [
    # Phase 3 device services: gpu-poweron, a72-up, battery-guard,
    # backlight/power CLIs, boot-recovery, wdt-reboot.
    ../services/gemini-pda.nix
    # Audio: PipeWire/WirePlumber/pipewire-pulse root system session,
    # ALSA S16 pinning, speaker-amp output mode (ported from the
    # verified GeminiPDA bring-up).
    ../services/audio.nix
    # Wi-Fi: internal MT6630 CONSYS stack (mtk_wcn + wlan_gen3 + WMT
    # pwr-on) + USB RTL8821CU dongle auto-connect + factory NVRAM
    # (ported from the verified GeminiPDA bring-up).
    ../services/wifi.nix
    # Phase 4 (preview): gemwl — the wlroots-0.18 compositor that owns
    # the LK framebuffer (/dev/gemfb, GPU-direct), with tinytest smoke
    # clients + the pinned wlroots 0.18.2 (pkgs/gemwl.nix,
    # pkgs/wlroots-geminipda.nix). Auto-starts at boot; disable with
    # `systemctl disable gemwl` for a console-only boot.
    ../services/desktop.nix
  ];

  system.stateVersion = "26.11";

  # ---- Console --------------------------------------------------------
  # The kernel config bakes the console setup
  # (console=tty0 console=ttyS0,921600n1 earlycon fbcon=rotate:3
  #  fbcon=font:TER16x32) via CONFIG_CMDLINE + CMDLINE_FORCE until the
  # docs-R4 A/B switch happens. Keep the NixOS side consistent with it.
  console.font = "TER16x32";

  # Gemini built-in keyboard layout: UK base + the Fn layer as
  # AltGr/Shift combos (outstanding.md item 3). Vendored verbatim from the sibling
  # GeminiPDA project (build/rootfs-files/keyboard/gemini-uk.map, kbd
  # text format, header `keymaps 0-127`; provenance note in
  # config/keymaps/README). NixOS accepts a store path here — the
  # console module writes KEYMAP=<path> to /etc/vconsole.conf and
  # systemd-vconsole-setup loads it with kbd's loadkeys directly (the
  # device-only busybox .bkmap binary from the Debian rootfs is NOT
  # needed; loadkeys --validate passes on the text map).
  console.keyMap = ./keymaps/gemini-uk.map;

  # Same parameters, declared on the NixOS side too. Inert while the
  # kernel enforces CONFIG_CMDLINE; this is the bridge for dropping
  # CMDLINE_FORCE (LK appends the boot.img cmdline to /chosen/bootargs).
  boot.kernelParams = [
    "console=tty0"
    "console=ttyS0,921600n1"
    "earlycon"
    "maxcpus=8"
    "nokaslr"
    "fbcon=rotate:3"
    "fbcon=font:TER16x32"
    "g_ether.dev_addr=42:00:15:19:82:01"
    "g_ether.host_addr=42:00:15:19:82:00"
    "clk_ignore_unused"
    "pd_ignore_unused"
    "regulator_ignore_unused"
    "consoleblank=0"
  ];

  # ---- Users / access ---------------------------------------------------
  # The bring-up interface is ssh over g_ether as root@10.15.19.82, keyed
  # by ~/.ssh/id_ed25519_gemini (bin/device-ssh.sh). NixOS's default sshd
  # (PermitRootLogin prohibit-password) allows root pubkey login, but root
  # must actually carry the key: without it there is NO ssh path into the
  # system (root is locked, the only non-root user has no keys; serial is
  # the only login). pubkey == the host's id_ed25519_gemini.pub, the same
  # key the GeminiPDA Debian rootfs has in /root/.ssh/authorized_keys.
  users.users.root.openssh.authorizedKeys.keys = [
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJ1AU4h4b3z6GFRHVRgXCJ5UMfJU5F8B7A38u7migkuh gemini-pda-root (device-ssh.sh)"
  ];
  # This PDA is the device called `gemini` everywhere else (hostname was
  # NixOS's default `nixos` in the rootfs; the Debian rootfs it replaces
  # uses `gemini`).
  networking.hostName = "gemini";

  users.users.gemini = {
    isNormalUser = true;
    extraGroups = [ "wheel" "video" "networkmanager" ];
  };
  security.sudo.enable = true;
  security.sudo.wheelNeedsPassword = false;
  services.getty.autologinUser = "gemini";

  services.openssh.enable = true;

  # No suspend/resume path exists on this unit (only the LK-configured WDT
  # EXRST path self-boots; a software reset powers the PDA off). The silver
  # key reports KEY_SLEEP (mt6351-keys) and would make logind attempt a
  # suspend the device cannot resume from; ESC/On reports KEY_ESC (never
  # KEY_POWER), but keep all three inert. Mirrors the verified Debian
  # drop-in /etc/systemd/logind.conf.d/99-gemini-sidekeys.conf.
  services.logind.settings.Login = {
    HandleSuspendKey = "ignore";
    HandleHibernateKey = "ignore";
    HandlePowerKey = "ignore";
  };

  # No host firewall on the trusted g_ether link. Beyond that, NixOS's
  # default nftables firewall demands a set of NF_TABLES/NETFILTER_XT_*
  # options as =y, which the (verified) bring-up kernel config ships as
  # =m — the kernel builder's config validator rejects that mismatch.
  networking.firewall.enable = false;

  # ---- Cross-build workaround ------------------------------------------
  # The pinned nixpkgs systemd (261) cross-build for aarch64 fails in the
  # BPF programs: meson invokes the *host* clang with `-target bpf` and no
  # sysroot ("fatal error: 'errno.h' file not found" in
  # src/bpf/restrict-fs.bpf.c). Disable the BPF framework for the whole
  # package set (not just `systemd.package`): packages like dbus-broker
  # depend on `pkgs.systemd` directly and would otherwise pull the
  # BPF-enabled build back into the closure. The systemd-bpf LSM units
  # (restrict-fs, io_uring restrictions) are irrelevant for a trusted
  # single-user PDA.
  # The Wi-Fi firmware blobs (MediaTek WMT/ROMv3/CONSYS + Realtek
  # rtw88) are vendor-furnished and tracked in pkgs/gemini-firmware/
  # (unfreeRedistributable) — the same blobs the verified device runs.
  nixpkgs.config.allowUnfree = true;

  # List-typed option: definitions from all modules (including Mobile
  # NixOS's own overlays) are concatenated in module order, so a plain
  # definition here appends after theirs.
  nixpkgs.overlays = [
    (final: prev: {
      systemd = prev.systemd.override { withLibBPF = false; };
    })
    (final: prev: {
      # nixpkgs' ffmpeg builds (ffmpeg + ffmpeg-headless; the latter is a
      # hard buildInput of pipewire/chromaprint, the former of
      # alsa-plugins) enable cuda-llvm by default, which is broken for
      # cross-aarch64 in this pin ("ERROR: cuda_llvm requested but not
      # found" — no CUDA toolchain exists for the target). This device
      # has no CUDA (Mali-T880, panfrost only), so disable it.
      ffmpeg = prev.ffmpeg_8.override { withCudaLLVM = false; };
      ffmpeg-headless = prev.ffmpeg_8-headless.override { withCudaLLVM = false; };
    })
  ];

  # ---- Networking: g_ether (CDC-ECM) ------------------------------------
  # The kernel auto-instantiates g_ether (CONFIG_USB_GADGET=y,
  # g_ether.dev_addr above); no userspace configfs setup is needed.
  # The interface is usb0, on the fixed 10.15.19.0/24 link with the
  # host side at 10.15.19.1 (GeminiPDA build/net-up.sh).
  networking.interfaces.usb0.ipv4.addresses = [
    { address = "10.15.19.82"; prefixLength = 24; }
  ];
  networking.defaultGateway = "10.15.19.1";
  networking.nameservers = [ "1.1.1.1" ];

  # Static link only — no DHCP client needed.
  networking.dhcpcd.enable = false;

  # ---- Graphics (phase 4 preview) ------------------------------------
  # The forked Mesa provides the glvnd ICD (libEGL_mesa.so + 50_mesa.json);
  # libglvnd provides the client libs (libEGL.so.1 / libGLESv2.so.2) that
  # dispatch to it. hardware.graphics stays off: the stock nixpkgs mesa
  # (26.1.x, all drivers) would be the wrong driver for the Mali-T880
  # and a much larger closure.
  environment.systemPackages = [
    mesaGeminipda
    pkgs.libglvnd
  ];

  # ICD manifest discovery: the compiled-in libglvnd scan list is
  # /run/opengl-driver/share/glvnd/egl_vendor.d (only exists with
  # hardware.graphics), /etc/glvnd/egl_vendor.d, /usr/share/... (no
  # /usr). NixOS does not merge a package's $out/etc into the system
  # /etc, so point environment.etc at the mesa manifest explicitly.
  environment.etc."glvnd/egl_vendor.d/50_mesa.json" = {
    source = "${mesaGeminipda}/etc/glvnd/egl_vendor.d/50_mesa.json";
  };

  # The Mobile NixOS stage-1 is disabled for this device (docs R1: its
  # initrd cannot fit the 16 MiB boot partition). The boot ramdisk is a
  # minimal partition-scanning busybox initrd (devices/planet-geminipda/
  # initrd.nix). Nothing in stage-1 (USB gadget, boot GUI, boot SSH) is
  # available; serial (ttyS0) + fbcon are the bring-up interfaces.
  #
  # Also stop NixOS from building its own initrd into the system closure
  # (we boot from the LK-packed boot.img, never from an NixOS initrd).
  boot.initrd.enable = false;

  # Keep volatile state out of the 27.7 GiB rootfs for now.
  fileSystems = {
    "/tmp" = {
      device = "tmpfs";
      fsType = "tmpfs";
      neededForBoot = true;
    };
  };

  # Skip the documentation HTML build.
  documentation.enable = false;

  # kexec-based stage-0 recovery is not usable on this hardware.
  mobile.quirks.supportsStage-0 = lib.mkForce false;
}
