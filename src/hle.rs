use crate::fs::GuestFs;
use crate::loader::ImportEntry;
use crate::mem::Memory;
use crate::mips::Cpu;

use sdl2::audio::AudioQueue;
use sdl2::AudioSubsystem;
use sdl2::event::Event;
use sdl2::keyboard::Scancode;
use sdl2::render::{Canvas, Texture};
use sdl2::video::Window;
use sdl2::EventPump;

use std::collections::HashMap;
use std::time::Instant;

// ── Constants ────────────────────────────────────────────────────────────────

#[cfg(not(feature = "reloc"))]
const HLE_BASE: u32 = 0x8000_0000;
#[cfg(feature = "reloc")]
const HLE_BASE: u32 = 0x0800_0000;

#[cfg(not(feature = "reloc"))]
const LCD_FRAMEBUF: u32 = 0x80F0_0000;
#[cfg(feature = "reloc")]
const LCD_FRAMEBUF: u32 = 0x08F0_0000;

#[cfg(not(feature = "reloc"))]
const LOCALE_ANSI_BUF: u32 = 0x8001_2000;
#[cfg(feature = "reloc")]
const LOCALE_ANSI_BUF: u32 = 0x0801_2000;

#[cfg(not(feature = "reloc"))]
const UNICODE_LE_BUF: u32 = 0x8001_3000;
#[cfg(feature = "reloc")]
const UNICODE_LE_BUF: u32 = 0x0801_3000;
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

// ── Task System ─────────────────────────────────────────────────────────────

const TASK_SENTINEL: u32 = 0xDEAD_0004;

#[derive(Clone)]
struct SavedCpuState {
    gpr: [u32; 32],
    pc: u32,
    next_pc: u32,
    hi: u32,
    lo: u32,
}

#[derive(Clone, Debug, PartialEq)]
enum TaskStatus {
    Ready,
    Running,
    WaitSem(u32),     // blocked on semaphore address
    Sleeping(u32),    // wake at this tick count
    Dead,
}

struct Task {
    status: TaskStatus,
    priority: u8,
    state: SavedCpuState,
}

struct Semaphore {
    count: i32,
}

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
    // Task system
    tasks: Vec<Task>,
    current_task: usize,
    semaphores: HashMap<u32, Semaphore>,
    context_switched: bool, // set by HLE handlers that do a context switch
}

pub struct SdlState {
    pub canvas: Canvas<Window>,
    pub event_pump: EventPump,
    pub texture: Texture<'static>,
    pub audio: AudioSubsystem,
    pub audio_queue: Option<AudioQueue<i16>>,
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
            heap_ptr: HLE_BASE + 0x1800_0000,
            alloc_sizes: HashMap::new(),
            sem_counter: 0,
            framebuf_addr: LCD_FRAMEBUF,
            buttons: 0,
            quit: false,
            start_time: Instant::now(),
            suppress: HashMap::new(),
            frame_count: 0,
            tasks: Vec::new(),
            current_task: 0,
            semaphores: HashMap::new(),
            context_switched: false,
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
                "OSSemPost" => hle_ossem_post,
                "OSSemDel" => hle_ossem_del,
                "OSTimeGet" => hle_get_tick,
                "GetTickCount" => hle_get_tick_ms,
                "OSTimeDly" => hle_os_time_dly,
                "OSTaskCreate" => hle_os_task_create,
                "OSTaskDel" => hle_os_task_del,
                "OSCPUSaveSR" => hle_stub_zero,
                "OSCPURestoreSR" => hle_nop,

                // String conversion
                "__to_unicode_le" => hle_to_unicode_le,
                "__to_locale_ansi" => hle_to_locale_ansi,
                "get_current_language" => hle_stub_zero,

                // Audio
                "waveout_open" => hle_waveout_open,
                "waveout_write" => hle_waveout_write,
                "waveout_can_write" => hle_waveout_can_write,
                "waveout_close" | "waveout_close_at_once" => hle_waveout_close,
                "waveout_set_volume" => hle_stub_zero,
                "pcm_can_write" => hle_stub_one,
                "pcm_ioctl" => hle_stub_zero,
                "HP_Mute_sw" => hle_nop,

                // Misc no-ops
                "vxGoHome" => hle_exit,
                "__icache_invalidate_all" | "__dcache_writeback_all" => hle_nop,
                "StartSwTimer" | "free_irq" | "TaskMediaFunStop" => hle_nop,
                "USB_Connect" | "USB_No_Connect" | "udc_attached" => hle_stub_zero,
                "serial_putc" | "serial_getc" => hle_stub_zero,

                _ => {
                    eprintln!("[HLE] WARNING: unknown import '{}' at 0x{:08x} — will return 0",
                        imp.name, imp.target_vaddr);
                    hle_default
                }
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

    /// Register the current CPU state as Task 0 (AppMain). Call before Phase 2 execution.
    pub fn init_main_task(&mut self, cpu: &Cpu) {
        self.tasks.clear();
        self.current_task = 0;
        self.tasks.push(Task {
            status: TaskStatus::Running,
            priority: 63, // lower than audio task (prio 16) so audio gets scheduled first
            state: SavedCpuState {
                gpr: cpu.gpr,
                pc: cpu.pc,
                next_pc: cpu.next_pc,
                hi: cpu.hi,
                lo: cpu.lo,
            },
        });
        eprintln!("[TASK] Task 0 (AppMain) registered, priority=0");
    }

    /// Handle a task returning to TASK_SENTINEL — mark dead and switch.
    pub fn task_returned(&mut self, cpu: &mut Cpu) {
        let tid = self.current_task;
        eprintln!("[TASK] Task {} returned (hit TASK_SENTINEL)", tid);
        self.tasks[tid].status = TaskStatus::Dead;
        self.schedule(cpu);
    }

    pub fn task_sentinel(&self) -> u32 {
        TASK_SENTINEL
    }

    fn save_cpu(&mut self, cpu: &Cpu) {
        let t = &mut self.tasks[self.current_task];
        t.state.gpr = cpu.gpr;
        t.state.pc = cpu.pc;
        t.state.next_pc = cpu.next_pc;
        t.state.hi = cpu.hi;
        t.state.lo = cpu.lo;
    }

    fn load_cpu(&self, cpu: &mut Cpu) {
        let t = &self.tasks[self.current_task];
        cpu.gpr = t.state.gpr;
        cpu.pc = t.state.pc;
        cpu.next_pc = t.state.next_pc;
        cpu.hi = t.state.hi;
        cpu.lo = t.state.lo;
    }

