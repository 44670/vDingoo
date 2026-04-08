#!/usr/bin/env python3
"""
Pure-binary relocation table generator for qiye.app.

Scans the entire RAWD section for relocatable references:
  - HI16/LO16 pairs (lui + addiu/ori/lw/sw)
  - J26 (j/jal)
  - DATA32 (any 32-bit word that is a pointer into the binary's range)

False positives in DATA32 are acceptable — the loader can
skip relocs that point to code (instructions whose bytes
happen to match an address).
"""

import struct
import sys
from pathlib import Path
from collections import defaultdict

# ── Binary layout ────────────────────────────────────────────────────────────
LOAD_ADDR   = 0x80A00000
FILE_OFFSET = 0x970
DATA_SIZE   = 0x13A2E0
MEM_SIZE    = 0x144880
END_ADDR    = LOAD_ADDR + MEM_SIZE

def in_range(addr):
    return LOAD_ADDR <= addr < END_ADDR

def read_u32(data, off):
    return struct.unpack_from('<I', data, off)[0]

def get_opcode(w):   return (w >> 26) & 0x3F
def get_rs(w):       return (w >> 21) & 0x1F
def get_rt(w):       return (w >> 16) & 0x1F
def get_imm16(w):    return w & 0xFFFF
def get_imm16s(w):
    v = w & 0xFFFF
    return v - 0x10000 if v & 0x8000 else v
def get_j_target(w, pc):
    return ((w & 0x03FFFFFF) << 2) | (pc & 0xF0000000)

# ── Reloc types ──────────────────────────────────────────────────────────────
RELOC_HI16   = 0
RELOC_LO16   = 1  # addiu (sign-extended)
RELOC_LO16U  = 2  # ori (zero-extended)
RELOC_J26    = 3
RELOC_DATA32 = 4
RELOC_LO16_LOAD  = 5  # lw/lh/lb offset
RELOC_LO16_STORE = 6  # sw/sh/sb offset

RELOC_NAMES = {
    0: "HI16", 1: "LO16", 2: "LO16U", 3: "J26",
    4: "DATA32", 5: "LO16LD", 6: "LO16ST",
}


def scan_instructions(code):
    """Scan all words for HI16/LO16 pairs and J26."""
    relocs = []
    hi16_addrs = set()  # addresses that are part of HI16 relocs
    lo16_addrs = set()  # addresses that are part of LO16 relocs
    j26_addrs = set()   # addresses that are j/jal

    hi16_pending = {}  # reg -> (addr, hi_val)

    for off in range(0, DATA_SIZE, 4):
        addr = LOAD_ADDR + off
        inst = read_u32(code, off)
        opcode = get_opcode(inst)

        # J / JAL
        if opcode in (2, 3):
            target = get_j_target(inst, addr)
            if in_range(target):
                relocs.append((addr, RELOC_J26, target))
                j26_addrs.add(addr)
            continue

        # LUI
        if opcode == 0x0F:
            rt = get_rt(inst)
            hi_val = get_imm16(inst) << 16
            hi16_pending[rt] = (addr, hi_val)
            continue

        rs = get_rs(inst)

        def try_pair(rtype, lo_val_fn):
            hi_addr, hi_val = hi16_pending[rs]
            full = lo_val_fn(hi_val) & 0xFFFFFFFF
            if in_range(full):
                relocs.append((hi_addr, RELOC_HI16, full))
                relocs.append((addr, rtype, full))
                hi16_addrs.add(hi_addr)
                lo16_addrs.add(addr)
            del hi16_pending[rs]

        # addiu
        if opcode == 0x09 and rs in hi16_pending:
            try_pair(RELOC_LO16, lambda hi: hi + get_imm16s(inst))
            continue
        # ori
        if opcode == 0x0D and rs in hi16_pending:
            try_pair(RELOC_LO16U, lambda hi: hi | get_imm16(inst))
            continue
        # loads
        if opcode in (0x20, 0x21, 0x23, 0x24, 0x25) and rs in hi16_pending:
            try_pair(RELOC_LO16_LOAD, lambda hi: hi + get_imm16s(inst))
            continue
        # stores
        if opcode in (0x28, 0x29, 0x2B) and rs in hi16_pending:
            try_pair(RELOC_LO16_STORE, lambda hi: hi + get_imm16s(inst))
            continue

        if rs in hi16_pending:
            del hi16_pending[rs]

    return relocs, hi16_addrs | lo16_addrs | j26_addrs


