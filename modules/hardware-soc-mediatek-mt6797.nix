# Out-of-tree SoC fragment for the Mediatek MT6797 (Helio X25).
#
# Mirrors the in-tree fragments in mobile-nixos `modules/hardware-mediatek.nix`
# (e.g. `mediatek-mt8183`), but lives here so the Gemini port does not need
# an in-tree change. `modules/hardware-soc.nix` only asserts that an option
# exists for the configured SOC name, so this definition is sufficient.
#
# Upstreaming is tracked as phase 6 in docs/mobile-nixos-port-feasibility.md.
{ config, lib, ... }:
{
  options.mobile.hardware.socs.mediatek-mt6797.enable = lib.mkOption {
    type = lib.types.bool;
    default = false;
    description = "enable when SOC is Mediatek MT6797 (Helio X25)";
  };

  config = lib.mkIf config.mobile.hardware.socs.mediatek-mt6797.enable {
    mobile.system.system = "aarch64-linux";
    # mkAfter: the mobile-nixos default structured-config entries are
    # plain assignments from a later-evaluated module; appending keeps
    # this entry in the merged list regardless of module ordering.
    mobile.kernel.structuredConfig = lib.mkAfter [
      (helpers: with helpers; {
        ARCH_MEDIATEK = lib.mkDefault yes;
      })
    ];
  };
}
