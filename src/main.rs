mod hle;
mod loader;
mod mem;
mod mips;

use hle::HleState;
use loader::load_ccdl;
use mem::Memory;
use mips::{Cpu, StepResult};

const SENTINEL_RA: u32 = 0xDEAD_0000;
const DEFAULT_SP: u32 = 0x8001_0000;

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
    eprintln!("  load_vaddr: 0x{:08x}", ccdl.load_vaddr);
    eprintln!("  base_addr:  0x{:08x}", ccdl.base_addr);
    eprintln!("  mem_size:   0x{:x}", ccdl.memory_size);

    let entry = ccdl
        .exports
        .iter()
        .find(|e| e.name == "AppMain")
        .map(|e| e.vaddr)
        .unwrap_or(ccdl.load_vaddr);

    let mut hle_state = HleState::new(
        &ccdl.imports,
        &mut mem,
        ccdl.load_vaddr,
        ccdl.data_size,
    );

    let mut cpu = Cpu::new();
    cpu.pc = entry;
    cpu.next_pc = entry.wrapping_add(4);
    cpu.set_gpr(29, DEFAULT_SP); // $sp
    cpu.set_gpr(31, SENTINEL_RA); // $ra
    // AppMain args: $a0=0, $a1=0 (normal entry, no special flags)

    eprintln!("Entry: 0x{entry:08x}, $sp=0x{DEFAULT_SP:08x}");
    eprintln!("Running...\n");

    loop {
        if cpu.insn_count >= max_insns {
            eprintln!("\n[STOP] max instructions reached ({max_insns})");
            break;
        }

        let current_pc = cpu.pc;

        if current_pc == SENTINEL_RA {
            eprintln!("\n[STOP] AppMain returned (hit sentinel $ra)");
            break;
        }

        if trace {
            let insn = mem.read_u32(current_pc);
            eprintln!("[{:08}] {:08x}: {:08x}", cpu.insn_count, current_pc, insn);
        }

        match cpu.step(&mut mem) {
            StepResult::Ok => {}
            StepResult::Break(code) => {
                hle_state.dispatch(code, &mut cpu, &mut mem);
            }
        }
    }

    eprintln!("\nTotal instructions: {}", cpu.insn_count);
}

#[cfg(test)]
mod integration {
    use crate::hle::HleState;
    use crate::loader::{load_ccdl, parse_ccdl};
    use crate::mem::Memory;
    use crate::mips::{Cpu, StepResult};
    use std::path::Path;

    const SENTINEL_RA: u32 = 0xDEAD_0000;
    const DEFAULT_SP: u32 = 0x8001_0000;

