//! AOT-compiled functions for qiye.app
//!
//! Native Rust reimplementations of hot MIPS functions, called instead of
//! interpreting. Each function takes `&mut Cpu, &mut Memory`, reads args
//! from GPRs, operates on guest memory, and sets PC = $ra on return.

use crate::mem::Memory;
use crate::mips::Cpu;

const SIN_TABLE_BASE: u32 = 0x80ae_ddf0;

#[cfg(not(feature = "reloc"))]
const LCD_FRAMEBUF: u32 = 0x80F0_0000;
#[cfg(feature = "reloc")]
const LCD_FRAMEBUF: u32 = 0x08F0_0000;

/// 16.16 fixed-point signed multiply (matches MIPS mult/madd/mflo sequence)
#[inline(always)]
fn fpmul(a: i32, b: i32) -> i32 {
    ((a as i64).wrapping_mul(b as i64) >> 16) as i32
}

/// Lookup from sin/interpolation table in guest memory
#[inline(always)]
fn lookup_sin(mem: &Memory, index: u32) -> i32 {
    mem.read_u32(SIN_TABLE_BASE.wrapping_add((index & 0xffff) << 2)) as i32
}

/// Write one textured+fogged pixel to the framebuffer
#[inline(always)]
fn write_pixel(
    mem: &mut Memory,
    dst: u32,
    fog: i32,
    fog_u: i32,
    fog_v: i32,
    tex_base: u32,
    s7: u32,
    mask: i32,
) {
    let alpha = ((fog >> 8) << 16) as u32;
    let u_coord = (fog_u >> 16) as i16 as i32;
    let v_coord = (fog_v >> s7) & mask;
    let tex_addr = tex_base.wrapping_add((u_coord.wrapping_add(v_coord) << 1) as u32);
    let texel = mem.read_u16(tex_addr) as u32;
    mem.write_u32(dst, alpha | texel);
}

