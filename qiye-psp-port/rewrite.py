#!/usr/bin/env python3
"""
rewrite.py — Pre-patch qiye.app for PSP Allegrex.

SPECIAL2 (opcode 0x1C) patches:
  madd/maddu: in-place → SPECIAL func 0x1C/0x1D
  msub/msubu: in-place → SPECIAL func 0x2E/0x2F
  mul rd,rs,rt → trampoline with mult+mflo, appended after BSS

Strategy for mul (simplified):
  Always: patch site = J trampoline; NOP
  Trampoline: mult; mflo; <next_insn>; J back
  If <next_insn> is also a mul, expand it inline (mult;mflo) and
  fetch the instruction after that, recursively.
"""

import struct, sys
from pathlib import Path

# ── Instruction helpers ──────────────────────────────────────────────────────

def rd32(d, o): return struct.unpack_from('<I', d, o)[0]
def wr32(d, o, v): struct.pack_into('<I', d, o, v & 0xFFFFFFFF)
def pk(*insns): return b''.join(struct.pack('<I', w & 0xFFFFFFFF) for w in insns)
def make_j(a): return 0x08000000 | ((a >> 2) & 0x03FFFFFF)
NOP = 0

def is_mul(insn):
    return ((insn >> 26) & 0x3F) == 0x1C and (insn & 0x3F) == 0x02

def mul_parts(insn):
    rs = (insn >> 21) & 0x1F
    rt = (insn >> 16) & 0x1F
    rd = (insn >> 11) & 0x1F
    return rs, rt, rd

def make_mult(rs, rt): return (rs << 21) | (rt << 16) | 0x18
def make_mflo(rd):     return (rd << 11) | 0x12

def is_branch_or_jump(insn):
    op = (insn >> 26) & 0x3F
    if op in (1, 2, 3, 4, 5, 6, 7):
        return True
    if op == 0 and (insn & 0x3F) in (8, 9):
        return True
    return False

def is_conditional_branch(insn):
    op = (insn >> 26) & 0x3F
    return op in (1, 4, 5, 6, 7)

def branch_target(insn, pc):
    imm = insn & 0xFFFF
    if imm & 0x8000: imm -= 0x10000
    return (pc + 4 + (imm << 2)) & 0xFFFFFFFF

def invert_branch(insn):
    op = (insn >> 26) & 0x3F
    if op == 4: return (insn & ~(0x3F << 26)) | (5 << 26)
    if op == 5: return (insn & ~(0x3F << 26)) | (4 << 26)
    if op == 6: return (insn & ~(0x3F << 26)) | (7 << 26)
    if op == 7: return (insn & ~(0x3F << 26)) | (6 << 26)
    if op == 1: return insn ^ (1 << 16)
    raise ValueError(f"cannot invert branch op={op}")

# ── Main ─────────────────────────────────────────────────────────────────────

