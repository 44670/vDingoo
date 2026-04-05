use crate::hle::{read_vararg, HleState};
use crate::mem::Memory;
use crate::mips::Cpu;

pub fn hle_malloc(cpu: &mut Cpu, _mem: &mut Memory, state: &mut HleState) {
    let size = cpu.gpr(4);
    // Align to 8 bytes
    let aligned = (state.heap_ptr + 7) & !7;
    state.alloc_sizes.insert(aligned, size);
    state.heap_ptr = aligned + size;
    cpu.set_gpr(2, aligned); // $v0 = pointer
}

pub fn hle_free(_cpu: &mut Cpu, _mem: &mut Memory, _state: &mut HleState) {
    // bump allocator — no-op
}

pub fn hle_realloc(cpu: &mut Cpu, mem: &mut Memory, state: &mut HleState) {
    let old_ptr = cpu.gpr(4);
    let new_size = cpu.gpr(5);

    // Allocate new block
    let aligned = (state.heap_ptr + 7) & !7;
    state.heap_ptr = aligned + new_size;

    // Copy old data if ptr != 0
    if old_ptr != 0 {
        if let Some(&old_size) = state.alloc_sizes.get(&old_ptr) {
            let copy_size = old_size.min(new_size) as usize;
            // Copy byte by byte (no overlap possible with bump allocator)
            for i in 0..copy_size {
                let b = mem.read_u8(old_ptr + i as u32);
                mem.write_u8(aligned + i as u32, b);
            }
        }
    }

    state.alloc_sizes.insert(aligned, new_size);
    cpu.set_gpr(2, aligned);
}

pub fn hle_printf(cpu: &mut Cpu, mem: &mut Memory, _state: &mut HleState) {
    let fmt_addr = cpu.gpr(4);
    let output = format_guest_string(mem, cpu, fmt_addr, 1);
    print!("{}", output);
    cpu.set_gpr(2, output.len() as u32);
}

pub fn hle_sprintf(cpu: &mut Cpu, mem: &mut Memory, _state: &mut HleState) {
    let buf_addr = cpu.gpr(4);
    let fmt_addr = cpu.gpr(5);
    let output = format_guest_string(mem, cpu, fmt_addr, 2);

    // Write to guest buffer
    for (i, b) in output.bytes().enumerate() {
        mem.write_u8(buf_addr + i as u32, b);
    }
    mem.write_u8(buf_addr + output.len() as u32, 0); // null terminate

    cpu.set_gpr(2, output.len() as u32);
}

pub fn hle_fprintf(cpu: &mut Cpu, mem: &mut Memory, _state: &mut HleState) {
    // arg0 = stream (ignored), arg1 = fmt, args start at 2
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

pub fn hle_strlen(cpu: &mut Cpu, mem: &mut Memory, _state: &mut HleState) {
    let s = cpu.gpr(4);
    let len = mem.read_string(s).len();
    cpu.set_gpr(2, len as u32);
}

pub fn hle_strncasecmp(cpu: &mut Cpu, mem: &mut Memory, _state: &mut HleState) {
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

pub fn hle_abort(_cpu: &mut Cpu, _mem: &mut Memory, _state: &mut HleState) {
    eprintln!("[HLE] abort() called");
    std::process::exit(1);
}

/// Parse a printf-style format string from guest memory and format using varargs.
/// `first_arg` is the vararg index of the first format argument (1 for printf, 2 for sprintf).
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
            Some('%') => {
                chars.next();
                result.push('%');
            }
            _ => {
                // Skip flags, width, precision
                while matches!(chars.peek(), Some('-' | '+' | ' ' | '0' | '#')) {
                    chars.next();
                }
                while matches!(chars.peek(), Some('0'..='9')) {
                    chars.next();
                }
                if matches!(chars.peek(), Some('.')) {
                    chars.next();
                    while matches!(chars.peek(), Some('0'..='9')) {
                        chars.next();
                    }
                }
                // Skip length modifiers
                while matches!(chars.peek(), Some('l' | 'h' | 'z')) {
                    chars.next();
                }

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
                    Some(other) => {
                        result.push('%');
                        result.push(other);
                    }
                    None => result.push('%'),
                }
            }
        }
    }

    result
}