/// AOT: Renderer_drawTexturedSpans (0x80a86c40, 3572 bytes, 23 basic blocks)
///
/// Draws textured spans with fog/lightmap lookup. Processes spans in 16-pixel
/// chunks with per-pixel fog_u/fog_v interpolation. Calls lookupSin for
/// fog intensity and easing curves.
///
/// Original signature: void Renderer_drawTexturedSpans(Renderer* a0, SpanList* a1)
pub fn renderer_draw_textured_spans(cpu: &mut Cpu, mem: &mut Memory) {
    let renderer = cpu.gpr[4]; // $a0
    let span_ptr = cpu.gpr[5]; // $a1

    // ── Setup (mirroring Block 1) ───────────────────────────────────────────

    // Per-chunk (16 pixel) texture coordinate steps
    let u_step = (mem.read_u32(renderer.wrapping_add(0x2a264)) as i32) << 4;
    let v_step = (mem.read_u32(renderer.wrapping_add(0x2a26c)) as i32) << 4;

    // Fog step: per-pixel and per-chunk (×16)
    let ppf_step = mem.read_u32(span_ptr.wrapping_add(0x18)) as i32;
    let fog_chunk_step = ppf_step << 4;

    // Texture addressing
    let tex_base = mem.read_u32(renderer.wrapping_add(0x2a28c));
    let shift = mem.read_u32(renderer.wrapping_add(0x2a2a0));
    let mask = 0xFFFF_FFFFu32.wrapping_shl(shift) as i32;
    let s7 = 16u32.wrapping_sub(shift); // right-shift amount for v coord

    // Transform matrix coefficients
    let mat_ux = mem.read_u32(renderer.wrapping_add(0x2a264)) as i32;
    let mat_uy = mem.read_u32(renderer.wrapping_add(0x2a268)) as i32;
    let mat_vx = mem.read_u32(renderer.wrapping_add(0x2a26c)) as i32;
    let mat_vy = mem.read_u32(renderer.wrapping_add(0x2a270)) as i32;
    let u_origin = mem.read_u32(renderer.wrapping_add(0x2a274)) as i32;
    let v_origin = mem.read_u32(renderer.wrapping_add(0x2a278)) as i32;

    // Fog/lightmap bounds
    let fog_u_base = mem.read_u32(renderer.wrapping_add(0x2a27c)) as i32;
    let fog_v_base = mem.read_u32(renderer.wrapping_add(0x2a280)) as i32;
    let fog_u_max = mem.read_u32(renderer.wrapping_add(0x2a284)) as i32;
    let fog_v_max = mem.read_u32(renderer.wrapping_add(0x2a288)) as i32;

    // Screen origin (16.16 fixed-point)
    let screen_ox = mem.read_u32(renderer.wrapping_add(0x2a254)) as i32;
    let screen_oy = mem.read_u32(renderer.wrapping_add(0x2a258)) as i32;

    // Fog plane coefficients from span struct
    let fog_init = mem.read_u32(span_ptr.wrapping_add(0x14)) as i32;
    let fog_dx = mem.read_u32(span_ptr.wrapping_add(0x18)) as i32;
    let fog_dy = mem.read_u32(span_ptr.wrapping_add(0x1c)) as i32;

    // ── Outer span-entry loop (linked list at span_ptr + 0x24) ──────────────

    let mut span_entry = mem.read_u32(span_ptr.wrapping_add(0x24));

    while span_entry != 0 {
        // ── Block 20: per-entry setup ───────────────────────────────────────

        let fb_width = mem.read_u32(renderer.wrapping_add(0x40)) as i32;
        let entry_ptr = span_entry;
        let x = mem.read_u32(entry_ptr) as i32;
        let y = mem.read_u32(entry_ptr.wrapping_add(4)) as i32;
        let total_count = mem.read_u32(entry_ptr.wrapping_add(8)) as i32;

        // Destination: chase pointer chain to framebuffer
        //   *(*(*(*(renderer+8) + 0x1a374) + 0x1a68) + 0x4c)
        let raster = mem.read_u32(renderer.wrapping_add(8));
        let p1 = mem.read_u32(raster.wrapping_add(0x1a374));
        let p2 = mem.read_u32(p1.wrapping_add(0x1a68));
        let framebuf = mem.read_u32(p2.wrapping_add(0x4c));
        let mut dst = framebuf
            .wrapping_add((fb_width.wrapping_mul(y).wrapping_add(x) << 2) as u32);

        // Screen-relative position (16.16 fixed-point)
        let dx = (x << 16).wrapping_sub(screen_ox);
        let dy = (y << 16).wrapping_sub(screen_oy);

        // Initial texture coordinates
        let mut tex_u = u_origin
            .wrapping_add(fpmul(dx, mat_ux))
            .wrapping_add(fpmul(dy, mat_uy));
        let mut tex_v = v_origin
            .wrapping_add(fpmul(dx, mat_vx))
            .wrapping_add(fpmul(dy, mat_vy));

        // Initial fog value
        let mut fog = fog_init
            .wrapping_add(fpmul(dx, fog_dx))
            .wrapping_add(fpmul(dy, fog_dy));

        // Fog lightmap coordinates via sin lookup
        let sin_val = lookup_sin(mem, (fog as u32) >> 7) << 3;
        let mut fog_u = fpmul(tex_u, sin_val).wrapping_add(fog_u_base);
        let mut fog_v = fpmul(tex_v, sin_val).wrapping_add(fog_v_base);

        // Initial clamp: [0, max]
        if fog_u > fog_u_max {
            fog_u = fog_u_max;
        }
        if fog_u < 0 {
            fog_u = 0;
        }
        if fog_v > fog_v_max {
            fog_v = fog_v_max;
        }
        if fog_v < 0 {
            fog_v = 0;
        }

        let mut count = total_count;

        // ── Chunk loop (Block 14) ───────────────────────────────────────────

        loop {
            // k = min(count, 16); count -= k
            let k = if count < 16 { count } else { 16 };
            count -= k;

            if count != 0 {
                // ── Full chunk, more to come (Block 3) ──────────────────────

                tex_u = tex_u.wrapping_add(u_step);
                tex_v = tex_v.wrapping_add(v_step);

                // Predict fog at end of chunk for lightmap lookup
                let end_sin =
                    lookup_sin(mem, (fog.wrapping_add(fog_chunk_step) as u32) >> 7) << 3;
                let mut new_fog_u = fpmul(tex_u, end_sin).wrapping_add(fog_u_base);
                let mut new_fog_v = fpmul(tex_v, end_sin).wrapping_add(fog_v_base);

                // Clamp: [16, max]
                if new_fog_u > fog_u_max {
                    new_fog_u = fog_u_max;
                }
                if new_fog_u < 16 {
                    new_fog_u = 16;
                }
                if new_fog_v > fog_v_max {
                    new_fog_v = fog_v_max;
                }
                if new_fog_v < 16 {
                    new_fog_v = 16;
                }

                // Per-pixel deltas (÷16 via arithmetic shift)
                let u_delta = new_fog_u.wrapping_sub(fog_u) >> 4;
                let v_delta = new_fog_v.wrapping_sub(fog_v) >> 4;

                // ── Pixel loop (Block 18) ───────────────────────────────────
                for _ in 0..k {
                    write_pixel(mem, dst, fog, fog_u, fog_v, tex_base, s7, mask);
                    dst = dst.wrapping_add(4);
                    fog_u = fog_u.wrapping_add(u_delta);
                    fog_v = fog_v.wrapping_add(v_delta);
                    fog = fog.wrapping_add(ppf_step);
                }

                // Carry over end values (Block 8)
                fog_u = new_fog_u;
                fog_v = new_fog_v;
            } else if k < 2 {
                // ── 0 or 1 pixel remaining (Block 2 → Block 4/9) ───────────

                if k == 1 {
                    write_pixel(mem, dst, fog, fog_u, fog_v, tex_base, s7, mask);
                }
                break;
            } else {
                // ── Last chunk, k ≥ 2 (Block 5) ────────────────────────────

                let frac = (k - 1) << 16;

                // Partial tex coord advance
                tex_u = tex_u.wrapping_add(fpmul(mat_ux, frac));
                tex_v = tex_v.wrapping_add(fpmul(mat_vx, frac));

                // Fog prediction at last pixel of chunk
                let fog_advance = fpmul(ppf_step, frac);
                let end_sin =
                    lookup_sin(mem, (fog.wrapping_add(fog_advance) as u32) >> 7) << 3;
                let mut new_fog_u = fpmul(tex_u, end_sin).wrapping_add(fog_u_base);
                let mut new_fog_v = fpmul(tex_v, end_sin).wrapping_add(fog_v_base);

                // Clamp: [16, max]
                if new_fog_u > fog_u_max {
                    new_fog_u = fog_u_max;
                }
                if new_fog_u < 16 {
                    new_fog_u = 16;
                }
                if new_fog_v > fog_v_max {
                    new_fog_v = fog_v_max;
                }
                if new_fog_v < 16 {
                    new_fog_v = 16;
                }

                // Easing-based per-pixel delta (sin table interpolation)
                let easing_idx = (((k - 1) as u32) << 12) & 0xf000;
                let easing = lookup_sin(mem, easing_idx) >> 2;
                let u_delta = fpmul(new_fog_u.wrapping_sub(fog_u), easing);
                let v_delta = fpmul(new_fog_v.wrapping_sub(fog_v), easing);

                // ── Pixel loop (Block 22) ───────────────────────────────────
                for _ in 0..k {
                    write_pixel(mem, dst, fog, fog_u, fog_v, tex_base, s7, mask);
                    dst = dst.wrapping_add(4);
                    fog_u = fog_u.wrapping_add(u_delta);
                    fog_v = fog_v.wrapping_add(v_delta);
                    fog = fog.wrapping_add(ppf_step);
                }

                break;
            }

            if count <= 0 {
                break;
            }
        }

        // Next span entry (Block 15): linked list via offset 0x0c
        span_entry = mem.read_u32(entry_ptr.wrapping_add(12));
    }

    // Return: PC = $ra
    cpu.pc = cpu.gpr[31];
    cpu.next_pc = cpu.pc.wrapping_add(4);
}

