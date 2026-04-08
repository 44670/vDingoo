use crate::mem::Memory;

#[derive(Debug, Clone)]
pub struct ImportEntry {
    pub name: String,
    pub target_vaddr: u32,
}

#[derive(Debug, Clone)]
pub struct ExportEntry {
    pub name: String,
    pub vaddr: u32,
}

#[derive(Debug)]
pub struct CcdlBinary {
    pub imports: Vec<ImportEntry>,
    pub exports: Vec<ExportEntry>,
    pub entry_point: u32,
    pub load_address: u32,
    pub data_size: u32,
    pub memory_size: u32,
}

fn read_u32_le(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn parse_section_hdr(data: &[u8], off: usize, expected: &[u8; 4]) -> (u32, u32) {
    assert_eq!(
        &data[off..off + 4],
        expected,
        "Expected {expected:?} at 0x{off:x}",
    );
    let data_offset = read_u32_le(data, off + 8);
    let data_size = read_u32_le(data, off + 12);
    (data_offset, data_size)
}

fn parse_table(data: &[u8], table_offset: u32) -> Vec<(String, u32, u32)> {
    let off = table_offset as usize;
    let count = read_u32_le(data, off) as usize;
    let entries_off = off + 16;
    let names_off = entries_off + count * 16;

    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let eo = entries_off + i * 16;
        let name_off = read_u32_le(data, eo) as usize;
        let rtype = read_u32_le(data, eo + 8);
        let vaddr = read_u32_le(data, eo + 12);

        let nstart = names_off + name_off;
        let nend = data[nstart..].iter().position(|&b| b == 0).unwrap() + nstart;
        let name = String::from_utf8_lossy(&data[nstart..nend]).into_owned();
        entries.push((name, rtype, vaddr));
    }
    entries
}

pub fn parse_ccdl(data: &[u8]) -> CcdlBinary {
    assert_eq!(&data[0..4], b"CCDL", "Bad magic: {:?}", &data[0..4]);

    let (impt_off, _impt_sz) = parse_section_hdr(data, 0x20, b"IMPT");
    let (expt_off, _expt_sz) = parse_section_hdr(data, 0x40, b"EXPT");
    let (_rawd_off, rawd_sz) = parse_section_hdr(data, 0x60, b"RAWD");

    let entry_point = read_u32_le(data, 0x74);
    let load_address = read_u32_le(data, 0x78);
    let memory_size = read_u32_le(data, 0x7C);

    let imports: Vec<ImportEntry> = parse_table(data, impt_off)
        .into_iter()
        .map(|(name, _rtype, vaddr)| ImportEntry {
            name,
            target_vaddr: vaddr,
        })
        .collect();

    let exports: Vec<ExportEntry> = parse_table(data, expt_off)
        .into_iter()
        .map(|(name, _rtype, vaddr)| ExportEntry { name, vaddr })
        .collect();

    CcdlBinary {
        imports,
        exports,
        entry_point,
        load_address,
        data_size: rawd_sz,
        memory_size,
    }
}

pub fn load_ccdl(data: &[u8], mem: &mut Memory) -> CcdlBinary {
    let mut ccdl = parse_ccdl(data);

    // Check for patched RAWD binary (code+BSS+trampolines from rewrite.py)
    let rawd_override = std::fs::read("nand/qiye.patched.rawd.bin").ok();
    if let Some(ref rawd_data) = rawd_override {
        // Patched RAWD: entire file is code+BSS+trampolines, load it all
        let dest = mem.slice_mut(ccdl.load_address, rawd_data.len());
        dest.copy_from_slice(rawd_data);
        // Update sizes: data_size = full file, memory_size = same (no extra BSS)
        ccdl.data_size = rawd_data.len() as u32;
        ccdl.memory_size = rawd_data.len() as u32;
        eprintln!("[LOADER] using patched RAWD ({} bytes)", rawd_data.len());
    } else {
        // Load code+data into guest memory at load_address
        let rawd_off = {
            let (off, _) = parse_section_hdr(data, 0x60, b"RAWD");
            off as usize
        };
        let code = &data[rawd_off..rawd_off + ccdl.data_size as usize];
        let dest = mem.slice_mut(ccdl.load_address, code.len());
        dest.copy_from_slice(code);

        // Zero BSS
        let bss_start = ccdl.load_address + ccdl.data_size;
        let bss_size = ccdl.memory_size - ccdl.data_size;
        if bss_size > 0 {
            let bss = mem.slice_mut(bss_start, bss_size as usize);
            bss.fill(0);
        }
    }

    ccdl
}

