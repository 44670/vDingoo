mod loader;
mod mem;
mod mips;

use loader::ImportEntry;
use loader::load_ccdl;
use mem::Memory;
use mips::{Cpu, StepResult};

// ── Constants ──────────────────────────────────────────────────────────────────

const SENTINEL_RA: u32 = 0xDEAD_0000;
const DEFAULT_SP: u32 = 0x8001_0000;
const HEAP_BASE: u32 = 0x9800_0000;
const HLE_BASE: u32 = 0x8000_0000;
const LCD_FRAMEBUF: u32 = 0x80F0_0000;

// ── HLE State ──────────────────────────────────────────────────────────────────

type HleFn = fn(&mut Cpu, &mut Memory, &mut HleState);

pub struct HleState {
    handlers: Vec<HleFn>,
    names: Vec<String>,
    pub heap_ptr: u32,
    pub alloc_sizes: std::collections::HashMap<u32, u32>,
    sem_counter: u32,
}

impl HleState {
    pub fn new(imports: &[ImportEntry], mem: &mut Memory, code_start: u32, code_size: u32) -> Self {
        let mut state = Self {
            handlers: Vec::new(),
            names: Vec::new(),
            heap_ptr: HEAP_BASE,
            alloc_sizes: std::collections::HashMap::new(),
            sem_counter: 0,
        };

        // Build map: import_target_vaddr → (index, name)
        let mut import_map = std::collections::HashMap::new();
        for (i, imp) in imports.iter().enumerate() {
            import_map.insert(imp.target_vaddr, i);

            let handler: HleFn = match imp.name.as_str() {
                "malloc" => hle_malloc,
                "free" => hle_free,
                "realloc" => hle_realloc,
                "printf" => hle_printf,
                "sprintf" => hle_sprintf,
                "fprintf" => hle_fprintf,
                "strlen" => hle_strlen,
                "strncasecmp" => hle_strncasecmp,
                "abort" => hle_abort,
                "_lcd_get_frame" | "lcd_get_cframe" => hle_lcd_get_frame,
                "OSSemCreate" => hle_ossem_create,
                "OSTimeGet" | "GetTickCount" => hle_get_tick,
                "vxGoHome" => hle_exit,
                _ => hle_default,
            };

            state.handlers.push(handler);
            state.names.push(imp.name.clone());
        }

        // Scan code for JAL instructions targeting import addresses, patch them
        let mut patch_count = 0u32;
        let code_end = code_start + code_size;
        let mut addr = code_start;
        while addr < code_end {
            let insn = mem.read_u32(addr);
            let opcode = (insn >> 26) & 0x3F;
            if opcode == 0x03 {
                // JAL instruction: target = (addr & 0xF0000000) | ((insn & 0x03FFFFFF) << 2)
                let target26 = insn & 0x03FF_FFFF;
                let target = (addr & 0xF000_0000) | (target26 << 2);
                if let Some(&idx) = import_map.get(&target) {
                    // Patch JAL target to HLE_BASE + idx*4
                    let hle_addr = HLE_BASE + (idx as u32) * 4;
                    let new_target26 = (hle_addr >> 2) & 0x03FF_FFFF;
                    let new_insn = (0x03u32 << 26) | new_target26;
                    mem.write_u32(addr, new_insn);
                    patch_count += 1;
                }
            }
            addr += 4;
        }

        eprintln!("[HLE] Patched {} JAL call sites for {} imports", patch_count, imports.len());
        state
    }

    pub fn is_hle_addr(&self, pc: u32) -> Option<usize> {
        if pc >= HLE_BASE && pc < HLE_BASE + (self.handlers.len() as u32) * 4 {
            Some(((pc - HLE_BASE) / 4) as usize)
        } else {
            None
        }
    }

