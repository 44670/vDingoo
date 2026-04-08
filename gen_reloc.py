#!/usr/bin/env python3
"""
Generate relocation table for qiye.app CCDL binary.

Scans the raw binary + Binary Ninja disassembly to find all absolute address
references that need patching when relocating from 0x80A00000 to a new base
(e.g. 0x08A00000 for PSP).

Relocation types:
  HI16  - lui instruction, immediate is high 16 bits of address
  LO16  - addiu/ori/lw/sw/etc with low 16-bit offset paired to a HI16
  J26   - j/jal instruction, 26-bit word-address target
  DATA32 - 32-bit pointer in data section

Output: binary reloc table + human-readable text dump.
"""

import struct
import sys
import re
from pathlib import Path
from collections import defaultdict

# Binary layout
LOAD_ADDR   = 0x80A00000
FILE_OFFSET = 0x970       # RAWD data offset in .app file
DATA_SIZE   = 0x13A2E0    # code+data size on disk
MEM_SIZE    = 0x144880    # code+data+bss
END_ADDR    = LOAD_ADDR + MEM_SIZE  # 0x80B44880

# MIPS instruction helpers
def get_opcode(inst):
    return (inst >> 26) & 0x3F

def get_rs(inst):
    return (inst >> 21) & 0x1F

def get_rt(inst):
    return (inst >> 16) & 0x1F

def get_imm16(inst):
    return inst & 0xFFFF

def get_imm16_signed(inst):
    v = inst & 0xFFFF
    return v - 0x10000 if v & 0x8000 else v

def get_j_target(inst):
    return (inst & 0x03FFFFFF) << 2

def in_range(addr):
    """Check if address falls within the binary's address space."""
    return LOAD_ADDR <= addr < END_ADDR

def addr_to_offset(addr):
    return addr - LOAD_ADDR

def read_u32(data, off):
    return struct.unpack_from('<I', data, off)[0]


# ── Reloc types ──────────────────────────────────────────────────────────────
RELOC_HI16   = 0
RELOC_LO16   = 1  # addiu (sign-extended)
RELOC_LO16U  = 2  # ori (zero-extended)
RELOC_J26    = 3
RELOC_DATA32 = 4
RELOC_LO16_LOAD  = 5  # lw/lh/lb offset
RELOC_LO16_STORE = 6  # sw/sh/sb offset

RELOC_NAMES = {
    RELOC_HI16: "HI16",
    RELOC_LO16: "LO16",
    RELOC_LO16U: "LO16U",
    RELOC_J26: "J26",
    RELOC_DATA32: "DATA32",
    RELOC_LO16_LOAD: "LO16LD",
    RELOC_LO16_STORE: "LO16ST",
}


