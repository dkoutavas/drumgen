#!/usr/bin/env bash
# Build and install the drumgen VST3 + CLAP for native Linux (per-user, no sudo).
#
# Usage:
#   ./build-linux.sh            # build + install to ~/.vst3 and ~/.clap
#   ./build-linux.sh --check    # build only (no install)
#
# Host: Bitwig Studio on native Linux (prefer CLAP; VST3 also works). After
# installing, RESTART the DAW — a loaded .so stays in memory — then add drumgen
# to a MIDI track ahead of your drum sampler in the same device chain.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

echo "==> Bundling drumgen-vst (release)..."
cargo xtask bundle drumgen-vst --release

VST3_SRC="target/bundled/drumgen-vst.vst3"
CLAP_SRC="target/bundled/drumgen-vst.clap"

if [ ! -d "$VST3_SRC" ] || [ ! -f "$CLAP_SRC" ]; then
    echo "ERROR: build output missing (expected $VST3_SRC and $CLAP_SRC)" >&2
    exit 1
fi

if [ "${1:-}" = "--check" ]; then
    echo "==> Check only — skipping install."
    exit 0
fi

VST3_DEST="$HOME/.vst3"
CLAP_DEST="$HOME/.clap"
mkdir -p "$VST3_DEST" "$CLAP_DEST"

rm -rf "$VST3_DEST/drumgen-vst.vst3"
cp -r "$VST3_SRC" "$VST3_DEST/"
cp -f "$CLAP_SRC" "$CLAP_DEST/"

# `cargo xtask bundle` reuses target/bundled/ across targets, so a previous
# ./build-windows.sh run leaves an x86_64-win/ dir inside the VST3 bundle.
# Harmless (hosts pick their own arch) but it drags a 4MB Windows DLL into the
# Linux install — drop any non-Linux arch dirs.
find "$VST3_DEST/drumgen-vst.vst3/Contents" -mindepth 1 -maxdepth 1 -type d \
    ! -name "x86_64-linux" -exec rm -rf {} +

echo ""
echo "=== INSTALLED ==="
echo "  VST3 -> $VST3_DEST/drumgen-vst.vst3"
echo "  CLAP -> $CLAP_DEST/drumgen-vst.clap"
echo ""
echo "Next: RESTART Bitwig (a loaded .so stays in memory), then add drumgen"
echo "ahead of your drum sampler in the same device chain and press play."