    pub fn dispatch(&mut self, idx: usize, cpu: &mut Cpu, mem: &mut Memory) {
        let name = &self.names[idx];
        eprintln!(
            "[HLE] [{:08}] {}(0x{:08x}, 0x{:08x}, 0x{:08x}, 0x{:08x})  $ra=0x{:08x}",
            cpu.insn_count, name,
            cpu.gpr(4), cpu.gpr(5), cpu.gpr(6), cpu.gpr(7), cpu.gpr(31),
        );

        let func = self.handlers[idx];
        func(cpu, mem, self);

        eprintln!(
            "[HLE]   -> $v0=0x{:08x}  $v1=0x{:08x}",
            cpu.gpr(2), cpu.gpr(3),
        );

        cpu.pc = cpu.gpr(31);
        cpu.next_pc = cpu.pc.wrapping_add(4);
    }

    pub fn name(&self, idx: usize) -> &str {
        &self.names[idx]
    }
}

// ── HLE Handlers ───────────────────────────────────────────────────────────────

fn hle_malloc(cpu: &mut Cpu, _mem: &mut Memory, state: &mut HleState) {
    let size = cpu.gpr(4);
    let aligned = (state.heap_ptr + 7) & !7;
    state.alloc_sizes.insert(aligned, size);
    state.heap_ptr = aligned + size;
    cpu.set_gpr(2, aligned);
}

fn hle_free(_cpu: &mut Cpu, _mem: &mut Memory, _state: &mut HleState) {}

fn hle_realloc(cpu: &mut Cpu, mem: &mut Memory, state: &mut HleState) {
    let old_ptr = cpu.gpr(4);
    let new_size = cpu.gpr(5);
    let aligned = (state.heap_ptr + 7) & !7;
    state.heap_ptr = aligned + new_size;
    if old_ptr != 0 {
        if let Some(&old_size) = state.alloc_sizes.get(&old_ptr) {
            let copy_size = old_size.min(new_size) as usize;
            for i in 0..copy_size {
                let b = mem.read_u8(old_ptr + i as u32);
                mem.write_u8(aligned + i as u32, b);
            }
        }
    }
    state.alloc_sizes.insert(aligned, new_size);
    cpu.set_gpr(2, aligned);
}

fn hle_printf(cpu: &mut Cpu, mem: &mut Memory, _state: &mut HleState) {
    let fmt_addr = cpu.gpr(4);
    let output = format_guest_string(mem, cpu, fmt_addr, 1);
    print!("{}", output);
    cpu.set_gpr(2, output.len() as u32);
}

fn hle_sprintf(cpu: &mut Cpu, mem: &mut Memory, _state: &mut HleState) {
    let buf_addr = cpu.gpr(4);
    let fmt_addr = cpu.gpr(5);
    let output = format_guest_string(mem, cpu, fmt_addr, 2);
    for (i, b) in output.bytes().enumerate() {
        mem.write_u8(buf_addr + i as u32, b);
    }
    mem.write_u8(buf_addr + output.len() as u32, 0);
    cpu.set_gpr(2, output.len() as u32);
}

fn hle_fprintf(cpu: &mut Cpu, mem: &mut Memory, _state: &mut HleState) {
    let fmt_addr = cpu.gpr(5);
    if fmt_addr < 0x8000_0000 || fmt_addr >= 0xA000_0000 {
        eprintln!("[HLE] fprintf: bad fmt addr 0x{fmt_addr:08x}, skipping");
        cpu.set_gpr(2, 0);
        return;
    }
    let output = format_guest_string(mem, cpu, fmt_addr, 2);
    print!("{}", output);
    cpu.set_gpr(2, output.len() as u32);
}

fn hle_strlen(cpu: &mut Cpu, mem: &mut Memory, _state: &mut HleState) {
    let s = cpu.gpr(4);
    let len = mem.read_string(s).len();
    cpu.set_gpr(2, len as u32);
}