    fn current_tick(&self) -> u32 {
        (self.start_time.elapsed().as_millis() / 10) as u32
    }

    fn schedule(&mut self, cpu: &mut Cpu) {
        self.context_switched = true;

        // Save current task (unless Dead — state doesn't matter)
        if self.tasks[self.current_task].status != TaskStatus::Dead {
            self.save_cpu(cpu);
        }

        // Wake sleeping tasks
        let now = self.current_tick();
        for t in &mut self.tasks {
            if let TaskStatus::Sleeping(wake_at) = t.status {
                if now >= wake_at {
                    t.status = TaskStatus::Ready;
                }
            }
        }

        // Find highest-priority (lowest number) Ready task
        let mut best: Option<usize> = None;
        for (i, t) in self.tasks.iter().enumerate() {
            if t.status == TaskStatus::Ready {
                if best.is_none() || t.priority < self.tasks[best.unwrap()].priority {
                    best = Some(i);
                }
            }
        }

        if let Some(next) = best {
            self.tasks[next].status = TaskStatus::Running;
            self.current_task = next;
            self.load_cpu(cpu);
        } else {
            // No ready tasks — sleep until the earliest wake time
            let mut min_wake: Option<u32> = None;
            for t in &self.tasks {
                if let TaskStatus::Sleeping(wake_at) = t.status {
                    min_wake = Some(min_wake.map_or(wake_at, |m: u32| m.min(wake_at)));
                }
            }
            if let Some(wake_at) = min_wake {
                let now = self.current_tick();
                if wake_at > now {
                    let ms = ((wake_at - now) * 10) as u64;
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                } else {
                    // Wake time already passed but tick granularity too coarse — sleep 5ms
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                // Retry after sleeping
                self.schedule(cpu);
                return;
            }
            // All tasks dead or waiting on semaphores
            let has_waiting = self.tasks.iter().any(|t| matches!(t.status, TaskStatus::WaitSem(_)));
            if has_waiting {
                // Sem waiters exist — sleep briefly and retry (sem may be posted by interrupt/timer)
                std::thread::sleep(std::time::Duration::from_millis(5));
                self.schedule(cpu);
                return;
            }
            eprintln!("[TASK] WARNING: all tasks blocked or dead — possible deadlock");
            // Fall back to task 0 if it exists and isn't dead
            if self.tasks[0].status != TaskStatus::Dead {
                self.tasks[0].status = TaskStatus::Running;
                self.current_task = 0;
                self.load_cpu(cpu);
            }
        }
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

    ctx.hle.context_switched = false;
    let func = ctx.hle.handlers[idx];
    func(ctx);

    if ctx.hle.context_switched {
        // Handler did a context switch — CPU state already set by schedule()
        return;
    }

    // Return to caller
    ctx.cpu.pc = ctx.cpu.gpr(31);
    ctx.cpu.next_pc = ctx.cpu.pc.wrapping_add(4);

    // Preemptive scheduling: if a higher-priority task is ready (or a
    // sleeping task just woke up), switch to it (matches uC/OS-II).
    if ctx.hle.tasks.len() > 1 {
        let cur_prio = ctx.hle.tasks[ctx.hle.current_task].priority;
        let now = ctx.hle.current_tick();
        let mut need_preempt = false;
        for t in &mut ctx.hle.tasks {
            if let TaskStatus::Sleeping(wake_at) = t.status {
                if now >= wake_at {
                    t.status = TaskStatus::Ready;
                }
            }
            if t.status == TaskStatus::Ready && t.priority < cur_prio {
                need_preempt = true;
            }
        }
        if need_preempt {
            let tid = ctx.hle.current_task;
            ctx.hle.tasks[tid].status = TaskStatus::Ready;
            ctx.hle.schedule(ctx.cpu);
        }
    }
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
    if fmt_addr < HLE_BASE || fmt_addr >= HLE_BASE + 0x2000_0000 {
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
    // Single arg: converts ANSI string to UCS-2 LE in a static buffer, returns pointer
    let src = ctx.cpu.gpr(4);
    let s = ctx.mem.read_string(src);
    let buf = UNICODE_LE_BUF;
    let mut off = buf;
    for b in s.bytes() {
        ctx.mem.write_u16(off, b as u16);
        off += 2;
    }
    ctx.mem.write_u16(off, 0);
    ctx.cpu.set_gpr(2, buf);
}

fn hle_to_locale_ansi(ctx: &mut EmuCtx) {
    // char* __to_locale_ansi(wchar_t* src)
    // Single arg: converts UCS-2 LE wide string to ANSI in a static buffer, returns pointer
    let src = ctx.cpu.gpr(4);
    let ws = GuestFs::read_wstring(ctx.mem, src);
    // Write to static scratch buffer (not in-place — caller may call twice with same src)
    let buf = LOCALE_ANSI_BUF;
    let mut off = buf;
    for b in ws.bytes() {
        ctx.mem.write_u8(off, b);
        off += 1;
    }
    ctx.mem.write_u8(off, 0);
    ctx.cpu.set_gpr(2, buf);
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

    ctx.sdl.texture
        .update(None, fb_data, (LCD_W * 2) as usize)
        .expect("Failed to update texture");

    ctx.sdl.canvas.copy(&ctx.sdl.texture, None, None).expect("Failed to copy texture");
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
    poll_sdl_input(ctx);
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
    let count = ctx.cpu.gpr(4) as i32;
    ctx.hle.sem_counter += 1;
    let addr = HLE_BASE + 0x00E0_0000 + ctx.hle.sem_counter * 4;
    ctx.hle.semaphores.insert(addr, Semaphore { count });
    eprintln!("[TASK] OSSemCreate(count={}) → 0x{:08x}", count, addr);
    ctx.cpu.set_gpr(2, addr);
}

fn hle_ossem_pend(ctx: &mut EmuCtx) {
    // OSSemPend(sem, timeout, &err)
    let sem_addr = ctx.cpu.gpr(4);
    let err_ptr = ctx.cpu.gpr(6);

    if let Some(sem) = ctx.hle.semaphores.get_mut(&sem_addr) {
        if sem.count > 0 {
            sem.count -= 1;
            if err_ptr != 0 { ctx.mem.write_u32(err_ptr, 0); }
            // Fast path: semaphore available, no context switch
            return;
        }
    }

    // Semaphore not available — block current task and switch
    if err_ptr != 0 { ctx.mem.write_u32(err_ptr, 0); }

    if ctx.hle.tasks.is_empty() {
        // No task system yet (Phase 1) — just return immediately
        return;
    }

    let tid = ctx.hle.current_task;
    ctx.hle.tasks[tid].status = TaskStatus::WaitSem(sem_addr);
    // Set CPU pc to return address so schedule saves the correct resume point
    let ra = ctx.cpu.gpr(31);
    ctx.cpu.pc = ra;
    ctx.cpu.next_pc = ra.wrapping_add(4);
    ctx.hle.schedule(ctx.cpu);
}

fn hle_ossem_post(ctx: &mut EmuCtx) {
    let sem_addr = ctx.cpu.gpr(4);

    if let Some(sem) = ctx.hle.semaphores.get_mut(&sem_addr) {
        sem.count += 1;

        // Wake highest-priority task waiting on this semaphore
        let mut best: Option<usize> = None;
        for (i, t) in ctx.hle.tasks.iter().enumerate() {
            if t.status == TaskStatus::WaitSem(sem_addr) {
                if best.is_none() || t.priority < ctx.hle.tasks[best.unwrap()].priority {
                    best = Some(i);
                }
            }
        }
        if let Some(waiter) = best {
            ctx.hle.semaphores.get_mut(&sem_addr).unwrap().count -= 1;
            ctx.hle.tasks[waiter].status = TaskStatus::Ready;
        }
    }
    ctx.cpu.set_gpr(2, 0); // OS_ERR_NONE
}

fn hle_ossem_del(ctx: &mut EmuCtx) {
    let sem_addr = ctx.cpu.gpr(4);
    // Wake all tasks waiting on this semaphore before removing it
    for t in &mut ctx.hle.tasks {
        if t.status == TaskStatus::WaitSem(sem_addr) {
            t.status = TaskStatus::Ready;
        }
    }
    ctx.hle.semaphores.remove(&sem_addr);
    ctx.cpu.set_gpr(2, 0);
}

fn hle_get_tick(ctx: &mut EmuCtx) {
    // OSTimeGet: 100Hz tick rate (10ms per tick)
    let elapsed = ctx.hle.start_time.elapsed();
    let ticks = (elapsed.as_millis() / 10) as u32;
    ctx.cpu.set_gpr(2, ticks);
}

fn hle_get_tick_ms(ctx: &mut EmuCtx) {
    // GetTickCount: returns milliseconds
    // Use instruction count to ensure time advances even in tight polling loops
    // ~1M insns/sec on original hardware → 1 insn ≈ 1µs → 1000 insns ≈ 1ms
    let ms = (ctx.cpu.insn_count / 1000) as u32;
    ctx.cpu.set_gpr(2, ms);
}

fn hle_os_time_dly(ctx: &mut EmuCtx) {
    let ticks = ctx.cpu.gpr(4);

    if ctx.hle.tasks.is_empty() {
        // No task system (Phase 1) — just sleep
        let ms = (ticks * 10).max(1);
        std::thread::sleep(std::time::Duration::from_millis(ms as u64));
        return;
    }

    let wake_at = ctx.hle.current_tick().wrapping_add(ticks);
    let tid = ctx.hle.current_task;
    ctx.hle.tasks[tid].status = TaskStatus::Sleeping(wake_at);
    let ra = ctx.cpu.gpr(31);
    ctx.cpu.pc = ra;
    ctx.cpu.next_pc = ra.wrapping_add(4);
    ctx.hle.schedule(ctx.cpu);
}

fn hle_os_task_create(ctx: &mut EmuCtx) {
    let task_fn = ctx.cpu.gpr(4);
    let task_arg = ctx.cpu.gpr(5);
    let stack_top = ctx.cpu.gpr(6);
    let prio = (ctx.cpu.gpr(7) & 0xFF) as u8;

    let tid = ctx.hle.tasks.len();
    let mut state = SavedCpuState {
        gpr: [0; 32],
        pc: task_fn,
        next_pc: task_fn.wrapping_add(4),
        hi: 0,
        lo: 0,
    };
    state.gpr[4] = task_arg;        // $a0 = task argument
    state.gpr[29] = stack_top;      // $sp = caller-provided stack
    state.gpr[31] = TASK_SENTINEL;  // $ra = sentinel for task return

    ctx.hle.tasks.push(Task {
        status: TaskStatus::Ready,
        priority: prio,
        state,
    });

    eprintln!("[TASK] OSTaskCreate(fn=0x{:08x}, arg=0x{:08x}, sp=0x{:08x}, prio={}) → task #{}",
        task_fn, task_arg, stack_top, prio, tid);
    ctx.cpu.set_gpr(2, 0); // OS_ERR_NONE
}

fn hle_os_task_del(ctx: &mut EmuCtx) {
    let prio = ctx.cpu.gpr(4);

    if prio == 0xFF {
        // Self-delete
        let tid = ctx.hle.current_task;
        eprintln!("[TASK] OSTaskDel(self) — killing task #{}", tid);
        ctx.hle.tasks[tid].status = TaskStatus::Dead;
        ctx.hle.schedule(ctx.cpu);
    } else {
        // Delete task by priority
        for t in &mut ctx.hle.tasks {
            if t.priority == prio as u8 && t.status != TaskStatus::Dead {
                eprintln!("[TASK] OSTaskDel(prio={}) — killed", prio);
                t.status = TaskStatus::Dead;
                break;
            }
        }
    }
    ctx.cpu.set_gpr(2, 0);
}

fn hle_exit(ctx: &mut EmuCtx) {
    eprintln!("[HLE] vxGoHome() — requesting quit");
    ctx.hle.quit = true;
}

// ── Audio (waveout) ────────────────────────────────────────────────────────

/// Number of samples per waveout_write call (0x320 bytes / 2 = 400 i16 samples)
const WAVEOUT_SAMPLES: usize = 400;
/// Max queued bytes before waveout_write blocks (~50ms at 16kHz mono i16)
const WAVEOUT_MAX_QUEUE: u32 = 800 * 2; // 1600 bytes (50ms)

fn hle_waveout_open(ctx: &mut EmuCtx) {
    // waveout_open(params_ptr) — params: { u32 sample_rate, u16 bits_per_sample }
    let params = ctx.cpu.gpr(4);
    let sample_rate = ctx.mem.read_u32(params);
    let bits = ctx.mem.read_u16(params + 4);
    eprintln!("[AUDIO] waveout_open(rate={}, bits={})", sample_rate, bits);

    let desired = sdl2::audio::AudioSpecDesired {
        freq: Some(sample_rate as i32),
        channels: Some(1), // mono
        samples: Some(WAVEOUT_SAMPLES as u16),
    };

    match AudioQueue::open_queue(&ctx.sdl.audio, None, &desired) {
        Ok(queue) => {
            eprintln!("[AUDIO] opened: freq={} channels={} samples={}",
                      queue.spec().freq, queue.spec().channels, queue.spec().samples);
            queue.resume(); // start playback
            ctx.sdl.audio_queue = Some(queue);
            ctx.cpu.set_gpr(2, 1); // return non-zero handle
        }
        Err(e) => {
            eprintln!("[AUDIO] Failed to open audio: {}", e);
            ctx.cpu.set_gpr(2, 0);
        }
    }
}

fn hle_waveout_write(ctx: &mut EmuCtx) {
    // waveout_write(handle, buf_ptr) — buf is 400 i16 samples (800 bytes)
    // On real hardware this blocks when DMA buffer is full — emulate by sleeping
    // the audio task when SDL2 queue exceeds threshold.
    let buf_ptr = ctx.cpu.gpr(5);

    if let Some(ref queue) = ctx.sdl.audio_queue {
        if queue.size() > WAVEOUT_MAX_QUEUE && ctx.hle.tasks.len() > 1 {
            // Too much buffered — put this task to sleep for ~5ms (1 tick)
            let wake_at = ctx.hle.current_tick().wrapping_add(1);
            let tid = ctx.hle.current_task;
            ctx.hle.tasks[tid].status = TaskStatus::Sleeping(wake_at);
            // Don't advance PC — re-execute waveout_write when we wake up
            let ra = ctx.cpu.gpr(31);
            ctx.cpu.pc = ra;
            ctx.cpu.next_pc = ra.wrapping_add(4);
            ctx.hle.schedule(ctx.cpu);
            return;
        }

        let mut samples = [0i16; WAVEOUT_SAMPLES];
        for i in 0..WAVEOUT_SAMPLES {
            samples[i] = ctx.mem.read_u16(buf_ptr + (i as u32) * 2) as i16;
        }
        let _ = queue.queue_audio(&samples);
    }
    ctx.cpu.set_gpr(2, 0);
}

fn hle_waveout_can_write(ctx: &mut EmuCtx) {
    if let Some(ref queue) = ctx.sdl.audio_queue {
        let queued = queue.size();
        ctx.cpu.set_gpr(2, if queued < WAVEOUT_MAX_QUEUE { 1 } else { 0 });
    } else {
        ctx.cpu.set_gpr(2, 0);
    }
}

fn hle_waveout_close(ctx: &mut EmuCtx) {
    if let Some(queue) = ctx.sdl.audio_queue.take() {
        queue.pause();
        eprintln!("[AUDIO] waveout_close");
    }
    ctx.cpu.set_gpr(2, 0);
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

/// Format a C-style printf string using guest memory and CPU register state.
/// `first_arg` is the vararg slot index of the first format argument (e.g. 1 for sprintf, 2 for snprintf).
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
                // Flags
                let mut flag_minus = false;
                let mut flag_zero = false;
                let mut flag_plus = false;
                let mut flag_space = false;
                let mut flag_hash = false;
                loop {
                    match chars.peek() {
                        Some('-') => { flag_minus = true; chars.next(); }
                        Some('0') => { flag_zero = true; chars.next(); }
                        Some('+') => { flag_plus = true; chars.next(); }
                        Some(' ') => { flag_space = true; chars.next(); }
                        Some('#') => { flag_hash = true; chars.next(); }
                        _ => break,
                    }
                }

                // Width (* or digits)
                let width: Option<usize> = if matches!(chars.peek(), Some('*')) {
                    chars.next();
                    let w = read_vararg(cpu, mem, arg_idx) as i32;
                    arg_idx += 1;
                    if w < 0 { flag_minus = true; Some((-w) as usize) }
                    else { Some(w as usize) }
                } else {
                    let mut w = String::new();
                    while matches!(chars.peek(), Some('0'..='9')) { w.push(chars.next().unwrap()); }
                    w.parse().ok()
                };

                // Precision
                let precision: Option<usize> = if matches!(chars.peek(), Some('.')) {
                    chars.next();
                    if matches!(chars.peek(), Some('*')) {
                        chars.next();
                        let p = read_vararg(cpu, mem, arg_idx) as i32;
                        arg_idx += 1;
                        Some(p.max(0) as usize)
                    } else {
                        let mut p = String::new();
                        while matches!(chars.peek(), Some('0'..='9')) { p.push(chars.next().unwrap()); }
                        Some(p.parse().unwrap_or(0))
                    }
                } else {
                    None
                };

                // Length modifier
                while matches!(chars.peek(), Some('l' | 'h' | 'z' | 'j' | 't')) { chars.next(); }

                // Format the raw value (no padding yet)
                // Don't pre-read val — %f needs two slots with alignment
                let (raw, is_negative) = match chars.next() {
                    Some('d' | 'i') => {
                        let v = read_vararg(cpu, mem, arg_idx) as i32;
                        arg_idx += 1;
                        (format!("{}", v.abs()), v < 0)
                    }
                    Some('u') => {
                        let v = read_vararg(cpu, mem, arg_idx);
                        arg_idx += 1;
                        (format!("{}", v), false)
                    }
                    Some('x') => {
                        let v = read_vararg(cpu, mem, arg_idx);
                        arg_idx += 1;
                        let s = format!("{:x}", v);
                        (if flag_hash && v != 0 { format!("0x{s}") } else { s }, false)
                    }
                    Some('X') => {
                        let v = read_vararg(cpu, mem, arg_idx);
                        arg_idx += 1;
                        let s = format!("{:X}", v);
                        (if flag_hash && v != 0 { format!("0X{s}") } else { s }, false)
                    }
                    Some('o') => {
                        let v = read_vararg(cpu, mem, arg_idx);
                        arg_idx += 1;
                        let s = format!("{:o}", v);
                        (if flag_hash && v != 0 { format!("0{s}") } else { s }, false)
                    }
                    Some('c') => {
                        let v = read_vararg(cpu, mem, arg_idx);
                        arg_idx += 1;
                        (String::from(char::from(v as u8)), false)
                    }
                    Some('s') => {
                        let v = read_vararg(cpu, mem, arg_idx);
                        arg_idx += 1;
                        let mut s = mem.read_string(v);
                        if let Some(prec) = precision {
                            s.truncate(prec);
                        }
                        (s, false)
                    }
                    Some('p') => {
                        let v = read_vararg(cpu, mem, arg_idx);
                        arg_idx += 1;
                        (format!("0x{:08x}", v), false)
                    }
                    Some('f' | 'e' | 'g') => {
                        // MIPS o32: doubles are aligned to even-numbered arg slots
                        if arg_idx % 2 != 0 { arg_idx += 1; }
                        let lo = read_vararg(cpu, mem, arg_idx);
                        let hi = read_vararg(cpu, mem, arg_idx + 1);
                        arg_idx += 2;
                        let bits = ((hi as u64) << 32) | (lo as u64);
                        let fval = f64::from_bits(bits);
                        let prec = precision.unwrap_or(6);
                        let neg = fval < 0.0;
                        (format!("{:.prec$}", fval.abs()), neg)
                    }
                    Some(other) => { result.push('%'); result.push(other); continue; }
                    None => { result.push('%'); continue; }
                };

                // Build final padded string
                let sign = if is_negative { "-" }
                    else if flag_plus { "+" }
                    else if flag_space { " " }
                    else { "" };

                let content = format!("{sign}{raw}");
                let w = width.unwrap_or(0);
                if content.len() >= w {
                    result.push_str(&content);
                } else {
                    let pad = w - content.len();
                    if flag_minus {
                        // left-align
                        result.push_str(&content);
                        for _ in 0..pad { result.push(' '); }
                    } else if flag_zero {
                        // zero-pad: sign then zeros then digits
                        result.push_str(sign);
                        for _ in 0..pad { result.push('0'); }
                        result.push_str(&raw);
                    } else {
                        // right-align with spaces
                        for _ in 0..pad { result.push(' '); }
                        result.push_str(&content);
                    }
                }
            }
        }
    }
    result
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::Memory;
    use crate::mips::Cpu;

