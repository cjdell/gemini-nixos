# Published base + in-repo delta (self-hosting libraries)

**Long-standing goal (recorded 2026-09-07):** make this repo stand on
its own. Everything a build needs must come either from **published
libraries** (nixpkgs, upstream release artifacts) or from **deltas we
track in this repo** — never from the GeminiPDA repo's local state
(local-only git commits, ephemeral `/tmp` trees) and never by copying a
whole library in (67 MiB source tarballs sitting in the repo, snapshot
scripts that must be re-run before a fresh clone can build).

The desired shape for every forked library is **“nixpkgs/upstream
base + delta”**, expressed as reproducible Nix configuration. Mesa is
the first library converted (this document + `pkgs/mesa-geminipda.nix`);
the kernel and other libraries follow the same pattern.

> **GOLDEN-REPO note [2026-09-07]:** gemini-nixos is now the primary
> knowledge repo (AGENTS.md); GeminiPDA is legacy, being folded in. The
> "never from the GeminiPDA repo's local state" goal below is unchanged
> — its blobs/trees are being migrated here (M3/M5), after which the
> wording means "no dependency on the legacy repo at all".

## The pattern, in one line

> Take the published artifact whose version is in your fork's major
> line (prefer *exactly* the fork base; same major is acceptable),
> fetch it by hash, and apply your fork as git-tracked patch(es) on
> top. The only thing in this repo is the delta.

Concretely, for a forked library the recipe is:

1. **Identify the base + delta in the fork source.**
   - The GeminiPDA mesa fork is a git submodule pinned two commits:
     vanilla `mesa-25.0.7` (tag commit `742a20f`) + the fork commit
     `ac19be0d2800f547f78f6acee149d3699d479d6c` — `git describe` =
     `mesa-25.0.7-1-gac19be0`, i.e. the fork is **one commit** on the
     tag.
   - The delta is preserved byte-for-byte as
     `patches/mesa-panfrost-geminipda-25.0.7.patch` (272 lines,
     sha256 `bd8bcb4f…`, 5 files under `src/panfrost` +
     `src/gallium/drivers/panfrost`, +166/−7), identical in both repos.
   - A fresh clone can never fetch `ac19be0` (local-only commit), so
     the patch file *is* the canonical delta.

2. **Pick the published base.**
   - Mesa 25.0.7 was released upstream and also shipped by nixpkgs:
     `nixos-25.05` tip (`ac62194c3917d5f474c1a844b6fd6da2db95077d`)
     carries `pkgs.mesa` version **25.0.7** — the fork's exact base.
     (`nixos-25.11` has 25.2.6, same major 25; `nixos-unstable` and the
     Mobile NixOS npins pin used by this system have 26.x — a
     different major, so not eligible without rebasing the delta.)
   - For the source fetch, use the byte-stable canonical GitLab
     `/-/archive/` tarball of the tag
     (`https://gitlab.freedesktop.org/mesa/mesa/-/archive/mesa-25.0.7/mesa-mesa-25.0.7.tar.gz`,
     sha256 `a0c8a2db…` / SRI `sha256-oMii2/…`), **not** nixpkgs'
     `fetchFromGitLab`: that helper hits the GitLab **API** archive
     endpoint (`/api/v4/…`), which was measured byte-unstable on
     gitlab.freedesktop.org (three fetches → three different tar
     bytes on 2026-09-05) and would break a fixed-output hash. The
     canonical URL was re-verified byte-stable on 2026-09-07 (fresh
     fetch hashed identically).

