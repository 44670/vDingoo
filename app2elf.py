#!/usr/bin/env python3
"""Convert a CCDL .app binary (Dingoo) to a MIPS32 ELF executable."""

import struct
import sys
from pathlib import Path

# ELF constants
ELFCLASS32 = 1
ELFDATA2LSB = 1
ET_EXEC = 2
EM_MIPS = 8
EF_MIPS_NOREORDER = 0x00000001
EF_MIPS_ABI_O32 = 0x00001000
EF_MIPS_ARCH_32R2 = 0x70000000
PT_LOAD = 1
PF_R, PF_W, PF_X = 4, 2, 1
SHT_NULL, SHT_PROGBITS, SHT_SYMTAB, SHT_STRTAB, SHT_NOBITS = 0, 1, 2, 3, 8
SHF_WRITE, SHF_ALLOC, SHF_EXECINSTR = 1, 2, 4
STB_GLOBAL = 1
STT_FUNC = 2


def _ehdr(e_entry, e_phoff, e_shoff, e_flags, e_phnum, e_shnum, e_shstrndx):
    e_ident = b'\x7fELF' + bytes([ELFCLASS32, ELFDATA2LSB, 1]) + b'\x00' * 9
    return struct.pack('<16sHHIIIIIHHHHHH',
                       e_ident, ET_EXEC, EM_MIPS, 1,
                       e_entry, e_phoff, e_shoff, e_flags,
                       52, 32, e_phnum, 40, e_shnum, e_shstrndx)


def _phdr(p_offset, p_vaddr, p_filesz, p_memsz, p_flags, p_align=0x1000):
    return struct.pack('<IIIIIIII',
                       PT_LOAD, p_offset, p_vaddr, p_vaddr,
                       p_filesz, p_memsz, p_flags, p_align)


def _shdr(name, typ, flags, addr, offset, size, link=0, info=0, align=4, entsize=0):
    return struct.pack('<IIIIIIIIII',
                       name, typ, flags, addr, offset, size,
                       link, info, align, entsize)


def _sym(name, value, size, bind, typ, shndx):
    return struct.pack('<IIIBBH', name, value, size, (bind << 4) | typ, 0, shndx)


def parse_ccdl(data):
    if data[:4] != b'CCDL':
        raise ValueError(f"Bad magic: {data[:4]!r}")

    def parse_section_hdr(off, expected):
        magic = data[off:off + 4]
        if magic != expected:
            raise ValueError(f"Expected {expected!r} at 0x{off:x}, got {magic!r}")
        type_id, data_off, data_sz = struct.unpack_from('<III', data, off + 4)
        return {'type_id': type_id, 'data_offset': data_off, 'data_size': data_sz}

    impt = parse_section_hdr(0x20, b'IMPT')
    expt = parse_section_hdr(0x40, b'EXPT')
    rawd = parse_section_hdr(0x60, b'RAWD')
    _, load_vaddr, base_addr, mem_size = struct.unpack_from('<IIII', data, 0x70)
    rawd.update(load_vaddr=load_vaddr, base_addr=base_addr, memory_size=mem_size)

    def parse_table(sec):
        off = sec['data_offset']
        count = struct.unpack_from('<I', data, off)[0]
        entries_off = off + 16
        names_off = entries_off + count * 16
        entries = []
        for i in range(count):
            eo = entries_off + i * 16
            name_off, _, rtype, vaddr = struct.unpack_from('<IIII', data, eo)
            nstart = names_off + name_off
            nend = data.index(b'\x00', nstart)
            name = data[nstart:nend].decode('ascii', errors='replace')
            entries.append({'name': name, 'type': rtype, 'vaddr': vaddr})
        return entries

    code = data[rawd['data_offset']:rawd['data_offset'] + rawd['data_size']]
    return {'rawd': rawd, 'imports': parse_table(impt),
            'exports': parse_table(expt), 'code': code}