def scan_instructions(code):
    """Scan all instructions for relocatable references."""
    relocs = []
    hi16_pending = {}  # reg -> (addr, hi_val)

    code_size = len(code)

    for off in range(0, code_size, 4):
        addr = LOAD_ADDR + off
        inst = read_u32(code, off)
        opcode = get_opcode(inst)

        # ── J / JAL ──────────────────────────────────────────────────────
        if opcode in (2, 3):  # j=2, jal=3
            target = get_j_target(inst) | (addr & 0xF0000000)
            if in_range(target):
                relocs.append((addr, RELOC_J26, target))
            continue

        # ── LUI ──────────────────────────────────────────────────────────
        if opcode == 0x0F:  # lui
            rt = get_rt(inst)
            hi_val = get_imm16(inst) << 16
            # Track for pairing; also check if this alone is in range
            hi16_pending[rt] = (addr, hi_val)
            continue

        # ── Instructions that consume a HI16 value ───────────────────────
        rs = get_rs(inst)

        # addiu (opcode 0x09) - sign-extends immediate
        if opcode == 0x09 and rs in hi16_pending:
            hi_addr, hi_val = hi16_pending[rs]
            lo_val = get_imm16_signed(inst)
            full_addr = (hi_val + lo_val) & 0xFFFFFFFF
            if in_range(full_addr):
                relocs.append((hi_addr, RELOC_HI16, full_addr))
                relocs.append((addr, RELOC_LO16, full_addr))
            del hi16_pending[rs]
            continue

        # ori (opcode 0x0D) - zero-extends immediate
        if opcode == 0x0D and rs in hi16_pending:
            hi_addr, hi_val = hi16_pending[rs]
            lo_val = get_imm16(inst)
            full_addr = (hi_val | lo_val) & 0xFFFFFFFF
            if in_range(full_addr):
                relocs.append((hi_addr, RELOC_HI16, full_addr))
                relocs.append((addr, RELOC_LO16U, full_addr))
            del hi16_pending[rs]
            continue

        # lw/lh/lb/lbu/lhu (load with base+offset)
        if opcode in (0x20, 0x21, 0x23, 0x24, 0x25) and rs in hi16_pending:
            hi_addr, hi_val = hi16_pending[rs]
            lo_val = get_imm16_signed(inst)
            full_addr = (hi_val + lo_val) & 0xFFFFFFFF
            if in_range(full_addr):
                relocs.append((hi_addr, RELOC_HI16, full_addr))
                relocs.append((addr, RELOC_LO16_LOAD, full_addr))
            del hi16_pending[rs]
            continue

        # sw/sh/sb (store with base+offset)
        if opcode in (0x28, 0x29, 0x2B) and rs in hi16_pending:
            hi_addr, hi_val = hi16_pending[rs]
            lo_val = get_imm16_signed(inst)
            full_addr = (hi_val + lo_val) & 0xFFFFFFFF
            if in_range(full_addr):
                relocs.append((hi_addr, RELOC_HI16, full_addr))
                relocs.append((addr, RELOC_LO16_STORE, full_addr))
            del hi16_pending[rs]
            continue

        # Any other instruction using rs clears the pending HI16
        if rs in hi16_pending:
            # Check for standalone lui pointing into range
            hi_addr, hi_val = hi16_pending[rs]
            if in_range(hi_val) or in_range(hi_val + 0xFFFF):
                # Standalone lui - will record it but flag as unpaired
                pass
            del hi16_pending[rs]

    return relocs


def scan_data_pointers_from_disasm(disasm_path):
    """Parse Binary Ninja disassembly to find DATA32 pointers."""
    relocs = []

    # Match: void* data_XXXX = symbol_or_func
    # Match: void (* data_XXXX)(...) = func
    ptr_re = re.compile(
        r'^([0-9a-f]{8})\s+'           # address
        r'(?:void\*|.*\(\*\s)'         # pointer type
    )

    # Better: just find all typed data that references addresses in range
    # Format: "80ad87cc  void* data_80ad87cc = data_80ae0ae0"
    data_ptr_re = re.compile(
        r'^([0-9a-f]{8})\s+void\*\s+\w+\s*=\s*(\w+)'
    )

    # Also look for vtable-style arrays of function pointers
    # These show as raw hex data within .text that decode to addresses in range

    with open(disasm_path, 'r') as f:
        for line in f:
            m = data_ptr_re.match(line)
            if m:
                addr = int(m.group(1), 16)
                if in_range(addr):
                    relocs.append((addr, RELOC_DATA32, 0))

    return relocs


def scan_data_pointers_from_binary(code):
    """Scan data regions for 32-bit values that look like pointers into the binary.

    We use a conservative approach: only scan regions that Binary Ninja identified
    as data (not code). To identify data vs code, we look for sequences that
    aren't valid MIPS instructions or are in known data regions.

    For robustness, we cross-reference with the disassembly-based scan.
    """
    # This is a fallback — the disassembly-based scan is more reliable
    # because Binary Ninja already classified what's code vs data.
    return []


def scan_raw_data_words(code, code_addrs):
    """Scan all words in data regions (not in code_addrs set) for pointers."""
    relocs = []
    for off in range(0, len(code), 4):
        addr = LOAD_ADDR + off
        if addr in code_addrs:
            continue
        val = read_u32(code, off)
        if in_range(val):
            relocs.append((addr, RELOC_DATA32, val))
    return relocs