fn hle_strncasecmp(cpu: &mut Cpu, mem: &mut Memory, _state: &mut HleState) {
    let a_addr = cpu.gpr(4);
    let b_addr = cpu.gpr(5);
    let n = cpu.gpr(6) as usize;
    let a = mem.read_string(a_addr);
    let b = mem.read_string(b_addr);
    let a_bytes: Vec<u8> = a.bytes().take(n).map(|c| c.to_ascii_lowercase()).collect();
    let b_bytes: Vec<u8> = b.bytes().take(n).map(|c| c.to_ascii_lowercase()).collect();
    let result = a_bytes.cmp(&b_bytes) as i32;
    cpu.set_gpr(2, result as u32);
}

fn hle_abort(_cpu: &mut Cpu, _mem: &mut Memory, _state: &mut HleState) {
    eprintln!("[HLE] abort() called");
    std::process::exit(1);
}

fn hle_default(cpu: &mut Cpu, _mem: &mut Memory, _state: &mut HleState) {
    cpu.set_gpr(2, 0);
}

fn hle_lcd_get_frame(cpu: &mut Cpu, _mem: &mut Memory, _state: &mut HleState) {
    cpu.set_gpr(2, LCD_FRAMEBUF);
}

fn hle_ossem_create(cpu: &mut Cpu, _mem: &mut Memory, state: &mut HleState) {
    state.sem_counter += 1;
    cpu.set_gpr(2, 0x80E0_0000 + state.sem_counter * 4);
}

fn hle_get_tick(cpu: &mut Cpu, _mem: &mut Memory, _state: &mut HleState) {
    cpu.set_gpr(2, (cpu.insn_count / 336) as u32);
}

fn hle_exit(_cpu: &mut Cpu, _mem: &mut Memory, _state: &mut HleState) {
    eprintln!("[HLE] vxGoHome() — exiting");
    std::process::exit(0);
}

// ── printf format engine ───────────────────────────────────────────────────────

fn read_vararg(cpu: &Cpu, mem: &Memory, arg_index: usize) -> u32 {
    match arg_index {
        0 => cpu.gpr(4),
        1 => cpu.gpr(5),
        2 => cpu.gpr(6),
        3 => cpu.gpr(7),
        _ => {
            let sp = cpu.gpr(29);
            mem.read_u32(sp + 16 + ((arg_index - 4) as u32) * 4)
        }
    }
}

fn format_guest_string(mem: &Memory, cpu: &Cpu, fmt_addr: u32, first_arg: usize) -> String {
    let fmt = mem.read_string(fmt_addr);
    let mut result = String::new();
    let mut arg_idx = first_arg;
    let mut chars = fmt.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '%' {
            result.push(c);
            continue;
        }
        match chars.peek() {
            Some('%') => { chars.next(); result.push('%'); }
            _ => {
                while matches!(chars.peek(), Some('-' | '+' | ' ' | '0' | '#')) { chars.next(); }
                while matches!(chars.peek(), Some('0'..='9')) { chars.next(); }
                if matches!(chars.peek(), Some('.')) {
                    chars.next();
                    while matches!(chars.peek(), Some('0'..='9')) { chars.next(); }
                }
                while matches!(chars.peek(), Some('l' | 'h' | 'z')) { chars.next(); }

                let val = read_vararg(cpu, mem, arg_idx);
                arg_idx += 1;

                match chars.next() {
                    Some('d' | 'i') => result.push_str(&format!("{}", val as i32)),
                    Some('u') => result.push_str(&format!("{}", val)),
                    Some('x') => result.push_str(&format!("{:x}", val)),
                    Some('X') => result.push_str(&format!("{:X}", val)),
                    Some('o') => result.push_str(&format!("{:o}", val)),
                    Some('c') => result.push(char::from(val as u8)),
                    Some('s') => result.push_str(&mem.read_string(val)),
                    Some('p') => result.push_str(&format!("0x{:08x}", val)),
                    Some(other) => { result.push('%'); result.push(other); }
                    None => result.push('%'),
                }
            }
        }
    }
    result
}