    const FMT_ADDR: u32 = HLE_BASE + 0x0002_0000;
    const STR_ADDR: u32 = HLE_BASE + 0x0002_1000;

    /// Set up CPU + Memory with a format string and up to 8 varargs.
    /// Returns the formatted result using first_arg=0 (args start at $a0).
    fn fmt(format: &str, args: &[u32]) -> String {
        let mut mem = Memory::new();
        let mut cpu = Cpu::new();
        cpu.set_gpr(29, HLE_BASE + 0x0001_0000); // $sp

        // Write format string
        for (i, b) in format.bytes().enumerate() {
            mem.write_u8(FMT_ADDR + i as u32, b);
        }
        mem.write_u8(FMT_ADDR + format.len() as u32, 0);

        // Set args: $a0..$a3 then stack
        for (i, &val) in args.iter().enumerate() {
            match i {
                0 => cpu.set_gpr(4, val),
                1 => cpu.set_gpr(5, val),
                2 => cpu.set_gpr(6, val),
                3 => cpu.set_gpr(7, val),
                _ => {
                    let sp = cpu.gpr(29);
                    mem.write_u32(sp + 16 + ((i - 4) as u32) * 4, val);
                }
            }
        }

        format_guest_string(&mem, &cpu, FMT_ADDR, 0)
    }