// ── Relocation ─────────────────────────────────────────────────────────────

/// Relocation types matching gen_reloc_raw.py
const RELOC_HI16: u16 = 0;
const RELOC_LO16: u16 = 1;
const RELOC_LO16U: u16 = 2;
const RELOC_J26: u16 = 3;
const RELOC_DATA32: u16 = 4;
const RELOC_LO16_LOAD: u16 = 5;
const RELOC_LO16_STORE: u16 = 6;

/// Load CCDL binary with relocation to a new base address.
///
/// 1. Parses the CCDL header (original base = 0x80A00000)
/// 2. Computes delta from original base to new base (derived from feature cfg)
/// 3. Loads code/data at the new address
/// 4. Applies relocation table to patch all address references
/// 5. Adjusts entry point, imports, and exports
#[cfg(feature = "reloc")]
pub fn load_ccdl_relocated(app_data: &[u8], reloc_data: &[u8], mem: &mut Memory) -> CcdlBinary {
    let mut ccdl = parse_ccdl(app_data);
    let old_base = ccdl.load_address;

    // New base: 0x80xxxxxx -> 0x08xxxxxx (strip KSEG0, add PSP user-space prefix)
    let new_base = (old_base & 0x1FFF_FFFF) | 0x0800_0000;
    let delta = new_base.wrapping_sub(old_base);

    eprintln!("[RELOC] old_base=0x{old_base:08x} new_base=0x{new_base:08x} delta=0x{delta:08x}");

    // Adjust load address before loading into memory
    ccdl.load_address = new_base;

    // Check for patched RAWD binary
    let rawd_override = std::fs::read("nand/qiye.patched.rawd.bin").ok();
    if let Some(ref rawd_data) = rawd_override {
        let dest = mem.slice_mut(new_base, rawd_data.len());
        dest.copy_from_slice(rawd_data);
        ccdl.data_size = rawd_data.len() as u32;
        ccdl.memory_size = rawd_data.len() as u32;
        eprintln!("[LOADER] using patched RAWD ({} bytes)", rawd_data.len());
    } else {
        // Load code+data at new address
        let rawd_off = {
            let (off, _) = parse_section_hdr(app_data, 0x60, b"RAWD");
            off as usize
        };
        let code = &app_data[rawd_off..rawd_off + ccdl.data_size as usize];
        let dest = mem.slice_mut(new_base, code.len());
        dest.copy_from_slice(code);

        // Zero BSS at new address
        let bss_start = new_base + ccdl.data_size;
        let bss_size = ccdl.memory_size - ccdl.data_size;
        if bss_size > 0 {
            let bss = mem.slice_mut(bss_start, bss_size as usize);
            bss.fill(0);
        }
    }

    // Apply relocations
    apply_relocs(reloc_data, mem, new_base, delta);

    // Adjust CCDL addresses
    ccdl.entry_point = ccdl.entry_point.wrapping_add(delta);
    for imp in &mut ccdl.imports {
        imp.target_vaddr = imp.target_vaddr.wrapping_add(delta);
    }
    for exp in &mut ccdl.exports {
        exp.vaddr = exp.vaddr.wrapping_add(delta);
    }

    eprintln!("[RELOC] relocated entry=0x{:08x} load=0x{:08x}", ccdl.entry_point, ccdl.load_address);
    ccdl
}

