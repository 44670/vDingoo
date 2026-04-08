//! AOT-compiled functions for qiye.app
//!
//! Native Rust reimplementations of hot MIPS functions, called instead of
//! interpreting. Each function takes `&mut Cpu, &mut Memory`, reads args
//! from GPRs, operates on guest memory, and sets PC = $ra on return.

use crate::mem::Memory;
use crate::mips::Cpu;

const SIN_TABLE_BASE: u32 = 0x80ae_ddf0;

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