    /// Helper: write a C string to guest memory, return its address.
    fn fmt_with_str(format: &str, s: &str, args: &[u32]) -> String {
        let mut mem = Memory::new();
        let mut cpu = Cpu::new();
        cpu.set_gpr(29, HLE_BASE + 0x0001_0000);

        // Write format string
        for (i, b) in format.bytes().enumerate() {
            mem.write_u8(FMT_ADDR + i as u32, b);
        }
        mem.write_u8(FMT_ADDR + format.len() as u32, 0);

        // Write the string argument
        for (i, b) in s.bytes().enumerate() {
            mem.write_u8(STR_ADDR + i as u32, b);
        }
        mem.write_u8(STR_ADDR + s.len() as u32, 0);

        // Build full args list with STR_ADDR replacing the first arg
        let mut full_args = vec![STR_ADDR];
        full_args.extend_from_slice(args);

        for (i, &val) in full_args.iter().enumerate() {
            match i {
                0 => cpu.set_gpr(4, val),
                1 => cpu.set_gpr(5, val),
                2 => cpu.set_gpr(6, val),
                3 => cpu.set_gpr(7, val),
                _ => {
                    let sp = cpu.gpr(29);
                    mem.write_u32(sp + 16 + ((i - 4) as u32) * 4, val);
                }
            }
        }

        format_guest_string(&mem, &cpu, FMT_ADDR, 0)
    }

