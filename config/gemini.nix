# System configuration for the Gemini PDA (stage-2).
#
# Phase 0/1 scope: a headless NixOS that boots from the `linux` partition
# and is reachable over the g_ether USB network (10.15.19.82). The GPU
# desktop port (gemwl / nested Plasma) comes in phase 4.
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
  ];

  system.stateVersion = "26.11";

  # ---- Console --------------------------------------------------------
  # The kernel config bakes the console setup
  # (console=tty0 console=ttyS0,921600n1 earlycon fbcon=rotate:3
  #  fbcon=font:TER16x32) via CONFIG_CMDLINE + CMDLINE_FORCE until the
  # docs-R4 A/B switch happens. Keep the NixOS side consistent with it.
  console.font = "TER16x32";

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
  users.users.gemini = {
    isNormalUser = true;
    extraGroups = [ "wheel" "video" "networkmanager" ];
  };
  security.sudo.enable = true;
  security.sudo.wheelNeedsPassword = false;
  services.getty.autologinUser = "gemini";

  services.openssh.enable = true;

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
  # List-typed option: definitions from all modules (including Mobile
  # NixOS's own overlays) are concatenated in module order, so a plain
  # definition here appends after theirs.
  nixpkgs.overlays = [
    (final: prev: {
      systemd = prev.systemd.override { withLibBPF = false; };
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
