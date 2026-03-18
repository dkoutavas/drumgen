#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"

echo "=== drumgen setup ==="

# Create virtual environment
if [ ! -d ".venv" ]; then
    echo "Creating virtual environment..."
    python3 -m venv .venv
else
    echo "Virtual environment already exists."
fi

# Activate and install deps
echo "Installing dependencies..."
source .venv/bin/activate
pip install -q -r requirements.txt

# Create required directories
mkdir -p output user_cells

echo ""
echo "Setup complete! Next steps:"
echo ""
echo "  CLI:  source .venv/bin/activate"
echo "        python drumgen.py --style screamo --tempo 180 --bars 4"
echo ""
echo "  GUI:  ./run-drumgen"
echo ""
echo "  See QUICKSTART.md for more examples."