def main():
    app_path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path('nand/qiye.app')
    reloc_path = Path(sys.argv[2]) if len(sys.argv) > 2 else Path('nand/qiye.reloc.bin')

    app = bytearray(app_path.read_bytes())
    reloc = bytearray(reloc_path.read_bytes())

    assert app[:4] == b'CCDL' and app[0x60:0x64] == b'RAWD'
    assert reloc[:4] == b'RLOC'

    rawd_off  = rd32(app, 0x68)
    data_size = rd32(app, 0x6C)
    load_addr = rd32(app, 0x78)
    mem_size  = rd32(app, 0x7C)

    print(f"load=0x{load_addr:08x}  data=0x{data_size:x}  mem=0x{mem_size:x}  bss=0x{mem_size - data_size:x}")

    code = bytearray(app[rawd_off : rawd_off + data_size])

    # --- Pass 1: identify all mul positions and branch targets ---
    mul_offsets = set()
    branch_targets = set()
    for i in range(0, data_size, 4):
        insn = struct.unpack_from('<I', code, i)[0]
        if is_mul(insn):
            mul_offsets.add(i)
        op = (insn >> 26) & 0x3F
        if op in (1, 4, 5, 6, 7):
            imm = insn & 0xFFFF
            if imm & 0x8000: imm -= 0x10000
            branch_targets.add(load_addr + i + 4 + (imm << 2))

    # --- Pass 2: patch ---
    tramp = bytearray()
    tramp_base = mem_size
    new_relocs = []       # offsets needing J26 reloc
    killed_offsets = set() # code offsets we modified
    n_madd = n_msub = n_mul = 0
    patched = set()

    REMAP = {0x00: 0x1C, 0x01: 0x1D, 0x04: 0x2E, 0x05: 0x2F}
    n_delay = 0

    i = 0
    while i < data_size:
        insn = struct.unpack_from('<I', code, i)[0]

        if (insn >> 26) & 0x3F != 0x1C:
            i += 4; continue

        func = insn & 0x3F

        # madd/maddu/msub/msubu: simple opcode remap
        if func in REMAP:
            rs = (insn >> 21) & 0x1F
            rt = (insn >> 16) & 0x1F
            wr32(code, i, (rs << 21) | (rt << 16) | REMAP[func])
            if func <= 1: n_madd += 1
            else:         n_msub += 1
            i += 4
            continue

        if func != 0x02:
            i += 4; continue

        if i in patched:
            i += 4; continue

        # --- mul rd,rs,rt ---

        # Case: mul in delay slot of branch/jump at i-4
        if i >= 4:
            prev = struct.unpack_from('<I', code, i - 4)[0]
            if is_branch_or_jump(prev) and (i - 4) not in patched:
                patched.add(i)
                n_mul += 1
                n_delay += 1
                branch_off = i - 4
                branch_va = load_addr + branch_off

                t_off = tramp_base + len(tramp)
                t_addr = load_addr + t_off
                rs, rt, rd = mul_parts(insn)

                if is_conditional_branch(prev):
                    # mult first (doesn't affect GPRs used by branch condition),
                    # then inverted branch to skip taken path, mflo as delay slot
                    target = branch_target(prev, branch_va)
                    fall = branch_va + 8

                    inv = invert_branch(prev)
                    inv = (inv & 0xFFFF0000) | (2 & 0xFFFF)  # skip 2 insns

                    tramp.extend(pk(make_mult(rs, rt), inv, make_mflo(rd)))
                    j_taken = tramp_base + len(tramp)
                    tramp.extend(pk(make_j(target), NOP))
                    new_relocs.append(j_taken)
                    j_fall = tramp_base + len(tramp)
                    tramp.extend(pk(make_j(fall), NOP))
                    new_relocs.append(j_fall)
                elif ((prev >> 26) & 0x3F) == 2:  # J (no link)
                    j_target = (prev & 0x03FFFFFF) << 2 | (branch_va & 0xF0000000)
                    tramp.extend(pk(make_mult(rs, rt), make_mflo(rd)))
                    j_off = tramp_base + len(tramp)
                    tramp.extend(pk(make_j(j_target), NOP))
                    new_relocs.append(j_off)
                elif ((prev >> 26) & 0x3F) == 3:  # JAL
                    j_target = (prev & 0x03FFFFFF) << 2 | (branch_va & 0xF0000000)
                    ra_val = branch_va + 8
                    tramp.extend(pk(make_mult(rs, rt), make_mflo(rd),
                                    0x3C1F0000 | (ra_val >> 16),
                                    0x37FF0000 | (ra_val & 0xFFFF)))
                    j_off = tramp_base + len(tramp)
                    tramp.extend(pk(make_j(j_target), NOP))
                    new_relocs.append(j_off)
                else:
                    # JR/JALR — copy the instruction
                    tramp.extend(pk(make_mult(rs, rt), make_mflo(rd), prev, NOP))

                killed_offsets.add(branch_off)
                killed_offsets.add(i)
                wr32(code, branch_off, make_j(t_addr))
                wr32(code, i, NOP)
                new_relocs.append(branch_off)

                i += 4
                continue

        # Normal case: J trampoline; NOP at patch site
        # Trampoline: mult;mflo; then fetch next instruction.
        # If next is also mul, expand it too. Repeat until non-mul.

        t_off = tramp_base + len(tramp)
        t_addr = load_addr + t_off

        # Expand this mul and any consecutive muls
        pos = i
        while pos < data_size and is_mul(rd32(code, pos)):
            mul_insn = rd32(code, pos)
            rs, rt, rd = mul_parts(mul_insn)
            tramp.extend(pk(make_mult(rs, rt), make_mflo(rd)))
            patched.add(pos)
            n_mul += 1
            killed_offsets.add(pos)
            pos += 4

        # pos now points to the first non-mul instruction after the run.
        # Fetch it into the trampoline (NOP it at patch site),
        # UNLESS it's a branch target (other code jumps to it).
        next_va = load_addr + pos
        if pos < data_size and next_va not in branch_targets:
            next_insn = rd32(code, pos)

            if is_branch_or_jump(next_insn):
                # Next instruction is a branch/jump — also grab delay slot.
                ds = rd32(code, pos + 4) if pos + 4 < data_size else NOP
                op = (next_insn >> 26) & 0x3F
                if op in (2, 3):  # J or JAL — needs reloc
                    j_insn_off = tramp_base + len(tramp)
                    tramp.extend(pk(next_insn, ds))
                    new_relocs.append(j_insn_off)
                else:
                    tramp.extend(pk(next_insn, ds))
                # JAL/JALR: after the call returns, need J-back
                if op == 3 or (op == 0 and (next_insn & 0x3F) == 9):
                    ret_addr = load_addr + pos + 8
                    j_off = tramp_base + len(tramp)
                    tramp.extend(pk(make_j(ret_addr), NOP))
                    new_relocs.append(j_off)
                killed_offsets.add(pos)
                killed_offsets.add(pos + 4)
                wr32(code, pos, NOP)
                wr32(code, pos + 4, NOP)
                pos += 4  # account for delay slot
            else:
                tramp.extend(pk(next_insn))
                killed_offsets.add(pos)
                wr32(code, pos, NOP)

                # J back to pos+4
                ret_addr = load_addr + pos + 4
                j_off = tramp_base + len(tramp)
                tramp.extend(pk(make_j(ret_addr), NOP))
                new_relocs.append(j_off)
        else:
            # Can't NOP: it's a branch target. J back to pos directly.
            ret_addr = load_addr + pos
            j_off = tramp_base + len(tramp)
            tramp.extend(pk(make_j(ret_addr), NOP))
            new_relocs.append(j_off)

        # Patch site: J trampoline; NOP
        wr32(code, i, make_j(t_addr))
        wr32(code, i + 4, NOP)
        new_relocs.append(i)
        killed_offsets.add(i)
        if i + 4 != pos:
            killed_offsets.add(i + 4)

        i = pos + 4

    print(f"madd={n_madd}  msub={n_msub}  mul={n_mul} (trampoline, {n_delay} in delay slots)")
    print(f"trampolines: {len(tramp)} bytes")

    # --- Build output ---
    bss_pad = mem_size - data_size
    rawd_bin = code + b'\x00' * bss_pad + tramp
    new_mem_size = mem_size + len(tramp)

    # Filter out old reloc entries pointing to modified code offsets
    old_count = rd32(reloc, 8)
    filtered = bytearray(reloc[:16])  # keep header
    kept = 0
    killed = 0
    for idx in range(old_count):
        entry_off = 16 + idx * 8
        entry = reloc[entry_off : entry_off + 8]
        off = rd32(reloc, entry_off)
        if off in killed_offsets:
            killed += 1
        else:
            filtered += entry
            kept += 1
    # Append new reloc entries
    for off in new_relocs:
        filtered += struct.pack('<IHH', off, 3, 0)
    wr32(filtered, 8, kept + len(new_relocs))
    reloc = filtered
    print(f"relocs: {old_count} original, -{killed} killed, +{len(new_relocs)} new = {kept + len(new_relocs)}")

    out_dir = app_path.parent
    rawd_path = out_dir / 'qiye.patched.rawd.bin'
    out_reloc_path = reloc_path.with_suffix('.patched.bin')
    rawd_path.write_bytes(rawd_bin)
    out_reloc_path.write_bytes(reloc)
    print(f"\n{rawd_path}  ({len(rawd_bin)} bytes, mem_size=0x{new_mem_size:x})")
    print(f"{out_reloc_path}  ({len(reloc)} bytes)")

if __name__ == '__main__':
    main()