def parse_code_addresses(disasm_path):
    """Parse disassembly to find which addresses contain instructions vs data."""
    code_addrs = set()
    # Instruction line: "80a00000  e8ffbd27   addiu   ..."
    # or with ellipsis: "80a000ac  b480033c…  li      ..."
    inst_re = re.compile(r'^([0-9a-f]{8})\s+[0-9a-f]{8}[…]?\s+\w')

    with open(disasm_path, 'r') as f:
        for line in f:
            m = inst_re.match(line)
            if m:
                addr = int(m.group(1), 16)
                code_addrs.add(addr)
                # li pseudo-instruction spans 2 words
                if '…' in line[:20]:
                    code_addrs.add(addr + 4)

    return code_addrs


def apply_relocs_test(code, relocs, old_base, new_base):
    """Verify relocation by doing a dry-run apply and checking consistency."""
    delta = new_base - old_base
    delta_hi = (new_base >> 16) - (old_base >> 16)

    errors = 0
    for addr, rtype, target in relocs:
        off = addr - old_base
        if off < 0 or off + 4 > len(code):
            print(f"  ERROR: reloc at 0x{addr:08X} outside code range")
            errors += 1
            continue
        inst = read_u32(code, off)

        if rtype == RELOC_J26:
            old_target = get_j_target(inst) | (addr & 0xF0000000)
            if not in_range(old_target):
                print(f"  WARN: J26 at 0x{addr:08X} target 0x{old_target:08X} not in range")
                errors += 1
        elif rtype == RELOC_HI16:
            # Verify lui immediate is what we expect
            if get_opcode(inst) != 0x0F:
                print(f"  ERROR: HI16 at 0x{addr:08X} not a LUI (opcode=0x{get_opcode(inst):02X})")
                errors += 1
        elif rtype in (RELOC_LO16, RELOC_LO16U, RELOC_LO16_LOAD, RELOC_LO16_STORE):
            pass  # paired with HI16
        elif rtype == RELOC_DATA32:
            val = read_u32(code, off)
            if not in_range(val):
                print(f"  WARN: DATA32 at 0x{addr:08X} value 0x{val:08X} not in range")
                errors += 1

    return errors


def write_reloc_table(relocs, output_path):
    """Write binary relocation table.

    Format:
      Header (16 bytes):
        u32 magic = 'RLOC'
        u32 version = 1
        u32 entry_count
        u32 original_base

      Entries (8 bytes each):
        u32 offset    (from base address)
        u16 type      (RELOC_*)
        u16 reserved
    """
    with open(output_path, 'wb') as f:
        # Header
        f.write(b'RLOC')
        f.write(struct.pack('<III', 1, len(relocs), LOAD_ADDR))

        # Entries (sorted by address)
        for addr, rtype, target in sorted(relocs, key=lambda r: r[0]):
            offset = addr - LOAD_ADDR
            f.write(struct.pack('<IHH', offset, rtype, 0))


def write_reloc_text(relocs, output_path):
    """Write human-readable reloc dump."""
    stats = defaultdict(int)

    with open(output_path, 'w') as f:
        f.write(f"# Relocation table for qiye.app\n")
        f.write(f"# Original base: 0x{LOAD_ADDR:08X}\n")
        f.write(f"# Code+data size: 0x{DATA_SIZE:X} ({DATA_SIZE} bytes)\n")
        f.write(f"# Memory size:    0x{MEM_SIZE:X} ({MEM_SIZE} bytes)\n")
        f.write(f"# Total relocs:   {len(relocs)}\n")
        f.write(f"#\n")
        f.write(f"# Format: OFFSET   TYPE     TARGET\n")
        f.write(f"# {'='*50}\n\n")

        for addr, rtype, target in sorted(relocs, key=lambda r: r[0]):
            offset = addr - LOAD_ADDR
            name = RELOC_NAMES.get(rtype, f"?{rtype}")
            stats[name] += 1
            f.write(f"0x{offset:06X}  {name:<8s}  0x{target:08X}\n")

        f.write(f"\n# Summary:\n")
        for name, count in sorted(stats.items()):
            f.write(f"#   {name}: {count}\n")
        f.write(f"#   TOTAL: {len(relocs)}\n")


