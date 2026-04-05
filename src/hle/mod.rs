mod stdlib;
mod stubs;

use crate::loader::ImportEntry;
use crate::mem::Memory;
use crate::mips::Cpu;

const HEAP_BASE: u32 = 0x9800_0000;
const TRAMPOLINE_BASE: u32 = 0x80C0_0000;

pub struct HleState {
    handlers: Vec<HleHandler>,
    names: Vec<String>,
    pub heap_ptr: u32,
    pub alloc_sizes: std::collections::HashMap<u32, u32>,
    sem_counter: u32,
}

struct HleHandler {
    func: fn(&mut Cpu, &mut Memory, &mut HleState),
}

impl HleState {
    pub fn new(imports: &[ImportEntry], mem: &mut Memory, code_base: u32, code_size: u32) -> Self {
        let mut state = Self {
            handlers: Vec::new(),
            names: Vec::new(),
            heap_ptr: HEAP_BASE,
            alloc_sizes: std::collections::HashMap::new(),
            sem_counter: 0,
        };

        // Build import address → trampoline mapping
        // Write BREAK trampolines at TRAMPOLINE_BASE, 8 bytes each
        // Also write J stubs at the import addresses (like the real OS loader does)
        for (i, imp) in imports.iter().enumerate() {
            let tramp_addr = TRAMPOLINE_BASE + (i as u32) * 8;
            let break_insn: u32 = ((i as u32) << 6) | 0x0D;
            mem.write_u32(tramp_addr, break_insn);    // BREAK i
            mem.write_u32(tramp_addr + 4, 0);          // NOP

            // Write J-to-trampoline at the import address (8-byte stub)
            // This is what the real CCDL OS loader does — CRT code may fall
            // through into import slots expecting jump stubs.
            let j_insn = (0x02u32 << 26) | ((tramp_addr >> 2) & 0x03FF_FFFF);
            mem.write_u32(imp.target_vaddr, j_insn);     // J trampoline
            mem.write_u32(imp.target_vaddr + 4, 0);       // NOP (delay slot)

            let handler = match imp.name.as_str() {
                "malloc" => stdlib::hle_malloc,
                "free" => stdlib::hle_free,
                "realloc" => stdlib::hle_realloc,
                "printf" => stdlib::hle_printf,
                "sprintf" => stdlib::hle_sprintf,
                "fprintf" => stdlib::hle_fprintf,
                "strlen" => stdlib::hle_strlen,
                "strncasecmp" => stdlib::hle_strncasecmp,
                "abort" => stdlib::hle_abort,
                "_lcd_get_frame" => stubs::hle_lcd_get_frame,
                "lcd_get_cframe" => stubs::hle_lcd_get_frame,
                "OSSemCreate" => stubs::hle_ossem_create,
                "OSTimeGet" | "GetTickCount" => stubs::hle_get_tick,
                "vxGoHome" => stubs::hle_exit,
                _ => stubs::hle_default,
            };

            state.handlers.push(HleHandler { func: handler });
            state.names.push(imp.name.clone());
        }

        // Scan code for JAL/J instructions targeting import addresses, redirect to trampolines
        let import_map: std::collections::HashMap<u32, u32> = imports
            .iter()
            .enumerate()
            .map(|(i, imp)| (imp.target_vaddr, TRAMPOLINE_BASE + (i as u32) * 8))
            .collect();

        let code_end = code_base + code_size;
        let mut addr = code_base;
        while addr < code_end {
            let insn = mem.read_u32(addr);
            let opcode = (insn >> 26) & 0x3F;

            // J (0x02) or JAL (0x03)
            if opcode == 0x02 || opcode == 0x03 {
                let target26 = insn & 0x03FF_FFFF;
                let target = (addr & 0xF000_0000) | (target26 << 2);

                if let Some(&tramp) = import_map.get(&target) {
                    let new_target26 = (tramp >> 2) & 0x03FF_FFFF;
                    let new_insn = (opcode << 26) | new_target26;
                    mem.write_u32(addr, new_insn);
                }
            }

            addr += 4;
        }

        eprintln!("[HLE] Patched JAL/J → trampolines for {} imports", imports.len());

        state
    }

    pub fn dispatch(&mut self, code: u32, cpu: &mut Cpu, mem: &mut Memory) {
        let idx = code as usize;
        if idx >= self.handlers.len() {
            panic!("HLE BREAK code {code} out of range (max {})", self.handlers.len());
        }

        let name = &self.names[idx];
        eprintln!(
            "[HLE] {}(0x{:08x}, 0x{:08x}, 0x{:08x}, 0x{:08x})",
            name,
            cpu.gpr(4),
            cpu.gpr(5),
            cpu.gpr(6),
            cpu.gpr(7),
        );

        let func = self.handlers[idx].func;
        func(cpu, mem, self);

        // Return to caller: pc = $ra
        cpu.pc = cpu.gpr(31);
        cpu.next_pc = cpu.pc.wrapping_add(4);
    }

    pub fn name(&self, code: u32) -> &str {
        &self.names[code as usize]
    }
}

pub fn read_vararg(cpu: &Cpu, mem: &Memory, arg_index: usize) -> u32 {
    match arg_index {
        0 => cpu.gpr(4),  // $a0
        1 => cpu.gpr(5),  // $a1
        2 => cpu.gpr(6),  // $a2
        3 => cpu.gpr(7),  // $a3
        _ => {
            // Stack args: $sp + 16 + (arg_index - 4) * 4
            let sp = cpu.gpr(29);
            mem.read_u32(sp + 16 + ((arg_index - 4) as u32) * 4)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_malloc_returns_heap_addr() {
        let mut mem = Memory::new();
        let mut cpu = Cpu::new();
        let mut state = HleState {
            handlers: Vec::new(),
            names: Vec::new(),
            heap_ptr: HEAP_BASE,
            alloc_sizes: std::collections::HashMap::new(),
            sem_counter: 0,
        };

        cpu.set_gpr(4, 100); // malloc(100)
        stdlib::hle_malloc(&mut cpu, &mut mem, &mut state);
        let ptr1 = cpu.gpr(2); // $v0
        assert!(ptr1 >= HEAP_BASE);
        assert!(ptr1 < 0xA000_0000);

        cpu.set_gpr(4, 200); // malloc(200)
        stdlib::hle_malloc(&mut cpu, &mut mem, &mut state);
        let ptr2 = cpu.gpr(2);
        assert!(ptr2 > ptr1); // non-overlapping
        assert!(ptr2 >= ptr1 + 100);
    }

    #[test]
    fn test_strlen() {
        let mut mem = Memory::new();
        let mut cpu = Cpu::new();
        let mut state = HleState {
            handlers: Vec::new(),
            names: Vec::new(),
            heap_ptr: HEAP_BASE,
            alloc_sizes: std::collections::HashMap::new(),
            sem_counter: 0,
        };

        let addr = 0x80B0_0000u32;
        for (i, &b) in b"hello\0".iter().enumerate() {
            mem.write_u8(addr + i as u32, b);
        }
        cpu.set_gpr(4, addr);
        stdlib::hle_strlen(&mut cpu, &mut mem, &mut state);
        assert_eq!(cpu.gpr(2), 5);
    }
}
