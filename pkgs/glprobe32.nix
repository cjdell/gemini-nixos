# glprobe32 — minimal 32-bit Windows OpenGL present probe (i686-windows PE).
#
# WHY: diagnosing dsd_lm.exe (Doomsday "Lego Mania", Assembly 2003 — a
# 32-bit GL demo) showing a BLACK window under wine-wow64+box64 on the
# PDA while presenting ~69 flushes/s. Question: does wine's opengl32 →
# EGL → wayland present path produce visible pixels at all for a 32-bit
# guest under wow64? The probe does the demo's exact dance (CreateWindow
# + SetPixelFormat + wglMakeCurrent + glClearColor cycling + gdi32
# SwapBuffers) and glReadPixels its own buffer back to BMP files every
# 10th frame — so we can SEE what wine actually presents, independent of
# the demo's scene complexity.
#
# BUILD: i686-windows PE via pkgsCross.mingw32 (mingw-w64's i686 cross —
# unlike winegcc which emits native ELF; see pkgs/d3d9test.nix for the
# same trap). Evaluate from x86_64-linux with allowUnsupportedSystem.
#
# RUN ON THE PDA: 32-bit guest => the wine-wow64 build
# (/root/wine-x86/wine-wow; WINEPREFIX=/root/.wine-wow), NOT wine64.
#   wine-wow glprobe32.exe
# Output BMPs: C:\probe32\frameNN.bmp (= /root/.wine-wow/drive_c/probe32/).
{ stdenv }:

stdenv.mkDerivation {
  pname = "glprobe32";
  version = "1.0";

  src = ./glprobe32;

  dontConfigure = true;
  buildPhase = ''
    # stdenv = i686-w64-mingw32 cross stdenv (callPackage from
    # pkgsCross.mingw32). Links opengl32 (wgl+GL 1.1), gdi32 (SwapBuffers),
    # user32 — all wine builtins at runtime.
    $CC $CFLAGS -O1 -mwindows -o glprobe32.exe glprobe32.c \
      -lopengl32 -lgdi32 -luser32 -static
  '';
  installPhase = ''
    mkdir -p $out/bin
    mv glprobe32.exe $out/bin/glprobe32.exe
    ${stdenv.cc}/bin/i686-w64-mingw32-strip --strip-unneeded $out/bin/glprobe32.exe || true
  '';

  meta = {
    description = "32-bit OpenGL present probe (i686-windows PE) for the wine-wow64 stack on the Gemini PDA";
    platforms = [ "x86_64-linux" ];
    mainProgram = "glprobe32.exe";
  };
}