/// AOT: Raster_copyBuffer16to32 (0x80a5d648, 52 bytes, 3 basic blocks)
///
/// Copies width*height pixels from a u32 source buffer to a u16 destination,
/// truncating each 32-bit value to 16-bit (keeping low halfword).
///
/// Original: void Raster_copyBuffer16to32(Raster* a0, u32* a1_src, u16* a2_dst)
pub fn raster_copy_buffer_16to32(cpu: &mut Cpu, mem: &mut Memory) {
    let raster = cpu.gpr[4]; // $a0
    let mut src = cpu.gpr[5]; // $a1 — u32* source
    let mut dst = cpu.gpr[6]; // $a2 — u16* destination

    let height = mem.read_u32(raster.wrapping_add(8)) as i32;
    let width = mem.read_u32(raster.wrapping_add(4)) as i32;
    let count = width.wrapping_mul(height);

    for _ in 0..count {
        let val = mem.read_u32(src);
        mem.write_u16(dst, val as u16);
        src = src.wrapping_add(4);
        dst = dst.wrapping_add(2);
    }

    cpu.pc = cpu.gpr[31];
    cpu.next_pc = cpu.pc.wrapping_add(4);
}

/// AOT: scanline_texturedRGB_fogF (0x80a4a1dc, 232 bytes, 7 basic blocks)
///
/// Textured scanline rasterizer with fog. For each row, iterates columns
/// sampling a paletted texture (u8→u16 via palette), ORs fog color, writes u32.
///
/// Original: int scanline_texturedRGB_fogF(ScanlineState* a0)
pub fn scanline_textured_rgb_fog_f(cpu: &mut Cpu, mem: &mut Memory) {
    let scan = cpu.gpr[4]; // $a0

    let width = (mem.read_u32(scan.wrapping_add(0x44)) as i32)
        .wrapping_sub(mem.read_u32(scan.wrapping_add(0x3c)) as i32); // x_end - x_start
    let height = (mem.read_u32(scan.wrapping_add(0x48)) as i32)
        .wrapping_sub(mem.read_u32(scan.wrapping_add(0x40)) as i32); // y_end - y_start

    let initial_v = mem.read_u32(scan.wrapping_add(0x30)) as i32;
    let mut t0 = initial_v << 16;

    if height <= 0 {
        cpu.set_gpr(2, 0);
        cpu.pc = cpu.gpr[31];
        cpu.next_pc = cpu.pc.wrapping_add(4);
        return;
    }

    let u_step = mem.read_u32(scan.wrapping_add(0x4c)) as i32;
    let v_step = mem.read_u32(scan.wrapping_add(0x50)) as i32;
    let fog_color = mem.read_u32(scan.wrapping_add(0x58));
    let stride = mem.read_u32(scan) as i32;
    let tex_offset = mem.read_u32(scan.wrapping_add(0x2c)) as i32;

    for _ in 0..height {
        if width > 0 {
            let src_row = mem.read_u32(scan.wrapping_add(0x28));
            let dst_base = mem.read_u32(scan.wrapping_add(0x20));
            let tex_info = mem.read_u32(scan.wrapping_add(0x10));
            let palette = mem.read_u32(tex_info.wrapping_add(0x54));

            let mut u_accum: i32 = 0;
            for col in 0..width {
                let tex_idx_addr = src_row.wrapping_add((u_accum >> 16) as u32);
                let tex_idx = mem.read_u8(tex_idx_addr) as u32;
                let color = mem.read_u16(palette.wrapping_add(tex_idx << 1)) as u32;
                let pixel = color | fog_color;
                let dst_addr = dst_base.wrapping_add((col as u32) << 2);
                mem.write_u32(dst_addr, pixel);
                u_accum = u_accum.wrapping_add(u_step);
            }
        }

        // Advance to next row
        let dst_ptr = mem.read_u32(scan.wrapping_add(0x20));
        mem.write_u32(
            scan.wrapping_add(0x20),
            dst_ptr.wrapping_add((stride << 2) as u32),
        );

        t0 = t0.wrapping_add(v_step);

        // Recompute source row pointer
        let tex_info = mem.read_u32(scan.wrapping_add(0x10));
        let tex_data = mem.read_u32(tex_info.wrapping_add(0x4c));
        let pitch = mem.read_u32(tex_info.wrapping_add(0x28)) as i32;
        let v = t0 >> 16;
        let new_src = tex_data.wrapping_add(v.wrapping_mul(pitch).wrapping_add(tex_offset) as u32);
        mem.write_u32(scan.wrapping_add(0x28), new_src);
    }

    cpu.set_gpr(2, 0);
    cpu.pc = cpu.gpr[31];
    cpu.next_pc = cpu.pc.wrapping_add(4);
}

