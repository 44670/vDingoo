#!/bin/bash
#
# Build, deploy to PPSSPP memstick, and run with stdio tracing.
#
# PSP homebrew printf() goes through Kernel sceIo syscalls — PPSSPP logs them
# on the "sceKernel" / "HLE" channels. Use --log to capture all output.
#
# Usage:
#   ./test.sh          # build + run
#   ./test.sh --run    # skip build, just run
#   ./test.sh --log    # build + run with full log to file

set -e

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
for f in qiye.app qiye.reloc.bin; do
    src="$PROJECT_DIR/nand/$f"
    if [ ! -f "$src" ]; then
        # Try project root
        src="$PROJECT_DIR/$f"
    fi
    if [ -f "$src" ]; then
        cp "$src" "$GAME_DIR/nand/"
        echo "  copied $f ($(stat -c%s "$src") bytes)"
    else
        echo "  WARNING: $f not found!"
    fi
done

# Symlink the nand/ game assets directory (save files, PAK resources)
# Only link content subdirs that exist in the project's nand/ dir
if [ -d "$PROJECT_DIR/nand" ]; then
    for item in "$PROJECT_DIR/nand"/*; do
        basename="$(basename "$item")"
        # Skip the files we already copied
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

LOGFILE="$SCRIPT_DIR/ppsspp.log"
EBOOT_PATH="$GAME_DIR/EBOOT.PBP"

echo "=== Running in PPSSPP ==="
echo "EBOOT: $EBOOT_PATH"
echo "Log:   $LOGFILE"
echo ""

# PPSSPP flags:
#   -d           = debug log level (captures PSP printf via sceIo)
#   --log=FILE   = write log to file
#   --graphics=software  = no GPU required (headless-friendly)
#
# PSP printf output appears in log lines like:
#   HLE: sceIoWrite(fd=1, ...) = "..."  (stdout)
#   or in PPSSPP's "Kernel" log channel
#
# To view stdio in real-time, tail the log in another terminal:
#   tail -f qiye-psp-port/ppsspp.log | grep -i "sceIo\|HLE\|printf\|stdout"

if [ "$1" = "--log" ] || [ "$2" = "--log" ]; then
    echo "(Logging to $LOGFILE — tail -f $LOGFILE in another terminal)"
    echo ""
    "$PPSSPP_BIN" "$EBOOT_PATH" -d --log="$LOGFILE" 2>&1 | tee -a "$LOGFILE"
else
    # Default: show output on terminal, also save to log
    "$PPSSPP_BIN" "$EBOOT_PATH" -d 2>&1 | tee "$LOGFILE"
fi

echo ""
echo "=== Done (exit code: $?) ==="
