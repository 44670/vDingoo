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
    let ccdl = parse_ccdl(data);

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

    ccdl
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
