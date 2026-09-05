# kernel/ — pinned kernel source snapshot

The flake builds the kernel from the vendored tarball in this
directory (see `devices/planet-geminipda/kernel/default.nix`). The
tarball is gitignored (`*.tar.gz`) because it is 225 MiB.

To build, the file must be present here:

    geminipda-bringup-733c0c7ea.tar.gz
    sha256: 92a33d3750a19f4037e00d817bf81c4948c816d47e1ec2fd87dc4704bb52be1a

It is a `git archive` of the pinned commit (clean tree, no `.git`,
no in-tree build artifacts — the working tree of the bring-up clone
carries ~4 GB of build outputs and must not be fed to the kernel
builder directly):

    repo:   GeminiPDA `repos/linux-6.6` (mainline 6.6 + bring-up patches)
    commit: 733c0c7ea74195bd30734f599f37e69febfd38e0  (`geminipda-bringup`)

Regenerate (needs the bring-up clone locally):

    bash bin/snapshot-kernel.sh [git-dir] [rev]

If the rev changes, rename the tarball reference in
`devices/planet-geminipda/kernel/default.nix` and update the sha256
above.
