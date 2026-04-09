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
        // Block 3: per-span setup — update renderer position tracking
        let pos = mem.read_u32(renderer.wrapping_add(0x1dc0)) as i32;
        let span_x = mem.read_u32(span_ptr) as i32;
        let count = pos.wrapping_sub(span_x); // $t3

        let dir_accum = mem.read_u32(renderer.wrapping_add(0x1dc4)) as i32;
        let dir_delta = mem.read_u32(renderer.wrapping_add(0x1dc8)) as i32;
        let new_dir = dir_accum.wrapping_add(dir_delta);
        mem.write_u32(renderer.wrapping_add(0x1dc4), new_dir as u32);

        if new_dir >= 0 {
            // Block 5: positive direction
            let dir_step = mem.read_u32(renderer.wrapping_add(0x1dd4)) as i32;
            let new_pos = pos.wrapping_add(dir_step);
            mem.write_u32(renderer.wrapping_add(0x1dc0), new_pos as u32);
            let sub_val = mem.read_u32(renderer.wrapping_add(0x1dcc)) as i32;
            mem.write_u32(
                renderer.wrapping_add(0x1dc4),
                new_dir.wrapping_sub(sub_val) as u32,
            );
        } else {
            // Block 4: negative direction
            let add_val = mem.read_u32(renderer.wrapping_add(0x1dd0)) as i32;
            let new_pos = pos.wrapping_add(add_val);
            mem.write_u32(renderer.wrapping_add(0x1dc0), new_pos as u32);
        }

        // Block 8: skip if count <= 0
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
