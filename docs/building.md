# Building — which build system, when

Last updated: 2026-09-17

**There is ONE entry point: `bash bin/build.sh`.** It looks at the platform
and dispatches. Do not hand-roll a `nix build` for this repo's aarch64
outputs — the two platforms need different machinery, and the dispatcher
already knows which:

| Where you are | `bin/build.sh` runs | How the image actually gets built |
|---|---|---|
| **Linux** (the workstation) | `bin/build-linux.sh` | native aarch64 drvs, as root, against the LOCAL store with `--option builders @/etc/nix/machines --fallback`: the Pi (192.168.49.191) compiles, cache.nixos.org substitutes. Unchanged since before the macOS work. |
| **macOS** (any Apple-silicon Mac) | `bin/macos/build.sh` | Apple's `container` runtime runs a persisted aarch64 NixOS VM, and the build happens there — nix on darwin can *evaluate/substitute* aarch64-linux drvs but can never *run* their builders, so the VM is the builder. Receipts + gotchas: `docs/macos-build.md`. |

## The verbs (identical on both platforms)

```sh
bash bin/build.sh start bootimg    # build DETACHED, returns immediately (rule 8)
bash bin/build.sh wait  bootimg    # poll: rc 0 done-ok / 1 failed / 2 still running / 3 no job
bash bin/build.sh log   bootimg    # tail the build log
bash bin/build.sh status           # container/jobs, egress, artifacts, hashes
bash bin/build.sh shell            # this platform's devshell (`nix develop`)
bash bin/build.sh vm               # macOS only: a shell inside the build VM
bash bin/build.sh help
```

Poll, never sleep (rule 8b): `wait` returns immediately while the build
runs — re-run it, do not `sleep`. On Linux `start`/`wait`/`log` are
`bin/run-job.sh` (`logs/jobs/build-<TARGET>/`); on macOS they are the
container job (`~/.cache/gemini-macos/out/<target>.{log,rc}`). Same rc
protocol either way.

## Targets

`TARGET` is a `packages.aarch64-linux.<TARGET>` attribute (short name).

| TARGET | What you get | Where it goes |
|---|---|---|
| `bootimg` (default) | `boot.img` — kernel + minimal initrd. **This is the firmware image.** | p22 `boot` (16 MiB) |
| `kernel` | the kernel package alone (Image.gz + dtbs + modules) | — |
| `rootfs` | the NixOS rootfs image | p27 `linux` |
| `initrd` | the minimal busybox initrd (size checks) | — |
| `default` | `boot.img` + rootfs image + flash script | both of the above |
| `toplevel` | the system generation (deployment, not flashing) | on Linux this routes to `bin/deploy.sh build`, which also pins GC roots (rule 0) |
| `mesa`, `wlroots`, `gemwl`, `gemshell`, `gemcli`, `gemdemo`, `gemini-xkb`, … | single components, for iteration | — |

Cost (measured 2026-09-12/17, 10-core M5): `bootimg` ≈ 17 drvs to
compile + 380 substituted (486 MiB), ~6.5 min. `rootfs`/`toplevel` are
much heavier (≈552 compiled, 2.9 GiB substituted) because they include
mesa, wlroots, the Rust workspace and the whole NixOS glue.

## Where the artifacts land

| Platform | Artifact | Log / state |
|---|---|---|
| Linux | store paths, printed by the build | `logs/jobs/build-<TARGET>/{log,status,pid}` — read with `bash bin/build.sh log <TARGET>` |
| macOS | copied to `~/.cache/gemini-macos/out/`: the image plus `<target>.paths` (store paths), `.sha256`, `.manifest` (target, attribute, host revision, egress, build time, VM/nix versions) — rule 0 | `~/.cache/gemini-macos/out/<target>.log`, `<target>.rc` |

**Nothing here flashes.** The flash cycle is `bin/flash-nixos.sh` /
`bin/boot-switch.sh` (docs/disaster-recovery/, AGENTS.md cheat sheet) and
still assumes the Linux host. Flashing from a Mac is future work — the
building blocks are `adb` (works from macOS; in the darwin devshell) and
a LAN/Wi-Fi ssh path, since macOS has no driver for the device's USB
RNDIS gadget (`0525:a4a2`) — see docs/macos-build.md.

## Building for another Gemini: `local.nix` (added 2026-09-25)

