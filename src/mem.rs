const BASE: u32 = 0x8000_0000;
const SIZE: usize = 512 * 1024 * 1024; // 512 MB

pub struct Memory {
    data: Vec<u8>,
}

impl Memory {
    pub fn new() -> Self {
        Self {
            data: vec![0u8; SIZE],
        }
    }

    fn offset(&self, addr: u32) -> usize {
        let off = addr.wrapping_sub(BASE) as usize;
        assert!(off < SIZE, "OOB memory access: 0x{addr:08x}");
        off
    }

    pub fn read_u8(&self, addr: u32) -> u8 {
        self.data[self.offset(addr)]
    }

    pub fn read_u16(&self, addr: u32) -> u16 {
        let off = self.offset(addr);
        u16::from_le_bytes([self.data[off], self.data[off + 1]])
    }

    pub fn read_u32(&self, addr: u32) -> u32 {
        let off = self.offset(addr);
        u32::from_le_bytes([
            self.data[off],
            self.data[off + 1],
            self.data[off + 2],
            self.data[off + 3],
        ])
    }

    pub fn write_u8(&mut self, addr: u32, val: u8) {
        let off = self.offset(addr);
        self.data[off] = val;
    }

    pub fn write_u16(&mut self, addr: u32, val: u16) {
        let off = self.offset(addr);
        let bytes = val.to_le_bytes();
        self.data[off] = bytes[0];
        self.data[off + 1] = bytes[1];
    }

    pub fn write_u32(&mut self, addr: u32, val: u32) {
        let off = self.offset(addr);
        let bytes = val.to_le_bytes();
        self.data[off] = bytes[0];
        self.data[off + 1] = bytes[1];
        self.data[off + 2] = bytes[2];
        self.data[off + 3] = bytes[3];
    }

    pub fn read_string(&self, addr: u32) -> String {
        let mut off = self.offset(addr);
        let mut bytes = Vec::new();
        while off < SIZE && self.data[off] != 0 {
            bytes.push(self.data[off]);
            off += 1;
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }

    pub fn slice(&self, addr: u32, len: usize) -> &[u8] {
        let off = self.offset(addr);
        &self.data[off..off + len]
    }

    pub fn slice_mut(&mut self, addr: u32, len: usize) -> &mut [u8] {
        let off = self.offset(addr);
        &mut self.data[off..off + len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_write_u8() {
        let mut mem = Memory::new();
        mem.write_u8(0x80A0_0000, 0x42);
        assert_eq!(mem.read_u8(0x80A0_0000), 0x42);
    }

    #[test]
    fn test_read_write_u16() {
        let mut mem = Memory::new();
        mem.write_u16(0x80A0_0000, 0xBEEF);
        assert_eq!(mem.read_u16(0x80A0_0000), 0xBEEF);
    }

    #[test]
    fn test_read_write_u32() {
        let mut mem = Memory::new();
        mem.write_u32(0x80A0_0000, 0xDEAD_BEEF);
        assert_eq!(mem.read_u32(0x80A0_0000), 0xDEAD_BEEF);
    }

    #[test]
    fn test_endianness() {
        let mut mem = Memory::new();
        mem.write_u32(0x80A0_0000, 0xDEAD_BEEF);
        assert_eq!(mem.read_u8(0x80A0_0000), 0xEF); // LE: LSB first
        assert_eq!(mem.read_u8(0x80A0_0001), 0xBE);
        assert_eq!(mem.read_u8(0x80A0_0002), 0xAD);
        assert_eq!(mem.read_u8(0x80A0_0003), 0xDE);
    }

    #[test]
    fn test_read_string() {
        let mut mem = Memory::new();
        let s = b"hello\0";
        for (i, &b) in s.iter().enumerate() {
            mem.write_u8(0x80A0_0000 + i as u32, b);
        }
        assert_eq!(mem.read_string(0x80A0_0000), "hello");
    }

    #[test]
    #[should_panic(expected = "OOB")]
    fn test_oob_below() {
        let mem = Memory::new();
        mem.read_u8(0x7FFF_FFFF);
    }

    #[test]
    #[should_panic(expected = "OOB")]
    fn test_oob_above() {
        let mem = Memory::new();
        mem.read_u8(0xA000_0000);
    }
}
