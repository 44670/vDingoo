# vDingoo

Rust user-space HLE emulator for **Dingoo A320** native apps (`.app` / CCDL format).

## Goal

Run Dingoo A320 `.app` binaries on desktop by emulating the MIPS32 CPU in
user-space and implementing the uC/OS-II SDK as high-level host-native stubs
(HLE). No hardware emulation, no kernel, no firmware dump needed.

## Architecture

- **CPU**: MIPS32R2 little-endian interpreter (or dynarec later)
- **Loader**: Parse CCDL format, load code at virtual address, patch import table
- **HLE stubs**: Implement each imported OS/SDK function natively:
  - LCD/framebuffer -> SDL2/wgpu window (320x240)
  - Audio (waveout/pcm) -> SDL2 audio or cpal
  - Filesystem (fsys_*) -> host filesystem passthrough
  - Input (_kbd_*) -> keyboard/gamepad mapping
  - RTOS (OSTaskCreate, OSSemPend, ...) -> host threads/sync primitives
  - C stdlib (malloc, printf, ...) -> libc or custom allocator in guest memory
- **Memory**: Flat 32-bit address space, guest memory mapped via Vec/mmap

## Key Files

- `CCDL.md` - Complete CCDL binary format specification
- `app2elf.py` - Converts `.app` to ELF for analysis in Binary Ninja / Ghidra / IDA
- `qiye.app` - Sample CCDL binary (七夜 / Seven Nights, a visual novel game)

## Reference Repos

- `../qemu-JZ` - QEMU fork with JZ4740/Pavo board support (C). Key files:
  - `hw/mips_jz.c` / `hw/mips_jz.h` - JZ4740 SoC peripherals
  - `hw/mips_jz_clk.c` - Clock/PLL emulation
  - `hw/mips_pavo.c` - Pavo dev board (similar to A320)
  - `target-mips/` - MIPS CPU emulation (translate.c, op_helper.c)
  - `target-mips/translate_init.c` - CPU definitions (jz4740 entry)
- `../mcps/binja` - Binary Ninja MCP server plugin (Python). For reverse engineering
  `.app` binaries via Claude Code
- `../binaryninja-api` - Binary Ninja C++ API / Python bindings reference

## CCDL Import Categories (HLE Surface)

These are the OS/SDK functions that `.app` binaries call. Each must be stubbed:

| Category | Functions | HLE Strategy |
|----------|-----------|--------------|
| C stdlib | malloc, free, printf, sprintf, strlen, fread, fwrite, fseek | Host libc on guest heap |
| LCD | LcdGetDisMode, _lcd_set_frame, lcd_flip | SDL2 texture blit |
| Filesystem | fsys_fopen, fsys_fread, fsys_fwrite, fsys_fseek, fsys_fclose | Host fs passthrough |
| Audio | waveout_open, waveout_write, pcm_ioctl | SDL2 audio queue |
| Input | _kbd_get_key, _kbd_get_status | SDL2 key events |
| RTOS | OSTaskCreate, OSTimeDly, OSSemPend/Post/Create | std::thread + Mutex |
| Cache | __icache_invalidate_all, __dcache_writeback_all | No-op |
| System | vxGoHome, GetTickCount, USB_Connect | Stub / host clock |

## Conventions

- Use Binary Ninja MCP tools to reverse-engineer `.app` binaries and discover
  undocumented SDK behavior
- Use `app2elf.py` to produce ELF files with symbols for better analysis
- The Dingoo A320 uses an Actions Semiconductor SoC (not Ingenic JZ4740), but
  `qemu-JZ` is the closest reference for MIPS32R2 SoC emulation patterns
- CCDL apps run in KSEG0 (0x80000000+), cached, unmapped virtual addresses