/// AOT: Raster_presentFramebuffer (0x80a486e4, 136 bytes, 5 basic blocks)
///
/// Copies the internal render buffer (u16 RGB565 pixels) to the LCD
/// framebuffer, then calls lcd_set_frame (HLE) to present.
///
/// This AOT replaces the 76800-iteration copy loop with a native memcpy,
/// then sets PC to the lcd_set_frame tail so the interpreter handles HLE.
///
/// Original signature: int Raster_presentFramebuffer(Raster* a0, ?)
pub fn raster_present_framebuffer(cpu: &mut Cpu, mem: &mut Memory) {
    let raster = cpu.gpr[4]; // $a0

    // Block 1: check dirty flag
    let dirty = mem.read_u8(raster.wrapping_add(8));
    if dirty == 0 {
        // Early return: not dirty, return 0
        cpu.set_gpr(2, 0); // $v0 = 0
        cpu.pc = cpu.gpr[31];
        cpu.next_pc = cpu.pc.wrapping_add(4);
        return;
    }

    // ── Simulate prologue (so interpreter can run epilogue) ──────────────
    let sp = cpu.gpr[29].wrapping_sub(0x18);
    mem.write_u32(sp.wrapping_add(0x14), cpu.gpr[31]); // save $ra
    mem.write_u32(sp.wrapping_add(0x10), cpu.gpr[16]); // save $s0
    cpu.gpr[29] = sp;
    cpu.gpr[16] = raster; // $s0 = raster

    // Block 3: clear dirty flag, get LCD framebuffer
    mem.write_u8(raster.wrapping_add(8), 0);
    mem.write_u32(raster.wrapping_add(0xc), LCD_FRAMEBUF);

    // Native bulk copy: src=*(raster+4), dst=LCD_FRAMEBUF, count=width*height*2
    let src = mem.read_u32(raster.wrapping_add(4));
    let width = mem.read_u32(raster.wrapping_add(0x10)) as i32;
    let height = mem.read_u32(raster.wrapping_add(0x14)) as i32;
    let pixel_count = width.wrapping_mul(height);

    if pixel_count > 0 {
        let byte_count = (pixel_count as u32) << 1;
        mem.guest_memcpy(LCD_FRAMEBUF, src, byte_count as usize);
    }

    // Set PC to the lcd_set_frame tail (0x80a4874c: jal lcd_set_frame_wrapper)
    // The interpreter will handle: HLE call → set dirty=1 → epilogue → return
    cpu.pc = 0x80a4874c;
    cpu.next_pc = 0x80a48750;
}

/// AOT: scanline_zrwTexturedPalKey_opaque (0x80a55b18, 304 bytes, 12 basic blocks)
///
/// Z-buffered textured scanline with palette color-key transparency.
/// Iterates spans (0x18 bytes each, sentinel 0xFE7961), per-pixel:
///   - Z-test against zbuffer (skip if behind)
///   - Texture coord → byte index → palette lookup (u16)
///   - Color key test (skip if texel == transparent index)
///   - Write (z_masked | palette_color) to zbuffer
///
/// Original: void scanline_zrwTexturedPalKey_opaque(Renderer* a0, SpanList* a1)
pub fn scanline_zrw_textured_pal_key_opaque(cpu: &mut Cpu, mem: &mut Memory) {
    let renderer = cpu.gpr[4]; // $a0
    let mut span_ptr = cpu.gpr[5]; // $a1

    const SENTINEL: u32 = 0xFFFE_7961;

    // Block 1: load texture data pointer (renderer->texture->data)
    let tex_info = mem.read_u32(renderer.wrapping_add(0x1a6c));
    let tex_data = mem.read_u32(tex_info.wrapping_add(0x4c));

    // Check first span sentinel
    let first = mem.read_u32(span_ptr);
    if first == SENTINEL {
        cpu.pc = cpu.gpr[31];
        cpu.next_pc = cpu.pc.wrapping_add(4);
        return;
    }

    // Texture addressing masks/shifts
    let mask_1db8 = mem.read_u32(renderer.wrapping_add(0x1db8));
    let shift_1dbc = mem.read_u32(renderer.wrapping_add(0x1dbc));
    let mask_1db4 = mem.read_u32(renderer.wrapping_add(0x1db4));

    // Color key (transparent index)
    let color_key = mem.read_u8(renderer.wrapping_add(0x1e50));

    // Per-pixel advance steps
    let t2_step = mem.read_u32(renderer.wrapping_add(0x1ddc)) as i32;
    let t1_step = mem.read_u32(renderer.wrapping_add(0x1de0)) as i32;
    let t0_step = mem.read_u32(renderer.wrapping_add(0x1dd8)) as i32;

    loop {
        let count = zrw_span_setup(renderer, span_ptr, mem);

        if count > 0 {
            // Block 7: load span fields
            let mut zbuf_ptr = mem.read_u32(span_ptr.wrapping_add(4)); // $a3 = zbuf
            let mut t2 = mem.read_u32(span_ptr.wrapping_add(0x0c)) as i32; // $t2
            let mut t1 = mem.read_u32(span_ptr.wrapping_add(0x10)) as i32; // $t1
            let mut t0 = mem.read_u32(span_ptr.wrapping_add(0x08)) as i32; // $t0

            // Block 11: inner pixel loop
            for _ in 0..count {
                // Z-test: (t0 << 8) & 0xffff0000 vs zbuf[pixel]
                let z_masked = ((t0 << 8) as u32) & 0xffff_0000;
                let zbuf_val = mem.read_u32(zbuf_ptr);

                if z_masked >= zbuf_val {
                    // Block 10: texture coordinate computation
                    let tex_v = ((t1 as u32) & mask_1db8) << (shift_1dbc & 31);
                    let tex_u = (t2 as u32) & mask_1db4;
                    let tex_offset = (tex_v.wrapping_add(tex_u)) >> 16;
                    let texel_addr = tex_data.wrapping_add(tex_offset);
                    let texel_idx = mem.read_u8(texel_addr);

                    // Color key test
                    if texel_idx != color_key {
                        // Block 12: palette lookup and write
                        let pal_ptr = mem.read_u32(
                            mem.read_u32(renderer.wrapping_add(0x1a6c)).wrapping_add(0x54),
                        );
                        let pal_offset = ((texel_idx as u32) << 6) | 0x1e;
                        let pal_color = mem.read_u16(pal_ptr.wrapping_add(pal_offset)) as u32;
                        mem.write_u32(zbuf_ptr, z_masked | pal_color);
                    }
                }

                // Block 9: advance
                zbuf_ptr = zbuf_ptr.wrapping_add(4);
                t2 = t2.wrapping_add(t2_step);
                t1 = t1.wrapping_add(t1_step);
                t0 = t0.wrapping_add(t0_step);
            }
        }

        // Block 6: next span
        span_ptr = span_ptr.wrapping_add(0x18);
        let next = mem.read_u32(span_ptr);
        if next == SENTINEL {
            break;
        }
    }

    cpu.pc = cpu.gpr[31];
    cpu.next_pc = cpu.pc.wrapping_add(4);
}