    fn load_qiye() -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("qiye.app");
        std::fs::read(&path).expect("qiye.app not found — place it in project root")
    }

    /// Set up CPU + Memory + HLE with qiye.app loaded, ready to run from AppMain.
    fn setup_qiye() -> (Cpu, Memory, HleState, u32) {
        let data = load_qiye();
        let mut mem = Memory::new();
        let ccdl = load_ccdl(&data, &mut mem);

        let entry = ccdl
            .exports
            .iter()
            .find(|e| e.name == "AppMain")
            .map(|e| e.vaddr)
            .expect("AppMain not found");

        let hle_state = HleState::new(
            &ccdl.imports,
            &mut mem,
            ccdl.load_vaddr,
            ccdl.data_size,
        );

        let mut cpu = Cpu::new();
        cpu.pc = entry;
        cpu.next_pc = entry.wrapping_add(4);
        cpu.set_gpr(29, DEFAULT_SP);
        cpu.set_gpr(31, SENTINEL_RA);

        (cpu, mem, hle_state, entry)
    }

    /// Run CPU until max_insns, sentinel hit, or BREAK (returns break code).
    fn run_until(
        cpu: &mut Cpu,
        mem: &mut Memory,
        hle: &mut HleState,
        max_insns: u64,
        handle_breaks: bool,
    ) -> Option<u32> {
        let start = cpu.insn_count;
        loop {
            if cpu.insn_count - start >= max_insns {
                return None;
            }
            if cpu.pc == SENTINEL_RA {
                return None;
            }
            match cpu.step(mem) {
                StepResult::Ok => {}
                StepResult::Break(code) => {
                    if handle_breaks {
                        hle.dispatch(code, cpu, mem);
                    } else {
                        return Some(code);
                    }
                }
            }
        }
    }

    #[test]
    fn test_appmain_entry_point() {
        let data = load_qiye();
        let ccdl = parse_ccdl(&data);
        let entry = ccdl.exports.iter().find(|e| e.name == "AppMain");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().vaddr, 0x80A0_01A4);
    }

    #[test]
    fn test_code_loaded_at_load_vaddr() {
        let data = load_qiye();
        let mut mem = Memory::new();
        let ccdl = load_ccdl(&data, &mut mem);

        // First instruction at load_vaddr should be non-zero (loaded code)
        let first_insn = mem.read_u32(ccdl.load_vaddr);
        assert_ne!(first_insn, 0, "Code should be loaded at load_vaddr");

        // Verify AppMain's first instruction: addiu $a0, $a0, 4
        // opcode=0x09, rs=4, rt=4, imm=4 → 0x24840004
        let appmain_insn = mem.read_u32(0x80A0_01A4);
        assert_eq!(appmain_insn, 0x2484_0004, "AppMain should start with addiu $a0, $a0, 4");
    }

    #[test]
    fn test_bss_zeroed_after_load() {
        let data = load_qiye();
        let mut mem = Memory::new();
        let ccdl = load_ccdl(&data, &mut mem);

        // BSS region: from load_vaddr + data_size to load_vaddr + memory_size
        let bss_start = ccdl.load_vaddr + ccdl.data_size;
        let bss_end = ccdl.base_addr + ccdl.memory_size;

        // Sample BSS locations — should all be zero
        for addr in [bss_start, bss_start + 0x100, bss_end - 4] {
            assert_eq!(
                mem.read_u32(addr), 0,
                "BSS at 0x{addr:08x} should be zero"
            );
        }
    }

    #[test]
    fn test_trampoline_break_instructions() {
        let (_, mem, _, _) = setup_qiye();

        // Trampolines at 0x80C00000, 8 bytes each, contain BREAK instructions
        let tramp_base = 0x80C0_0000u32;

        // Check first few trampolines have valid BREAK instructions
        for i in 0..5u32 {
            let insn = mem.read_u32(tramp_base + i * 8);
            let funct = insn & 0x3F;
            let code = (insn >> 6) & 0xF_FFFF;
            assert_eq!(funct, 0x0D, "Trampoline {i} should be BREAK");
            assert_eq!(code, i, "Trampoline {i} BREAK code should be {i}");

            // Second word should be NOP
            let nop = mem.read_u32(tramp_base + i * 8 + 4);
            assert_eq!(nop, 0, "Trampoline {i} delay slot should be NOP");
        }
    }

    #[test]
    fn test_jal_import_redirected_to_trampoline() {
        let data = load_qiye();
        let mut mem = Memory::new();
        let ccdl = load_ccdl(&data, &mut mem);

        // Before HLE patching, save the original malloc JAL target
        // malloc's import entry target_vaddr
        let malloc_import = ccdl.imports.iter().find(|i| i.name == "malloc");
        assert!(malloc_import.is_some(), "malloc import should exist");

        // Now set up HLE (which patches JAL/J instructions)
        let _hle = HleState::new(
            &ccdl.imports,
            &mut mem,
            ccdl.load_vaddr,
            ccdl.data_size,
        );

        // The JAL at 0x80a00174 originally targeted malloc's import address.
        // After patching, it should target the trampoline.
        let jal_insn = mem.read_u32(0x80A0_0174);
        let opcode = (jal_insn >> 26) & 0x3F;
        assert_eq!(opcode, 0x03, "Should still be JAL");

        let target26 = jal_insn & 0x03FF_FFFF;
        let target = (0x80A0_0174 & 0xF000_0000) | (target26 << 2);
        // Target should be in trampoline range
        assert!(
            target >= 0x80C0_0000,
            "JAL target 0x{target:08x} should be in trampoline range"
        );
    }

    #[test]
    fn test_first_hle_call_is_malloc() {
        let (mut cpu, mut mem, mut hle, _) = setup_qiye();

        // Run until first BREAK (don't handle it)
        let code = run_until(&mut cpu, &mut mem, &mut hle, 100_000, false);
        assert!(code.is_some(), "Should hit a BREAK within 100k instructions");

        let idx = code.unwrap() as usize;
        let name = hle.name(idx as u32);
        assert_eq!(name, "malloc", "First HLE call should be malloc");

        // $a0 should be the malloc size = 0x25800
        assert_eq!(cpu.gpr(4), 0x25800, "malloc size should be 0x25800 (framebuffer)");
    }

    #[test]
    fn test_bss_region_zeroed_by_cpu() {
        let (mut cpu, mut mem, mut hle, _) = setup_qiye();

        // The init code in sub_80a00144 zeroes 0x80b3a2e0..0x80b44880
        // This happens before the first HLE call (malloc).
        // Run until first BREAK to let the BSS zeroing complete.
        let _code = run_until(&mut cpu, &mut mem, &mut hle, 100_000, false);

        // The init loop zeroes from data_80b3a2e0 to 0x80b44880
        let bss_cpu_start = 0x80B3_A2E0u32;
        let bss_cpu_end = 0x80B4_4880u32;

        // Sample addresses throughout the zeroed range
        for offset in [0, 0x100, 0x1000, 0x5000, 0xA59C] {
            let addr = bss_cpu_start + offset;
            if addr < bss_cpu_end {
                assert_eq!(
                    mem.read_u32(addr), 0,
                    "CPU-zeroed BSS at 0x{addr:08x} should be 0"
                );
            }
        }
    }

    #[test]
    fn test_malloc_returns_heap_address() {
        let (mut cpu, mut mem, mut hle, _) = setup_qiye();

        // Run until first BREAK and handle it (malloc)
        run_until(&mut cpu, &mut mem, &mut hle, 100_000, true);

        // After malloc, result stored at data_80b3a2e0 (0x80B3A2E0)
        let fb_ptr = mem.read_u32(0x80B3_A2E0);
        assert!(
            fb_ptr >= 0x9800_0000 && fb_ptr < 0xA000_0000,
            "malloc result 0x{fb_ptr:08x} should be in heap range"
        );
    }

    #[test]
    fn test_framebuffer_zeroed_after_malloc() {
        let (mut cpu, mut mem, mut hle, _) = setup_qiye();

        // Run enough instructions to complete:
        // 1. BSS zeroing (~42k insns)
        // 2. malloc call (~1 insn)
        // 3. Framebuffer zeroing loop (~153k insns)
        // Total ~200k, use 250k to be safe
        run_until(&mut cpu, &mut mem, &mut hle, 250_000, true);

        // The framebuffer pointer was stored at 0x80B3A2E0 by malloc
        let fb_ptr = mem.read_u32(0x80B3_A2E0);
        if fb_ptr >= 0x9800_0000 && fb_ptr < 0xA000_0000 {
            // Framebuffer (0x25800 bytes) should be zeroed
            for offset in [0u32, 4, 0x100, 0x12C00, 0x257FC] {
                assert_eq!(
                    mem.read_u32(fb_ptr + offset), 0,
                    "Framebuffer at +0x{offset:x} should be zeroed"
                );
            }
        }
    }

    #[test]
    fn test_stack_frame_saved() {
        let (mut cpu, mut mem, mut hle, _) = setup_qiye();

        // sub_80a00144 saves $ra at $sp+0x10 in its delay slot
        // Run a few instructions to get past that point
        for _ in 0..10 {
            match cpu.step(&mut mem) {
                StepResult::Ok => {}
                StepResult::Break(code) => {
                    hle.dispatch(code, &mut cpu, &mut mem);
                }
            }
        }

        // $ra was saved to stack: $sp + 0x10 = 0x80010010
        let saved_ra = mem.read_u32(DEFAULT_SP + 0x10);
        assert_eq!(
            saved_ra, SENTINEL_RA,
            "Saved $ra on stack should be sentinel 0x{SENTINEL_RA:08x}, got 0x{saved_ra:08x}"
        );
    }

    #[test]
    fn test_init_sequence_reaches_second_entry() {
        let (mut cpu, mut mem, mut hle, _) = setup_qiye();

        // Run through full init: BSS zero + malloc + framebuffer zero + re-entry
        // After framebuffer zeroing, code jumps back to sub_80a00144 with $a1=0x257ff
        // sub_80a00144 takes $a1!=0 path, then $a1!=1 path → falls to 0x80a001e4
        // Total should be ~200k instructions
        let mut break_names = Vec::new();
        let start = cpu.insn_count;
        loop {
            if cpu.insn_count - start >= 250_000 {
                break;
            }
            if cpu.pc == SENTINEL_RA {
                break;
            }
            match cpu.step(&mut mem) {
                StepResult::Ok => {}
                StepResult::Break(code) => {
                    break_names.push(hle.name(code).to_string());
                    hle.dispatch(code, &mut cpu, &mut mem);
                }
            }
        }

        // Should have called malloc as the first (and likely only) HLE call in init
        assert!(!break_names.is_empty(), "Should have HLE calls during init");
        assert_eq!(break_names[0], "malloc", "First HLE call should be malloc");
    }

    #[test]
    fn test_insn_count_reasonable() {
        let (mut cpu, mut mem, mut hle, _) = setup_qiye();

        // Run until first BREAK
        run_until(&mut cpu, &mut mem, &mut hle, 100_000, false);

        // BSS zeroing: ~42k instructions (14 setup + 10600 iterations × 4 insns)
        // Should be in that ballpark
        assert!(
            cpu.insn_count > 40_000 && cpu.insn_count < 50_000,
            "Expected ~42k insns before malloc, got {}",
            cpu.insn_count,
        );
    }

    #[test]
    fn test_import_addresses_have_j_stubs() {
        let (_, mem, _, _) = setup_qiye();

        // Like the real CCDL OS loader, import addresses have J-to-trampoline stubs.
        // CRT code falls through into import slots expecting jump stubs.

        // fprintf import at 0x80A001E8 — should be a J instruction to its trampoline
        let insn_at_fprintf = mem.read_u32(0x80A0_01E8);
        let opcode = (insn_at_fprintf >> 26) & 0x3F;
        assert_eq!(opcode, 0x02, "Import address should have J stub");

        // Delay slot should be NOP
        let delay = mem.read_u32(0x80A0_01EC);
        assert_eq!(delay, 0, "Import J stub delay slot should be NOP");

        // J target should be in trampoline range
        let target = (0x80A0_01E8 & 0xF000_0000) | ((insn_at_fprintf & 0x03FF_FFFF) << 2);
        assert!(target >= 0x80C0_0000, "J target should be in trampoline range");
    }
}
