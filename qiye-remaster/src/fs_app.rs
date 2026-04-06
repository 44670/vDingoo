use std::collections::HashMap;

fn read_u16_le(data: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([data[off], data[off + 1]])
}

fn read_u32_le(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

struct PakEntry {
    path: String,
    start: usize,
    end: usize,
}

pub struct AppFs {
    data: Vec<u8>,
    entries: Vec<PakEntry>,
    index: HashMap<String, usize>,
}

impl AppFs {
    pub fn open(path: &str) -> Self {
        let data = std::fs::read(path).expect("Failed to read .app file");

        assert_eq!(&data[0..4], b"CCDL", "Not a CCDL file");

        // Parse RAWD section header at offset 0x60
        assert_eq!(&data[0x60..0x64], b"RAWD", "Missing RAWD section");
        let rawd_off = read_u32_le(&data, 0x68) as usize;
        let rawd_sz = read_u32_le(&data, 0x6c) as usize;

        // PAK archive starts after RAWD data + 0xFF padding
        let mut pak_offset = rawd_off + rawd_sz;
        while pak_offset < data.len() && data[pak_offset] == 0xFF {
            pak_offset += 1;
        }

        let entry_count = read_u16_le(&data, pak_offset) as usize;
        let entries_start = pak_offset + 2;

        println!(
            "PAK: {entry_count} entries at file offset 0x{pak_offset:x}",
        );

        // Cumulative offsets are relative to pak_offset.
        // File i data spans [cum_offsets[i], cum_offsets[i+1]) relative to pak_offset.
        // Last file ends at the end of the .app file.
        let mut cum_offsets = Vec::with_capacity(entry_count);
        let mut entries = Vec::with_capacity(entry_count);
        let mut index = HashMap::with_capacity(entry_count);

        for i in 0..entry_count {
            let eo = entries_start + i * 68;
            let path_bytes = &data[eo..eo + 64];
            let nul = path_bytes.iter().position(|&b| b == 0).unwrap_or(64);
            let path = String::from_utf8_lossy(&path_bytes[..nul]).into_owned();
            let cum = read_u32_le(&data, eo + 64) as usize;
            cum_offsets.push(cum);
            entries.push(PakEntry {
                path,
                start: 0,
                end: 0,
            });
        }

        let total_pak_size = data.len() - pak_offset;
        for i in 0..entries.len() {
            let abs_start = pak_offset + cum_offsets[i];
            let abs_end = if i + 1 < entries.len() {
                pak_offset + cum_offsets[i + 1]
            } else {
                pak_offset + total_pak_size
            };
            entries[i].start = abs_start;
            entries[i].end = abs_end;
            index.insert(entries[i].path.clone(), i);
        }

        Self {
            data,
            entries,
            index,
        }
    }

    pub fn read(&self, path: &str) -> Option<&[u8]> {
        let &idx = self.index.get(path)?;
        let entry = &self.entries[idx];
        if entry.start >= entry.end || entry.end > self.data.len() {
            return None;
        }
        Some(&self.data[entry.start..entry.end])
    }

    pub fn file_count(&self) -> usize {
        self.entries.len()
    }

    pub fn list_files(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.path.as_str())
    }
}