/// AOT: softfloat_mul (0x80ad26e0, 488 bytes, 383 callers)
///
/// Software IEEE 754 single-precision multiply. Replaced with native f32 mul.
pub fn softfloat_mul(cpu: &mut Cpu, _mem: &mut Memory) {
    let a = f32::from_bits(cpu.gpr[4]);
    let b = f32::from_bits(cpu.gpr[5]);
    cpu.set_gpr(2, (a * b).to_bits());
    cpu.pc = cpu.gpr[31];
    cpu.next_pc = cpu.pc.wrapping_add(4);
}

/// AOT: softfloat_div (0x80ad28d0, 356 bytes, 115 callers)
///
/// Software IEEE 754 single-precision divide. Replaced with native f32 div.
pub fn softfloat_div(cpu: &mut Cpu, _mem: &mut Memory) {
    let a = f32::from_bits(cpu.gpr[4]);
    let b = f32::from_bits(cpu.gpr[5]);
    cpu.set_gpr(2, (a / b).to_bits());
    cpu.pc = cpu.gpr[31];
    cpu.next_pc = cpu.pc.wrapping_add(4);
}

/// AOT: softfloat_pack (0x80ad7420, 360 bytes, 27 basic blocks)
///
/// Packs an unpacked float struct {class, sign, exponent, mantissa_with_guard}
/// into IEEE 754 single-precision format with round-to-nearest-even.
///
/// Struct at $a0: [0]=class (0/1=normal, 2=zero, 4=inf/nan), [1]=sign, [2]=exp, [3]=mantissa
pub fn softfloat_pack(cpu: &mut Cpu, mem: &mut Memory) {
    let struct_ptr = cpu.gpr[4];
    let arg3 = cpu.gpr[6]; // $a2 template
    let class = mem.read_u32(struct_ptr);
    let sign = mem.read_u32(struct_ptr.wrapping_add(4));
    let mut mantissa = mem.read_u32(struct_ptr.wrapping_add(12));
    let mut exp_out: u32 = 0;

    if class < 2 {
        // Normal number
        mantissa |= 0x0010_0000;
        let exp = mem.read_u32(struct_ptr.wrapping_add(8)) as i32;

        if exp < -126 {
            // Denormal
            let shift = (-126 - exp) as u32;
            if shift < 26 {
                let sticky = if (mantissa & ((1u32 << shift) - 1)) != 0 { 1u32 } else { 0 };
                mantissa = (mantissa >> shift) | sticky;
            } else {
                mantissa = 0;
            }
            // Round denormal
            let guard = mantissa & 0x7f;
            if guard == 0x40 {
                if (mantissa & 0x80) != 0 {
                    mantissa = mantissa.wrapping_add(0x40);
                }
            } else {
                mantissa = mantissa.wrapping_add(0x3f);
            }
            if mantissa > 0x3fff_ffff {
                exp_out = 1;
            }
            mantissa >>= 7;
        } else if exp < 128 {
            // Normal range
            exp_out = (exp + 127) as u32;
            let guard = mantissa & 0x7f;
            if guard == 0x40 {
                if (mantissa & 0x80) != 0 {
                    mantissa = mantissa.wrapping_add(0x40);
                }
            } else {
                mantissa = mantissa.wrapping_add(0x3f);
            }
            if (mantissa as i32) < 0 {
                mantissa >>= 1;
                exp_out = exp_out.wrapping_add(1);
            }
            mantissa >>= 7;
        } else {
            // Overflow → infinity
            exp_out = 0xff;
            mantissa = 0;
        }
    } else if class == 4 {
        exp_out = 0xff;
        mantissa = 0;
    } else if class == 2 {
        mantissa = 0;
    } else if mantissa != 0 {
        // Unknown class with nonzero mantissa: load exponent and process
        let exp = mem.read_u32(struct_ptr.wrapping_add(8)) as i32;
        if exp < -126 {
            let shift = (-126 - exp) as u32;
            if shift < 26 {
                let sticky = if (mantissa & ((1u32 << shift) - 1)) != 0 { 1u32 } else { 0 };
                mantissa = (mantissa >> shift) | sticky;
            } else {
                mantissa = 0;
            }
            let guard = mantissa & 0x7f;
            if guard == 0x40 {
                if (mantissa & 0x80) != 0 {
                    mantissa = mantissa.wrapping_add(0x40);
                }
            } else {
                mantissa = mantissa.wrapping_add(0x3f);
            }
            if mantissa > 0x3fff_ffff {
                exp_out = 1;
            }
            mantissa >>= 7;
        } else if exp < 128 {
            exp_out = (exp + 127) as u32;
            let guard = mantissa & 0x7f;
            if guard == 0x40 {
                if (mantissa & 0x80) != 0 {
                    mantissa = mantissa.wrapping_add(0x40);
                }
            } else {
                mantissa = mantissa.wrapping_add(0x3f);
            }
            if (mantissa as i32) < 0 {
                mantissa >>= 1;
                exp_out = exp_out.wrapping_add(1);
            }
            mantissa >>= 7;
        } else {
            exp_out = 0xff;
            mantissa = 0;
        }
    }
    // else: class unknown, mantissa == 0 → pack as zero with exp_out=0

    // Pack: sign | exponent | mantissa
    let mant_bits = mantissa & 0x007f_ffff;
    let template_bits = arg3 & 0xff80_0000;
    let combined = (template_bits | mant_bits) & 0x807f_ffff;
    let with_exp = combined | ((exp_out & 0xff) << 23);
    let result = (with_exp & 0x7fff_ffff) | (sign << 31);

    cpu.set_gpr(2, result);
    cpu.pc = cpu.gpr[31];
    cpu.next_pc = cpu.pc.wrapping_add(4);
}

