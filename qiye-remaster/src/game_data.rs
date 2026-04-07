/// Game state persistence — day progression, inventory, flags.

/// Persistent game state that survives across scenes.
pub struct GameData {
    pub day: i32,            // current day (1-13)
    pub floor: i32,          // current floor/episode
    pub flags: [bool; 256],  // game progress flags (set/get/clear by script)
    pub items: [bool; 64],   // collected items
    pub item_count: i32,     // total items collected
    pub player_hp: i32,      // persistent HP across scenes
    pub player_max_hp: i32,
}

impl GameData {
    pub fn new() -> Self {
        Self {
            day: 1,
            floor: 0,
            flags: [false; 256],
            items: [false; 64],
            item_count: 0,
            player_hp: 50,
            player_max_hp: 100,
        }
    }

    pub fn set_flag(&mut self, idx: i32) {
        if idx >= 0 && (idx as usize) < self.flags.len() {
            self.flags[idx as usize] = true;
        }
    }

    pub fn get_flag(&self, idx: i32) -> bool {
        if idx >= 0 && (idx as usize) < self.flags.len() {
            self.flags[idx as usize]
        } else {
            false
        }
    }

    pub fn clear_flag(&mut self, idx: i32) {
        if idx >= 0 && (idx as usize) < self.flags.len() {
            self.flags[idx as usize] = false;
        }
    }

    pub fn collect_item(&mut self, idx: i32) -> bool {
        if idx >= 0 && (idx as usize) < self.items.len() && !self.items[idx as usize] {
            self.items[idx as usize] = true;
            self.item_count += 1;
            true
        } else {
            false
        }
    }

    pub fn has_item(&self, idx: i32) -> bool {
        if idx >= 0 && (idx as usize) < self.items.len() {
            self.items[idx as usize]
        } else {
            false
        }
    }

    pub fn advance_day(&mut self) -> i32 {
        self.day += 1;
        if self.day > 13 {
            self.day = 13;
        }
        println!("GameData: advancing to day {}", self.day);
        self.day
    }

    /// Serialize to bytes for saving.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(128);

        // Signature
        data.extend_from_slice(b"7days-remaster\0\0");
        // Version
        data.extend_from_slice(&1u32.to_le_bytes());
        // Day/floor
        data.extend_from_slice(&self.day.to_le_bytes());
        data.extend_from_slice(&self.floor.to_le_bytes());
        // HP
        data.extend_from_slice(&self.player_hp.to_le_bytes());
        data.extend_from_slice(&self.player_max_hp.to_le_bytes());
        // Item count
        data.extend_from_slice(&self.item_count.to_le_bytes());

        // Flags (32 bytes = 256 bits)
        for chunk in self.flags.chunks(8) {
            let mut byte = 0u8;
            for (i, &flag) in chunk.iter().enumerate() {
                if flag {
                    byte |= 1 << i;
                }
            }
            data.push(byte);
        }

        // Items (8 bytes = 64 bits)
        for chunk in self.items.chunks(8) {
            let mut byte = 0u8;
            for (i, &item) in chunk.iter().enumerate() {
                if item {
                    byte |= 1 << i;
                }
            }
            data.push(byte);
        }

        data
    }

    /// Deserialize from bytes.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 80 {
            return None;
        }

        // Check signature
        if &data[..14] != b"7days-remaster" {
            return None;
        }

        let off = 20; // skip signature (16) + version (4)
        let day = i32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]);
        let floor = i32::from_le_bytes([data[off+4], data[off+5], data[off+6], data[off+7]]);
        let player_hp = i32::from_le_bytes([data[off+8], data[off+9], data[off+10], data[off+11]]);
        let player_max_hp = i32::from_le_bytes([data[off+12], data[off+13], data[off+14], data[off+15]]);
        let item_count = i32::from_le_bytes([data[off+16], data[off+17], data[off+18], data[off+19]]);

        let flag_off = off + 20;
        let mut flags = [false; 256];
        for i in 0..32 {
            if flag_off + i >= data.len() { break; }
            let byte = data[flag_off + i];
            for bit in 0..8 {
                flags[i * 8 + bit] = byte & (1 << bit) != 0;
            }
        }

        let item_off = flag_off + 32;
        let mut items = [false; 64];
        for i in 0..8 {
            if item_off + i >= data.len() { break; }
            let byte = data[item_off + i];
            for bit in 0..8 {
                items[i * 8 + bit] = byte & (1 << bit) != 0;
            }
        }

        Some(Self {
            day,
            floor,
            flags,
            items,
            item_count,
            player_hp,
            player_max_hp,
        })
    }

    /// Save to file.
    pub fn save(&self, path: &str) -> bool {
        let data = self.to_bytes();
        match std::fs::write(path, &data) {
            Ok(_) => {
                println!("Saved game to {path} ({} bytes)", data.len());
                true
            }
            Err(e) => {
                eprintln!("Failed to save: {e}");
                false
            }
        }
    }

    /// Load from file.
    pub fn load(path: &str) -> Option<Self> {
        match std::fs::read(path) {
            Ok(data) => {
                let gd = Self::from_bytes(&data)?;
                println!("Loaded save from {path}: day {}, HP {}/{}", gd.day, gd.player_hp, gd.player_max_hp);
                Some(gd)
            }
            Err(_) => None,
        }
    }
}
