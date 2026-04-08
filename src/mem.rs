#[cfg(not(feature = "reloc"))]
pub(crate) const BASE: u32 = 0x8000_0000;
#[cfg(feature = "reloc")]
pub(crate) const BASE: u32 = 0x0800_0000;

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
        // SAFETY: offset() guarantees off < SIZE, and SIZE has room for +1
        unsafe {
            let ptr = self.data.as_ptr().add(off) as *const u16;
            ptr.read_unaligned()
        }
    }

    pub fn read_u32(&self, addr: u32) -> u32 {
        let off = self.offset(addr);
        // SAFETY: offset() guarantees off < SIZE, and SIZE has room for +3
        unsafe {
            let ptr = self.data.as_ptr().add(off) as *const u32;
            ptr.read_unaligned()
        }
    }

    pub fn write_u8(&mut self, addr: u32, val: u8) {
        let off = self.offset(addr);
        self.data[off] = val;
    }

    pub fn write_u16(&mut self, addr: u32, val: u16) {
        let off = self.offset(addr);
        unsafe {
            let ptr = self.data.as_mut_ptr().add(off) as *mut u16;
            ptr.write_unaligned(val);
        }
    }

    pub fn write_u32(&mut self, addr: u32, val: u32) {
        let off = self.offset(addr);
        unsafe {
            let ptr = self.data.as_mut_ptr().add(off) as *mut u32;
            ptr.write_unaligned(val);
        }
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

    const TEST_ADDR: u32 = BASE + 0x00A0_0000;

    #[test]
    fn test_read_write_u8() {
        let mut mem = Memory::new();
        mem.write_u8(TEST_ADDR, 0x42);
        assert_eq!(mem.read_u8(TEST_ADDR), 0x42);
    }

    #[test]
    fn test_read_write_u16() {
        let mut mem = Memory::new();
        mem.write_u16(TEST_ADDR, 0xBEEF);
        assert_eq!(mem.read_u16(TEST_ADDR), 0xBEEF);
    }

    #[test]
    fn test_read_write_u32() {
        let mut mem = Memory::new();
        mem.write_u32(TEST_ADDR, 0xDEAD_BEEF);
        assert_eq!(mem.read_u32(TEST_ADDR), 0xDEAD_BEEF);
    }

    #[test]
    fn test_endianness() {
        let mut mem = Memory::new();
        mem.write_u32(TEST_ADDR, 0xDEAD_BEEF);
        assert_eq!(mem.read_u8(TEST_ADDR), 0xEF); // LE: LSB first
        assert_eq!(mem.read_u8(TEST_ADDR + 1), 0xBE);
        assert_eq!(mem.read_u8(TEST_ADDR + 2), 0xAD);
        assert_eq!(mem.read_u8(TEST_ADDR + 3), 0xDE);
    }

    #[test]
    fn test_read_string() {
        let mut mem = Memory::new();
        let s = b"hello\0";
        for (i, &b) in s.iter().enumerate() {
            mem.write_u8(TEST_ADDR + i as u32, b);
        }
        assert_eq!(mem.read_string(TEST_ADDR), "hello");
    }

    #[test]
    #[should_panic(expected = "OOB")]
    fn test_oob_below() {
        let mem = Memory::new();
        mem.read_u8(BASE.wrapping_sub(1));
    }

    #[test]
    #[should_panic(expected = "OOB")]
    fn test_oob_above() {
        let mem = Memory::new();
        mem.read_u8(BASE + SIZE as u32);
    }
}