    #[test]
    fn test_sprintf_plain_text() {
        assert_eq!(fmt("hello world", &[]), "hello world");
    }

    #[test]
    fn test_sprintf_percent_escape() {
        assert_eq!(fmt("100%%", &[]), "100%");
    }

    #[test]
    fn test_sprintf_decimal() {
        assert_eq!(fmt("%d", &[42]), "42");
        assert_eq!(fmt("%d", &[(-1i32) as u32]), "-1");
        assert_eq!(fmt("%d", &[0]), "0");
    }

    #[test]
    fn test_sprintf_unsigned() {
        assert_eq!(fmt("%u", &[42]), "42");
        assert_eq!(fmt("%u", &[0xFFFF_FFFF]), "4294967295");
    }

    #[test]
    fn test_sprintf_hex() {
        assert_eq!(fmt("%x", &[0xFF]), "ff");
        assert_eq!(fmt("%X", &[0xFF]), "FF");
        assert_eq!(fmt("%x", &[0]), "0");
    }

    #[test]
    fn test_sprintf_octal() {
        assert_eq!(fmt("%o", &[8]), "10");
        assert_eq!(fmt("%o", &[0]), "0");
    }

    #[test]
    fn test_sprintf_char() {
        assert_eq!(fmt("%c", &[65]), "A");
    }

    #[test]
    fn test_sprintf_pointer() {
        assert_eq!(fmt("%p", &[0x80A0_0000]), "0x80a00000");
    }

    #[test]
    fn test_sprintf_string() {
        assert_eq!(fmt_with_str("%s", "hello", &[]), "hello");
    }

    // ── Width / padding ────────────────────────────────────────────────

    #[test]
    fn test_sprintf_width_right_align() {
        assert_eq!(fmt("%5d", &[42]), "   42");
        assert_eq!(fmt("%10d", &[(-1i32) as u32]), "        -1");
    }

    #[test]
    fn test_sprintf_width_left_align() {
        assert_eq!(fmt("%-5d", &[42]), "42   ");
    }

    #[test]
    fn test_sprintf_zero_pad() {
        assert_eq!(fmt("%04x", &[0xA]), "000a");
        assert_eq!(fmt("%08X", &[0xDEAD]), "0000DEAD");
        assert_eq!(fmt("%05d", &[42]), "00042");
    }

    #[test]
    fn test_sprintf_zero_pad_negative() {
        assert_eq!(fmt("%06d", &[(-42i32) as u32]), "-00042");
    }