def scan_data_pointers(code, inst_addrs):
    """Scan entire RAWD for 32-bit words that look like pointers.

    Skip addresses already claimed by instruction relocs.
    False positives are acceptable.
    """
    relocs = []
    for off in range(0, DATA_SIZE, 4):
        addr = LOAD_ADDR + off
        if addr in inst_addrs:
            continue
        val = read_u32(code, off)
        if in_range(val):
            relocs.append((addr, RELOC_DATA32, val))
    return relocs


def write_reloc_table(relocs, path):
    """Binary reloc table: RLOC header + 8-byte entries."""
    with open(path, 'wb') as f:
        f.write(b'RLOC')
        f.write(struct.pack('<III', 1, len(relocs), LOAD_ADDR))
        for addr, rtype, target in sorted(relocs, key=lambda r: r[0]):
            f.write(struct.pack('<IHH', addr - LOAD_ADDR, rtype, 0))


def write_reloc_text(relocs, path):
    stats = defaultdict(int)
    with open(path, 'w') as f:
        f.write(f"# Relocation table (raw binary scan)\n")
        f.write(f"# Original base: 0x{LOAD_ADDR:08X}\n")
        f.write(f"# Total relocs:   {len(relocs)}\n")
        f.write(f"# Format: OFFSET   TYPE     TARGET\n")
        f.write(f"# {'='*50}\n\n")
        for addr, rtype, target in sorted(relocs, key=lambda r: r[0]):
            name = RELOC_NAMES[rtype]
            stats[name] += 1
            f.write(f"0x{addr - LOAD_ADDR:06X}  {name:<8s}  0x{target:08X}\n")
        f.write(f"\n# Summary:\n")
        for name, count in sorted(stats.items()):
            f.write(f"#   {name}: {count}\n")
        f.write(f"#   TOTAL: {len(relocs)}\n")


def main():
    app_path = Path(__file__).parent / "qiye.app"
    if not app_path.exists():
        print(f"Error: {app_path} not found"); sys.exit(1)

    raw = app_path.read_bytes()
    code = raw[FILE_OFFSET:FILE_OFFSET + DATA_SIZE]
    print(f"Loaded {len(code)} bytes from RAWD")

    # Phase 1: instruction relocs
    print("Scanning instructions (HI16/LO16/J26)...")
    inst_relocs, inst_addrs = scan_instructions(code)
    print(f"  {len(inst_relocs)} instruction relocs at {len(inst_addrs)} addresses")

    # Phase 2: data pointers (brute-force, entire RAWD)
    print("Scanning all words for DATA32 pointers...")
    data_relocs = scan_data_pointers(code, inst_addrs)
    print(f"  {len(data_relocs)} data pointers (may include false positives)")

    # Merge + dedup
    all_relocs = inst_relocs + data_relocs
    all_relocs.sort(key=lambda r: r[0])
    seen = set()
    unique = []
    for r in all_relocs:
        key = (r[0], r[1])
        if key not in seen:
            seen.add(key)
            unique.append(r)

    print(f"\nTotal unique relocs: {len(unique)}")
    stats = defaultdict(int)
    for _, rt, _ in unique:
        stats[RELOC_NAMES[rt]] += 1
    for name, count in sorted(stats.items()):
        print(f"  {name}: {count}")

    # Write outputs
    bin_path = Path(__file__).parent / "qiye_reloc_raw.bin"
    txt_path = Path(__file__).parent / "qiye_reloc_raw.txt"
    write_reloc_table(unique, bin_path)
    write_reloc_text(unique, txt_path)
    print(f"\nWritten: {bin_path} ({bin_path.stat().st_size} bytes)")
    print(f"Written: {txt_path}")


if __name__ == "__main__":
    main()
