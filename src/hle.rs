use crate::fs::GuestFs;
use crate::loader::ImportEntry;
use crate::mem::Memory;
use crate::mips::Cpu;

use sdl2::event::Event;
use sdl2::keyboard::Scancode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::render::Canvas;
use sdl2::video::Window;
use sdl2::EventPump;

use std::collections::HashMap;
use std::time::Instant;

// ── Constants ────────────────────────────────────────────────────────────────

const HLE_BASE: u32 = 0x8000_0000;
const LCD_FRAMEBUF: u32 = 0x80F0_0000;
const LCD_W: u32 = 320;
const LCD_H: u32 = 240;

// Dingoo A320 key bits (GPIO bitmask from _kbd_get_status)
const KEY_UP: u32 = 0x0010_0000;
const KEY_DOWN: u32 = 0x0800_0000;
const KEY_LEFT: u32 = 0x1000_0000;
const KEY_RIGHT: u32 = 0x0004_0000;
const KEY_A: u32 = 0x8000_0000;
const KEY_B: u32 = 0x0020_0000;
const KEY_X: u32 = 0x0001_0000;
const KEY_Y: u32 = 0x0000_0040;
const KEY_LS: u32 = 0x0000_0100;
const KEY_RS: u32 = 0x2000_0000;
const KEY_SELECT: u32 = 0x0000_0400;
const KEY_START: u32 = 0x0000_0800;

// ── HLE Dispatch Table ──────────────────────────────────────────────────────

type HleFn = fn(&mut EmuCtx);

pub struct HleState {
    handlers: Vec<HleFn>,
    names: Vec<String>,
    pub heap_ptr: u32,
    pub alloc_sizes: HashMap<u32, u32>,
    sem_counter: u32,
    framebuf_addr: u32, // legacy, unused now
    pub buttons: u32,
    pub quit: bool,
    start_time: Instant,
    suppress: HashMap<String, u32>,
    pub frame_count: u64,
}

pub struct SdlState {
    pub canvas: Canvas<Window>,
    pub event_pump: EventPump,
    // texture stored separately due to lifetime issues with TextureCreator
}

/// Everything the HLE handlers need access to.
pub struct EmuCtx<'a> {
    pub cpu: &'a mut Cpu,
    pub mem: &'a mut Memory,
    pub hle: &'a mut HleState,
    pub fs: &'a mut GuestFs,
    pub sdl: &'a mut SdlState,
}