3. **Express it in Nix.** Two layers, chosen per library:

   **(a) Source de-vendoring (always do this first).** The derivation
   keeps whatever build recipe is right for the device, but `src` is a
   hash-pinned fetch of the published artifact and `patches = [ … ]`
   points at the tracked delta. This is where Mesa is today:
   `pkgs/mesa-geminipda.nix` = published 25.0.7 archive + the fork
   patch, built with the exact verified (surfaceless-EGL/panfrost-only)
   flag set against the system nixpkgs' toolchain. No `mesa/` tarball
   in the repo, no snapshot script, no `git add -Nf` dance for mesa.
   Fresh clones build straight away (the fetch is a normal fixed-output
   download).

   **(b) Expression reuse (preferred when the base major is in the
   consumed nixpkgs).** If/when the fork's base version is what a
   *published nixpkgs revision* ships, build it as that nixpkgs'
   package plus an override, instead of a hand-maintained recipe:

   ```nix
   # base nixpkgs carrying the fork's major (e.g. mesa 25.0.x):
   inputs.mesa-nixpkgs.url = "github:nixos/nixpkgs/nixos-25.05";
   # …or pinned: github:nixos/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d
   #
   # mesaPkgs = import inputs.mesa-nixpkgs {
   #   localSystem  = { system = "x86_64-linux"; };
   #   crossSystem  = { system = "aarch64-linux"; }; # match the device
   # };
   #
   # mesa = mesaPkgs.mesa.override {
   #   galliumDrivers = [ "panfrost" ];
   #   vulkanDrivers  = [ ];
   #   eglPlatforms   = [ ];   # surfaceless-only device build
   # }.overrideAttrs (o: {
   #   pname   = "mesa-geminipda";
   #   patches = o.patches ++ [ ./patches/mesa-panfrost-geminipda-25.0.7.patch ];
   #   src     = fetchurl { /* canonical /-/archive/ bytes, see (a) */ … };
   #   # + device postFixup (ICD manifest /etc copy, --add-rpath)
   # });
   ```

   Why Mesa still uses (a) rather than (b): the system nixpkgs (26.x)
   cannot take the 25.0.7 delta, and importing nixpkgs-25.05 wholesale
   for one package drags in its desktop-shaped recipe — all-driver
   defaults, forced `gallium-rusticl`/LLVM/libclc build inputs (a large
   cross build and closure) — which is exactly what the standalone
   device build was created to avoid. Layer (b) is the template for the
   **kernel**: nixpkgs still carries the 6.6 LTS line, so
   `pkgs.linux_6_6.overrideAttrs { patches = [ …consys/audio/sidekey… ]; }`
   against the system nixpkgs should work *without* any extra input.
   Revisit (b) for mesa if the delta is ever rebased onto a mesa major
   that the consumed nixpkgs ships.

4. **Verification checklist** (each new library):
   - base provenance recorded: upstream tag/commit + published-archive
     URL + hash (the derivation comment);
   - delta provenance recorded: fork commit id + `git describe`
     relation to the tag, sha256 of the patch file;
   - the patch applies cleanly to the fetched base
     (`patch -p1 --dry-run` in the unpacked source);
   - `nix build .#packages.x86_64-linux.<pkg>` succeeds (cross) and the
     store path contents match expectations.

## Mesa instance — facts (2026-09-07)

| | |
|---|---|
| Fork base | upstream Mesa **25.0.7**, tag commit `742a20f` |
| Fork delta | GeminiPDA `mesa/` submodule commit `ac19be0` = `mesa-25.0.7-1-gac19be0` (1 commit on the tag; local-only, unfetchable) |
| Delta file | `patches/mesa-panfrost-geminipda-25.0.7.patch` (272 lines, sha256 `bd8bcb4ff26eaab06d6d5ce70b479dd65e2c34af1a30363b54fd4beac1cf77d0`), byte-identical in gemini-nixos and GeminiPDA |
| Delta content | panfrost: dma-buf IMPORT\|EXPORT caps (`pan_screen.c`), per-batch polygon-list whole-BO memset + dump hooks (`pan_cmdstream.c`), heap pre-map debug (`pan_device.c`), TILERDBG census (`pan_desc.c`), tiler-mask oracle hook (`pan_tiler.c`); env hooks silent by default |
| Published base | canonical `/-/archive/` tarball of `mesa-25.0.7`, sha256 `a0c8a2dbf99bf639bd9d42a0f4a749906b76df8371e11fdbc2858918a3e8cf93` (re-verified byte-stable 2026-09-07) |
| Nixpkgs that shipped the same base | `nixos-25.05` @ `ac62194c3917d5f474c1a844b6fd6da2db95077d` (`pkgs.mesa` = 25.0.7); `nixos-25.11` = 25.2.6 (same major, delta would need re-checking); system pin (npins unstable) = 26.1.4 (different major) |

## Next libraries (not this session)

- **Kernel** (`geminipda-bringup`, 6.6 line + CONSYS Wi-Fi / audio S16 /
  side-key / sramldo patches): convert from the vendored snapshot
  (`kernel/geminipda-bringup-733c0c7ea.tar.gz` +
  `bin/snapshot-kernel.sh`) to nixpkgs' `linux_6_6` base + tracked
  patch series (the GeminiPDA `repos/linux-6.6` fork + `patches/`
  `0001-*.patch` series), same-major base. Verify the in-repo kernel
  pin is re-synced to the #329+ line first (README “Kernel phase”).
- Other libraries: same two-layer decision tree — source de-vendoring
  always; expression reuse when the consumed nixpkgs carries the base
  major.
