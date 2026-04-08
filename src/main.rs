mod fs;
mod hle;
mod loader;
mod mem;
mod mips;

use fs::GuestFs;
use hle::{dispatch, EmuCtx, HleState, SdlState};
use loader::load_ccdl;
use mem::Memory;
use mips::{Cpu, StepResult};

use std::path::PathBuf;

// ── Constants ────────────────────────────────────────────────────────────────

const SENTINEL_RA: u32 = 0xDEAD_0000;
const DEFAULT_SP: u32 = 0x8001_0000;
const SCRATCH_ADDR: u32 = 0x8001_1000; // scratch area for writing strings etc.

// ── Main ────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <app-file> [--trace] [--max-insns N]", args[0]);
        std::process::exit(1);
    }

    let app_path = &args[1];
    if !app_path.starts_with("nand/") {
        panic!("app path must begin with \"nand/\": {app_path}");
    }
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

    // nand/ directory: all guest file I/O is confined here
    let base_dir = PathBuf::from("nand");
    std::fs::create_dir_all(&base_dir).expect("Failed to create nand/ directory");
    let app_abs = std::fs::canonicalize(app_path).unwrap_or_else(|_| PathBuf::from(app_path));

    let mut mem = Memory::new();
    let ccdl = load_ccdl(&data, &mut mem);

    eprintln!("Loaded: {} imports, {} exports", ccdl.imports.len(), ccdl.exports.len());
    eprintln!("  load_addr:  0x{:08x}", ccdl.load_address);
    eprintln!("  entry_pt:   0x{:08x}", ccdl.entry_point);
    eprintln!("  base_dir:   {}", base_dir.display());

    let code_start = ccdl.load_address;
    let code_end = ccdl.load_address + ccdl.data_size;

    let mut hle_state = HleState::new(&ccdl.imports, &mut mem, code_start, ccdl.data_size);
    let mut guest_fs = GuestFs::new(base_dir.clone());

    // Init SDL2
    let sdl_context = sdl2::init().expect("Failed to init SDL2");
    let video = sdl_context.video().expect("Failed to init SDL2 video");
    let audio = sdl_context.audio().expect("Failed to init SDL2 audio");
    let window = video
        .window("vDingoo — qiye", 320 * 2, 240 * 2)
        .position_centered()
        .build()
        .expect("Failed to create window");
    let canvas = window.into_canvas().accelerated().build().expect("Failed to create canvas");
    let event_pump = sdl_context.event_pump().expect("Failed to get event pump");

    let creator = canvas.texture_creator();
    let texture = creator
        .create_texture_streaming(sdl2::pixels::PixelFormatEnum::RGB565, 320, 240)
        .expect("Failed to create texture");
    // Safety: creator is kept alive (leaked) for the program's lifetime.
    // The texture references the creator's GPU context, so we must ensure it outlives the texture.
    let texture: sdl2::render::Texture<'static> = unsafe { std::mem::transmute(texture) };
    std::mem::forget(creator);

    let mut sdl_state = SdlState { canvas, event_pump, texture, audio, audio_queue: None };

    let mut cpu = Cpu::new();
    cpu.pc = ccdl.entry_point;
    cpu.next_pc = cpu.pc.wrapping_add(4);
    cpu.set_gpr(29, DEFAULT_SP);
    cpu.set_gpr(31, SENTINEL_RA);
    cpu.code_start = code_start;
    cpu.code_end = code_end;

    mem.write_u32(DEFAULT_SP + 0x10, SENTINEL_RA);

    eprintln!("=== Phase 1: _start(0, 0) ===");

    // Phase 1: Run _start(0, 0) — init
    {
        let mut ctx = EmuCtx {
            cpu: &mut cpu,
            mem: &mut mem,
            hle: &mut hle_state,
            fs: &mut guest_fs,
            sdl: &mut sdl_state,
        };
        run_until_sentinel(&mut ctx, trace, max_insns);
    }

    // Look up AppMain from exports
    let appmain_addr = ccdl.exports.iter()
        .find(|e| e.name == "AppMain")
        .unwrap_or_else(|| {
            eprintln!("No AppMain export found");
            std::process::exit(1);
        })
        .vaddr;

    eprintln!("=== Phase 2: AppMain(path) @ 0x{appmain_addr:08x} ===");

    // Write wide-string app path to scratch memory
    // The game extracts the directory part (before last '\') as its working dir.
    // We pass "\qiye.app" so the extracted dir is "" — all file paths resolve to nand/.
    let app_filename = app_abs.file_name().unwrap().to_string_lossy();
    let app_wpath = format!("\\{}", app_filename);
    write_wstring(&mut mem, SCRATCH_ADDR, &app_wpath);

    // Reset CPU for AppMain call
    cpu.pc = appmain_addr;
    cpu.next_pc = appmain_addr.wrapping_add(4);
    cpu.set_gpr(4, SCRATCH_ADDR); // $a0 = path
    cpu.set_gpr(5, 0);            // $a1
    cpu.set_gpr(6, 0);            // $a2
    cpu.set_gpr(29, DEFAULT_SP);
    cpu.set_gpr(31, SENTINEL_RA);
    mem.write_u32(DEFAULT_SP + 0x10, SENTINEL_RA);

    // Register AppMain as Task 0
    hle_state.init_main_task(&cpu);

    // Phase 2: Run AppMain — game loop
    {
        let mut ctx = EmuCtx {
            cpu: &mut cpu,
            mem: &mut mem,
            hle: &mut hle_state,
            fs: &mut guest_fs,
            sdl: &mut sdl_state,
        };
        run_until_sentinel(&mut ctx, trace, max_insns);
    }

    eprintln!("\nTotal instructions: {}", cpu.insn_count);
}