def ccdl_to_elf(ccdl):
    rawd = ccdl['rawd']
    load_vaddr = rawd['load_vaddr']
    base_addr = rawd['base_addr']
    code = ccdl['code']
    code_size = len(code)
    bss_size = rawd['memory_size'] - rawd['data_size']
    pre_gap = load_vaddr - base_addr

    entry = load_vaddr
    for exp in ccdl['exports']:
        if exp['name'] == 'AppMain':
            entry = exp['vaddr']
            break

    # String table (.strtab)
    strtab = bytearray(b'\x00')

    def add_str(s):
        idx = len(strtab)
        strtab.extend(s.encode() + b'\x00')
        return idx

    # Symbol table
    syms = [_sym(0, 0, 0, 0, 0, 0)]  # NULL symbol
    for imp in ccdl['imports']:
        syms.append(_sym(add_str(imp['name']), imp['vaddr'], 0, STB_GLOBAL, STT_FUNC, 1))
    for exp in ccdl['exports']:
        syms.append(_sym(add_str(exp['name']), exp['vaddr'], 0, STB_GLOBAL, STT_FUNC, 1))
    symtab_data = b''.join(syms)
    strtab_data = bytes(strtab)

    # Section header string table (.shstrtab)
    shstrtab = bytearray(b'\x00')

    def add_shstr(s):
        idx = len(shstrtab)
        shstrtab.extend(s.encode() + b'\x00')
        return idx

    n_text = add_shstr('.text')
    n_bss = add_shstr('.bss')
    n_symtab = add_shstr('.symtab')
    n_strtab = add_shstr('.strtab')
    n_shstrtab = add_shstr('.shstrtab')
    shstrtab_data = bytes(shstrtab)

    # Layout
    phnum = 1
    code_off = 52 + phnum * 32  # right after ELF header + phdrs
    full_code = b'\x00' * pre_gap + code  # zero-fill base_addr..load_vaddr gap
    symtab_off = code_off + len(full_code)
    strtab_off = symtab_off + len(symtab_data)
    shstrtab_off = strtab_off + len(strtab_data)
    shdr_off = (shstrtab_off + len(shstrtab_data) + 3) & ~3

    e_flags = EF_MIPS_NOREORDER | EF_MIPS_ABI_O32 | EF_MIPS_ARCH_32R2

    # Sections: NULL, .text, .bss, .symtab, .strtab, .shstrtab
    shdrs = [
        _shdr(0, SHT_NULL, 0, 0, 0, 0),
        _shdr(n_text, SHT_PROGBITS, SHF_ALLOC | SHF_EXECINSTR | SHF_WRITE,
              base_addr, code_off, len(full_code)),
        _shdr(n_bss, SHT_NOBITS, SHF_ALLOC | SHF_WRITE,
              load_vaddr + code_size, code_off + len(full_code), bss_size),
        _shdr(n_symtab, SHT_SYMTAB, 0, 0, symtab_off, len(symtab_data),
              link=4, info=1, entsize=16),
        _shdr(n_strtab, SHT_STRTAB, 0, 0, strtab_off, len(strtab_data), align=1),
        _shdr(n_shstrtab, SHT_STRTAB, 0, 0, shstrtab_off, len(shstrtab_data), align=1),
    ]

    out = bytearray()
    out += _ehdr(entry, 52, shdr_off, e_flags, phnum, len(shdrs), 5)
    out += _phdr(code_off, base_addr, len(full_code),
                 pre_gap + code_size + bss_size, PF_R | PF_W | PF_X)
    out += full_code
    out += symtab_data
    out += strtab_data
    out += shstrtab_data
    out += b'\x00' * (shdr_off - len(out))
    for sh in shdrs:
        out += sh
    return bytes(out)


def main():
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <input.app> [output.elf]")
        sys.exit(1)

    inp = Path(sys.argv[1])
    outp = Path(sys.argv[2]) if len(sys.argv) >= 3 else inp.with_suffix('.elf')
    data = inp.read_bytes()
    ccdl = parse_ccdl(data)

    rawd = ccdl['rawd']
    bss = rawd['memory_size'] - rawd['data_size']
    print(f"CCDL: {inp.name}")
    print(f"  Load address: 0x{rawd['load_vaddr']:08X}")
    print(f"  Base address: 0x{rawd['base_addr']:08X}")
    print(f"  Code size:    0x{rawd['data_size']:X} ({rawd['data_size']:,} bytes)")
    print(f"  Memory size:  0x{rawd['memory_size']:X} ({rawd['memory_size']:,} bytes)")
    print(f"  BSS size:     0x{bss:X} ({bss:,} bytes)")
    print(f"  Imports:      {len(ccdl['imports'])}")
    print(f"  Exports:      {len(ccdl['exports'])}")

    elf = ccdl_to_elf(ccdl)
    outp.write_bytes(elf)
    print(f"\nWrote {outp} ({len(elf):,} bytes)")


if __name__ == '__main__':
    main()