/// AOT: vec3_dot (0x80a08814, 392 bytes, 1 basic block, 90 callers)
///
/// 16.16 fixed-point dot product: *result = a.x*b.x + a.y*b.y + a.z*b.z
/// Returns result pointer in $v0.
pub fn vec3_dot(cpu: &mut Cpu, mem: &mut Memory) {
    let result_ptr = cpu.gpr[4];
    let a_ptr = cpu.gpr[5];
    let b_ptr = cpu.gpr[6];

    let ax = mem.read_u32(a_ptr) as i32;
    let bx = mem.read_u32(b_ptr) as i32;
    let ay = mem.read_u32(a_ptr.wrapping_add(4)) as i32;
    let by = mem.read_u32(b_ptr.wrapping_add(4)) as i32;
    let az = mem.read_u32(a_ptr.wrapping_add(8)) as i32;
    let bz = mem.read_u32(b_ptr.wrapping_add(8)) as i32;

    let dot = fpmul(ax, bx)
        .wrapping_add(fpmul(ay, by))
        .wrapping_add(fpmul(az, bz));

    mem.write_u32(result_ptr, dot as u32);
    cpu.set_gpr(2, result_ptr);
    cpu.pc = cpu.gpr[31];
    cpu.next_pc = cpu.pc.wrapping_add(4);
}

/// Shared span iteration + position tracking for z-buffered scanlines.
#[inline(always)]
fn zrw_span_setup(renderer: u32, span_ptr: u32, mem: &mut Memory) -> i32 {
    let pos = mem.read_u32(renderer.wrapping_add(0x1dc0)) as i32;
    let span_x = mem.read_u32(span_ptr) as i32;
    let count = pos.wrapping_sub(span_x);

    let dir_accum = mem.read_u32(renderer.wrapping_add(0x1dc4)) as i32;
    let dir_delta = mem.read_u32(renderer.wrapping_add(0x1dc8)) as i32;
    let new_dir = dir_accum.wrapping_add(dir_delta);
    mem.write_u32(renderer.wrapping_add(0x1dc4), new_dir as u32);

    if new_dir >= 0 {
        let dir_step = mem.read_u32(renderer.wrapping_add(0x1dd4)) as i32;
        mem.write_u32(renderer.wrapping_add(0x1dc0), pos.wrapping_add(dir_step) as u32);
        let sub_val = mem.read_u32(renderer.wrapping_add(0x1dcc)) as i32;
        mem.write_u32(renderer.wrapping_add(0x1dc4), new_dir.wrapping_sub(sub_val) as u32);
    } else {
        let add_val = mem.read_u32(renderer.wrapping_add(0x1dd0)) as i32;
        mem.write_u32(renderer.wrapping_add(0x1dc0), pos.wrapping_add(add_val) as u32);
    }

    count
}

/// AOT: scanline_zrwTexturedPalFog_opaque (0x80a553c4, 316 bytes, 11 blocks)
///
/// Z-buffered textured scanline with fog-based palette lookup (no transparency).
/// Palette index = (texel << 5 | (fog >> 8)) << 1
pub fn scanline_zrw_textured_pal_fog_opaque(cpu: &mut Cpu, mem: &mut Memory) {
    let renderer = cpu.gpr[4];
    let mut span_ptr = cpu.gpr[5];
    const SENTINEL: u32 = 0xFFFE_7961;

    let tex_info = mem.read_u32(renderer.wrapping_add(0x1a6c));
    let tex_data = mem.read_u32(tex_info.wrapping_add(0x4c));

    if mem.read_u32(span_ptr) == SENTINEL {
        cpu.pc = cpu.gpr[31];
        cpu.next_pc = cpu.pc.wrapping_add(4);
        return;
    }

    let mask_1db8 = mem.read_u32(renderer.wrapping_add(0x1db8));
    let shift_1dbc = mem.read_u32(renderer.wrapping_add(0x1dbc));
    let mask_1db4 = mem.read_u32(renderer.wrapping_add(0x1db4));
    let t3_step = mem.read_u32(renderer.wrapping_add(0x1ddc)) as i32;
    let t2_step = mem.read_u32(renderer.wrapping_add(0x1de0)) as i32;
    let t1_step = mem.read_u32(renderer.wrapping_add(0x1de4)) as i32;
    let t0_step = mem.read_u32(renderer.wrapping_add(0x1dd8)) as i32;

    loop {
        let count = zrw_span_setup(renderer, span_ptr, mem);

        if count > 0 {
            let mut zbuf_ptr = mem.read_u32(span_ptr.wrapping_add(4));
            let mut t3 = mem.read_u32(span_ptr.wrapping_add(0x0c)) as i32;
            let mut t2 = mem.read_u32(span_ptr.wrapping_add(0x10)) as i32;
            let mut t1 = mem.read_u32(span_ptr.wrapping_add(0x14)) as i32;
            let mut t0 = mem.read_u32(span_ptr.wrapping_add(0x08)) as i32;

            for _ in 0..count {
                let z_masked = ((t0 << 8) as u32) & 0xffff_0000;
                let zbuf_val = mem.read_u32(zbuf_ptr);

                if z_masked >= zbuf_val {
                    let tex_v = ((t2 as u32) & mask_1db8) << (shift_1dbc & 31);
                    let tex_u = (t3 as u32) & mask_1db4;
                    let tex_offset = tex_v.wrapping_add(tex_u) >> 16;
                    let texel = mem.read_u8(tex_data.wrapping_add(tex_offset)) as u32;

                    let pal_idx = ((texel << 5) | ((t1 >> 8) as u32 & 0x1f)) << 1;
                    let pal_ptr = mem.read_u32(
                        mem.read_u32(renderer.wrapping_add(0x1a6c)).wrapping_add(0x54),
                    );
                    let color = mem.read_u16(pal_ptr.wrapping_add(pal_idx)) as u32;
                    mem.write_u32(zbuf_ptr, z_masked | color);
                }

                zbuf_ptr = zbuf_ptr.wrapping_add(4);
                t3 = t3.wrapping_add(t3_step);
                t2 = t2.wrapping_add(t2_step);
                t1 = t1.wrapping_add(t1_step);
                t0 = t0.wrapping_add(t0_step);
            }
        }

        span_ptr = span_ptr.wrapping_add(0x18);
        if mem.read_u32(span_ptr) == SENTINEL {
            break;
        }
    }

    cpu.pc = cpu.gpr[31];
    cpu.next_pc = cpu.pc.wrapping_add(4);
}

