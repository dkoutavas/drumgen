#!/usr/bin/env bash
# Build drumgen VST3 for Windows from WSL2 and install to system VST3 folder.
#
# Prerequisites:
#   sudo apt install gcc-mingw-w64-x86-64
#   rustup target add x86_64-pc-windows-gnu
#
# Usage:
#   ./build-windows.sh           # Build and install
#   ./build-windows.sh --check   # Build only (no install)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

TARGET="x86_64-pc-windows-gnu"
VST3_DEST="/mnt/c/Program Files/Common Files/VST3/"

echo "==> Building drumgen-vst for Windows ($TARGET)..."
cargo xtask bundle drumgen-vst --target "$TARGET" --release

# Find the VST3 bundle — nih-plug xtask outputs to target/bundled/
VST3_SRC="target/bundled/drumgen-vst.vst3"

if [ ! -d "$VST3_SRC" ]; then
    # Fallback: check release target directory
    VST3_SRC="target/$TARGET/release/bundle/drumgen-vst.vst3"
fi

if [ ! -d "$VST3_SRC" ]; then
    echo "ERROR: Build output not found."
    echo "Expected at: target/bundled/drumgen-vst.vst3"
    echo "Check cargo xtask output above for errors."
    exit 1
fi

echo "==> Build complete: $VST3_SRC"

if [ "${1:-}" = "--check" ]; then
    echo "Check mode — skipping install."
    exit 0
fi

echo "==> Installing to $VST3_DEST ..."
sudo cp -r "$VST3_SRC" "$VST3_DEST"

echo ""
echo "=== SUCCESS ==="
echo "drumgen-vst.vst3 installed to: C:\\Program Files\\Common Files\\VST3\\"
echo ""
echo "Next steps:"
echo "  1. Open Ableton Live"
echo "  2. Preferences -> Plug-ins -> Rescan"
echo "  3. Find 'drumgen' in the plugin browser"
echo "  4. Drop it on a MIDI track"
echo "  5. Create a second MIDI track with Ugritone"
echo "  6. Set Ugritone track input to 'drumgen' (MIDI From)"
echo "  7. Set Monitor to 'In'"
echo "  8. Press Play — you should hear drum patterns!"
