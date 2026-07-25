#!/usr/bin/env bash
# Install the drumgen on-save hook: every SAVE .MID in the plugin also renders
# a MuseScore-ready .musicxml score next to it, via notation.py.
# Run once from the repo root: ./scripts/install-notation-hook.sh
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
PY="$REPO/.venv/bin/python"
[ -x "$PY" ] || PY="$(command -v python3)"

mkdir -p "$HOME/.config/drumgen"
cat > "$HOME/.config/drumgen/on_save" <<HOOK
#!/usr/bin/env bash
# drumgen on-save hook — renders a score for every saved .mid.
# Installed by $REPO/scripts/install-notation-hook.sh
exec "$PY" "$REPO/notation.py" "\$1" >/dev/null 2>&1
HOOK
chmod +x "$HOME/.config/drumgen/on_save"
echo "Installed: ~/.config/drumgen/on_save -> notation.py ($PY)"