    // ── Flags ──────────────────────────────────────────────────────────

    #[test]
    fn test_sprintf_plus_flag() {
        assert_eq!(fmt("%+d", &[42]), "+42");
        assert_eq!(fmt("%+d", &[(-42i32) as u32]), "-42");
    }

    #[test]
    fn test_sprintf_space_flag() {
        assert_eq!(fmt("% d", &[42]), " 42");
        assert_eq!(fmt("% d", &[(-42i32) as u32]), "-42");
    }

    #[test]
    fn test_sprintf_hash_flag() {
        assert_eq!(fmt("%#x", &[0xFF]), "0xff");
        assert_eq!(fmt("%#X", &[0xFF]), "0XFF");
        assert_eq!(fmt("%#o", &[8]), "010");
        // # with zero value: no prefix
        assert_eq!(fmt("%#x", &[0]), "0");
    }

    // ── Precision ──────────────────────────────────────────────────────

    #[test]
    fn test_sprintf_string_precision() {
        assert_eq!(fmt_with_str("%.3s", "hello", &[]), "hel");
        assert_eq!(fmt_with_str("%.10s", "hi", &[]), "hi");
    }

    // ── Multiple args ──────────────────────────────────────────────────

    #[test]
    fn test_sprintf_multiple_args() {
        assert_eq!(fmt("%d + %d = %d", &[1, 2, 3]), "1 + 2 = 3");
    }

    #[test]
    fn test_sprintf_mixed_types() {
        assert_eq!(fmt("%d 0x%04x", &[255, 255]), "255 0x00ff");
    }

    // ── Float ──────────────────────────────────────────────────────────

    #[test]
    fn test_sprintf_float() {
        let bits = f64::to_bits(3.14);
        let lo = bits as u32;
        let hi = (bits >> 32) as u32;
        // %f with even-aligned args: arg0=lo, arg1=hi
        assert_eq!(fmt("%f", &[lo, hi]), "3.140000");
    }

    #[test]
    fn test_sprintf_float_precision() {
        let bits = f64::to_bits(3.14159);
        let lo = bits as u32;
        let hi = (bits >> 32) as u32;
        assert_eq!(fmt("%.2f", &[lo, hi]), "3.14");
    }

    #[test]
    fn test_sprintf_float_negative() {
        let bits = f64::to_bits(-1.5);
        let lo = bits as u32;
        let hi = (bits >> 32) as u32;
        assert_eq!(fmt("%f", &[lo, hi]), "-1.500000");
    }

    #[test]
    fn test_sprintf_float_alignment() {
        // If first arg is an int, float should align to even slot
        // %d consumes slot 0, then %f should skip slot 1, use slots 2+3
        let bits = f64::to_bits(2.5);
        let lo = bits as u32;
        let hi = (bits >> 32) as u32;
        assert_eq!(fmt("%d %f", &[42, 0xDEAD, lo, hi]), "42 2.500000");
    }

    // ── Star width/precision ───────────────────────────────────────────

    #[test]
    fn test_sprintf_star_width() {
        assert_eq!(fmt("%*d", &[5, 42]), "   42");
    }

    #[test]
    fn test_sprintf_star_precision() {
        // %.*s: arg0 = precision (3), arg1 = string pointer
        let mut mem = Memory::new();
        let mut cpu = Cpu::new();
        cpu.set_gpr(29, HLE_BASE + 0x0001_0000);
        for (i, b) in b"%.*s".iter().enumerate() {
            mem.write_u8(FMT_ADDR + i as u32, *b);
        }
        mem.write_u8(FMT_ADDR + 4, 0);
        for (i, b) in b"hello".iter().enumerate() {
            mem.write_u8(STR_ADDR + i as u32, *b);
        }
        mem.write_u8(STR_ADDR + 5, 0);
        cpu.set_gpr(4, 3);        // precision
        cpu.set_gpr(5, STR_ADDR); // string
        assert_eq!(format_guest_string(&mem, &cpu, FMT_ADDR, 0), "hel");
    }

    // ── Width smaller than value (no truncation) ───────────────────────

    #[test]
    fn test_sprintf_width_no_truncate() {
        assert_eq!(fmt("%2d", &[12345]), "12345");
    }

    // ── Task system ───────────────────────────────────────────────────────

    fn make_hle() -> HleState {
        HleState {
            handlers: Vec::new(),
            names: Vec::new(),
            heap_ptr: HLE_BASE + 0x1800_0000,
            alloc_sizes: HashMap::new(),
            sem_counter: 0,
            framebuf_addr: LCD_FRAMEBUF,
            buttons: 0,
            quit: false,
            start_time: Instant::now(),
            suppress: HashMap::new(),
            frame_count: 0,
            tasks: Vec::new(),
            current_task: 0,
            semaphores: HashMap::new(),
            context_switched: false,
        }
    }

    const T_CODE: u32 = HLE_BASE + 0x00A0_0000;
    const T_SP: u32 = HLE_BASE + 0x0001_0000;

    #[test]
    fn test_init_main_task() {
        let mut hle = make_hle();
        let mut cpu = Cpu::new();
        cpu.pc = T_CODE + 0x01A4;
        cpu.next_pc = T_CODE + 0x01A8;
        cpu.set_gpr(29, T_SP);
        hle.init_main_task(&cpu);

        assert_eq!(hle.tasks.len(), 1);
        assert_eq!(hle.tasks[0].priority, 0);
        assert_eq!(hle.tasks[0].status, TaskStatus::Running);
        assert_eq!(hle.tasks[0].state.pc, T_CODE + 0x01A4);
        assert_eq!(hle.current_task, 0);
    }

    #[test]
    fn test_task_create() {
        let mut hle = make_hle();
        let mut cpu = Cpu::new();
        cpu.pc = T_CODE + 0x0100;
        cpu.next_pc = T_CODE + 0x0104;
        hle.init_main_task(&cpu);

        let task_fn = T_CODE + 0x4_7770;
        let task_arg = HLE_BASE + 0x1800_1000;
        let stack_top = HLE_BASE + 0x00B4_472C;
        cpu.set_gpr(4, task_fn);
        cpu.set_gpr(5, task_arg);
        cpu.set_gpr(6, stack_top);
        cpu.set_gpr(7, 16);

        let prio = (cpu.gpr(7) & 0xFF) as u8;

        let mut state = SavedCpuState {
            gpr: [0; 32],
            pc: task_fn,
            next_pc: task_fn.wrapping_add(4),
            hi: 0,
            lo: 0,
        };
        state.gpr[4] = task_arg;
        state.gpr[29] = stack_top;
        state.gpr[31] = TASK_SENTINEL;

        hle.tasks.push(Task {
            status: TaskStatus::Ready,
            priority: prio,
            state,
        });

        assert_eq!(hle.tasks.len(), 2);
        assert_eq!(hle.tasks[1].priority, 16);
        assert_eq!(hle.tasks[1].status, TaskStatus::Ready);
        assert_eq!(hle.tasks[1].state.pc, task_fn);
        assert_eq!(hle.tasks[1].state.gpr[4], task_arg);
        assert_eq!(hle.tasks[1].state.gpr[29], stack_top);
        assert_eq!(hle.tasks[1].state.gpr[31], TASK_SENTINEL);
    }

