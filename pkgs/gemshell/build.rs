// Link the native libraries the hand-rolled FFI in src/compositor/
// uses. Some of these come from the dependency crates' own
// #[link]/pkg-config machinery, but the crate set is pinned and the
// extern blocks are hand-written — declaring the full set here is
// deterministic and keeps the nix build from drifting on a
// dependency's link emission. Runtime resolution (which mesa/panfrost
// actually serves the GL/EGL calls) goes through the /run/opengl-driver
// libglvnd ICD; these -l flags only satisfy the linker.
fn main() {
    for lib in [
        // wayland-server crate (system feature): the display + resources
        "wayland-server",
        // wayland-client crate (gemsettings): the client protocol
        "wayland-client",
        // hand-declared xkbcommon FFI (src/compositor/input.rs)
        "xkbcommon",
        // hand-declared gbm FFI (src/compositor/gbm.rs)
        "gbm",
        // hand-declared EGL FFI (src/compositor/render.rs)
        "EGL",
        // hand-declared GL ES FFI (src/compositor/render.rs)
        "GLESv2",
        // the `png` crate (icon decoding, src/common/icons.rs)
        "png",
        "z",
    ] {
        println!("cargo:rustc-link-lib={lib}");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
