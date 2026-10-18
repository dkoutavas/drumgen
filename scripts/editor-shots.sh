#!/usr/bin/env bash
# Screenshot the plugin's editor without a DAW: build a standalone copy of the
# plugin, run it on a virtual X display, click through STEPS, grab PNGs.
#
# The plugin-editor counterpart of gui-smoke.py. It is how the FORM controls
# were checked before Bitwig, and it found two glyphs the pixel font lacks
# (the DICE die and the telegraph arrow drew as boxes for months).
#
# Nothing in the repo changes. The script works on a throwaway copy of plugin/
# that enables nih-plug's `standalone` feature and patches baseview: Xvfb's
# software GL (llvmpipe) offers no sRGB framebuffer and baseview insists on
# one, so the copy asks for none. Colours may differ slightly from a real GPU.
#
#   sudo apt install xvfb libgl1-mesa-dri xdotool ffmpeg
#   scripts/editor-shots.sh OUT_DIR
#
# Steps are mouse actions at window coordinates (the editor opens at 0,0 at
# 720x440): "x,y" clicks, "h:x,y" only moves the pointer there (to catch a
# hover text). They follow the current layout; when the layout moves, update
# them from a fresh 00-start.png.
set -euo pipefail

OUT=${1:?usage: editor-shots.sh OUT_DIR}
REPO=$(cd "$(dirname "$0")/.." && pwd)
WORK=${DRUMGEN_SHOTS_WORK:-${TMPDIR:-/tmp}/drumgen-editor-shots}
DISPLAY_NUM=:97

# name | actions ("x,y" click, "h:x,y" hover, space separated) | seconds before the grab
STEPS=(
  "01-help-overlay|132,12|1"
  "02-hover-humanize|132,12 h:20,85|1.5"
  "03-form-maze|276,157 276,157 276,157 276,157 276,157 h:220,157|1.5"
  "04-hover-bars-under-form|h:172,73|1.5"
  "05-form-rolled|327,157|1.5"
  "06-pad-playing|28,193 78,193 78,193 h:600,120 h:78,193|4"
)

for tool in Xvfb xdotool ffmpeg cargo git; do
  command -v "$tool" >/dev/null || { echo "missing: $tool (see the header)" >&2; exit 1; }
done
mkdir -p "$OUT" "$WORK"
OUT=$(cd "$OUT" && pwd)

# 1. A copy of the plugin with a standalone binary. target/ lives in WORK and
#    is kept between runs, so only the first build is slow.
rm -rf "$WORK/plugin"
mkdir -p "$WORK/plugin"
(cd "$REPO/plugin" && tar --exclude=./target --exclude=./dist -cf - .) | tar -xf - -C "$WORK/plugin"
cd "$WORK/plugin"
sed -i 's|^\(nih_plug = {.*rev = "[0-9a-f]*"\) }|\1, features = ["standalone"] }|' Cargo.toml
grep -q 'features = \["standalone"\]' Cargo.toml || { echo "could not enable nih-plug's standalone feature" >&2; exit 1; }
sed -i 's|^crate-type = \["cdylib"\]|crate-type = ["cdylib", "lib"]|' Cargo.toml
echo 'pub fn run_standalone() { nih_plug::nih_export_standalone::<Drumgen>(); }' >> src/lib.rs
mkdir -p src/bin
echo 'fn main() { drumgen_vst::run_standalone(); }' > src/bin/standalone.rs

# 2. baseview at the rev the plugin locks, asking for no sRGB and no stencil.
REV=$(grep -o 'baseview.git?rev=[0-9a-f]*' Cargo.lock | head -1 | cut -d= -f2)
if [ ! -d "$WORK/baseview-$REV" ]; then
  git clone -q https://github.com/RustAudio/baseview.git "$WORK/baseview-$REV"
  git -C "$WORK/baseview-$REV" checkout -q "$REV"
  sed -i 's/GLX_FRAMEBUFFER_SRGB_CAPABLE_ARB, config.srgb as i32,/GLX_FRAMEBUFFER_SRGB_CAPABLE_ARB, 0,/;
          s/glx::GLX_STENCIL_SIZE, config.stencil_bits as i32,/glx::GLX_STENCIL_SIZE, 0,/' \
    "$WORK/baseview-$REV/src/gl/x11.rs"
fi
printf '\n[patch."https://github.com/RustAudio/baseview.git"]\nbaseview = { path = "%s" }\n' \
  "$WORK/baseview-$REV" >> Cargo.toml
CARGO_TARGET_DIR="$WORK/target" cargo build -q --bin standalone

# 3. Run it on a virtual display with a scratch HOME (it writes a session log
#    and songs.txt there, never in your real home).
mkdir -p "$WORK/home"
rm -rf "$WORK/home/drumgen_output"
Xvfb "$DISPLAY_NUM" -screen 0 1000x700x24 +extension GLX -noreset >/dev/null 2>&1 &
XVFB=$!
trap 'kill $APP $XVFB 2>/dev/null || true' EXIT
sleep 1
HOME="$WORK/home" DISPLAY="$DISPLAY_NUM" LIBGL_ALWAYS_SOFTWARE=1 \
  "$WORK/target/debug/standalone" --backend dummy > "$OUT/standalone.log" 2>&1 &
APP=$!
sleep 6

grab() { DISPLAY="$DISPLAY_NUM" ffmpeg -y -loglevel error -f x11grab -video_size 720x440 -i "$DISPLAY_NUM" -frames:v 1 "$OUT/$1.png"; }
grab 00-start
for step in "${STEPS[@]}"; do
  IFS='|' read -r name clicks wait <<<"$step"
  for xy in $clicks; do
    if [[ $xy == h:* ]]; then
      xy=${xy#h:}
      DISPLAY="$DISPLAY_NUM" xdotool mousemove "${xy%,*}" "${xy#*,}"
    else
      DISPLAY="$DISPLAY_NUM" xdotool mousemove "${xy%,*}" "${xy#*,}" click 1
    fi
    sleep 0.6
  done
  sleep "$wait"
  grab "$name"
done

cp "$WORK/home/drumgen_output/drumgen.log" "$OUT/drumgen.log" 2>/dev/null || true
if grep -q "ERROR" "$OUT/standalone.log"; then
  echo "the standalone logged errors, see $OUT/standalone.log" >&2
  exit 1
fi
echo "done -> $OUT (PNGs, the plugin's session log, standalone.log)"