fn run_until_sentinel(ctx: &mut EmuCtx, trace: bool, max_insns: u64) {
    loop {
        if ctx.cpu.insn_count >= max_insns {
            eprintln!("\n[STOP] max instructions reached ({max_insns}) at PC=0x{:08x}", ctx.cpu.pc);
            break;
        }
        if ctx.hle.quit {
            eprintln!("\n[STOP] quit requested");
            break;
        }

        let pre_pc = ctx.cpu.pc;
        match ctx.cpu.step(ctx.mem) {
            StepResult::Ok => {
                if trace {
                    let insn = ctx.mem.read_u32(pre_pc);
                    eprintln!("[{:08}] {:08x}: {:08x}", ctx.cpu.insn_count, pre_pc, insn);
                }
            }
            StepResult::OutOfText => {
                let pc = ctx.cpu.pc;

                if pc == SENTINEL_RA {
                    eprintln!("[STOP] returned to sentinel ($ra=0x{SENTINEL_RA:08x})");
                    break;
                }

                if pc == ctx.hle.task_sentinel() {
                    ctx.hle.task_returned(ctx.cpu);
                    continue;
                }

                if let Some(idx) = ctx.hle.is_hle_addr(pc) {
                    dispatch(ctx, idx);
                } else {
                    eprintln!("\n[FATAL] PC 0x{pc:08x} outside text segment");
                    break;
                }
            }
            StepResult::Break(code) => {
                eprintln!("\n[FATAL] BREAK code={code} at PC=0x{:08x}", ctx.cpu.pc);
                break;
            }
        }
    }
}

/// Write a UTF-8 string as UCS-2 LE wide string to guest memory
fn write_wstring(mem: &mut Memory, addr: u32, s: &str) {
    let mut off = addr;
    for c in s.encode_utf16() {
        mem.write_u16(off, c);
        off += 2;
    }
    mem.write_u16(off, 0); // null terminator
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod integration {
    use super::*;
    use crate::loader::parse_ccdl;
    use std::path::Path;

    fn load_qiye() -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("qiye.app");
        std::fs::read(&path).expect("qiye.app not found — place it in project root")
    }

    fn setup_qiye() -> (Cpu, Memory, u32, u32) {
        let data = load_qiye();
        let mut mem = Memory::new();
        let ccdl = load_ccdl(&data, &mut mem);
        let entry = ccdl.entry_point;
        let code_start = ccdl.load_address;
        let code_end = ccdl.load_address + ccdl.data_size;
        // Note: we skip HleState::new here to avoid patching, keeping tests simple
        let mut cpu = Cpu::new();
        cpu.pc = entry;
        cpu.next_pc = entry.wrapping_add(4);
        cpu.set_gpr(29, DEFAULT_SP);
        cpu.set_gpr(31, SENTINEL_RA);
        cpu.code_start = code_start;
        cpu.code_end = code_end;
        mem.write_u32(DEFAULT_SP + 0x10, SENTINEL_RA);
        (cpu, mem, code_start, code_end)
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
    fn test_write_wstring() {
        let mut mem = Memory::new();
        write_wstring(&mut mem, 0x8001_1000, "test");
        assert_eq!(mem.read_u16(0x8001_1000), b't' as u16);
        assert_eq!(mem.read_u16(0x8001_1002), b'e' as u16);
        assert_eq!(mem.read_u16(0x8001_1004), b's' as u16);
        assert_eq!(mem.read_u16(0x8001_1006), b't' as u16);
        assert_eq!(mem.read_u16(0x8001_1008), 0);
    }
}