    #[test]
    fn test_semaphore_create() {
        let mut hle = make_hle();

        // Create semaphore with count=1
        hle.sem_counter += 1;
        let addr = HLE_BASE + 0x00E0_0000 + hle.sem_counter * 4;
        hle.semaphores.insert(addr, Semaphore { count: 1 });

        assert_eq!(addr, HLE_BASE + 0x00E0_0004);
        assert_eq!(hle.semaphores[&addr].count, 1);
    }

    #[test]
    fn test_semaphore_pend_available() {
        let mut hle = make_hle();

        // Create sem with count=1
        let sem_addr = HLE_BASE + 0x00E0_0004;
        hle.semaphores.insert(sem_addr, Semaphore { count: 1 });

        // Pend — should decrement and return immediately
        let sem = hle.semaphores.get_mut(&sem_addr).unwrap();
        assert!(sem.count > 0);
        sem.count -= 1;
        assert_eq!(hle.semaphores[&sem_addr].count, 0);
    }

    #[test]
    fn test_semaphore_post_wakes_waiter() {
        let mut hle = make_hle();
        let mut cpu = Cpu::new();
        cpu.pc = T_CODE + 0x0100;
        cpu.next_pc = T_CODE + 0x0104;
        hle.init_main_task(&cpu);

        // Create a second task that's waiting on a semaphore
        let sem_addr = HLE_BASE + 0x00E0_0004;
        hle.semaphores.insert(sem_addr, Semaphore { count: 0 });
        hle.tasks.push(Task {
            status: TaskStatus::WaitSem(sem_addr),
            priority: 16,
            state: SavedCpuState {
                gpr: [0; 32],
                pc: T_CODE + 0x4_7800,
                next_pc: T_CODE + 0x4_7804,
                hi: 0,
                lo: 0,
            },
        });

        // Post the semaphore — should wake the waiter
        let sem = hle.semaphores.get_mut(&sem_addr).unwrap();
        sem.count += 1;

        // Find and wake highest-priority waiter
        let mut best: Option<usize> = None;
        for (i, t) in hle.tasks.iter().enumerate() {
            if t.status == TaskStatus::WaitSem(sem_addr) {
                if best.is_none() || t.priority < hle.tasks[best.unwrap()].priority {
                    best = Some(i);
                }
            }
        }
        assert_eq!(best, Some(1));
        hle.semaphores.get_mut(&sem_addr).unwrap().count -= 1;
        hle.tasks[1].status = TaskStatus::Ready;

        assert_eq!(hle.tasks[1].status, TaskStatus::Ready);
        assert_eq!(hle.semaphores[&sem_addr].count, 0);
    }

    #[test]
    fn test_schedule_picks_highest_priority() {
        let mut hle = make_hle();
        let mut cpu = Cpu::new();
        cpu.pc = T_CODE + 0x0100;
        cpu.next_pc = T_CODE + 0x0104;
        cpu.set_gpr(29, T_SP);
        hle.init_main_task(&cpu);

        // Add two ready tasks with different priorities
        hle.tasks.push(Task {
            status: TaskStatus::Ready,
            priority: 20,
            state: SavedCpuState {
                gpr: [0; 32],
                pc: T_CODE + 0x4_0000,
                next_pc: T_CODE + 0x4_0004,
                hi: 0,
                lo: 0,
            },
        });
        hle.tasks.push(Task {
            status: TaskStatus::Ready,
            priority: 10,
            state: SavedCpuState {
                gpr: [0; 32],
                pc: T_CODE + 0x5_0000,
                next_pc: T_CODE + 0x5_0004,
                hi: 0,
                lo: 0,
            },
        });

        // Mark main task as sleeping so scheduler picks from the others
        hle.tasks[0].status = TaskStatus::Sleeping(u32::MAX);
        hle.schedule(&mut cpu);

        // Should pick task 2 (priority 10, lowest = highest priority)
        assert_eq!(hle.current_task, 2);
        assert_eq!(cpu.pc, T_CODE + 0x5_0000);
        assert_eq!(hle.tasks[2].status, TaskStatus::Running);
    }

    #[test]
    fn test_schedule_skips_dead_tasks() {
        let mut hle = make_hle();
        let mut cpu = Cpu::new();
        cpu.pc = T_CODE + 0x0100;
        cpu.next_pc = T_CODE + 0x0104;
        cpu.set_gpr(29, T_SP);
        hle.init_main_task(&cpu);

        // Add a dead task and a ready task
        hle.tasks.push(Task {
            status: TaskStatus::Dead,
            priority: 5,
            state: SavedCpuState {
                gpr: [0; 32],
                pc: 0xDEAD_BEEF,
                next_pc: 0xDEAD_BEF3,
                hi: 0,
                lo: 0,
            },
        });
        hle.tasks.push(Task {
            status: TaskStatus::Ready,
            priority: 10,
            state: SavedCpuState {
                gpr: [0; 32],
                pc: T_CODE + 0x5_0000,
                next_pc: T_CODE + 0x5_0004,
                hi: 0,
                lo: 0,
            },
        });

        hle.tasks[0].status = TaskStatus::Sleeping(u32::MAX);
        hle.schedule(&mut cpu);

        // Should pick task 2, skipping the dead task 1
        assert_eq!(hle.current_task, 2);
        assert_eq!(cpu.pc, T_CODE + 0x5_0000);
    }

    #[test]
    fn test_task_returned_marks_dead() {
        let mut hle = make_hle();
        let mut cpu = Cpu::new();
        cpu.pc = T_CODE + 0x0100;
        cpu.next_pc = T_CODE + 0x0104;
        cpu.set_gpr(29, T_SP);
        hle.init_main_task(&cpu);

        // Add a second task (currently running)
        hle.tasks.push(Task {
            status: TaskStatus::Running,
            priority: 16,
            state: SavedCpuState {
                gpr: [0; 32],
                pc: T_CODE + 0x4_7770,
                next_pc: T_CODE + 0x4_7774,
                hi: 0,
                lo: 0,
            },
        });
        hle.tasks[0].status = TaskStatus::Ready;
        hle.current_task = 1;
        cpu.pc = TASK_SENTINEL;

        hle.task_returned(&mut cpu);

        assert_eq!(hle.tasks[1].status, TaskStatus::Dead);
        // Should have switched back to task 0
        assert_eq!(hle.current_task, 0);
    }