`config/gemini.nix` describes the unit this repo was brought up on: its
operator SSH key, the `cjdell` password, its home Wi-Fi networks, and
(`services/wifi.nix`) its factory Wi-Fi record — the 512-byte
`APCFG/APRDEB/WIFI` file carrying that unit's MAC address and TX
calibration. Another unit overrides these in an optional, untracked
`local.nix` at the repo root, which `flake.nix` appends to the
configuration when it exists (`.gitignore`: `/local.nix`, `/local/`).
Example:

```nix
{ lib, ... }: {
  users.users.root.openssh.authorizedKeys.keys = lib.mkForce [ "ssh-ed25519 …" ];
  networking.networkmanager.ensureProfiles.profiles = lib.mkForce { };
  services.geminiWifi.profilesSeed = null;           # no seeded networks
  services.geminiWifi.factoryNvram = ./local/WIFI;    # this unit's record, or null
}
```

Only a `path:` evaluation sees untracked files: the macOS build
(`bin/macos/build.sh`) does; a `git+file:` one (`nix build .#…` or
`nixos-rebuild --flake .#gemini` in a Git checkout) does not, and builds
exactly what it built before. Without `local.nix` nothing changes.

`local.nix.example` (added 2026-09-26) is a commented template covering
the SSH key, key-only SSH, the `cjdell` password hash, Wi-Fi networks and
record, dropping the usb0 default gateway when no host runs
`bin/usb-tether-nat.sh` (otherwise it outranks the Wi-Fi route: no
internet, no network time), and time zone/locale.

**Warning:** because on-device rebuilds (`bin/device-rebuild.sh`,
`nixos-rebuild switch --flake .` in the device's checkout) are
`git+file:` evaluations, they ignore `local.nix` and put this unit's
settings back: the root SSH key, `cjdell`/`0000` with SSH password
login, its Wi-Fi networks and factory record. Until that is solved, a
unit using `local.nix` should be updated by building on the host and
flashing, not by rebuilding on the device.

## How the separation is organised

| Piece | Scope |
|---|---|
| `bin/build.sh` | **cross-platform dispatcher** — the only thing a caller/agent needs to know |
| `bin/build-linux.sh` | Linux implementation (wraps `nix build` + `bin/run-job.sh`; `toplevel` → `bin/deploy.sh`) |
| `bin/macos/` | **everything macOS-specific**: `build.sh` (host driver), `vm-build.sh` (in-VM builder), `proxy.py` (the VM's egress) |
| `flake-macos.nix` | **darwin-only flake outputs** (the devshell a Mac uses, the CA bundle its VM mounts), merged into `outputs` by `mergeOutputs`. The Linux outputs in `flake.nix` are not touched by it. |
| `docs/building.md` (this file) | the decision map |
| `docs/macos-build.md` | the macOS receipts, gotchas, and what a Mac still cannot do |

The Linux path is the original one, byte-for-byte: after the
reorganisation `packages.aarch64-linux.bootimg.outPath` was still
`/nix/store/gman244d9xaikqw2gzrc2nr90fw103gp-…boot.img`, and a rebuild
reproduced the identical sha256 `b2404b13…` (verified 2026-09-17).

## Flake devshells (nix, per platform)

`nix develop` picks the default devshell for the machine you are on:

- Linux → `devShells.x86_64-linux.default` (python3, adb, usbutils,
  mtkclient — rule 7: project CLIs live only there).
- macOS → `devShells.aarch64-darwin.default` (`flake-macos.nix`:
  python3, rsync, cacert, adb, git, curl). `bash bin/macos/build.sh`
  re-execs into it automatically when the host lacks `python3`/`rsync`.

## Two gotchas worth knowing before you edit

1. **Nix only sees tracked files.** Local path/git flakes evaluate the
   *committed* tree; a new file that is not `git add`ed is invisible
   ("Path 'flake-macos.nix' … is not tracked by Git"). The macOS build is
   immune — it rsyncs the working tree into the VM — but `nix develop`,
   `nix eval` and anything else nix-side on the Mac is not.
2. **`//` is a shallow merge.** The mac outputs are merged with
   `mergeOutputs`, which merges `devShells` and `packages` one level in;
   merging them with a bare `//` replaces whole shared attrsets (the
   first attempt swapped `packages` wholesale and broke
   `packages.aarch64-linux` — caught by the dispatch test on 2026-09-17).
