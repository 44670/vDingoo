# qiye.app Relocation False Positives

7 addresses in the RAWD section contain `lb`/`lbu` instructions whose raw
32-bit encoding coincidentally falls within the binary's address range
`[0x80A00000, 0x80B44880)`. These must NOT be patched as DATA32 relocs.

## False Positive List

| Offset | Address | Raw Value | Instruction | Context |
|--------|---------|-----------|-------------|---------|
| 0x00155C | 0x80A0155C | 0x80A70000 | `lb $a3, 0($a1)` | `strstr` — string search loop |
| 0x001834 | 0x80A01834 | 0x80B00000 | `lb $s0, 0($a1)` | `sscanf` — format parser |
| 0x003664 | 0x80A03664 | 0x80A20000 | `lb $v0, 0($a1)` | `sprintf` — format parser |
| 0x0037E4 | 0x80A037E4 | 0x80A20000 | `lb $v0, 0($a1)` | `sprintf` — format parser |
| 0x00DF48 | 0x80A0DF48 | 0x80A2006B | `lb $v0, 0x6b($a1)` | `Scene_updateTrigger` — flag check |
| 0x041930 | 0x80A41930 | 0x80A20068 | `lb $v0, 0x68($a1)` | `GameUnit_updateVisibility` — flag check |
| 0x0D97D8 | 0x80AD97D8 | 0x80A20000 | `lb $v0, 0($a1)` | `hashString` — hash loop |

## How They Were Identified

All 7 share two properties that distinguish them from real data pointers:

1. **Control flow context**: each has a conditional branch (`beq`/`bne`/`bnez`)
   at `addr+4` or `addr-4` that uses the loaded value, proving it is an
   executed instruction rather than a stored pointer.

2. **Isolation**: each appears alone among code, not in a contiguous run of
   pointer-sized values. Real vtable/RTTI regions always appear as clusters
   of 3+ consecutive pointers.

## Why They Exist

MIPS `lb` (load byte) has opcode `0x20` in bits[31:26]. When the base
register contains a KSEG0 address (`0x80xxxxxx`), the full 32-bit
instruction encoding starts with byte `0x80`, placing it inside the
binary's address range by coincidence.

This only affects `lb` (opcode `0x20`) because `0x80` is the only byte
value that is both a common MIPS opcode AND the high byte of the binary's
KSEG0 load address.

## Reloc Table Stats

| Source | Instruction relocs | DATA32 | Total |
|--------|-------------------|--------|-------|
| `gen_reloc.py` (disasm-based) | 17,559 | 2,705 | 20,264 |
| `gen_reloc_raw.py` (brute-force) | 17,559 | 3,891 | 21,450 |
| Correct (raw minus 7 FP) | 17,559 | 3,884 | 21,443 |

The disasm-based scanner missed 1,179 real DATA32 entries (vtables, RTTI
typeinfo, exception tables) because Binary Ninja misclassified them as
`lb $t5` instructions — the same opcode-0x80 coincidence, but in the
other direction.