/// AOT: scanline_zrwTexturedPalFogKey_opaque (0x80a56228, 324 bytes, 12 blocks)
///
/// Z-buffered textured scanline with fog palette AND color-key transparency.
pub fn scanline_zrw_textured_pal_fog_key_opaque(cpu: &mut Cpu, mem: &mut Memory) {
    let renderer = cpu.gpr[4];
    let mut span_ptr = cpu.gpr[5];
    const SENTINEL: u32 = 0xFFFE_7961;

    let tex_info = mem.read_u32(renderer.wrapping_add(0x1a6c));
    let tex_data = mem.read_u32(tex_info.wrapping_add(0x4c));

    if mem.read_u32(span_ptr) == SENTINEL {
        cpu.pc = cpu.gpr[31];
        cpu.next_pc = cpu.pc.wrapping_add(4);
        return;
    }

    let mask_1db8 = mem.read_u32(renderer.wrapping_add(0x1db8));
    let shift_1dbc = mem.read_u32(renderer.wrapping_add(0x1dbc));
    let mask_1db4 = mem.read_u32(renderer.wrapping_add(0x1db4));
    let color_key = mem.read_u8(renderer.wrapping_add(0x1e50));
    let t3_step = mem.read_u32(renderer.wrapping_add(0x1ddc)) as i32;
    let t2_step = mem.read_u32(renderer.wrapping_add(0x1de0)) as i32;
    let t4_step = mem.read_u32(renderer.wrapping_add(0x1de4)) as i32;
    let t0_step = mem.read_u32(renderer.wrapping_add(0x1dd8)) as i32;

    loop {
        let count = zrw_span_setup(renderer, span_ptr, mem);

        if count > 0 {
            let mut zbuf_ptr = mem.read_u32(span_ptr.wrapping_add(4));
            let mut t3 = mem.read_u32(span_ptr.wrapping_add(0x0c)) as i32;
            let mut t2 = mem.read_u32(span_ptr.wrapping_add(0x10)) as i32;
            let mut t4 = mem.read_u32(span_ptr.wrapping_add(0x14)) as i32;
            let mut t0 = mem.read_u32(span_ptr.wrapping_add(0x08)) as i32;

            for _ in 0..count {
                let z_masked = ((t0 << 8) as u32) & 0xffff_0000;
                let zbuf_val = mem.read_u32(zbuf_ptr);

                if z_masked >= zbuf_val {
                    let tex_v = ((t2 as u32) & mask_1db8) << (shift_1dbc & 31);
                    let tex_u = (t3 as u32) & mask_1db4;
                    let tex_offset = tex_v.wrapping_add(tex_u) >> 16;
                    let texel = mem.read_u8(tex_data.wrapping_add(tex_offset));

                    if texel != color_key {
                        let pal_idx = (((texel as u32) << 5) | ((t4 >> 8) as u32 & 0x1f)) << 1;
                        let pal_ptr = mem.read_u32(
                            mem.read_u32(renderer.wrapping_add(0x1a6c)).wrapping_add(0x54),
                        );
                        let color = mem.read_u16(pal_ptr.wrapping_add(pal_idx)) as u32;
                        mem.write_u32(zbuf_ptr, z_masked | color);
                    }
                }

                zbuf_ptr = zbuf_ptr.wrapping_add(4);
                t3 = t3.wrapping_add(t3_step);
                t2 = t2.wrapping_add(t2_step);
                t4 = t4.wrapping_add(t4_step);
                t0 = t0.wrapping_add(t0_step);
            }
        }

        span_ptr = span_ptr.wrapping_add(0x18);
        if mem.read_u32(span_ptr) == SENTINEL {
            break;
        }
    }

    cpu.pc = cpu.gpr[31];
    cpu.next_pc = cpu.pc.wrapping_add(4);
}



