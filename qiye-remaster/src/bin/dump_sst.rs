// Dump BSP entity data and SST script commands for analysis.
// Usage: cargo run --bin dump_sst [bsp_name]
#[path = "../fs_app.rs"]
mod fs_app;

fn read_u16_le(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}
fn read_i16_le(d: &[u8], o: usize) -> i16 {
    i16::from_le_bytes([d[o], d[o + 1]])
}
fn read_u32_le(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}
fn read_i32_le(d: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}
fn read_f32_le(d: &[u8], o: usize) -> f32 {
    f32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}
fn fixed(v: i32) -> f32 {
    v as f32 / 65536.0
}

fn dump_bsp_entities(fs: &fs_app::AppFs, bsp_name: &str) {
    let Some(bsp) = fs.read(bsp_name) else {
        println!("BSP not found: {bsp_name}");
        return;
    };
    println!("=== BSP entities: {bsp_name} ===");

    // Vertex bounds
    let v_off = read_u32_le(bsp, 0x20) as usize;
    let v_size = read_u32_le(bsp, 0x24) as usize;
    let v_count = v_size / 12;
    let mut vmin = [f32::MAX; 3];
    let mut vmax = [f32::MIN; 3];
    for i in 0..v_count {
        for j in 0..3 {
            let v = read_f32_le(bsp, v_off + i * 12 + j * 4);
            vmin[j] = vmin[j].min(v);
            vmax[j] = vmax[j].max(v);
        }
    }
    println!(
        "Vertex bounds: ({:.1},{:.1},{:.1}) to ({:.1},{:.1},{:.1})",
        vmin[0], vmin[1], vmin[2], vmax[0], vmax[1], vmax[2]
    );

    // EPair lump (32-byte strings)
    let ep_off = read_u32_le(bsp, 0x80) as usize;
    let ep_size = read_u32_le(bsp, 0x84) as usize;
    let ep_count = ep_size / 32;
    let mut epairs = Vec::with_capacity(ep_count);
    for i in 0..ep_count {
        let off = ep_off + i * 32;
        let s = &bsp[off..off + 32];
        let nul = s.iter().position(|&b| b == 0).unwrap_or(32);
        epairs.push(String::from_utf8_lossy(&s[..nul]).into_owned());
    }
    println!("EPairs: {ep_count}");

    // Ent lump (16 bytes each)
    let ent_off = read_u32_le(bsp, 0x90) as usize;
    let ent_size = read_u32_le(bsp, 0x94) as usize;
    let ent_count = ent_size / 16;
    println!("Entities: {ent_count}");
    for i in 0..ent_count {
        let off = ent_off + i * 16;
        let ent_type = read_i16_le(bsp, off);
        let model = read_i16_le(bsp, off + 2);
        let epair_start = read_u16_le(bsp, off + 8) as usize;
        let epair_count = read_u16_le(bsp, off + 10) as usize;
        print!("  [{i:3}] type={ent_type:3} model={model:3} epairs=[{epair_start}..{}]", epair_start + epair_count);
        if epair_count > 0 && epair_start + epair_count <= epairs.len() {
            print!("  →");
            for j in epair_start..epair_start + epair_count {
                print!(" {}", epairs[j]);
            }
        }
        println!();
    }
}

fn dump_sst(fs: &fs_app::AppFs, sst_name: &str) {
    let Some(data) = fs.read(sst_name) else {
        println!("SST not found: {sst_name}");
        return;
    };
    println!("\n=== SST: {sst_name} ({} bytes) ===", data.len());

    // Hex dump header
    for (i, chunk) in data.chunks(16).enumerate().take(16) {
        print!("{:04x}: ", i * 16);
        for b in chunk {
            print!("{:02x} ", b);
        }
        print!(" |");
        for b in chunk {
            print!("{}", if *b >= 0x20 && *b < 0x7f { *b as char } else { '.' });
        }
        println!("|");
    }

    // Scan for command delimiters: 24 00 00 00 00 00
    println!("\nCommands:");
    let mut off = 0;
    while off + 12 < data.len() {
        if data[off] == 0x24 && data[off + 1..off + 6] == [0, 0, 0, 0, 0] {
            let cmd_id = read_u16_le(data, off + 6);
            let extra = read_u16_le(data, off + 8);
            let arg_count = read_u16_le(data, off + 10);
            if cmd_id < 300 && arg_count < 20 {
                print!("  0x{off:04x}: CMD {cmd_id} (0x{cmd_id:02x}) extra={extra} args={arg_count}");
                let mut aoff = off + 12;
                let mut args = Vec::new();
                for _ in 0..arg_count {
                    if aoff + 4 > data.len() { break; }
                    let atype = read_u16_le(data, aoff);
                    let asize = read_u16_le(data, aoff + 2);
                    match atype {
                        5 if asize == 4 && aoff + 8 <= data.len() => {
                            args.push(format!("{:.2}", fixed(read_i32_le(data, aoff + 4))));
                            aoff += 8;
                        }
                        2 if asize == 4 && aoff + 8 <= data.len() => {
                            args.push(format!("int:{}", read_i32_le(data, aoff + 4)));
                            aoff += 8;
                        }
                        3 if aoff + 4 + asize as usize <= data.len() => {
                            let nul = data[aoff + 4..aoff + 4 + asize as usize]
                                .iter().position(|&b| b == 0).unwrap_or(asize as usize);
                            args.push(format!("\"{}\"", String::from_utf8_lossy(&data[aoff + 4..aoff + 4 + nul])));
                            aoff += 4 + asize as usize;
                        }
                        4 if asize == 4 && aoff + 8 <= data.len() => {
                            args.push(format!("f:{}", read_i32_le(data, aoff + 4)));
                            aoff += 8;
                        }
                        _ => {
                            args.push(format!("?t{}:s{}", atype, asize));
                            aoff += 4 + asize as usize;
                        }
                    }
                }
                println!(" → [{}]", args.join(", "));
            }
            off += 2;
        } else {
            off += 2;
        }
    }
}

fn main() {
    let fs = fs_app::AppFs::open("../qiye.app");

    let bsp_name = std::env::args().nth(1).unwrap_or_else(|| ".\\day1\\0101.sbp".to_string());
    dump_bsp_entities(&fs, &bsp_name);

    // Dump SST files for same day
    let day_prefix = bsp_name.rsplit('\\').nth(1).unwrap_or("day1");
    for path in fs.list_files() {
        if path.contains(day_prefix) && path.ends_with(".sst") {
            dump_sst(&fs, path);
        }
    }
}
