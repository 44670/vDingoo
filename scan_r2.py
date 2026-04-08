#!/usr/bin/env python3
"""Scan qiye.app for MIPS32R2-specific instructions."""

import struct
from collections import defaultdict

APP_PATH = "/home/john/work/vDingoo/qiye.app"
FILE_OFFSET = 0x970
LOAD_ADDR = 0x80A00000
CODE_SIZE = 0x13A2E0

results = defaultdict(list)  # name -> list of virtual addresses

with open(APP_PATH, "rb") as f:
    f.seek(FILE_OFFSET)
    code = f.read(CODE_SIZE)

assert len(code) == CODE_SIZE, f"Read {len(code)} bytes, expected {CODE_SIZE}"

for i in range(0, CODE_SIZE, 4):
    word = struct.unpack_from("<I", code, i)[0]
    opcode = (word >> 26) & 0x3F
    funct = word & 0x3F
    shamt = (word >> 6) & 0x1F
    rs = (word >> 21) & 0x1F
    vaddr = LOAD_ADDR + i

    if opcode == 0x1F:  # SPECIAL3
        if funct == 0x00:
            results["ext"].append(vaddr)
        elif funct == 0x04:
            results["ins"].append(vaddr)
        elif funct == 0x20:  # BSHFL
            if shamt == 0x10 and rs == 0:
                results["seb"].append(vaddr)
            elif shamt == 0x18 and rs == 0:
                results["seh"].append(vaddr)
            elif shamt == 0x02:
                results["wsbh"].append(vaddr)
    elif opcode == 0x00 and funct == 0x02:  # SRL
        bit21 = (word >> 21) & 1
        if bit21 == 1:
            results["rotr"].append(vaddr)

print("=== MIPS32R2 Instruction Scan ===")
print(f"Binary: {APP_PATH}")
print(f"Code range: 0x{LOAD_ADDR:08X} - 0x{LOAD_ADDR+CODE_SIZE:08X} ({CODE_SIZE} bytes, {CODE_SIZE//4} instructions)")
print()

total = 0
for name in ["ext", "ins", "seb", "seh", "rotr", "wsbh"]:
    addrs = results[name]
    total += len(addrs)
    print(f"{name:5s}: {len(addrs):6d} occurrences")
    for a in addrs[:5]:
        # Read the raw word for display
        off = a - LOAD_ADDR
        w = struct.unpack_from("<I", code, off)[0]
        print(f"        0x{a:08X}  {w:08X}")
    if len(addrs) > 5:
        print(f"        ... and {len(addrs)-5} more")
    print()

print(f"Total MIPS32R2 instructions: {total}")