/// AOT: softfloat_from_int (0x80ad2ad0, 180 bytes, 230 callers)
///
/// Converts int32 to IEEE 754 single-precision float. Replaced with native cast.
pub fn softfloat_from_int(cpu: &mut Cpu, _mem: &mut Memory) {
    let val = cpu.gpr[4] as i32;
    cpu.set_gpr(2, (val as f32).to_bits());
    cpu.pc = cpu.gpr[31];
    cpu.next_pc = cpu.pc.wrapping_add(4);
}

/// AOT: AnimTexture_update (0x80a931dc, 412 bytes, 20 basic blocks)
///
/// Updates animated texture: advances frame, computes blend factor (0-31),
/// then per-pixel RGB565 interpolation between source and target color.
/// Inlines AnimTexture_stop + DepthBuffer_clearAll for the end-of-animation path.
///
/// Original: int AnimTexture_update(AnimTex* a0, u32* a1_src, u16* a2_dst)
pub fn anim_texture_update(cpu: &mut Cpu, mem: &mut Memory) {
    let this = cpu.gpr[4]; // $a0
    let mut src = cpu.gpr[5]; // $a1 — u32* source pixels
    let dst_base = cpu.gpr[6]; // $a2 — u16* destination pixels

    // Check active flag
    if mem.read_u32(this.wrapping_add(0x20)) == 0 {
        cpu.set_gpr(2, 0);
        cpu.pc = cpu.gpr[31];
        cpu.next_pc = cpu.pc.wrapping_add(4);
        return;
    }

    // Frame tracking
    let current = mem.read_u32(this.wrapping_add(0x18));
    let total = mem.read_u32(this.wrapping_add(0x1c));

    if current != total {
        // Increment frame
        mem.write_u32(this.wrapping_add(0x18), current.wrapping_add(1));
    } else {
        // At end of animation
        let looping = mem.read_u32(this.wrapping_add(0x30));
        if looping == 0 {
            // Not looping — inline AnimTexture_stop
            mem.write_u32(this.wrapping_add(0x24), 1); // done = 1
            mem.write_u32(this.wrapping_add(0x20), 0); // active = 0
            if mem.read_u32(this.wrapping_add(0x28)) == 0 {
                // Forward: swap depth buffers and clear
                let renderer_ptr = mem.read_u32(this.wrapping_add(0x14));
                let v1 = mem.read_u32(renderer_ptr.wrapping_add(0x1a374));
                let flag = mem.read_u8(v1.wrapping_add(0x1e4f));
                let db = if flag == 0 {
                    mem.read_u32(v1.wrapping_add(0x1a80))
                } else {
                    mem.read_u32(v1.wrapping_add(0x1a84))
                };
                mem.write_u32(v1.wrapping_add(0x1a7c), db);
                // DepthBuffer_clearAll: *(db + 0x10) = 0
                mem.write_u32(db.wrapping_add(0x10), 0);
            }
        } else {
            // Looping — set done flag but continue
            mem.write_u32(this.wrapping_add(0x24), 1);
        }
    }

    // Re-read current frame (may have changed)
    let frame = mem.read_u32(this.wrapping_add(0x18)) as i32;

    // Blend factor: (frame * 31) / total_frames (0-31 range)
    let total_i = total as i32;
    if total_i == 0 {
        // Division by zero — original code hits break instruction
        // Just return to avoid crash
        cpu.pc = cpu.gpr[31];
        cpu.next_pc = cpu.pc.wrapping_add(4);
        return;
    }
    let blend = (frame.wrapping_mul(31)) / total_i;

    // If forward direction (reverse==0): invert blend
    let blend = if mem.read_u32(this.wrapping_add(0x28)) == 0 {
        32 - blend
    } else {
        blend
    };
    let blend_u = (blend & 0xff) as u32;

    // Pixel loop: width * height
    let height = mem.read_u32(this.wrapping_add(8)) as i32;
    let width = mem.read_u32(this.wrapping_add(4)) as i32;
    let count = height.wrapping_mul(width);

    if count <= 0 {
        cpu.set_gpr(2, 0);
        cpu.pc = cpu.gpr[31];
        cpu.next_pc = cpu.pc.wrapping_add(4);
        return;
    }

    let target_color = mem.read_u16(this.wrapping_add(0x2c)) as u32;
    let mut dst = dst_base;

    for _ in 0..count {
        let pixel = mem.read_u32(src);
        // Base copy: write low u16 to dest
        mem.write_u16(dst, pixel as u16);
        src = src.wrapping_add(4);

        let high = pixel & 0xffff_0000;
        if high != 0x6fff_0000 {
            // Not a marker pixel — apply blending
            if blend_u >= 2 {
                if blend_u >= 31 {
                    // Full target
                    mem.write_u16(dst, target_color as u16);
                } else {
                    // RGB565 interpolation
                    let tgt = target_color;
                    let tgt_exp = ((tgt & 0xf800) << 10)
                        | ((tgt & 0x7e0) << 5)
                        | (tgt & 0x1f);

                    let src_c = mem.read_u16(dst) as u32;
                    let src_exp = ((src_c & 0xf800) << 10)
                        | ((src_c & 0x7e0) << 5)
                        | (src_c & 0x1f);

                    let diff = tgt_exp.wrapping_sub(src_exp) as i32;
                    let blended = (diff.wrapping_mul(blend_u as i32))
                        .wrapping_add((src_exp << 5) as i32);
                    let blended = blended as u32;

                    let r = (blended & 0x7c00_0000) >> 15;
                    let g = (blended & 0x001f_8000) >> 10;
                    let b = (blended & 0x0000_03e0) >> 5;
                    mem.write_u16(dst, (r | g | b) as u16);
                }
            }
            // blend < 2: keep base copy as-is
        }

        dst = dst.wrapping_add(2);
    }

    cpu.set_gpr(2, 0);
    cpu.pc = cpu.gpr[31];
    cpu.next_pc = cpu.pc.wrapping_add(4);
}