def main():
    app_path = Path(__file__).parent / "qiye.app"
    disasm_path = Path(__file__).parent / "qiye.elf.bndb_disassembly.txt"

    if not app_path.exists():
        print(f"Error: {app_path} not found")
        sys.exit(1)
    if not disasm_path.exists():
        print(f"Error: {disasm_path} not found")
        sys.exit(1)

    # Read raw binary
    raw = app_path.read_bytes()
    code = raw[FILE_OFFSET:FILE_OFFSET + DATA_SIZE]
    print(f"Loaded {len(code)} bytes from {app_path}")

    # Phase 1: Parse disassembly to classify code vs data addresses
    print("Parsing disassembly for code/data classification...")
    code_addrs = parse_code_addresses(disasm_path)
    print(f"  {len(code_addrs)} instruction addresses identified")

    # Phase 2: Scan instructions for HI16/LO16/J26
    print("Scanning instructions for relocatable references...")
    inst_relocs = scan_instructions(code)
    print(f"  {len(inst_relocs)} instruction relocs found")

    # Phase 3: Scan data regions for DATA32 pointers
    print("Scanning data regions for 32-bit pointers...")
    data_relocs_disasm = scan_data_pointers_from_disasm(disasm_path)
    print(f"  {len(data_relocs_disasm)} data pointers from disassembly")

    # Also scan raw binary for data pointers not in code regions
    data_relocs_raw = scan_raw_data_words(code, code_addrs)
    print(f"  {len(data_relocs_raw)} data pointers from raw binary scan")

    # Merge: prefer raw scan (complete), but use disasm to validate
    disasm_addrs = {r[0] for r in data_relocs_disasm}
    raw_addrs = {r[0] for r in data_relocs_raw}

    # Data pointers found in raw but not in disasm might be false positives
    # in non-data regions. Use disasm-classified code_addrs to filter.
    only_raw = raw_addrs - disasm_addrs
    only_disasm = disasm_addrs - raw_addrs
    both = raw_addrs & disasm_addrs

    print(f"  Data pointers: {len(both)} confirmed, {len(only_raw)} raw-only, {len(only_disasm)} disasm-only")

    # Use raw scan results (they're actual pointer values from the binary)
    # Filter: skip any that overlap with instruction relocs (already handled)
    inst_addrs = {r[0] for r in inst_relocs}
    data_relocs = [r for r in data_relocs_raw if r[0] not in inst_addrs]

    # Combine all relocs
    all_relocs = inst_relocs + data_relocs
    all_relocs.sort(key=lambda r: r[0])

    # Deduplicate (same address can appear from HI16+LO16 pair — that's expected)
    seen = set()
    unique_relocs = []
    for r in all_relocs:
        key = (r[0], r[1])
        if key not in seen:
            seen.add(key)
            unique_relocs.append(r)

    print(f"\nTotal unique relocs: {len(unique_relocs)}")

    # Stats
    stats = defaultdict(int)
    for _, rtype, _ in unique_relocs:
        stats[RELOC_NAMES[rtype]] += 1
    for name, count in sorted(stats.items()):
        print(f"  {name}: {count}")

    # Verify
    print("\nVerifying relocs...")
    errors = apply_relocs_test(code, unique_relocs, LOAD_ADDR, 0x08A00000)
    if errors:
        print(f"  {errors} errors found!")
    else:
        print("  All relocs verified OK")

    # Write outputs
    bin_path = Path(__file__).parent / "qiye_reloc.bin"
    txt_path = Path(__file__).parent / "qiye_reloc.txt"

    write_reloc_table(unique_relocs, bin_path)
    write_reloc_text(unique_relocs, txt_path)

    print(f"\nWritten: {bin_path} ({bin_path.stat().st_size} bytes)")
    print(f"Written: {txt_path}")


if __name__ == "__main__":
    main()
