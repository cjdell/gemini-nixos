# System configuration for the Gemini PDA (stage-2).
#
# Phase 0/1/3 scope: a headless NixOS that boots from the `linux`
# partition and is reachable over the g_ether USB network (10.15.19.82).
# Phase 4 (preview): the GPU desktop is now in-tree too — gemwl (custom
# wlroots 0.18 compositor, pkgs/gemwl.nix + pkgs/wlroots-geminipda.nix)
# is wired as services/desktop.nix and auto-starts at boot (console-
# less fb desktop; serial console unaffected). Since 2026-09-07 the
# desktop payload was the nested LXQt session (services/lxqt.nix: labwc
# 0.8.3 on the pinned wlroots 0.18.2 + nixpkgs lxqt 2.4, the verified
# GeminiPDA stack); since 2026-09-09 the DEFAULT desktop is Phosh
# (services/phosh.nix: phoc 0.54.0 nested the same way — the module
# defaults flipped so phosh boots by default and LXQt is the
# alternative; docs/phosh.md). `systemctl disable gemwl phosh-nested`
# = console boot.
{ config, lib, pkgs, ... }:
let
  # Mesa 25.0.7 + geminipda panfrost fork (see pkgs/mesa-geminipda.nix
  # for why this is a standalone derivation, not nixpkgs' mesa).
  mesaGeminipda = pkgs.callPackage ../pkgs/mesa-geminipda.nix { };

  # wine/wine64 shell wrappers (thin; the wine-wow64 stack itself is a
  # standalone device GC root — pkgs/wine-cli.nix header).
  wineCli = pkgs.callPackage ../pkgs/wine-cli.nix { };
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
    # Bluetooth: MT6630 CONSYS hci_stp stack as a persistent service —
    # bluetoothd on the system bus (hardware.bluetooth at this pin),
    # hci0 module bring-up, bluez CLIs + blueman GUI (bring-up story:
    # docs/bluetooth-bringup.md).
    ../services/bluetooth.nix
    # Phase 4 (preview): gemwl — the wlroots-0.18 compositor that owns
    # the LK framebuffer (/dev/gemfb, GPU-direct), with tinytest smoke
    # clients + the pinned wlroots 0.18.2 (pkgs/gemwl.nix,
    # pkgs/wlroots-geminipda.nix). Auto-starts at boot; disable with
    # `systemctl disable gemwl` for a console-only boot.
    ../services/desktop.nix
    # LXQt (Wayland) desktop nested inside gemwl — labwc 0.8.3 (pinned
    # against wlroots 0.18.2) hosting the nixpkgs lxqt 2.4 session
    # (panel, pcmanfm-qt desktop, qterminal, audio GUIs). The verified
    # GeminiPDA desktop stack as NixOS services (2026-09-07); the
    # ALTERNATIVE desktop since 2026-09-09 (services.lxqtNested.enable
    # now defaults false — phosh is the default).
    ../services/lxqt.nix
    # Phosh (mobile shell) desktop nested inside gemwl — phoc 0.54.0 on
    # wlroots 0.19 (fork-gbm override, pkgs/phoc-geminipda.nix) hosting
    # the nixpkgs phosh 0.54.0 shell; GPU-accelerated through the same
    # fork-mesa chain as LXQt. The DEFAULT desktop since 2026-09-09
    # (services.phoshDesktop.enable defaults true; LXQt off).
    ../services/phosh.nix
    # DE-agnostic desktop plumbing (2026-09-10): UPower battery/AC
    # status, brightness sysfs access + standard control CLIs
    # (brightnessctl; volume is wpctl via audio.nix) — the services
    # layer so ANY desktop's own controls work (phosh today, LXQt/
    # gemwl later). docs/desktop-plumbing.md.
    ../services/plumbing.nix
    # Power modes (2026-09-10): power-profiles-daemon + the GNOME Power
    # Mode selector, bridged to the A72 cluster — "performance" onlines
    # the A72s, balanced/power-saver powers them down. Patches
    # PPD's placeholder driver to advertise performance (GNOME hides it
    # otherwise). docs/power-modes.md.
    ../services/power-profiles.nix
    # GNOME application suite (2026-09-10): calculator/calendar/maps/
    # clocks/weather/contacts + viewer/etc. as Wayland clients on the
    # system profile, so they show up in the app grid of whichever
    # nested shell is enabled. NOT the GNOME session/shell — that still
    # needs a DRM/KMS device (docs/gnome-feasibility.md).
    ../services/gnome-apps.nix
    # Vanilla GNOME desktop on the KMS device (2026-09-10, default OFF):
    # the standard NixOS GNOME + GDM modules, which need the geminipda-drm
    # kernel driver (/dev/dri/card0). Mutually exclusive with gemwl/phosh/
    # LXQt, which it force-disables. docs/gnome-feasibility.md.
    ../services/gnome.nix
  ];

  system.stateVersion = "26.11";

  # ---- Desktop: vanilla GNOME is the DEFAULT (2026-09-10) ------------
  # The geminipda-drm KMS device (/dev/dri/card0) plus Mesa kmsro make
  # the STANDARD NixOS GNOME session possible; verified on glass
  # 2026-09-10 (kmscube on card0 -> OpenGL ES 3.1, renderer
  # "Mali-T880 (Panfrost)"). services/gnome.nix runs
  # services.desktopManager.gnome + GDM and force-disables the nested
  # gemwl/phosh/LXQt stack, so exactly one desktop owns the panel.
  # Evidence + on-glass receipts: docs/gnome-feasibility.md.
  services.gnomeDesktop.enable = true;

  # ---- Console --------------------------------------------------------
  # The kernel config bakes the console setup
  # (console=tty0 console=ttyS0,921600n1 earlycon fbcon=rotate:3
  #  fbcon=font:TER16x32) via CONFIG_CMDLINE + CMDLINE_FORCE until the
  # docs-R4 A/B switch happens. Keep the NixOS side consistent with it:
  # console.font stays null (the default) so systemd-vconsole-setup does
  # NOT run setfont — TER16x32 is a KERNEL fbcon font name, not a kbd
  # consolefont, so setting it here made setfont exit 66 and the unit
  # fail (phase-2 TODO P2). The kernel font (what the LCD shows) is
  # untouched by vconsole-setup either way. [fixed 2026-09-07]
  # console.font = "TER16x32";

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
  #
  # bootopt/log_buf_len are NOT for the kernel — they are consumed by
  # LK's platform_parse_bootopt (platform/mt6797/load_image.c:839) from
  # the boot.img cmdline FIELD before handoff. Without "bootopt=" the
  # boot hangs on the LK logo (~15 s, LK-WDT loop) before the kernel
  # console ever appears — observed + bisected 2026-09-07 (a boot.img
  # identical except for the field booted fine). Value copied from the
  # verified GeminiPDA image: bootopt=64S3,32N2,64N2 log_buf_len=4M.
  boot.kernelParams = [
    "bootopt=64S3,32N2,64N2"
    "log_buf_len=4M"
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
  # The bring-up/admin interface is ssh over g_ether as root@10.15.19.82,
  # keyed by ~/.ssh/id_ed25519_gemini (bin/device-ssh.sh). NixOS's
  # default sshd (PermitRootLogin prohibit-password) allows root pubkey
  # login, but root must actually carry the key: without it there is NO
  # ssh path into the system (root is locked; serial is the only login).
  # pubkey == the host's id_ed25519_gemini.pub, the same key the
  # GeminiPDA Debian rootfs has in /root/.ssh/authorized_keys.
  users.users.root.openssh.authorizedKeys.keys = [
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJ1AU4h4b3z6GFRHVRgXCJ5UMfJU5F8B7A38u7migkuh gemini-pda-root (device-ssh.sh)"
  ];
  # This PDA is the device called `gemini` everywhere else (hostname was
  # NixOS's default `nixos` in the rootfs; the Debian rootfs it replaces
  # uses `gemini`).
  networking.hostName = "gemini";

  # cjdell = the DEFAULT user of the device (2026-09-09; replaces the
  # original placeholder `gemini` account): the console getty autologins
  # as cjdell, and the DESKTOP sessions (LXQt services/lxqt.nix, Phosh
  # services/phosh.nix) run as cjdell — HOME=/home/cjdell, session
  # configs seeded there by services/scripts/start-lxqt-nested, the
  # audio session (services/audio.nix) under the same user so the
  # desktop can reach its sockets. Passwordless sudo comes from the
  # wheel NOPASSWD rule below — that is what makes "desktop user but
  # full admin" work: the compositor (gemwl) + device services stay
  # root, cjdell's apps sudo for the root-only CLIs (backlight,
  # battery, gemcli, ...).
  users.users.cjdell = {
    isNormalUser = true;
    uid = 1000; # gemini's old uid (the account replaces it; home is new)
    description = "Default Gemini PDA user (desktop + console)";
    # [added 2026-09-10] phosh's lockscreen PAM-authenticates the session
    # user — a locked (passwordless) account can never unlock (phosh
    # gen59 on glass showed the passcode pad against a locked account).
    # Passcode chosen by the user: 0000 (same hash the device has since
    # 2026-09-10; yescrypt). Single-user trusted PDA — the ssh key below
    # and passwordless sudo are the real admin paths.
    hashedPassword = "$y$j9T$rN7mlRnmUJwGrMTOei0xE.$yjjac4vlLlZKFC1MHpXmJYFvy80knuKN7q5a3ZEptE1";
    extraGroups = [
      "wheel" # passwordless sudo (security.sudo.wheelNeedsPassword = false)
      "video" # /dev/dri/card0 (uaccess does not cover the systemd-
      # service desktop — there is no logind session to tag)
      "audio" # /dev/snd* — the PipeWire session runs as cjdell too
      "networkmanager"
    ];
    # Same operator key as root: `ssh cjdell@10.15.19.82` debugs the
    # desktop session as the session user (root stays the admin path).
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJ1AU4h4b3z6GFRHVRgXCJ5UMfJU5F8B7A38u7migkuh gemini-pda-root (device-ssh.sh)"
    ];
  };
  security.sudo.enable = true;
  security.sudo.wheelNeedsPassword = false; # cjdell (wheel) = passwordless sudo
  services.getty.autologinUser = "cjdell";

  # ---- Shells ----------------------------------------------------------
  # Explicit bash (bashInteractive) as every account's login shell (ssh
  # root, getty autologin gemini). NixOS's inherited default already
  # resolves to /run/current-system/sw/bin/bash (bash-interactive in the
  # system profile — verified on glass gen9), but pin the store path so
  # the choice never depends on profile composition. The desktop session
  # (services/lxqt.nix) additionally exports SHELL to the same binary so
  # terminal apps (qterminal) spawn bash rather than /bin/sh (a systemd
  # system service has no SHELL env; qterminal falls back to sh).
  # [2026-09-08]
  users.defaultUserShell = "${pkgs.bashInteractive}/bin/bash";

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

  # ---- Root partition growth -------------------------------------------
  # mnx modules/rootfs.nix sets boot.growPartition = mkDefault true (and
  # fileSystems."/".autoResize = true) for its rootfs. autoResize is
  # what we want: systemd-growfs-root grows the fs to the FULL partition
  # (p32 is already 27.3 GiB; the fs inside the flashed image is small).
  # boot.growPartition is NOT: it runs cloud-utils `growpart` on the root
  # DEVICE to enlarge the *partition*, which is meaningless here (the
  # partition is already full-size and the by-label device path is not
  # something growpart can parse) — its unit failed on every boot
  # ("must supply partition-number"). Disable it; growfs-root does the
  # real fs growth. [fixed 2026-09-07]
  boot.growPartition = false;

  # ---- On-device Nix: self-sufficient build/switch + nix-shell -p ------
  # (2026-09-08) Make the PDA a first-class build/switch target: a repo
  # clone at /root/gemini-nixos can iterate config/programs on the go
  # via `bash /root/gemini-nixos/bin/device-rebuild.sh build|switch` and
  # `nix-shell -p <pkg>`. Since 2026-09-09 the flake also exposes
  # `nixosConfigurations.gemini` (flake.nix), so the stock loop works
  # too: `cd /root/gemini-nixos && nixos-rebuild switch --flake .` —
  # nixos-rebuild is in the closure by default (`system.tools.nixos-
  # rebuild.enable` = `config.nix.enable`, default-true at this pin;
  # the bash nixos-rebuild is gone from nixpkgs — this is the Python
  # nixos-rebuild-ng). It needs nothing beyond what the MNX rootfs
  # postBootCommands set up at first boot (/etc/NIXOS + the system
  # profile). Background facts verified on glass gen28
  # (2026-09-08):
  # - /nix/store is bind-mounted ro in the MAIN mount namespace while
  #   the nix-daemon runs in a PRIVATE mount namespace that sees it rw
  #   (mnt ns + mountinfo verified) — the read-only-store design. The
  #   socket-activated nix-daemon is therefore the only store writer,
  #   and root clients AUTO-connect to it when the socket exists (plain
  #   `nix-store --add` as root succeeded on gen28), so no `store =
  #   daemon` line is needed.
  # - The NixOS `nix` module is enabled (nix.conf is generated) but
  #   experimental-features was EMPTY on gen28 → flake builds failed;
  #   enabled below.
  nix.settings = {
    experimental-features = [ "nix-command" "flakes" ];
    # RAM-bound mobile builds (3.6 GiB total, ~1-2 GiB free with the
    # LXQt desktop up): bound the concurrent compilers. Normal on-device
    # switches are config-glue + cache.nixos.org substitutions (the
    # pinned nixpkgs rev IS the hydra-built channel snapshot — golden
    # rule 9), so they are quick; the custom drvs (mesa fork, kernel,
    # wlroots/labwc/gemwl, firmware, gemcli) only compile when their
    # sources change — long on the A72/A53 mix, prefer the host
    # deploy.sh loop for those.
    max-jobs = 2;
    cores = 2;
    # Trusted single-user root PDA (same trust model as the root LXQt
    # session): sandbox buys nothing here and risks lean-mobile-kernel
    # namespace edge cases; store writes are daemon-mediated either way.
    sandbox = false;
  };
  # Big on-device compiles (kernel/mesa when their sources change) need
  # GBs of build-dir space; /tmp is a 1.9 GiB tmpfs (RAM). Point the
  # nix daemon's build temp at the disk rootfs (/var/tmp — 20 GiB free
  # on gen28). [2026-09-08]
  systemd.services.nix-daemon.environment.TMPDIR = "/var/tmp";
  # `nix-shell -p <pkg>` / `<nixpkgs>` resolution. NixOS's default
  # NIX_PATH points at a `channels/nixos` entry that does not exist on
  # this device (falls through to the unpinned flake registry = master
  # nixpkgs — rule-9 violation + package drift). Point it at the
  # per-user channels dir, populated by `device-rebuild.sh channels`
  # with the SAME rev the flake pins (dc5d91f84032 — cache-healthy by
  # construction, package versions match the running system). nixPath
  # drives the login-shell NIX_PATH; belt+braces: the same list as the
  # nix.conf `nix-path` so non-login contexts (device-ssh.sh, the LXQt
  # system-service session) resolve <nixpkgs> too. [2026-09-08]
  nix.nixPath = [
    "nixpkgs=/nix/var/nix/profiles/per-user/root/channels/nixpkgs"
    "nixos-config=/etc/nixos/configuration.nix"
  ];
  nix.settings."nix-path" = [
    "nixpkgs=/nix/var/nix/profiles/per-user/root/channels/nixpkgs"
    "nixos-config=/etc/nixos/configuration.nix"
  ];

  # ---- Overlays (2026-09-08: pruned to the ones still needed) ----------
  # The cross-build workaround overlays (systemd withLibBPF=false,
  # ffmpeg/ffmpeg-headless withCudaLLVM=false, openblas dynamicArch=false,
  # the libfm/libfm-extra/menu-cache autoreconf AM_GLIB_GNU_GETTEXT fix)
  # were written against the OLD npins rev (26.11pre1031299) and the
  # abandoned cross model [2026-09-07]. After the 2026-09-08 nixpkgs
  # repin (flake.nix -> channel snapshot dc5d91f84032) they were REMOVED:
  # hydra built the un-overridden defaults for aarch64-linux in that
  # channel (their narinfos are 200), so each override only forced
  # non-cached drv hashes down its dependency subtree (systemd -> the
  # whole closure, ffmpeg -> pipewire/alsa-plugins, openblas ->
  # numpy/python3, libfm-* -> pcmanfm-qt) and cost cache misses. If a
  # real build of this rev hits one of the old bugs, re-add the specific
  # override (with a date + receipt). The systemd-bpf LSM units
  # (restrict-fs, io_uring restrictions) are irrelevant for a trusted
  # single-user PDA.
  # The Wi-Fi firmware blobs (MediaTek WMT/ROMv3/CONSYS + Realtek
  # rtw88) are vendor-furnished and tracked in pkgs/gemini-firmware/
  # (unfreeRedistributable) — the same blobs the verified device runs.
  nixpkgs.config.allowUnfree = true;

  # List-typed option: definitions from all modules (including Mobile
  # NixOS's own overlays) are concatenated in module order, so a plain
  # definition here appends after theirs.
  # Overlays run in list order (last wins); the mnx base modules also
  # append overlays here, so mkAfter guarantees OUR entries (the shim in
  # particular) come after theirs and actually take effect.
  nixpkgs.overlays = lib.mkAfter [
    (final: prev: {
      # mobile-nixos builds the rootfs image with the Android
      # make_ext4fs tool, whose ext4 geometry the kernel can only
      # online-grow to exactly 2x the image size (then EINVAL — the fs
      # stops at 819200 blocks/25 groups; observed on glass AND
      # reproduced on the host kernel with the real image). Replace it
      # with an mke2fs shim that produces a normal growable ext4
      # (defaults: flex_bg/64bit/metadata_csum) — every mke2fs geometry
      # tested grows 1.5G -> 27G online cleanly. See
      # pkgs/make-ext4fs-shim.nix (R13).
      make_ext4fs = final.callPackage ../pkgs/make-ext4fs-shim.nix { };
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
  # Nameservers for the g_ether host link (fed to resolvconf as the
  # "static" interface; NetworkManager's DHCP nameservers merge in
  # front when a wifi network is up — wifi.nix, NM mode).

  # ---- Graphics (phase 4 preview) ------------------------------------
  # The forked Mesa provides the glvnd ICD (libEGL_mesa.so + 50_mesa.json);
  # libglvnd provides the client libs (libEGL.so.1 / libGLESv2.so.2) that
  # dispatch to it. hardware.graphics stays off: the stock nixpkgs mesa
  # (26.1.x, all drivers) would be the wrong driver for the Mali-T880
  # and a much larger closure.
  # ---- Browsers (2026-09-08) -------------------------------------------
  # Real Google Chrome (nixpkgs google-chrome 152.0.7977.82, native
  # aarch64 deb — arm64 Linux stable only ships since ~2026-07, and the
  # repo's nixpkgs pin already carries it with aarch64 support; unfree,
  # allowed above) + Firefox 155.0.1. Both go through the SAME client GL
  # path the Debian rootfs verified (Firefox WebGL worked there on the
  # fork mesa): nixpkgs' firefox wrapper ships libglvnd on LD_LIBRARY_PATH
  # (withGlvnd defaults true on Linux), so its dlopen of libEGL.so.1
  # dispatches via the /etc/glvnd/egl_vendor.d/50_mesa.json manifest
  # below -> mesa-geminipda fork -> panfrost renderD128. Two gaps had to
  # close first (both 2026-09-08, on-glass failures): (1) the fork was
  # built with NO wayland EGL platform (-Dplatforms=) so browser GL had
  # no display path at all — pkgs/mesa-geminipda.nix now builds
  # surfaceless,wayland; (2) the fork libgbm needs GBM_BACKENDS_PATH to
  # find dri_gbm.so (browser glxtest GPU probe) — set session-wide in
  # services/lxqt.nix. Firefox must be DESKTOP-launched (session env).
  # Session-side bits (session PATH for bare-name launch, NIXOS_OZONE_WL
  # for chrome's ozone/wayland auto-flags) live in services/lxqt.nix.
  environment.systemPackages = [
    mesaGeminipda
    pkgs.libglvnd
    # --no-sandbox (2026-09-08, comment updated 2026-09-09): chrome was
    # first needed when the desktop ran as ROOT (system service,
    # HOME=/root) — the zygote refuses euid 0 without it. The desktop
    # now runs as the cjdell user, but the sandbox still needs a userns
    # or SUID helper this lean kernel/stack does not provide, so the
    # flag stays until a sandboxed run is verified on glass. Accepted
    # on this trusted single-user PDA (same trust model as the rest of
    # the cjdell desktop session).
    (pkgs.google-chrome.override { commandLineArgs = "--no-sandbox"; })
    pkgs.firefox
  ] ++ [
    # On-device iteration (2026-09-08): git for the device repo clone at
    # /root/gemini-nixos (bin/device-rebuild.sh + bin/device-repo.sh) and
    # micro as a small terminal editor for on-the-go config tweaks
    # (swap for vim/neovim if preferred).
    pkgs.git
    pkgs.micro
    pkgs.ripgrep  # on-the-go add (gen30 demo, 2026-09-08)
  ] ++ [
    # wine/wine64 on the shell PATH (gen36, 2026-09-09): thin wrappers
    # exec'ing the standalone wine-wow64 launcher /root/wine-x86/wine-wow
    # (the stack itself stays a standalone GC root, NOT a system
    # package — docs/wine-d3d.md §5).
    wineCli.wine
    wineCli.wine64
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