impl HleState {
    pub fn new(imports: &[ImportEntry], mem: &mut Memory, code_start: u32, code_size: u32) -> Self {
        let mut state = Self {
            handlers: Vec::new(),
            names: Vec::new(),
            heap_ptr: 0x9800_0000,
            alloc_sizes: HashMap::new(),
            sem_counter: 0,
            framebuf_addr: LCD_FRAMEBUF,
            buttons: 0,
            quit: false,
            start_time: Instant::now(),
            suppress: HashMap::new(),
            frame_count: 0,
        };

        let mut import_map = HashMap::new();
        for (i, imp) in imports.iter().enumerate() {
            import_map.insert(imp.target_vaddr, i);

            let handler: HleFn = match imp.name.as_str() {
                // C stdlib
                "malloc" => hle_malloc,
                "free" => hle_free,
                "realloc" => hle_realloc,
                "printf" => hle_printf,
                "sprintf" => hle_sprintf,
                "fprintf" => hle_fprintf,
                "strlen" => hle_strlen,
                "strncasecmp" => hle_strncasecmp,
                "abort" => hle_abort,

                // C stdio (host file ops via fd from fsys)
                "fread" => hle_fread,
                "fwrite" => hle_fwrite,
                "fseek" => hle_fseek,

                // Filesystem
                "fsys_fopen" => hle_fsys_fopen,
                "fsys_fopenW" => hle_fsys_fopen_wide,
                "fsys_fread" => hle_fsys_fread,
                "fsys_fwrite" => hle_fsys_fwrite,
                "fsys_fclose" => hle_fsys_fclose,
                "fsys_fseek" => hle_fsys_fseek,
                "fsys_ftell" => hle_fsys_ftell,
                "fsys_feof" => hle_fsys_feof,
                "fsys_ferror" => hle_fsys_ferror,
                "fsys_remove" | "fsys_rename" => hle_stub_zero,
                "fsys_findfirst" => hle_stub_neg1,
                "fsys_findnext" => hle_stub_neg1,
                "fsys_findclose" => hle_stub_zero,
                "fsys_RefreshCache" | "fsys_flush_cache" => hle_stub_zero,

                // LCD / Video
                "_lcd_get_frame" | "lcd_get_cframe" => hle_lcd_get_frame,
                "_lcd_set_frame" | "ap_lcd_set_frame" => hle_lcd_set_frame,
                "lcd_flip" => hle_lcd_flip,
                "LcdGetDisMode" => hle_stub_zero,

                // Input
                "_kbd_get_status" => hle_kbd_get_status,
                "_kbd_get_key" => hle_kbd_get_key,
                "get_game_vol" => hle_get_game_vol,

                // Event
                "_sys_judge_event" => hle_sys_judge_event,

                // OS / RTOS
                "OSSemCreate" => hle_ossem_create,
                "OSSemPend" => hle_ossem_pend,
                "OSSemPost" | "OSSemDel" => hle_stub_zero,
                "OSTimeGet" | "GetTickCount" => hle_get_tick,
                "OSTimeDly" => hle_os_time_dly,
                "OSTaskCreate" => hle_os_task_create,
                "OSTaskDel" => hle_stub_zero,
                "OSCPUSaveSR" => hle_stub_zero,
                "OSCPURestoreSR" => hle_nop,

                // String conversion
                "__to_unicode_le" => hle_to_unicode_le,
                "__to_locale_ansi" => hle_to_locale_ansi,
                "get_current_language" => hle_stub_zero,

                // Audio (no-op stubs for now)
                "waveout_open" | "waveout_close" | "waveout_close_at_once"
                | "waveout_set_volume" | "waveout_write" => hle_stub_zero,
                "waveout_can_write" | "pcm_can_write" => hle_stub_one,
                "pcm_ioctl" => hle_stub_zero,
                "HP_Mute_sw" => hle_nop,

                // Misc no-ops
                "vxGoHome" => hle_exit,
                "__icache_invalidate_all" | "__dcache_writeback_all" => hle_nop,
                "StartSwTimer" | "free_irq" | "TaskMediaFunStop" => hle_nop,
                "USB_Connect" | "USB_No_Connect" | "udc_attached" => hle_stub_zero,
                "serial_putc" | "serial_getc" => hle_stub_zero,

                _ => hle_default,
            };

            state.handlers.push(handler);
            state.names.push(imp.name.clone());
        }

        // Patch JAL call sites
        let mut patch_count = 0u32;
        let code_end = code_start + code_size;
        let mut addr = code_start;
        while addr < code_end {
            let insn = mem.read_u32(addr);
            let opcode = (insn >> 26) & 0x3F;
            if opcode == 0x03 {
                let target26 = insn & 0x03FF_FFFF;
                let target = (addr & 0xF000_0000) | (target26 << 2);
                if let Some(&idx) = import_map.get(&target) {
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

    pub fn name(&self, idx: usize) -> &str {
        &self.names[idx]
    }
}

pub fn dispatch(ctx: &mut EmuCtx, idx: usize) {
    let name = ctx.hle.names[idx].clone();

    // Suppress noisy repeated HLE calls
    let verbose = !matches!(
        name.as_str(),
        "OSTimeGet" | "GetTickCount" | "_sys_judge_event" | "_kbd_get_status"
        | "_kbd_get_key" | "lcd_flip" | "waveout_can_write" | "pcm_can_write"
        | "OSCPUSaveSR" | "OSCPURestoreSR" | "OSSemPend" | "OSSemPost"
        | "get_game_vol" | "__icache_invalidate_all" | "__dcache_writeback_all"
    );

    if verbose {
        let count = ctx.hle.suppress.entry(name.clone()).or_insert(0);
        *count += 1;
        let c = *count;
        if c <= 3 || c % 100 == 0 {
            eprintln!(
                "[HLE] {}(0x{:08x}, 0x{:08x}, 0x{:08x}, 0x{:08x}){}",
                name,
                ctx.cpu.gpr(4), ctx.cpu.gpr(5), ctx.cpu.gpr(6), ctx.cpu.gpr(7),
                if c > 3 { format!("  [call #{}]", c) } else { String::new() },
            );
        }
    }

    let func = ctx.hle.handlers[idx];
    func(ctx);

    // Return to caller
    ctx.cpu.pc = ctx.cpu.gpr(31);
    ctx.cpu.next_pc = ctx.cpu.pc.wrapping_add(4);
}

// ── HLE Handlers ────────────────────────────────────────────────────────────

fn hle_nop(_ctx: &mut EmuCtx) {}

fn hle_stub_zero(ctx: &mut EmuCtx) {
    ctx.cpu.set_gpr(2, 0);
}

fn hle_stub_one(ctx: &mut EmuCtx) {
    ctx.cpu.set_gpr(2, 1);
}

fn hle_stub_neg1(ctx: &mut EmuCtx) {
    ctx.cpu.set_gpr(2, !0);
}

fn hle_default(ctx: &mut EmuCtx) {
    ctx.cpu.set_gpr(2, 0);
}

// ── Memory ──────────────────────────────────────────────────────────────────

fn hle_malloc(ctx: &mut EmuCtx) {
    let size = ctx.cpu.gpr(4);
    let aligned = (ctx.hle.heap_ptr + 7) & !7;
    ctx.hle.alloc_sizes.insert(aligned, size);
    ctx.hle.heap_ptr = aligned + size;
    ctx.cpu.set_gpr(2, aligned);
}

fn hle_free(_ctx: &mut EmuCtx) {}

fn hle_realloc(ctx: &mut EmuCtx) {
    let old_ptr = ctx.cpu.gpr(4);
    let new_size = ctx.cpu.gpr(5);
    let aligned = (ctx.hle.heap_ptr + 7) & !7;
    ctx.hle.heap_ptr = aligned + new_size;
    if old_ptr != 0 {
        if let Some(&old_size) = ctx.hle.alloc_sizes.get(&old_ptr) {
            let copy_size = old_size.min(new_size) as usize;
            for i in 0..copy_size {
                let b = ctx.mem.read_u8(old_ptr + i as u32);
                ctx.mem.write_u8(aligned + i as u32, b);
            }
        }
    }
    ctx.hle.alloc_sizes.insert(aligned, new_size);
    ctx.cpu.set_gpr(2, aligned);
}

// ── Printf ──────────────────────────────────────────────────────────────────

fn hle_printf(ctx: &mut EmuCtx) {
    let fmt_addr = ctx.cpu.gpr(4);
    let output = format_guest_string(ctx.mem, ctx.cpu, fmt_addr, 1);
    print!("{}", output);
    ctx.cpu.set_gpr(2, output.len() as u32);
}

fn hle_sprintf(ctx: &mut EmuCtx) {
    let buf_addr = ctx.cpu.gpr(4);
    let fmt_addr = ctx.cpu.gpr(5);
    let output = format_guest_string(ctx.mem, ctx.cpu, fmt_addr, 2);
    for (i, b) in output.bytes().enumerate() {
        ctx.mem.write_u8(buf_addr + i as u32, b);
    }
    ctx.mem.write_u8(buf_addr + output.len() as u32, 0);
    ctx.cpu.set_gpr(2, output.len() as u32);
}

fn hle_fprintf(ctx: &mut EmuCtx) {
    let fmt_addr = ctx.cpu.gpr(5);
    if fmt_addr < 0x8000_0000 || fmt_addr >= 0xA000_0000 {
        ctx.cpu.set_gpr(2, 0);
        return;
    }
    let output = format_guest_string(ctx.mem, ctx.cpu, fmt_addr, 2);
    print!("{}", output);
    ctx.cpu.set_gpr(2, output.len() as u32);
}

// ── String ──────────────────────────────────────────────────────────────────

fn hle_strlen(ctx: &mut EmuCtx) {
    let s = ctx.cpu.gpr(4);
    let len = ctx.mem.read_string(s).len();
    ctx.cpu.set_gpr(2, len as u32);
}

fn hle_strncasecmp(ctx: &mut EmuCtx) {
    let a_addr = ctx.cpu.gpr(4);
    let b_addr = ctx.cpu.gpr(5);
    let n = ctx.cpu.gpr(6) as usize;
    let a = ctx.mem.read_string(a_addr);
    let b = ctx.mem.read_string(b_addr);
    let a_bytes: Vec<u8> = a.bytes().take(n).map(|c| c.to_ascii_lowercase()).collect();
    let b_bytes: Vec<u8> = b.bytes().take(n).map(|c| c.to_ascii_lowercase()).collect();
    let result = a_bytes.cmp(&b_bytes) as i32;
    ctx.cpu.set_gpr(2, result as u32);
}

fn hle_abort(_ctx: &mut EmuCtx) {
    eprintln!("[HLE] abort() called");
    std::process::exit(1);
}

// ── String Conversion ───────────────────────────────────────────────────────

fn hle_to_unicode_le(ctx: &mut EmuCtx) {
    // wchar_t* __to_unicode_le(char* src)
    // Single arg: converts ANSI string in-place to UCS-2 LE, returns pointer
    // Must work backwards to avoid overwriting source bytes
    let addr = ctx.cpu.gpr(4);
    let s = ctx.mem.read_string(addr);
    let mut off = addr;
    for b in s.bytes() {
        ctx.mem.write_u16(off, b as u16);
        off += 2;
    }
    ctx.mem.write_u16(off, 0);
    ctx.cpu.set_gpr(2, addr);
}

fn hle_to_locale_ansi(ctx: &mut EmuCtx) {
    // char* __to_locale_ansi(wchar_t* src)
    // Single arg: converts UCS-2 LE wide string in-place to ANSI, returns pointer
    let addr = ctx.cpu.gpr(4);
    let ws = GuestFs::read_wstring(ctx.mem, addr);
    let mut off = addr;
    for b in ws.bytes() {
        ctx.mem.write_u8(off, b);
        off += 1;
    }
    ctx.mem.write_u8(off, 0);
    ctx.cpu.set_gpr(2, addr);
}

// ── Filesystem ──────────────────────────────────────────────────────────────

fn hle_fsys_fopen(ctx: &mut EmuCtx) {
    let path_addr = ctx.cpu.gpr(4);
    let mode_addr = ctx.cpu.gpr(5);
    let path = ctx.mem.read_string(path_addr);
    let mode = ctx.mem.read_string(mode_addr);
    let fd = ctx.fs.fopen(&path, &mode);
    ctx.cpu.set_gpr(2, fd);
}

fn hle_fsys_fopen_wide(ctx: &mut EmuCtx) {
    let wpath_addr = ctx.cpu.gpr(4);
    let wmode_addr = ctx.cpu.gpr(5);
    let fd = ctx.fs.fopen_wide(ctx.mem, wpath_addr, wmode_addr);
    ctx.cpu.set_gpr(2, fd);
}

fn hle_fsys_fread(ctx: &mut EmuCtx) {
    let buf = ctx.cpu.gpr(4);
    let size = ctx.cpu.gpr(5);
    let count = ctx.cpu.gpr(6);
    let fd = ctx.cpu.gpr(7);
    let n = ctx.fs.fread(ctx.mem, buf, size, count, fd);
    ctx.cpu.set_gpr(2, n);
}

fn hle_fsys_fwrite(ctx: &mut EmuCtx) {
    let buf = ctx.cpu.gpr(4);
    let size = ctx.cpu.gpr(5);
    let count = ctx.cpu.gpr(6);
    let fd = ctx.cpu.gpr(7);
    let n = ctx.fs.fwrite(ctx.mem, buf, size, count, fd);
    ctx.cpu.set_gpr(2, n);
}

fn hle_fsys_fclose(ctx: &mut EmuCtx) {
    let fd = ctx.cpu.gpr(4);
    let r = ctx.fs.fclose(fd);
    ctx.cpu.set_gpr(2, r);
}

fn hle_fsys_fseek(ctx: &mut EmuCtx) {
    let fd = ctx.cpu.gpr(4);
    let offset = ctx.cpu.gpr(5) as i32;
    let whence = ctx.cpu.gpr(6);
    let r = ctx.fs.fseek(fd, offset, whence);
    ctx.cpu.set_gpr(2, r);
}

fn hle_fsys_ftell(ctx: &mut EmuCtx) {
    let fd = ctx.cpu.gpr(4);
    let pos = ctx.fs.ftell(fd);
    ctx.cpu.set_gpr(2, pos);
}

fn hle_fsys_feof(ctx: &mut EmuCtx) {
    let fd = ctx.cpu.gpr(4);
    let r = ctx.fs.feof(fd);
    ctx.cpu.set_gpr(2, r);
}

fn hle_fsys_ferror(ctx: &mut EmuCtx) {
    let fd = ctx.cpu.gpr(4);
    let r = ctx.fs.ferror(fd);
    ctx.cpu.set_gpr(2, r);
}

// C stdio fread/fwrite/fseek — same as fsys_ variants
fn hle_fread(ctx: &mut EmuCtx) { hle_fsys_fread(ctx); }
fn hle_fwrite(ctx: &mut EmuCtx) { hle_fsys_fwrite(ctx); }
fn hle_fseek(ctx: &mut EmuCtx) { hle_fsys_fseek(ctx); }

// ── LCD / Video ─────────────────────────────────────────────────────────────

fn hle_lcd_get_frame(ctx: &mut EmuCtx) {
    // Always return fixed framebuffer address — the game copies its render buffer here
    ctx.cpu.set_gpr(2, LCD_FRAMEBUF);
}

fn hle_lcd_set_frame(ctx: &mut EmuCtx) {
    // On real Dingoo, this sets the LCD DMA source address.
    // The game's Raster_presentFramebuffer copies pixels to lcd_get_frame() result,
    // then calls lcd_set_frame(past_end_of_buffer). We present the fixed buffer to SDL.
    present_framebuffer(ctx);
}

fn hle_lcd_flip(ctx: &mut EmuCtx) {
    present_framebuffer(ctx);
}

fn present_framebuffer(ctx: &mut EmuCtx) {
    ctx.hle.frame_count += 1;
    let fc = ctx.hle.frame_count;
    if fc <= 3 || fc % 60 == 0 {
        let elapsed = ctx.hle.start_time.elapsed().as_secs_f64();
        eprintln!("[LCD] frame #{fc} ({:.1} fps, {:.0}M insns)",
            fc as f64 / elapsed, ctx.cpu.insn_count as f64 / 1e6);
    }

    let fb_size = (LCD_W * LCD_H * 2) as usize;
    let fb_data = ctx.mem.slice(LCD_FRAMEBUF, fb_size);

    let creator = ctx.sdl.canvas.texture_creator();
    let mut texture = creator
        .create_texture_streaming(PixelFormatEnum::RGB565, LCD_W, LCD_H)
        .expect("Failed to create texture");

    texture
        .update(None, fb_data, (LCD_W * 2) as usize)
        .expect("Failed to update texture");

    ctx.sdl.canvas.copy(&texture, None, None).expect("Failed to copy texture");
    ctx.sdl.canvas.present();
}

// ── Input ───────────────────────────────────────────────────────────────────

fn poll_sdl_input(ctx: &mut EmuCtx) {
    for event in ctx.sdl.event_pump.poll_iter() {
        match event {
            Event::Quit { .. } => {
                ctx.hle.quit = true;
            }
            _ => {}
        }
    }

    // Read keyboard state
    let keys = ctx.sdl.event_pump.keyboard_state();
    let mut btns = 0u32;
    // D-pad: arrow keys
    if keys.is_scancode_pressed(Scancode::Up) { btns |= KEY_UP; }
    if keys.is_scancode_pressed(Scancode::Down) { btns |= KEY_DOWN; }
    if keys.is_scancode_pressed(Scancode::Left) { btns |= KEY_LEFT; }
    if keys.is_scancode_pressed(Scancode::Right) { btns |= KEY_RIGHT; }
    // Face buttons (PPSSPP-style layout)
    if keys.is_scancode_pressed(Scancode::X) { btns |= KEY_A; }  // Circle = A (confirm)
    if keys.is_scancode_pressed(Scancode::Z) { btns |= KEY_B; }  // Cross = B (cancel)
    if keys.is_scancode_pressed(Scancode::S) { btns |= KEY_X; }  // Triangle = X
    if keys.is_scancode_pressed(Scancode::A) { btns |= KEY_Y; }  // Square = Y
    // Shoulders
    if keys.is_scancode_pressed(Scancode::Q) { btns |= KEY_LS; }
    if keys.is_scancode_pressed(Scancode::W) { btns |= KEY_RS; }
    // System
    if keys.is_scancode_pressed(Scancode::V) { btns |= KEY_SELECT; }
    if keys.is_scancode_pressed(Scancode::Space) { btns |= KEY_START; }
    ctx.hle.buttons = btns;
}

fn hle_sys_judge_event(ctx: &mut EmuCtx) {
    poll_sdl_input(ctx);
    if ctx.hle.quit {
        ctx.cpu.set_gpr(2, (-1i32) as u32);
    } else {
        ctx.cpu.set_gpr(2, 0);
    }
}

fn hle_kbd_get_status(ctx: &mut EmuCtx) {
    let out_ptr = ctx.cpu.gpr(4);
    // 12-byte struct: u32 field0, u32 field1, u32 buttons
    // Game's input_dispatch reads buttons from offset 8 and does its own prev tracking
    ctx.mem.write_u32(out_ptr, 0);     // field0 (unused/x)
    ctx.mem.write_u32(out_ptr + 4, 0); // field1 (unused/y)
    ctx.mem.write_u32(out_ptr + 8, ctx.hle.buttons);
}

fn hle_kbd_get_key(ctx: &mut EmuCtx) {
    // Return current button state (game does its own edge detection)
    ctx.cpu.set_gpr(2, ctx.hle.buttons);
}

fn hle_get_game_vol(ctx: &mut EmuCtx) {
    ctx.cpu.set_gpr(2, 80); // volume 0-100
}

// ── OS / RTOS ───────────────────────────────────────────────────────────────

fn hle_ossem_create(ctx: &mut EmuCtx) {
    ctx.hle.sem_counter += 1;
    ctx.cpu.set_gpr(2, 0x80E0_0000 + ctx.hle.sem_counter * 4);
}

fn hle_ossem_pend(ctx: &mut EmuCtx) {
    // OSSemPend(sem, timeout, &err)
    let err_ptr = ctx.cpu.gpr(6);
    if err_ptr != 0 {
        ctx.mem.write_u8(err_ptr, 0); // OS_ERR_NONE
    }
}

fn hle_get_tick(ctx: &mut EmuCtx) {
    // 100Hz tick rate: elapsed_ms / 10
    let elapsed = ctx.hle.start_time.elapsed();
    let ticks = (elapsed.as_millis() / 10) as u32;
    ctx.cpu.set_gpr(2, ticks);
}

fn hle_os_time_dly(ctx: &mut EmuCtx) {
    let ticks = ctx.cpu.gpr(4);
    let ms = (ticks * 10).max(1);
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
}

fn hle_os_task_create(ctx: &mut EmuCtx) {
    let task_fn = ctx.cpu.gpr(4);
    let _stack = ctx.cpu.gpr(6);
    let prio = ctx.cpu.gpr(7);
    eprintln!("[HLE] OSTaskCreate(fn=0x{:08x}, prio={}) — NOT spawning (single-threaded mode)", task_fn, prio);
    ctx.cpu.set_gpr(2, 0); // OS_ERR_NONE
}

fn hle_exit(ctx: &mut EmuCtx) {
    eprintln!("[HLE] vxGoHome() — requesting quit");
    ctx.hle.quit = true;
}

// ── Printf format engine ────────────────────────────────────────────────────

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
