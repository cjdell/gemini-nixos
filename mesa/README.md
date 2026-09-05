# mesa/ — vendored Mesa source tarball

`pkgs/mesa-geminipda.nix` builds Mesa 25.0.7 from the vendored
tarball in this directory. The tarball is gitignored (`*.tar.gz`)
because it is 67 MiB.

To build, the file must be present here:

    mesa-25.0.7.tar.gz
    sha256: a0c8a2dbf99bf639bd9d42a0f4a749906b76df8371e11fdbc2858918a3e8cf93

It is the canonical `/-/archive/` tarball of the upstream tag
`mesa-25.0.7` (NOT the `/api/v4/.../archive.tar.gz` endpoint — that
one is byte-unstable: generated on the fly, different tar/gzip bytes
per fetch, which breaks fixed-output hashes). Its contents +
`patches/mesa-panfrost-geminipda-25.0.7.patch` are byte-identical to
the verified on-glass fork tree (GeminiPDA `mesa/` submodule commit
`ac19be0`).

Regenerate (network only):

    bash bin/snapshot-mesa.sh [version]

If the bytes change, update the sha256 above and in the comment in
`pkgs/mesa-geminipda.nix`.
