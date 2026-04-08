#!/usr/bin/env python3
"""
rewrite.py — Pre-patch qiye.app for PSP Allegrex.

SPECIAL2 (opcode 0x1C) patches:
  madd/maddu: in-place → SPECIAL func 0x1C/0x1D
  msub/msubu: in-place → SPECIAL func 0x2E/0x2F
  mul rd,rs,rt → trampoline with mult+mflo, appended after BSS

Strategy for mul:
  1. Consecutive muls (N≥2): J trampoline; NOP; NOP remaining.
     Trampoline: mult;mflo × N; j run[-1]+4; nop
  2. Single mul NOT in delay slot:
     a. If mul+4 is NOT a branch/jump: leave it as J's delay slot.
        It executes once with stale rd (harmless for ALU ops), then
        trampoline returns to mul+4 which re-executes with correct rd.
        Trampoline: mult; mflo; j mul+4; nop
     b. If mul+4 IS a branch/jump: NOP it, handle in trampoline.
  3. Mul in delay slot of a branch at mul-4:
     Replace the branch with J trampoline; NOP.
     Trampoline: mult; mflo; emulate branch (inverted+J for range).
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
    """BEQ/BNE/BLEZ/BGTZ/REGIMM — has 16-bit PC-relative offset."""
    op = (insn >> 26) & 0x3F
    return op in (1, 4, 5, 6, 7)

def branch_target(insn, pc):
    imm = insn & 0xFFFF
    if imm & 0x8000: imm -= 0x10000
    return (pc + 4 + (imm << 2)) & 0xFFFFFFFF

def invert_branch(insn):
    op = (insn >> 26) & 0x3F
    if op == 4: return (insn & ~(0x3F << 26)) | (5 << 26)   # BEQ → BNE
    if op == 5: return (insn & ~(0x3F << 26)) | (4 << 26)   # BNE → BEQ
    if op == 6: return (insn & ~(0x3F << 26)) | (7 << 26)   # BLEZ → BGTZ
    if op == 7: return (insn & ~(0x3F << 26)) | (6 << 26)   # BGTZ → BLEZ
    if op == 1: return insn ^ (1 << 16)                      # REGIMM: flip bit 16
    raise ValueError(f"cannot invert branch op={op}")

def is_j_no_link(insn):
    return ((insn >> 26) & 0x3F) == 2

def is_link(insn):
    op = (insn >> 26) & 0x3F
    if op == 3: return True
    if op == 0 and (insn & 0x3F) == 9: return True
    return False

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

    # --- Pass 1: identify all mul positions ---
    mul_offsets = set()
    for i in range(0, data_size, 4):
        insn = struct.unpack_from('<I', code, i)[0]
        if is_mul(insn):
            mul_offsets.add(i)

    # --- Pass 2: patch ---
    tramp = bytearray()
    tramp_base = mem_size
    new_relocs = []       # offsets needing J26 reloc (new J instructions)
    killed_offsets = set() # code offsets we modified (NOP'd or replaced) — remove old relocs
    n_madd = n_msub = n_mul_tramp = n_delay = 0
    counts = {'simple': 0, 'branch': 0, 'link': 0, 'j': 0, 'jr': 0, 'consecutive': 0}
    patched = set()

    # Pre-scan: collect all branch targets so we know which addrs are jumped to
    branch_targets = set()
    for ii in range(0, data_size, 4):
        w = struct.unpack_from('<I', code, ii)[0]
        op = (w >> 26) & 0x3F
        if op in (1, 4, 5, 6, 7):
            imm = w & 0xFFFF
            if imm & 0x8000: imm -= 0x10000
            branch_targets.add(load_addr + ii + 4 + (imm << 2))

    REMAP = {0x00: 0x1C, 0x01: 0x1D, 0x04: 0x2E, 0x05: 0x2F}

    def emit_tramp(*words):
        """Append instructions to trampoline, return offset of first."""
        off = tramp_base + len(tramp)
        for w in words:
            tramp.extend(struct.pack('<I', w & 0xFFFFFFFF))
        return off

    def emit_mul_expansion(mul_insn):
        """Emit mult;mflo for a mul instruction."""
        rs, rt, rd = mul_parts(mul_insn)
        tramp.extend(pk(make_mult(rs, rt), make_mflo(rd)))

    def emit_j_or_jal(insn):
        """Copy a J/JAL instruction into trampoline and add its reloc."""
        off = tramp_base + len(tramp)
        tramp.extend(pk(insn))
        new_relocs.append(off)

    def kill(code_off):
        """Mark a code offset as modified — its old reloc entry must be removed."""
        killed_offsets.add(code_off)

    def emit_branch_emulation(branch_insn, branch_va, ds_insn):
        """Emit inverted-branch + J to handle a conditional branch.

        Layout:
          inv_branch skip    ; if NOT taken, skip to fall-through path
          <ds>               ; branch's delay slot (always executes)
          j taken_target     ; branch WAS taken
          nop
        skip:
          j fall_through     ; branch NOT taken
          nop
        """
        target = branch_target(branch_insn, branch_va)
        fall = branch_va + 8  # past branch + delay slot

        inv = invert_branch(branch_insn)
        inv = (inv & 0xFFFF0000) | (2 & 0xFFFF)  # skip 2 insns (+2)

        tramp.extend(pk(inv, ds_insn))
        j_taken_off = tramp_base + len(tramp)
        tramp.extend(pk(make_j(target), NOP))
        new_relocs.append(j_taken_off)

        j_fall_off = tramp_base + len(tramp)
        tramp.extend(pk(make_j(fall), NOP))
        new_relocs.append(j_fall_off)

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

        # --- mul rd,rs,rt ---

        # Case 3: mul in delay slot of branch at i-4
        if i >= 4:
            prev = struct.unpack_from('<I', code, i - 4)[0]
            if is_branch_or_jump(prev):
                patched.add(i)
                n_delay += 1

                branch_off = i - 4
                branch_va = load_addr + branch_off
                mul_insn = insn

                t_off = emit_tramp()  # record start
                t_addr = load_addr + t_off
                rs, rt, rd = mul_parts(mul_insn)

                if is_conditional_branch(prev):
                    # Conditional branch with mul in delay slot.
                    # CRITICAL: the branch tests GPRs BEFORE the mul writes.
                    # So we must emit mult (writes only HI/LO, not GPRs)
                    # first, then test the branch condition with original GPRs,
                    # and use mflo as the delay slot of the inverted branch
                    # (always executes, just like the original delay slot).
                    tramp.extend(pk(make_mult(rs, rt)))
                    emit_branch_emulation(prev, branch_va, make_mflo(rd))
                elif is_j_no_link(prev):
                    # Unconditional J: mult+mflo then jump to target.
                    emit_mul_expansion(mul_insn)
                    j_target = (prev & 0x03FFFFFF) << 2 | (branch_va & 0xF0000000)
                    j_off = tramp_base + len(tramp)
                    tramp.extend(pk(make_j(j_target), NOP))
                    new_relocs.append(j_off)
                elif is_link(prev):
                    # JAL/JALR: mult+mflo then set $ra and jump.
                    emit_mul_expansion(mul_insn)
                    if ((prev >> 26) & 0x3F) == 3:  # JAL
                        j_target = (prev & 0x03FFFFFF) << 2 | (branch_va & 0xF0000000)
                        ra_val = branch_va + 8
                        tramp.extend(pk(
                            0x3C1F0000 | (ra_val >> 16),
                            0x37FF0000 | (ra_val & 0xFFFF),
                        ))
                        j_off = tramp_base + len(tramp)
                        tramp.extend(pk(make_j(j_target), NOP))
                        new_relocs.append(j_off)
                    else:
                        jalr_rs = (prev >> 21) & 0x1F
                        ra_val = branch_va + 8
                        tramp.extend(pk(
                            0x3C1F0000 | (ra_val >> 16),
                            0x37FF0000 | (ra_val & 0xFFFF),
                            (jalr_rs << 21) | 8,  # jr $rs
                            NOP,
                        ))
                else:
                    # JR (non-linking): mult+mflo then jr.
                    emit_mul_expansion(mul_insn)
                    tramp.extend(pk(prev, NOP))

                # Patch: replace branch with J trampoline; NOP
                kill(branch_off)
                kill(i)
                wr32(code, branch_off, make_j(t_addr))
                wr32(code, i, NOP)  # NOP the mul (was delay slot)
                new_relocs.append(branch_off)

                i += 4
                continue

        # Collect consecutive muls
        run = []
        j = i
        while j < data_size and j in mul_offsets and j not in patched:
            run.append(j)
            patched.add(j)
            j += 4

        # Build trampoline
        t_off = emit_tramp()
        t_addr = load_addr + t_off

        for off in run:
            emit_mul_expansion(rd32(code, off))

        if len(run) >= 2:
            # ── Consecutive muls ──────────────────────────────────────────
            # J delay slot clobbers run[1], which is a mul already in the
            # trampoline. Return to first insn after the run.
            ret = load_addr + run[-1] + 4
            j_off = tramp_base + len(tramp)
            tramp.extend(pk(make_j(ret), NOP))
            new_relocs.append(j_off)
            counts['consecutive'] += 1

            kill(run[0]); kill(run[0] + 4)
            wr32(code, run[0], make_j(t_addr))
            wr32(code, run[0] + 4, NOP)
            new_relocs.append(run[0])
            for off in run[1:]:
                kill(off)
                wr32(code, off, NOP)

        else:
            # ── Single mul ────────────────────────────────────────────────
            next_insn = rd32(code, run[0] + 4)

            if not is_branch_or_jump(next_insn):
                # Case 2a: next insn is NOT a branch/jump.
                # NOP it and execute it in the trampoline after mflo,
                # returning to mul+8.  This avoids the delay-slot
                # double-execution bug where next_insn can clobber
                # the mul's source registers before the trampoline
                # reads them for mult.
                #
                # Exception: if mul+4 is a branch target, we can't NOP it
                # (would break the back-edge).  In that case, leave it as
                # the J delay slot — only safe when next_insn doesn't
                # write to mul's rs or rt.
                mul_va = load_addr + run[0]
                next_va = mul_va + 4

                if next_va in branch_targets:
                    # mul+4 is a branch target — keep it intact (old approach)
                    ret = next_va
                    j_off = tramp_base + len(tramp)
                    tramp.extend(pk(make_j(ret), NOP))
                    new_relocs.append(j_off)
                    kill(run[0])
                    wr32(code, run[0], make_j(t_addr))
                    new_relocs.append(run[0])
                else:
                    # Safe to NOP: put next_insn in trampoline
                    tramp.extend(pk(next_insn))
                    ret = next_va + 4  # mul+8
                    j_off = tramp_base + len(tramp)
                    tramp.extend(pk(make_j(ret), NOP))
                    new_relocs.append(j_off)
                    kill(run[0]); kill(run[0] + 4)
                    wr32(code, run[0], make_j(t_addr))
                    wr32(code, run[0] + 4, NOP)
                    new_relocs.append(run[0])
                counts['simple'] += 1

            elif is_conditional_branch(next_insn):
                # Case 2b-branch: conditional branch at mul+4.
                # NOP it and its delay slot; handle in trampoline.
                branch_va = load_addr + run[0] + 4
                ds = rd32(code, run[0] + 8)
                emit_branch_emulation(next_insn, branch_va, ds)
                counts['branch'] += 1

                kill(run[0]); kill(run[0] + 4); kill(run[0] + 8)
                wr32(code, run[0], make_j(t_addr))
                wr32(code, run[0] + 4, NOP)
                wr32(code, run[0] + 8, NOP)
                new_relocs.append(run[0])

            elif is_link(next_insn):
                # Case 2b-link: JAL/JALR at mul+4.
                # Trampoline: mult; mflo; jal target; ds; j return; nop
                ds = rd32(code, run[0] + 8)
                emit_j_or_jal(next_insn)  # copy JAL with reloc
                tramp.extend(pk(ds))
                ret = load_addr + run[0] + 12
                j_off = tramp_base + len(tramp)
                tramp.extend(pk(make_j(ret), NOP))
                new_relocs.append(j_off)
                counts['link'] += 1

                kill(run[0]); kill(run[0] + 4); kill(run[0] + 8)
                wr32(code, run[0], make_j(t_addr))
                wr32(code, run[0] + 4, NOP)
                wr32(code, run[0] + 8, NOP)
                new_relocs.append(run[0])

            elif is_j_no_link(next_insn):
                # Case 2b-j: unconditional J at mul+4. No return needed.
                ds = rd32(code, run[0] + 8)
                emit_j_or_jal(next_insn)  # copy J with reloc
                tramp.extend(pk(ds))
                counts['j'] += 1

                kill(run[0]); kill(run[0] + 4); kill(run[0] + 8)
                wr32(code, run[0], make_j(t_addr))
                wr32(code, run[0] + 4, NOP)
                wr32(code, run[0] + 8, NOP)
                new_relocs.append(run[0])

            else:
                # JR (non-linking) — no absolute addr, no reloc needed
                ds = rd32(code, run[0] + 8)
                tramp.extend(pk(next_insn, ds))
                counts['jr'] += 1

                kill(run[0]); kill(run[0] + 4); kill(run[0] + 8)
                wr32(code, run[0], make_j(t_addr))
                wr32(code, run[0] + 4, NOP)
                wr32(code, run[0] + 8, NOP)
                new_relocs.append(run[0])

        n_mul_tramp += len(run)
        i = run[-1] + 4

    print(f"madd={n_madd}  msub={n_msub}  mul={n_mul_tramp} (trampoline)  delay_slot={n_delay}")
    print(f"  single: simple={counts['simple']}  branch={counts['branch']}  "
          f"jal={counts['link']}  j={counts['j']}  jr={counts['jr']}  "
          f"consecutive={counts['consecutive']}")
    print(f"trampolines: {len(tramp)} bytes")

    # --- Verify ---
    errors = 0
    for off in mul_offsets:
        if off < 4 or off not in patched: continue
        insn = struct.unpack_from('<I', code, off)[0]
        if not is_branch_or_jump(insn): continue
        prev = struct.unpack_from('<I', code, off - 4)[0]
        if is_branch_or_jump(prev):
            print(f"  WARN: branch-in-delay-slot at 0x{load_addr + off:08x}")
            errors += 1
    for k in range(0, len(tramp) - 4, 4):
        a = struct.unpack_from('<I', tramp, k)[0]
        b = struct.unpack_from('<I', tramp, k + 4)[0]
        if is_branch_or_jump(a) and is_branch_or_jump(b):
            # Inverted-branch trampolines have branch → j by design
            if is_conditional_branch(a) and (a & 0xFFFF) == 2:
                continue
            print(f"  WARN: jump+jump in trampoline at offset 0x{k:x}")
            errors += 1
    print(f"verify: {'OK' if not errors else f'{errors} issues!'}")

    # --- Build output ---
    # Output raw code+data+BSS+trampolines as a standalone binary.
    # The original .app file is left untouched (it has 55MB of resources after RAWD).
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