/// Parse RLOC binary and apply relocations in guest memory.
///
/// RLOC format: 16-byte header (magic "RLOC", u32 version, u32 count, u32 original_base)
/// followed by count * 8-byte entries (u32 offset, u16 type, u16 reserved).
#[cfg(feature = "reloc")]
fn apply_relocs(reloc_data: &[u8], mem: &mut Memory, new_base: u32, delta: u32) {
    assert_eq!(&reloc_data[0..4], b"RLOC", "Bad reloc magic");
    let count = read_u32_le(reloc_data, 8) as usize;
    let _original_base = read_u32_le(reloc_data, 12);

    let delta_hi = (delta >> 16) as u16;   // 0x8800
    let delta_j = (delta >> 2) & 0x03FF_FFFF; // 0x02000000

    let mut counts = [0u32; 7];

    for i in 0..count {
        let entry_off = 16 + i * 8;
        let offset = read_u32_le(reloc_data, entry_off);
        let rtype = u16::from_le_bytes([reloc_data[entry_off + 4], reloc_data[entry_off + 5]]);

        let addr = new_base + offset;

        match rtype {
            RELOC_HI16 => {
                let insn = mem.read_u32(addr);
                let old_imm = (insn & 0xFFFF) as u16;
                let new_imm = old_imm.wrapping_add(delta_hi);
                let new_insn = (insn & 0xFFFF_0000) | (new_imm as u32);
                mem.write_u32(addr, new_insn);
                counts[0] += 1;
            }
            RELOC_LO16 | RELOC_LO16U | RELOC_LO16_LOAD | RELOC_LO16_STORE => {
                // No change needed: delta & 0xFFFF == 0
                counts[rtype as usize] += 1;
            }
            RELOC_J26 => {
                let insn = mem.read_u32(addr);
                let old_target26 = insn & 0x03FF_FFFF;
                let new_target26 = (old_target26 + delta_j) & 0x03FF_FFFF;
                let new_insn = (insn & 0xFC00_0000) | new_target26;
                mem.write_u32(addr, new_insn);
                counts[3] += 1;
            }
            RELOC_DATA32 => {
                let val = mem.read_u32(addr);
                mem.write_u32(addr, val.wrapping_add(delta));
                counts[4] += 1;
            }
            _ => {
                panic!("[RELOC] unknown reloc type {} at offset 0x{:06x}", rtype, offset);
            }
        }
    }

    eprintln!("[RELOC] applied {} relocs: HI16={} LO16={} J26={} DATA32={}",
        count, counts[0], counts[1] + counts[2] + counts[5] + counts[6], counts[3], counts[4]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn load_test_app() -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("qiye.app");
        std::fs::read(&path).expect("qiye.app not found — place it in project root")
    }

    #[test]
    fn test_parse_qiye() {
        let data = load_test_app();
        let ccdl = parse_ccdl(&data);

        assert_eq!(ccdl.imports.len(), 72);
        assert_eq!(ccdl.imports[0].name, "abort");
        assert_eq!(ccdl.imports[0].target_vaddr, 0x80A0_01D0);

        assert_eq!(ccdl.exports.len(), 2);
        assert_eq!(ccdl.exports[0].name, "getext");
        assert_eq!(ccdl.exports[0].vaddr, 0x80A0_018C);
        assert_eq!(ccdl.exports[1].name, "AppMain");
        assert_eq!(ccdl.exports[1].vaddr, 0x80A0_01A4);

        assert_eq!(ccdl.entry_point, 0x80A0_00A0);
        assert_eq!(ccdl.load_address, 0x80A0_0000);
        assert_eq!(ccdl.memory_size, 0x14_4880);
    }

    #[test]
    #[should_panic(expected = "Bad magic")]
    fn test_bad_magic() {
        parse_ccdl(b"NOTCCDLxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx");
    }
}