    #[test]
    fn test_context_switch_saves_and_restores() {
        let mut hle = make_hle();
        let mut cpu = Cpu::new();

        // Set up task 0 with distinctive register values
        cpu.pc = T_CODE + 0x0100;
        cpu.next_pc = T_CODE + 0x0104;
        cpu.set_gpr(4, 0x1111);
        cpu.set_gpr(29, T_SP);
        cpu.hi = 0xAAAA;
        cpu.lo = 0xBBBB;
        hle.init_main_task(&cpu);

        // Add task 1 with different register values
        let mut t1_gpr = [0u32; 32];
        t1_gpr[4] = 0x2222;
        t1_gpr[29] = HLE_BASE + 0x00B4_472C;
        hle.tasks.push(Task {
            status: TaskStatus::Ready,
            priority: 16,
            state: SavedCpuState {
                gpr: t1_gpr,
                pc: T_CODE + 0x4_7770,
                next_pc: T_CODE + 0x4_7774,
                hi: 0xCCCC,
                lo: 0xDDDD,
            },
        });

        // Mark task 0 as sleeping, trigger schedule
        hle.tasks[0].status = TaskStatus::Sleeping(u32::MAX);
        hle.schedule(&mut cpu);

        // CPU should now have task 1's state
        assert_eq!(hle.current_task, 1);
        assert_eq!(cpu.pc, T_CODE + 0x4_7770);
        assert_eq!(cpu.gpr(4), 0x2222);
        assert_eq!(cpu.gpr(29), HLE_BASE + 0x00B4_472C);
        assert_eq!(cpu.hi, 0xCCCC);
        assert_eq!(cpu.lo, 0xDDDD);

        // Task 0's state should be saved
        assert_eq!(hle.tasks[0].state.pc, T_CODE + 0x0100);
        assert_eq!(hle.tasks[0].state.gpr[4], 0x1111);
        assert_eq!(hle.tasks[0].state.hi, 0xAAAA);
        assert_eq!(hle.tasks[0].state.lo, 0xBBBB);
    }

    #[test]
    fn test_semaphore_pend_blocks_and_post_wakes() {
        let mut hle = make_hle();
        let mut cpu = Cpu::new();
        cpu.pc = T_CODE + 0x0100;
        cpu.next_pc = T_CODE + 0x0104;
        cpu.set_gpr(29, T_SP);
        hle.init_main_task(&cpu);

        // Create sem with count=0
        let sem_addr = HLE_BASE + 0x00E0_0004;
        hle.semaphores.insert(sem_addr, Semaphore { count: 0 });

        // Task 1 waiting on this sem
        hle.tasks.push(Task {
            status: TaskStatus::WaitSem(sem_addr),
            priority: 16,
            state: SavedCpuState {
                gpr: [0; 32],
                pc: T_CODE + 0x4_7800,
                next_pc: T_CODE + 0x4_7804,
                hi: 0,
                lo: 0,
            },
        });

        // Post: increment count, wake waiter
        let sem = hle.semaphores.get_mut(&sem_addr).unwrap();
        sem.count += 1;
        // Find waiter
        let mut best: Option<usize> = None;
        for (i, t) in hle.tasks.iter().enumerate() {
            if t.status == TaskStatus::WaitSem(sem_addr) {
                if best.is_none() || t.priority < hle.tasks[best.unwrap()].priority {
                    best = Some(i);
                }
            }
        }
        if let Some(waiter) = best {
            hle.semaphores.get_mut(&sem_addr).unwrap().count -= 1;
            hle.tasks[waiter].status = TaskStatus::Ready;
        }

        assert_eq!(hle.tasks[1].status, TaskStatus::Ready);
        assert_eq!(hle.semaphores[&sem_addr].count, 0);
    }

    #[test]
    fn test_sem_del_wakes_all_waiters() {
        let mut hle = make_hle();
        let sem_addr = HLE_BASE + 0x00E0_0008;
        hle.semaphores.insert(sem_addr, Semaphore { count: 0 });

        let mut cpu = Cpu::new();
        cpu.pc = T_CODE + 0x0100;
        cpu.next_pc = T_CODE + 0x0104;
        hle.init_main_task(&cpu);

        // Two tasks waiting on the same sem
        for pc in [T_CODE + 0x4_0000, T_CODE + 0x5_0000] {
            hle.tasks.push(Task {
                status: TaskStatus::WaitSem(sem_addr),
                priority: 16,
                state: SavedCpuState {
                    gpr: [0; 32],
                    pc,
                    next_pc: pc + 4,
                    hi: 0,
                    lo: 0,
                },
            });
        }

        // Delete sem — should wake all
        for t in &mut hle.tasks {
            if t.status == TaskStatus::WaitSem(sem_addr) {
                t.status = TaskStatus::Ready;
            }
        }
        hle.semaphores.remove(&sem_addr);

        assert_eq!(hle.tasks[1].status, TaskStatus::Ready);
        assert_eq!(hle.tasks[2].status, TaskStatus::Ready);
        assert!(!hle.semaphores.contains_key(&sem_addr));
    }

    #[test]
    fn test_task_del_by_priority() {
        let mut hle = make_hle();
        let mut cpu = Cpu::new();
        cpu.pc = T_CODE + 0x0100;
        cpu.next_pc = T_CODE + 0x0104;
        hle.init_main_task(&cpu);

        hle.tasks.push(Task {
            status: TaskStatus::Ready,
            priority: 16,
            state: SavedCpuState {
                gpr: [0; 32],
                pc: T_CODE + 0x4_7770,
                next_pc: T_CODE + 0x4_7774,
                hi: 0,
                lo: 0,
            },
        });

        // Delete by priority 16
        for t in &mut hle.tasks {
            if t.priority == 16 && t.status != TaskStatus::Dead {
                t.status = TaskStatus::Dead;
                break;
            }
        }

        assert_eq!(hle.tasks[1].status, TaskStatus::Dead);
    }

    #[test]
    fn test_task_sentinel_value() {
        let hle = make_hle();
        assert_eq!(hle.task_sentinel(), 0xDEAD_0004);
    }
}
