#!/usr/bin/env bash
# Cross-compile the drumgen VST3 + CLAP for WINDOWS from Linux (mingw).
#
# Produces a fully self-contained Windows DLL — it imports only stock system
# DLLs (kernel32/user32/gdi32/ole32/opengl32), so there is no mingw runtime to
# ship alongside it. Copy the artifacts to your Windows box and go.
#
# Prerequisites:
#   rustup target add x86_64-pc-windows-gnu
#   openSUSE:      sudo zypper in mingw64-cross-gcc
#   Debian/Ubuntu: sudo apt install gcc-mingw-w64-x86-64
#
# Usage:
#   ./build-windows.sh                  # build -> dist/windows/
#   ./build-windows.sh --install DIR    # also copy the bundle into DIR
#
# Install on Windows by copying:
#   dist/windows/drumgen-vst.vst3  ->  C:\Program Files\Common Files\VST3\
#   dist/windows/drumgen-vst.clap  ->  C:\Program Files\Common Files\CLAP\
# Then rescan plugins in Ableton (Preferences -> Plug-ins -> Rescan).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

TARGET="x86_64-pc-windows-gnu"
DIST="dist/windows"

# ── prereq checks ─────────────────────────────────────────────────────────
if ! rustup target list --installed | grep -qx "$TARGET"; then
    echo "ERROR: rust target '$TARGET' not installed." >&2
    echo "  run: rustup target add $TARGET" >&2
    exit 1
fi

if ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
    echo "ERROR: mingw linker 'x86_64-w64-mingw32-gcc' not found." >&2
    echo "  openSUSE:      sudo zypper in mingw64-cross-gcc" >&2
    echo "  Debian/Ubuntu: sudo apt install gcc-mingw-w64-x86-64" >&2
    exit 1
fi

# ── build ─────────────────────────────────────────────────────────────────
# NOTE: `cargo xtask bundle` always writes to target/bundled/, regardless of
# --target. A Linux build and a Windows build therefore CLOBBER each other's
# .clap there (single file), and the .vst3 accumulates both arch dirs. We copy
# artifacts out to dist/<os>/ immediately so the two targets can coexist.
echo "==> Bundling drumgen-vst for Windows ($TARGET)..."
cargo xtask bundle drumgen-vst --target "$TARGET" --release

VST3_SRC="target/bundled/drumgen-vst.vst3"
CLAP_SRC="target/bundled/drumgen-vst.clap"

if [ ! -d "$VST3_SRC" ] || [ ! -f "$CLAP_SRC" ]; then
    echo "ERROR: build output missing (expected $VST3_SRC and $CLAP_SRC)" >&2
    exit 1
fi

# Sanity: the bundled binary must actually be a Windows PE, not a stale ELF
# left behind by a previous Linux build sharing target/bundled/.
WIN_DLL="$VST3_SRC/Contents/x86_64-win/drumgen-vst.vst3"
if [ ! -f "$WIN_DLL" ]; then
    echo "ERROR: no Windows binary in the VST3 bundle ($WIN_DLL missing)." >&2
    exit 1
fi
if ! file "$WIN_DLL" | grep -q "PE32+"; then
    echo "ERROR: $WIN_DLL is not a Windows PE binary:" >&2
    file "$WIN_DLL" >&2
    exit 1
fi
if ! file "$CLAP_SRC" | grep -q "PE32+"; then
    echo "ERROR: $CLAP_SRC is not a Windows PE binary (stale Linux build?):" >&2
    file "$CLAP_SRC" >&2
    exit 1
fi

# ── stage ─────────────────────────────────────────────────────────────────
rm -rf "$DIST"
mkdir -p "$DIST"

# Ship a Windows-only VST3 bundle: drop any other-arch dirs that the shared
# target/bundled/ may have accumulated from a previous Linux build.
cp -r "$VST3_SRC" "$DIST/"
find "$DIST/drumgen-vst.vst3/Contents" -mindepth 1 -maxdepth 1 -type d \
    ! -name "x86_64-win" -exec rm -rf {} +
cp -f "$CLAP_SRC" "$DIST/drumgen-vst.clap"

echo ""
echo "=== BUILT (Windows x86_64) ==="
echo "  VST3 -> $SCRIPT_DIR/$DIST/drumgen-vst.vst3"
echo "  CLAP -> $SCRIPT_DIR/$DIST/drumgen-vst.clap"

# ── optional install ──────────────────────────────────────────────────────
if [ "${1:-}" = "--install" ]; then
    DEST="${2:-}"
    if [ -z "$DEST" ]; then
        echo "ERROR: --install needs a destination directory" >&2
        exit 1
    fi
    mkdir -p "$DEST"
    rm -rf "$DEST/drumgen-vst.vst3"
    cp -r "$DIST/drumgen-vst.vst3" "$DEST/"
    echo "  installed VST3 -> $DEST/drumgen-vst.vst3"
fi

echo ""
echo "Next: copy the .vst3 bundle to C:\\Program Files\\Common Files\\VST3\\ on"
echo "Windows, rescan plugins in Ableton, add drumgen to a MIDI track, and set"
echo "your drum-sampler track's 'MIDI From' to the drumgen track (Monitor: In)."
