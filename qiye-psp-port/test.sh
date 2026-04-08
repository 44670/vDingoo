#!/bin/bash
#
# Build, deploy to PPSSPP memstick, and run.
#
# Usage:
#   ./test.sh          # build + run
#   ./test.sh --run    # skip build, just run

set -e

export DISPLAY=:0
export XAUTHORITY=$HOME/.Xauthority

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# PPSSPP setup
PPSSPP_DIR="$HOME/ppsspp/squashfs-root"
PPSSPP_BIN="$PPSSPP_DIR/bin/PPSSPPSDL"
export LD_LIBRARY_PATH="$PPSSPP_DIR/lib:${LD_LIBRARY_PATH:-}"

# PPSSPP memstick: ~/.config/ppsspp/PSP/
MEMSTICK="$HOME/.config/ppsspp/PSP"
GAME_DIR="$MEMSTICK/GAME/VDINGOO"

# ── Build ──────────────────────────────────────────────────────────────────

if [ "$1" != "--run" ]; then
    echo "=== Building ==="
    make -C "$SCRIPT_DIR" clean
    make -C "$SCRIPT_DIR" -j$(nproc)
    echo ""
fi

# ── Deploy to memstick ─────────────────────────────────────────────────────

echo "=== Deploying to $GAME_DIR ==="
mkdir -p "$GAME_DIR/nand"

# Copy EBOOT
cp "$SCRIPT_DIR/EBOOT.PBP" "$GAME_DIR/"

# Copy game data files (qiye.app, reloc table, and nand/ game assets)
for f in qiye.patched.app qiye.reloc.patched.bin; do
    src="$PROJECT_DIR/nand/$f"
    if [ ! -f "$src" ]; then
        src="$PROJECT_DIR/$f"
    fi
    if [ -f "$src" ]; then
        cp "$src" "$GAME_DIR/nand/"
        echo "  copied $f ($(stat -c%s "$src") bytes)"
    else
        echo "  WARNING: $f not found!"
    fi
done

# Symlink the nand/ game assets directory
if [ -d "$PROJECT_DIR/nand" ]; then
    for item in "$PROJECT_DIR/nand"/*; do
        basename="$(basename "$item")"
        [ "$basename" = "qiye.app" ] && continue
        [ "$basename" = "qiye.reloc.bin" ] && continue
        dest="$GAME_DIR/nand/$basename"
        if [ ! -e "$dest" ]; then
            ln -sf "$(realpath "$item")" "$dest"
            echo "  linked $basename"
        fi
    done
fi

echo ""

# ── Run PPSSPP ─────────────────────────────────────────────────────────────

EBOOT_PATH="$GAME_DIR/EBOOT.PBP"

LOGFILE="$SCRIPT_DIR/ppsspp.log"

echo "=== Running in PPSSPP ==="
echo "Log: $LOGFILE"
killall PPSSPPSDL 2>/dev/null || true
"$PPSSPP_BIN" "$EBOOT_PATH" "$@" >> "$LOGFILE" 2>&1