// ── Main ───────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <app-file> [--trace] [--max-insns N]", args[0]);
        std::process::exit(1);
    }

    let app_path = &args[1];
    let trace = args.iter().any(|a| a == "--trace");
    let max_insns: u64 = args
        .iter()
        .position(|a| a == "--max-insns")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(u64::MAX);

    let data = std::fs::read(app_path).unwrap_or_else(|e| {
        eprintln!("Failed to read {app_path}: {e}");
        std::process::exit(1);
    });

    let mut mem = Memory::new();
    let ccdl = load_ccdl(&data, &mut mem);

    eprintln!("Loaded: {} imports, {} exports", ccdl.imports.len(), ccdl.exports.len());
    eprintln!("  load_addr:  0x{:08x}", ccdl.load_address);
    eprintln!("  entry_pt:   0x{:08x}", ccdl.entry_point);
    eprintln!("  mem_size:   0x{:x}", ccdl.memory_size);

    let entry = ccdl.entry_point;

    let code_start = ccdl.load_address;
    let code_end = ccdl.load_address + ccdl.data_size;

    let mut hle = HleState::new(&ccdl.imports, &mut mem, code_start, ccdl.data_size);

    let mut cpu = Cpu::new();
    cpu.pc = entry;
    cpu.next_pc = entry.wrapping_add(4);
    cpu.set_gpr(29, DEFAULT_SP);
    cpu.set_gpr(31, SENTINEL_RA);
    cpu.code_start = code_start;
    cpu.code_end = code_end;

    // Place sentinel $ra on stack so CRT epilogue returns to us.
    mem.write_u32(DEFAULT_SP + 0x10, SENTINEL_RA);

    eprintln!("Entry: 0x{entry:08x}, $sp=0x{DEFAULT_SP:08x}");
    eprintln!("Text:  [0x{code_start:08x}, 0x{code_end:08x})");
    eprintln!("Running...\n");

    let mut pc_history: Vec<u32> = Vec::new();
    let mut last_hle_name = String::new();
    let mut hle_repeat_count = 0u32;

    loop {
        if cpu.insn_count >= max_insns {
            eprintln!("\n[STOP] max instructions reached ({max_insns})");
            break;
        }

        let pre_pc = cpu.pc;
        match cpu.step(&mut mem) {
            StepResult::Ok => {
                if trace {
                    let insn = mem.read_u32(pre_pc);
                    eprintln!("[{:08}] {:08x}: {:08x}", cpu.insn_count, pre_pc, insn);
                }
                pc_history.push(pre_pc);
                if pc_history.len() > 16 {
                    pc_history.remove(0);
                }
            }
            StepResult::OutOfText => {
                let pc = cpu.pc;

                if pc == SENTINEL_RA {
                    eprintln!("\n[STOP] AppMain returned (hit sentinel $ra)");
                    break;
                }

                if let Some(idx) = hle.is_hle_addr(pc) {
                    let name = hle.name(idx).to_string();
                    if trace {
                        let a0 = cpu.gpr[4];
                        let a1 = cpu.gpr[5];
                        let a2 = cpu.gpr[6];
                        let a3 = cpu.gpr[7];
                        eprintln!("[HLE] {}(0x{:08x}, 0x{:08x}, 0x{:08x}, 0x{:08x})", name, a0, a1, a2, a3);
                    }
                    if name == last_hle_name {
                        hle_repeat_count += 1;
                        if hle_repeat_count == 3 && !trace {
                            eprintln!("[HLE] ... {} repeating (suppressing)", name);
                        }
                    } else {
                        if hle_repeat_count > 3 && !trace {
                            eprintln!("[HLE] ... {} repeated {} times total", last_hle_name, hle_repeat_count);
                        }
                        hle_repeat_count = 0;
                        last_hle_name = name;
                    }
                    hle.dispatch(idx, &mut cpu, &mut mem);
                } else {
                    eprintln!("\n[FATAL] PC 0x{pc:08x} outside text segment [0x{code_start:08x}, 0x{code_end:08x})");
                    break;
                }
            }
            StepResult::Break(code) => {
                eprintln!("\n[FATAL] Unexpected BREAK code={code} at PC=0x{:08x}", cpu.pc);
                break;
            }
        }
    }
    if hle_repeat_count > 3 {
        eprintln!("[HLE] ... {} repeated {} times total", last_hle_name, hle_repeat_count);
    }

    eprintln!("\nTotal instructions: {}", cpu.insn_count);
    eprintln!("\n=== CPU State ===");
    eprintln!("  pc  = 0x{:08x}  hi = 0x{:08x}  lo = 0x{:08x}", cpu.pc, cpu.hi, cpu.lo);
    let names = [
        "zero", "at", "v0", "v1", "a0", "a1", "a2", "a3",
        "t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7",
        "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7",
        "t8", "t9", "k0", "k1", "gp", "sp", "fp", "ra",
    ];
    for row in 0..8 {
        let i = row * 4;
        eprintln!(
            "  ${:4}=0x{:08x}  ${:4}=0x{:08x}  ${:4}=0x{:08x}  ${:4}=0x{:08x}",
            names[i], cpu.gpr[i],
            names[i+1], cpu.gpr[i+1],
            names[i+2], cpu.gpr[i+2],
            names[i+3], cpu.gpr[i+3],
        );
    }
    eprintln!("\n=== Key Memory ===");
    eprintln!("  data_80b3a2e0 (fb_ptr) = 0x{:08x}", mem.read_u32(0x80B3_A2E0));
    eprintln!("  stack[$sp+0x10]        = 0x{:08x}", mem.read_u32(cpu.gpr[29].wrapping_add(0x10)));
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod integration {
    use super::*;
    use crate::loader::parse_ccdl;
    use std::path::Path;

    fn load_qiye() -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("qiye.app");
        std::fs::read(&path).expect("qiye.app not found — place it in project root")
    }

    fn setup_qiye() -> (Cpu, Memory, HleState, u32, u32) {
        let data = load_qiye();
        let mut mem = Memory::new();
        let ccdl = load_ccdl(&data, &mut mem);
        let entry = ccdl.entry_point;
        let code_start = ccdl.load_address;
        let code_end = ccdl.load_address + ccdl.data_size;
        let hle = HleState::new(&ccdl.imports, &mut mem, code_start, ccdl.data_size);
        let mut cpu = Cpu::new();
        cpu.pc = entry;
        cpu.next_pc = entry.wrapping_add(4);
        cpu.set_gpr(29, DEFAULT_SP);
        cpu.set_gpr(31, SENTINEL_RA);
        cpu.code_start = code_start;
        cpu.code_end = code_end;
        mem.write_u32(DEFAULT_SP + 0x10, SENTINEL_RA);
        (cpu, mem, hle, code_start, code_end)
    }

    /// Run until HLE call or limit. Returns Some(hle_index) on first HLE call
    /// when handle_hle=false, or None on limit/sentinel.
    fn run_until(cpu: &mut Cpu, mem: &mut Memory, hle: &mut HleState, max_insns: u64, handle_hle: bool) -> Option<usize> {
        let start = cpu.insn_count;
        loop {
            if cpu.insn_count - start >= max_insns { return None; }
            match cpu.step(mem) {
                StepResult::Ok => {}
                StepResult::OutOfText => {
                    let pc = cpu.pc;
                    if pc == SENTINEL_RA { return None; }
                    if let Some(idx) = hle.is_hle_addr(pc) {
                        if handle_hle {
                            hle.dispatch(idx, cpu, mem);
                        } else {
                            return Some(idx);
                        }
                    } else {
                        panic!("PC 0x{pc:08x} outside text [0x{:08x}, 0x{:08x})", cpu.code_start, cpu.code_end);
                    }
                }
                StepResult::Break(code) => panic!("Unexpected BREAK code={code}"),
            }
        }
    }

    #[test]
    fn test_entry_points() {
        let data = load_qiye();
        let ccdl = parse_ccdl(&data);
        assert_eq!(ccdl.entry_point, 0x80A0_00A0);
        assert_eq!(ccdl.load_address, 0x80A0_0000);
        let appmain = ccdl.exports.iter().find(|e| e.name == "AppMain");
        assert!(appmain.is_some());
        assert_eq!(appmain.unwrap().vaddr, 0x80A0_01A4);
    }

    #[test]
    fn test_code_loaded_at_load_address() {
        let data = load_qiye();
        let mut mem = Memory::new();
        let ccdl = load_ccdl(&data, &mut mem);
        let first_insn = mem.read_u32(ccdl.load_address);
        assert_ne!(first_insn, 0);
        // AppMain at 0x80A001A4: should be addiu sp,sp,-24 (0x27bdffe8)
        let appmain_insn = mem.read_u32(0x80A0_01A4);
        assert_eq!(appmain_insn, 0x27BD_FFE8);
    }

    #[test]
    fn test_bss_zeroed_after_load() {
        let data = load_qiye();
        let mut mem = Memory::new();
        let ccdl = load_ccdl(&data, &mut mem);
        let bss_start = ccdl.load_address + ccdl.data_size;
        let bss_end = ccdl.load_address + ccdl.memory_size;
        for addr in [bss_start, bss_start + 0x100, bss_end - 4] {
            assert_eq!(mem.read_u32(addr), 0, "BSS at 0x{addr:08x} should be zero");
        }
    }

    #[test]
    fn test_jal_call_sites_patched() {
        let (_, mem, _, _, _) = setup_qiye();
        // malloc stub is at 0x80A001F8; find a JAL that targets it
        // The CRT entry (0x80A000A0) region has JAL instructions to import stubs
        // Just verify that at least one JAL in the code has been patched to HLE range
        let mut found_patched = false;
        let mut addr = 0x80A0_0000u32;
        while addr < 0x80A0_0800 {
            let insn = mem.read_u32(addr);
            let opcode = (insn >> 26) & 0x3F;
            if opcode == 0x03 {
                let target = (addr & 0xF000_0000) | ((insn & 0x03FF_FFFF) << 2);
                if target >= HLE_BASE && target < HLE_BASE + 0x1000 {
                    found_patched = true;
                    break;
                }
            }
            addr += 4;
        }
        assert!(found_patched, "Should find at least one JAL patched to HLE range");

        // Import stub addresses should NOT be modified (stubs preserved)
        // malloc stub at 0x80A001F8 should still be lw $zero,1($zero) (trap)
        let stub = mem.read_u32(0x80A0_01F8);
        assert_eq!(stub, 0x8C00_0001, "malloc stub at 0x80a001f8 should be preserved");
    }

    #[test]
    fn test_first_hle_call() {
        let (mut cpu, mut mem, mut hle, _, _) = setup_qiye();
        let idx = run_until(&mut cpu, &mut mem, &mut hle, 500_000, false);
        assert!(idx.is_some(), "Should hit an HLE call");
    }

    #[test]
    fn test_crt_entry_runs() {
        let (mut cpu, mut mem, mut hle, _, _) = setup_qiye();
        // CRT entry at 0x80A000A0 should execute and eventually hit HLE or sentinel
        let mut hle_names = Vec::new();
        let start = cpu.insn_count;
        loop {
            if cpu.insn_count - start >= 500_000 { break; }
            match cpu.step(&mut mem) {
                StepResult::Ok => {}
                StepResult::OutOfText => {
                    let pc = cpu.pc;
                    if pc == SENTINEL_RA { break; }
                    if let Some(idx) = hle.is_hle_addr(pc) {
                        hle_names.push(hle.name(idx).to_string());
                        hle.dispatch(idx, &mut cpu, &mut mem);
                    } else {
                        panic!("PC 0x{pc:08x} outside text [0x{:08x}, 0x{:08x})", cpu.code_start, cpu.code_end);
                    }
                }
                StepResult::Break(_) => panic!("Unexpected BREAK"),
            }
        }
        assert!(!hle_names.is_empty(), "CRT should call at least one HLE function");
    }
}
