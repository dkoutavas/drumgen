#!/usr/bin/env bash
# Build and install the drumgen VST3 + CLAP for native Linux (per-user, no sudo).
#
# Usage:
#   ./build-linux.sh            # build + install to ~/.vst3 and ~/.clap
#   ./build-linux.sh --check    # build only (no install)
#
# Hosts: REAPER (VST3 or CLAP) and Bitwig (CLAP). After installing, rescan
# plugins in your host, add drumgen on a MIDI track, and route its MIDI output
# to the track hosting your drum sampler.

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

echo ""
echo "=== INSTALLED ==="
echo "  VST3 -> $VST3_DEST/drumgen-vst.vst3"
echo "  CLAP -> $CLAP_DEST/drumgen-vst.clap"
echo ""
echo "Next: rescan plugins in REAPER/Bitwig, add drumgen on a MIDI track,"
echo "route its MIDI output to your drum sampler track, and press play."
