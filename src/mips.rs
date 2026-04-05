use crate::mem::Memory;

pub enum StepResult {
    Ok,
    Break(u32),
}

pub struct Cpu {
    pub gpr: [u32; 32],
    pub pc: u32,
    pub next_pc: u32,
    pub hi: u32,
    pub lo: u32,
    pub insn_count: u64,
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            gpr: [0; 32],
            pc: 0,
            next_pc: 0,
            hi: 0,
            lo: 0,
            insn_count: 0,
        }
    }

    pub fn set_gpr(&mut self, r: u32, val: u32) {
        if r != 0 {
            self.gpr[r as usize] = val;
        }
    }

    pub fn gpr(&self, r: u32) -> u32 {
        self.gpr[r as usize]
    }

    pub fn step(&mut self, mem: &mut Memory) -> StepResult {
        let current_pc = self.pc;
        self.pc = self.next_pc;
        self.next_pc = self.pc.wrapping_add(4);
        self.insn_count += 1;

        let insn = mem.read_u32(current_pc);

        // NOP fast path
        if insn == 0 {
            return StepResult::Ok;
        }

        let opcode = (insn >> 26) & 0x3F;
        let rs = (insn >> 21) & 0x1F;
        let rt = (insn >> 16) & 0x1F;
        let rd = (insn >> 11) & 0x1F;
        let sa = (insn >> 6) & 0x1F;
        let funct = insn & 0x3F;
        let imm16 = (insn & 0xFFFF) as u16;
        let simm = imm16 as i16 as i32 as u32; // sign-extended
        let zimm = imm16 as u32; // zero-extended
        let target26 = insn & 0x03FF_FFFF;

        match opcode {
            // SPECIAL
            0x00 => match funct {
                // SLL
                0x00 => self.set_gpr(rd, self.gpr(rt).wrapping_shl(sa)),
                // SRL / ROTR
                0x02 => {
                    if rs & 1 == 0 {
                        self.set_gpr(rd, self.gpr(rt).wrapping_shr(sa));
                    } else {
                        self.set_gpr(rd, self.gpr(rt).rotate_right(sa));
                    }
                }
                // SRA
                0x03 => self.set_gpr(rd, (self.gpr(rt) as i32).wrapping_shr(sa) as u32),
                // SLLV
                0x04 => self.set_gpr(rd, self.gpr(rt).wrapping_shl(self.gpr(rs) & 0x1F)),
                // SRLV / ROTRV
                0x06 => {
                    let shift = self.gpr(rs) & 0x1F;
                    if sa & 1 == 0 {
                        self.set_gpr(rd, self.gpr(rt).wrapping_shr(shift));
                    } else {
                        self.set_gpr(rd, self.gpr(rt).rotate_right(shift));
                    }
                }
                // SRAV
                0x07 => {
                    let shift = self.gpr(rs) & 0x1F;
                    self.set_gpr(rd, (self.gpr(rt) as i32).wrapping_shr(shift) as u32);
                }
                // JR
                0x08 => {
                    self.next_pc = self.gpr(rs);
                }
                // JALR
                0x09 => {
                    let target = self.gpr(rs);
                    self.set_gpr(rd, current_pc.wrapping_add(8));
                    self.next_pc = target;
                }
                // MOVZ
                0x0A => {
                    if self.gpr(rt) == 0 {
                        self.set_gpr(rd, self.gpr(rs));
                    }
                }
                // MOVN
                0x0B => {
                    if self.gpr(rt) != 0 {
                        self.set_gpr(rd, self.gpr(rs));
                    }
                }
                // SYSCALL
                0x0C => panic!("SYSCALL at 0x{current_pc:08x}"),
                // BREAK
                0x0D => {
                    let code = (insn >> 6) & 0xF_FFFF;
                    return StepResult::Break(code);
                }
                // SYNC
                0x0F => {}
                // MFHI
                0x10 => self.set_gpr(rd, self.hi),
                // MTHI
                0x11 => self.hi = self.gpr(rs),
                // MFLO
                0x12 => self.set_gpr(rd, self.lo),
                // MTLO
                0x13 => self.lo = self.gpr(rs),
                // MULT
                0x18 => {
                    let result = (self.gpr(rs) as i32 as i64) * (self.gpr(rt) as i32 as i64);
                    self.lo = result as u32;
                    self.hi = (result >> 32) as u32;
                }
                // MULTU
                0x19 => {
                    let result = (self.gpr(rs) as u64) * (self.gpr(rt) as u64);
                    self.lo = result as u32;
                    self.hi = (result >> 32) as u32;
                }
                // DIV
                0x1A => {
                    let a = self.gpr(rs) as i32;
                    let b = self.gpr(rt) as i32;
                    if b != 0 {
                        self.lo = a.wrapping_div(b) as u32;
                        self.hi = a.wrapping_rem(b) as u32;
                    }
                }
                // DIVU
                0x1B => {
                    let b = self.gpr(rt);
                    if b != 0 {
                        self.lo = self.gpr(rs) / b;
                        self.hi = self.gpr(rs) % b;
                    }
                }
                // ADD / ADDU
                0x20 | 0x21 => self.set_gpr(rd, self.gpr(rs).wrapping_add(self.gpr(rt))),
                // SUB / SUBU
                0x22 | 0x23 => self.set_gpr(rd, self.gpr(rs).wrapping_sub(self.gpr(rt))),
                // AND
                0x24 => self.set_gpr(rd, self.gpr(rs) & self.gpr(rt)),
                // OR
                0x25 => self.set_gpr(rd, self.gpr(rs) | self.gpr(rt)),
                // XOR
                0x26 => self.set_gpr(rd, self.gpr(rs) ^ self.gpr(rt)),
                // NOR
                0x27 => self.set_gpr(rd, !(self.gpr(rs) | self.gpr(rt))),
                // SLT
                0x2A => {
                    self.set_gpr(rd, ((self.gpr(rs) as i32) < (self.gpr(rt) as i32)) as u32);
                }
                // SLTU
                0x2B => {
                    self.set_gpr(rd, (self.gpr(rs) < self.gpr(rt)) as u32);
                }
                _ => panic!(
                    "Unknown SPECIAL funct 0x{funct:02x} at 0x{current_pc:08x} (insn=0x{insn:08x})"
                ),
            },

            // REGIMM
            0x01 => {
                let s = self.gpr(rs) as i32;
                match rt {
                    // BLTZ
                    0x00 => {
                        if s < 0 {
                            self.next_pc = current_pc.wrapping_add(4).wrapping_add(simm << 2);
                        }
                    }
                    // BGEZ
                    0x01 => {
                        if s >= 0 {
                            self.next_pc = current_pc.wrapping_add(4).wrapping_add(simm << 2);
                        }
                    }
                    // BLTZL
                    0x02 => {
                        if s < 0 {
                            self.next_pc = current_pc.wrapping_add(4).wrapping_add(simm << 2);
                        } else {
                            self.pc = self.next_pc;
                            self.next_pc = self.pc.wrapping_add(4);
                        }
                    }
                    // BGEZL
                    0x03 => {
                        if s >= 0 {
                            self.next_pc = current_pc.wrapping_add(4).wrapping_add(simm << 2);
                        } else {
                            self.pc = self.next_pc;
                            self.next_pc = self.pc.wrapping_add(4);
                        }
                    }
                    // BLTZAL
                    0x10 => {
                        self.set_gpr(31, current_pc.wrapping_add(8));
                        if s < 0 {
                            self.next_pc = current_pc.wrapping_add(4).wrapping_add(simm << 2);
                        }
                    }
                    // BGEZAL
                    0x11 => {
                        self.set_gpr(31, current_pc.wrapping_add(8));
                        if s >= 0 {
                            self.next_pc = current_pc.wrapping_add(4).wrapping_add(simm << 2);
                        }
                    }
                    _ => panic!(
                        "Unknown REGIMM rt 0x{rt:02x} at 0x{current_pc:08x}"
                    ),
                }
            }

            // J
            0x02 => {
                self.next_pc = (current_pc & 0xF000_0000) | (target26 << 2);
            }
            // JAL
            0x03 => {
                self.set_gpr(31, current_pc.wrapping_add(8));
                self.next_pc = (current_pc & 0xF000_0000) | (target26 << 2);
            }
            // BEQ
            0x04 => {
                if self.gpr(rs) == self.gpr(rt) {
                    self.next_pc = current_pc.wrapping_add(4).wrapping_add(simm << 2);
                }
            }
            // BNE
            0x05 => {
                if self.gpr(rs) != self.gpr(rt) {
                    self.next_pc = current_pc.wrapping_add(4).wrapping_add(simm << 2);
                }
            }
            // BLEZ
            0x06 => {
                if (self.gpr(rs) as i32) <= 0 {
                    self.next_pc = current_pc.wrapping_add(4).wrapping_add(simm << 2);
                }
            }
            // BGTZ
            0x07 => {
                if (self.gpr(rs) as i32) > 0 {
                    self.next_pc = current_pc.wrapping_add(4).wrapping_add(simm << 2);
                }
            }
            // ADDI / ADDIU
            0x08 | 0x09 => self.set_gpr(rt, self.gpr(rs).wrapping_add(simm)),
            // SLTI
            0x0A => {
                self.set_gpr(rt, ((self.gpr(rs) as i32) < (simm as i32)) as u32);
            }
            // SLTIU
            0x0B => {
                self.set_gpr(rt, (self.gpr(rs) < simm) as u32);
            }
            // ANDI
            0x0C => self.set_gpr(rt, self.gpr(rs) & zimm),
            // ORI
            0x0D => self.set_gpr(rt, self.gpr(rs) | zimm),
            // XORI
            0x0E => self.set_gpr(rt, self.gpr(rs) ^ zimm),
            // LUI
            0x0F => self.set_gpr(rt, zimm << 16),

            // COP0 — minimal: MFC0 returns insn_count for rd=9, 0 otherwise
            0x10 => {
                if rs == 0 {
                    // MFC0
                    let val = if rd == 9 { self.insn_count as u32 } else { 0 };
                    self.set_gpr(rt, val);
                }
                // MTC0, etc. → ignore
            }

            // BEQL
            0x14 => {
                if self.gpr(rs) == self.gpr(rt) {
                    self.next_pc = current_pc.wrapping_add(4).wrapping_add(simm << 2);
                } else {
                    self.pc = self.next_pc;
                    self.next_pc = self.pc.wrapping_add(4);
                }
            }
            // BNEL
            0x15 => {
                if self.gpr(rs) != self.gpr(rt) {
                    self.next_pc = current_pc.wrapping_add(4).wrapping_add(simm << 2);
                } else {
                    self.pc = self.next_pc;
                    self.next_pc = self.pc.wrapping_add(4);
                }
            }
            // BLEZL
            0x16 => {
                if (self.gpr(rs) as i32) <= 0 {
                    self.next_pc = current_pc.wrapping_add(4).wrapping_add(simm << 2);
                } else {
                    self.pc = self.next_pc;
                    self.next_pc = self.pc.wrapping_add(4);
                }
            }
            // BGTZL
            0x17 => {
                if (self.gpr(rs) as i32) > 0 {
                    self.next_pc = current_pc.wrapping_add(4).wrapping_add(simm << 2);
                } else {
                    self.pc = self.next_pc;
                    self.next_pc = self.pc.wrapping_add(4);
                }
            }

            // SPECIAL2
            0x1C => match funct {
                // MADD
                0x00 => {
                    let result = ((self.hi as i64) << 32 | self.lo as i64)
                        + (self.gpr(rs) as i32 as i64) * (self.gpr(rt) as i32 as i64);
                    self.lo = result as u32;
                    self.hi = (result >> 32) as u32;
                }
                // MADDU
                0x01 => {
                    let result = ((self.hi as u64) << 32 | self.lo as u64)
                        + (self.gpr(rs) as u64) * (self.gpr(rt) as u64);
                    self.lo = result as u32;
                    self.hi = (result >> 32) as u32;
                }
                // MUL
                0x02 => {
                    let result = (self.gpr(rs) as i32).wrapping_mul(self.gpr(rt) as i32);
                    self.set_gpr(rd, result as u32);
                }
                // MSUB
                0x04 => {
                    let result = ((self.hi as i64) << 32 | self.lo as i64)
                        - (self.gpr(rs) as i32 as i64) * (self.gpr(rt) as i32 as i64);
                    self.lo = result as u32;
                    self.hi = (result >> 32) as u32;
                }
                // CLZ
                0x20 => self.set_gpr(rd, self.gpr(rs).leading_zeros()),
                // CLO
                0x21 => self.set_gpr(rd, (!self.gpr(rs)).leading_zeros()),
                _ => panic!(
                    "Unknown SPECIAL2 funct 0x{funct:02x} at 0x{current_pc:08x}"
                ),
            },

            // SPECIAL3
            0x1F => match funct {
                // EXT
                0x00 => {
                    let pos = sa;
                    let size = rd + 1;
                    let mask = if size >= 32 { !0u32 } else { (1u32 << size) - 1 };
                    self.set_gpr(rt, (self.gpr(rs) >> pos) & mask);
                }
                // INS
                0x04 => {
                    let lsb = sa;
                    let msb = rd;
                    let size = msb - lsb + 1;
                    let mask = if size >= 32 { !0u32 } else { ((1u32 << size) - 1) << lsb };
                    let val = (self.gpr(rt) & !mask) | ((self.gpr(rs) << lsb) & mask);
                    self.set_gpr(rt, val);
                }
                // BSHFL (SEB, SEH, WSBH)
                0x20 => match sa {
                    0x02 => {
                        // WSBH: swap bytes within halfwords
                        let v = self.gpr(rt);
                        let result = ((v & 0x00FF_00FF) << 8) | ((v & 0xFF00_FF00) >> 8);
                        self.set_gpr(rd, result);
                    }
                    0x10 => {
                        // SEB: sign-extend byte
                        self.set_gpr(rd, self.gpr(rt) as u8 as i8 as i32 as u32);
                    }
                    0x18 => {
                        // SEH: sign-extend halfword
                        self.set_gpr(rd, self.gpr(rt) as u16 as i16 as i32 as u32);
                    }
                    _ => panic!(
                        "Unknown BSHFL sa 0x{sa:02x} at 0x{current_pc:08x}"
                    ),
                },
                _ => panic!(
                    "Unknown SPECIAL3 funct 0x{funct:02x} at 0x{current_pc:08x}"
                ),
            },

            // LB
            0x20 => {
                let addr = self.gpr(rs).wrapping_add(simm);
                self.set_gpr(rt, mem.read_u8(addr) as i8 as i32 as u32);
            }
            // LH
            0x21 => {
                let addr = self.gpr(rs).wrapping_add(simm);
                self.set_gpr(rt, mem.read_u16(addr) as i16 as i32 as u32);
            }
            // LWL
            0x22 => {
                let addr = self.gpr(rs).wrapping_add(simm);
                let aligned = addr & !3;
                let word = mem.read_u32(aligned);
                let byte = (addr & 3) as u32;
                let old = self.gpr(rt);
                let val = match byte {
                    0 => (old & 0x00FF_FFFF) | (word << 24),
                    1 => (old & 0x0000_FFFF) | (word << 16),
                    2 => (old & 0x0000_00FF) | (word << 8),
                    3 => word,
                    _ => unreachable!(),
                };
                self.set_gpr(rt, val);
            }
            // LW
            0x23 => {
                let addr = self.gpr(rs).wrapping_add(simm);
                self.set_gpr(rt, mem.read_u32(addr));
            }
            // LBU
            0x24 => {
                let addr = self.gpr(rs).wrapping_add(simm);
                self.set_gpr(rt, mem.read_u8(addr) as u32);
            }
            // LHU
            0x25 => {
                let addr = self.gpr(rs).wrapping_add(simm);
                self.set_gpr(rt, mem.read_u16(addr) as u32);
            }
            // LWR
            0x26 => {
                let addr = self.gpr(rs).wrapping_add(simm);
                let aligned = addr & !3;
                let word = mem.read_u32(aligned);
                let byte = (addr & 3) as u32;
                let old = self.gpr(rt);
                let val = match byte {
                    0 => word,
                    1 => (old & 0xFF00_0000) | (word >> 8),
                    2 => (old & 0xFFFF_0000) | (word >> 16),
                    3 => (old & 0xFFFF_FF00) | (word >> 24),
                    _ => unreachable!(),
                };
                self.set_gpr(rt, val);
            }

            // SB
            0x28 => {
                let addr = self.gpr(rs).wrapping_add(simm);
                mem.write_u8(addr, self.gpr(rt) as u8);
            }
            // SH
            0x29 => {
                let addr = self.gpr(rs).wrapping_add(simm);
                mem.write_u16(addr, self.gpr(rt) as u16);
            }
            // SWL
            0x2A => {
                let addr = self.gpr(rs).wrapping_add(simm);
                let aligned = addr & !3;
                let word = mem.read_u32(aligned);
                let byte = (addr & 3) as u32;
                let val = match byte {
                    0 => (word & 0xFFFF_FF00) | (self.gpr(rt) >> 24),
                    1 => (word & 0xFFFF_0000) | (self.gpr(rt) >> 16),
                    2 => (word & 0xFF00_0000) | (self.gpr(rt) >> 8),
                    3 => self.gpr(rt),
                    _ => unreachable!(),
                };
                mem.write_u32(aligned, val);
            }
            // SW
            0x2B => {
                let addr = self.gpr(rs).wrapping_add(simm);
                mem.write_u32(addr, self.gpr(rt));
            }
            // SWR
            0x2E => {
                let addr = self.gpr(rs).wrapping_add(simm);
                let aligned = addr & !3;
                let word = mem.read_u32(aligned);
                let byte = (addr & 3) as u32;
                let val = match byte {
                    0 => self.gpr(rt),
                    1 => (self.gpr(rt) << 8) | (word & 0x0000_00FF),
                    2 => (self.gpr(rt) << 16) | (word & 0x0000_FFFF),
                    3 => (self.gpr(rt) << 24) | (word & 0x00FF_FFFF),
                    _ => unreachable!(),
                };
                mem.write_u32(aligned, val);
            }

            // CACHE — no-op
            0x2F => {}

            // LL — single-threaded, treat as LW
            0x30 => {
                let addr = self.gpr(rs).wrapping_add(simm);
                self.set_gpr(rt, mem.read_u32(addr));
            }

            // PREF — no-op
            0x33 => {}

            // SC — single-threaded, always succeed
            0x38 => {
                let addr = self.gpr(rs).wrapping_add(simm);
                mem.write_u32(addr, self.gpr(rt));
                self.set_gpr(rt, 1); // success
            }

            _ => panic!(
                "Unknown opcode 0x{opcode:02x} at 0x{current_pc:08x} (insn=0x{insn:08x})"
            ),
        }

        StepResult::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cpu_mem() -> (Cpu, Memory) {
        let mut cpu = Cpu::new();
        let mem = Memory::new();
        cpu.pc = 0x80A0_0000;
        cpu.next_pc = cpu.pc.wrapping_add(4);
        (cpu, mem)
    }

    fn write_insn(mem: &mut Memory, addr: u32, insn: u32) {
        mem.write_u32(addr, insn);
    }

    // ADDIU $a0, $a0, 10  — opcode=0x09, rs=4, rt=4, imm=10
    fn insn_addiu(rt: u32, rs: u32, imm: u16) -> u32 {
        (0x09 << 26) | (rs << 21) | (rt << 16) | (imm as u32)
    }

    // LUI $rt, imm
    fn insn_lui(rt: u32, imm: u16) -> u32 {
        (0x0F << 26) | (rt << 16) | (imm as u32)
    }

    // ORI $rt, $rs, imm
    fn insn_ori(rt: u32, rs: u32, imm: u16) -> u32 {
        (0x0D << 26) | (rs << 21) | (rt << 16) | (imm as u32)
    }

    // SW $rt, offset($rs)
    fn insn_sw(rt: u32, rs: u32, offset: u16) -> u32 {
        (0x2B << 26) | (rs << 21) | (rt << 16) | (offset as u32)
    }

    // LW $rt, offset($rs)
    fn insn_lw(rt: u32, rs: u32, offset: u16) -> u32 {
        (0x23 << 26) | (rs << 21) | (rt << 16) | (offset as u32)
    }

    // BEQ $rs, $rt, offset
    fn insn_beq(rs: u32, rt: u32, offset: i16) -> u32 {
        (0x04 << 26) | (rs << 21) | (rt << 16) | (offset as u16 as u32)
    }

    // BEQL $rs, $rt, offset
    fn insn_beql(rs: u32, rt: u32, offset: i16) -> u32 {
        (0x14 << 26) | (rs << 21) | (rt << 16) | (offset as u16 as u32)
    }

    // JAL target (within current 256MB segment)
    fn insn_jal(addr: u32) -> u32 {
        (0x03 << 26) | ((addr >> 2) & 0x03FF_FFFF)
    }

    // SLT $rd, $rs, $rt
    fn insn_slt(rd: u32, rs: u32, rt: u32) -> u32 {
        (rs << 21) | (rt << 16) | (rd << 11) | 0x2A
    }

    // BREAK code
    fn insn_break(code: u32) -> u32 {
        (code << 6) | 0x0D
    }

    // SPECIAL: rd = rs FUNCT rt
    fn insn_special(funct: u32, rd: u32, rs: u32, rt: u32) -> u32 {
        (rs << 21) | (rt << 16) | (rd << 11) | funct
    }

    // SLL $rd, $rt, sa
    fn insn_sll(rd: u32, rt: u32, sa: u32) -> u32 {
        (rt << 16) | (rd << 11) | (sa << 6)
    }

    // SRL $rd, $rt, sa
    fn insn_srl(rd: u32, rt: u32, sa: u32) -> u32 {
        (rt << 16) | (rd << 11) | (sa << 6) | 0x02
    }

    // SRA $rd, $rt, sa
    fn insn_sra(rd: u32, rt: u32, sa: u32) -> u32 {
        (rt << 16) | (rd << 11) | (sa << 6) | 0x03
    }

    // JR $rs
    fn insn_jr(rs: u32) -> u32 {
        (rs << 21) | 0x08
    }

    // BNE $rs, $rt, offset
    fn insn_bne(rs: u32, rt: u32, offset: i16) -> u32 {
        (0x05 << 26) | (rs << 21) | (rt << 16) | (offset as u16 as u32)
    }

    // BLEZ $rs, offset
    fn insn_blez(rs: u32, offset: i16) -> u32 {
        (0x06 << 26) | (rs << 21) | (offset as u16 as u32)
    }

    // BGTZ $rs, offset
    fn insn_bgtz(rs: u32, offset: i16) -> u32 {
        (0x07 << 26) | (rs << 21) | (offset as u16 as u32)
    }

    // REGIMM: BLTZ $rs, offset
    fn insn_bltz(rs: u32, offset: i16) -> u32 {
        (0x01 << 26) | (rs << 21) | (0x00 << 16) | (offset as u16 as u32)
    }

    // REGIMM: BGEZ $rs, offset
    fn insn_bgez(rs: u32, offset: i16) -> u32 {
        (0x01 << 26) | (rs << 21) | (0x01 << 16) | (offset as u16 as u32)
    }

    // ANDI $rt, $rs, imm
    fn insn_andi(rt: u32, rs: u32, imm: u16) -> u32 {
        (0x0C << 26) | (rs << 21) | (rt << 16) | (imm as u32)
    }

    // XORI $rt, $rs, imm
    fn insn_xori(rt: u32, rs: u32, imm: u16) -> u32 {
        (0x0E << 26) | (rs << 21) | (rt << 16) | (imm as u32)
    }

    // SLTI $rt, $rs, imm
    fn insn_slti(rt: u32, rs: u32, imm: i16) -> u32 {
        (0x0A << 26) | (rs << 21) | (rt << 16) | (imm as u16 as u32)
    }

    // SLTIU $rt, $rs, imm
    fn insn_sltiu(rt: u32, rs: u32, imm: i16) -> u32 {
        (0x0B << 26) | (rs << 21) | (rt << 16) | (imm as u16 as u32)
    }

    // LB $rt, offset($rs)
    fn insn_lb(rt: u32, rs: u32, offset: i16) -> u32 {
        (0x20 << 26) | (rs << 21) | (rt << 16) | (offset as u16 as u32)
    }

    // LBU $rt, offset($rs)
    fn insn_lbu(rt: u32, rs: u32, offset: i16) -> u32 {
        (0x24 << 26) | (rs << 21) | (rt << 16) | (offset as u16 as u32)
    }

    // LH $rt, offset($rs)
    fn insn_lh(rt: u32, rs: u32, offset: i16) -> u32 {
        (0x21 << 26) | (rs << 21) | (rt << 16) | (offset as u16 as u32)
    }

    // LHU $rt, offset($rs)
    fn insn_lhu(rt: u32, rs: u32, offset: i16) -> u32 {
        (0x25 << 26) | (rs << 21) | (rt << 16) | (offset as u16 as u32)
    }

    // SB $rt, offset($rs)
    fn insn_sb(rt: u32, rs: u32, offset: i16) -> u32 {
        (0x28 << 26) | (rs << 21) | (rt << 16) | (offset as u16 as u32)
    }

    // SH $rt, offset($rs)
    fn insn_sh(rt: u32, rs: u32, offset: i16) -> u32 {
        (0x29 << 26) | (rs << 21) | (rt << 16) | (offset as u16 as u32)
    }

    // MULT $rs, $rt (SPECIAL funct=0x18)
    fn insn_mult(rs: u32, rt: u32) -> u32 {
        (rs << 21) | (rt << 16) | 0x18
    }

    // MULTU $rs, $rt
    fn insn_multu(rs: u32, rt: u32) -> u32 {
        (rs << 21) | (rt << 16) | 0x19
    }

    // DIV $rs, $rt
    fn insn_div(rs: u32, rt: u32) -> u32 {
        (rs << 21) | (rt << 16) | 0x1A
    }

    // DIVU $rs, $rt
    fn insn_divu(rs: u32, rt: u32) -> u32 {
        (rs << 21) | (rt << 16) | 0x1B
    }

    // MFHI $rd
    fn insn_mfhi(rd: u32) -> u32 {
        (rd << 11) | 0x10
    }

    // MFLO $rd
    fn insn_mflo(rd: u32) -> u32 {
        (rd << 11) | 0x12
    }

    // SPECIAL2: MUL $rd, $rs, $rt (funct=0x02)
    fn insn_mul(rd: u32, rs: u32, rt: u32) -> u32 {
        (0x1C << 26) | (rs << 21) | (rt << 16) | (rd << 11) | 0x02
    }

    // SPECIAL2: CLZ $rd, $rs (funct=0x20)
    fn insn_clz(rd: u32, rs: u32) -> u32 {
        (0x1C << 26) | (rs << 21) | (rd << 11) | 0x20
    }

    // SPECIAL2: CLO $rd, $rs (funct=0x21)
    fn insn_clo(rd: u32, rs: u32) -> u32 {
        (0x1C << 26) | (rs << 21) | (rd << 11) | 0x21
    }

    // SPECIAL2: MADD $rs, $rt (funct=0x00)
    fn insn_madd(rs: u32, rt: u32) -> u32 {
        (0x1C << 26) | (rs << 21) | (rt << 16) | 0x00
    }

    // SPECIAL3: EXT $rt, $rs, pos, size
    fn insn_ext(rt: u32, rs: u32, pos: u32, size: u32) -> u32 {
        (0x1F << 26) | (rs << 21) | (rt << 16) | ((size - 1) << 11) | (pos << 6) | 0x00
    }

    // SPECIAL3: INS $rt, $rs, lsb, msb
    fn insn_ins(rt: u32, rs: u32, lsb: u32, msb: u32) -> u32 {
        (0x1F << 26) | (rs << 21) | (rt << 16) | (msb << 11) | (lsb << 6) | 0x04
    }

    // SPECIAL3/BSHFL: SEB $rd, $rt
    fn insn_seb(rd: u32, rt: u32) -> u32 {
        (0x1F << 26) | (rt << 16) | (rd << 11) | (0x10 << 6) | 0x20
    }

    // SPECIAL3/BSHFL: SEH $rd, $rt
    fn insn_seh(rd: u32, rt: u32) -> u32 {
        (0x1F << 26) | (rt << 16) | (rd << 11) | (0x18 << 6) | 0x20
    }

    // SPECIAL3/BSHFL: WSBH $rd, $rt
    fn insn_wsbh(rd: u32, rt: u32) -> u32 {
        (0x1F << 26) | (rt << 16) | (rd << 11) | (0x02 << 6) | 0x20
    }

    // LWL $rt, offset($rs)
    fn insn_lwl(rt: u32, rs: u32, offset: i16) -> u32 {
        (0x22 << 26) | (rs << 21) | (rt << 16) | (offset as u16 as u32)
    }

    // LWR $rt, offset($rs)
    fn insn_lwr(rt: u32, rs: u32, offset: i16) -> u32 {
        (0x26 << 26) | (rs << 21) | (rt << 16) | (offset as u16 as u32)
    }

    // MOVZ $rd, $rs, $rt
    fn insn_movz(rd: u32, rs: u32, rt: u32) -> u32 {
        (rs << 21) | (rt << 16) | (rd << 11) | 0x0A
    }

    // MOVN $rd, $rs, $rt
    fn insn_movn(rd: u32, rs: u32, rt: u32) -> u32 {
        (rs << 21) | (rt << 16) | (rd << 11) | 0x0B
    }

    // SLTU $rd, $rs, $rt
    fn insn_sltu(rd: u32, rs: u32, rt: u32) -> u32 {
        (rs << 21) | (rt << 16) | (rd << 11) | 0x2B
    }

    // J target
    fn insn_j(addr: u32) -> u32 {
        (0x02 << 26) | ((addr >> 2) & 0x03FF_FFFF)
    }

    // LL $rt, offset($rs)
    fn insn_ll(rt: u32, rs: u32, offset: i16) -> u32 {
        (0x30 << 26) | (rs << 21) | (rt << 16) | (offset as u16 as u32)
    }

    // SC $rt, offset($rs)
    fn insn_sc(rt: u32, rs: u32, offset: i16) -> u32 {
        (0x38 << 26) | (rs << 21) | (rt << 16) | (offset as u16 as u32)
    }

    #[test]
    fn test_addiu() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 5); // $a0 = 5
        write_insn(&mut mem, 0x80A0_0000, insn_addiu(4, 4, 10));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(4), 15);
    }

    #[test]
    fn test_lui_ori() {
        let (mut cpu, mut mem) = make_cpu_mem();
        write_insn(&mut mem, 0x80A0_0000, insn_lui(4, 0x80A0));
        write_insn(&mut mem, 0x80A0_0004, insn_ori(4, 4, 0x0000));
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(4), 0x80A0_0000);
    }

    #[test]
    fn test_sw_lw() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0xDEAD_BEEF); // value to store
        cpu.set_gpr(5, 0x80B0_0000); // base addr
        write_insn(&mut mem, 0x80A0_0000, insn_sw(4, 5, 0));
        write_insn(&mut mem, 0x80A0_0004, insn_lw(6, 5, 0));
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(6), 0xDEAD_BEEF);
    }

    #[test]
    fn test_branch_delay_slot() {
        let (mut cpu, mut mem) = make_cpu_mem();
        // BEQ $0, $0, +2  (always taken, target = pc+4 + 2*4 = pc+12)
        write_insn(&mut mem, 0x80A0_0000, insn_beq(0, 0, 2));
        // Delay slot: ADDIU $a0, $zero, 42
        write_insn(&mut mem, 0x80A0_0004, insn_addiu(4, 0, 42));
        // Should skip this
        write_insn(&mut mem, 0x80A0_0008, insn_addiu(5, 0, 99));
        // Branch target
        write_insn(&mut mem, 0x80A0_000C, 0); // NOP

        cpu.step(&mut mem); // execute BEQ (sets next_pc to target)
        cpu.step(&mut mem); // execute delay slot ADDIU
        assert_eq!(cpu.gpr(4), 42); // delay slot executed
        // PC should now be at branch target
        assert_eq!(cpu.pc, 0x80A0_000C);
    }

    #[test]
    fn test_branch_likely_nullify() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 1);
        cpu.set_gpr(5, 2);
        // BEQL $a0, $a1, +2  (not equal → not taken → nullify delay slot)
        write_insn(&mut mem, 0x80A0_0000, insn_beql(4, 5, 2));
        // Delay slot: ADDIU $t0, $zero, 42 — should be NULLIFIED
        write_insn(&mut mem, 0x80A0_0004, insn_addiu(8, 0, 42));
        // Fall-through
        write_insn(&mut mem, 0x80A0_0008, 0); // NOP

        cpu.step(&mut mem); // BEQL — not taken, nullifies
        assert_eq!(cpu.gpr(8), 0); // delay slot was NOT executed
        assert_eq!(cpu.pc, 0x80A0_0008); // fell through, skipping delay slot
    }

    #[test]
    fn test_jal_ra() {
        let (mut cpu, mut mem) = make_cpu_mem();
        // JAL 0x80A0_0100
        write_insn(&mut mem, 0x80A0_0000, insn_jal(0x80A0_0100));
        write_insn(&mut mem, 0x80A0_0004, 0); // delay slot NOP
        cpu.step(&mut mem); // JAL
        assert_eq!(cpu.gpr(31), 0x80A0_0008); // $ra = pc + 8
        cpu.step(&mut mem); // delay slot
        assert_eq!(cpu.pc, 0x80A0_0100); // jumped to target
    }

    #[test]
    fn test_slt_signed() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, (-1i32) as u32); // -1
        cpu.set_gpr(5, 1);
        // SLT $t0, $a0, $a1 → (-1 < 1) = 1
        write_insn(&mut mem, 0x80A0_0000, insn_slt(8, 4, 5));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 1);
    }

    #[test]
    fn test_r0_immutable() {
        let (mut cpu, mut mem) = make_cpu_mem();
        // ADDIU $zero, $zero, 42
        write_insn(&mut mem, 0x80A0_0000, insn_addiu(0, 0, 42));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(0), 0);
    }

    #[test]
    fn test_break() {
        let (mut cpu, mut mem) = make_cpu_mem();
        write_insn(&mut mem, 0x80A0_0000, insn_break(5));
        match cpu.step(&mut mem) {
            StepResult::Break(code) => assert_eq!(code, 5),
            StepResult::Ok => panic!("Expected Break"),
        }
    }

    // ---- ALU register ops ----

    #[test]
    fn test_and_or_xor_nor() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0xFF00_FF00);
        cpu.set_gpr(5, 0x0F0F_0F0F);
        let pc = 0x80A0_0000;
        write_insn(&mut mem, pc,     insn_special(0x24, 8, 4, 5));  // AND
        write_insn(&mut mem, pc + 4, insn_special(0x25, 9, 4, 5));  // OR
        write_insn(&mut mem, pc + 8, insn_special(0x26, 10, 4, 5)); // XOR
        write_insn(&mut mem, pc + 12, insn_special(0x27, 11, 4, 5)); // NOR
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0x0F00_0F00);  // AND
        assert_eq!(cpu.gpr(9), 0xFF0F_FF0F);  // OR
        assert_eq!(cpu.gpr(10), 0xF00F_F00F); // XOR
        assert_eq!(cpu.gpr(11), !(0xFF0F_FF0F)); // NOR
    }

    #[test]
    fn test_sub() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 100);
        cpu.set_gpr(5, 30);
        write_insn(&mut mem, 0x80A0_0000, insn_special(0x23, 8, 4, 5)); // SUBU
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 70);
    }

    #[test]
    fn test_sub_underflow() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 10);
        cpu.set_gpr(5, 20);
        write_insn(&mut mem, 0x80A0_0000, insn_special(0x23, 8, 4, 5)); // SUBU
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 10u32.wrapping_sub(20)); // 0xFFFF_FFF6
    }

    // ---- Shifts ----

    #[test]
    fn test_sll() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 1);
        write_insn(&mut mem, 0x80A0_0000, insn_sll(8, 4, 16));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0x0001_0000);
    }

    #[test]
    fn test_srl() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0x8000_0000);
        write_insn(&mut mem, 0x80A0_0000, insn_srl(8, 4, 16));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0x0000_8000); // logical shift, zero-fill
    }

    #[test]
    fn test_sra() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0x8000_0000); // negative number
        write_insn(&mut mem, 0x80A0_0000, insn_sra(8, 4, 16));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0xFFFF_8000); // arithmetic shift, sign-extend
    }

    #[test]
    fn test_sllv_srlv_srav() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0x0000_0001);
        cpu.set_gpr(5, 8); // shift amount
        cpu.set_gpr(6, 0xFF00_0000);
        let pc = 0x80A0_0000;
        // SLLV: rd=8, rt=4, rs=5
        write_insn(&mut mem, pc,     insn_special(0x04, 8, 5, 4));
        // SRLV: rd=9, rt=6, rs=5
        write_insn(&mut mem, pc + 4, insn_special(0x06, 9, 5, 6));
        // SRAV: rd=10, rt=6, rs=5
        write_insn(&mut mem, pc + 8, insn_special(0x07, 10, 5, 6));
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0x0000_0100);  // 1 << 8
        assert_eq!(cpu.gpr(9), 0x00FF_0000);  // 0xFF000000 >> 8 logical
        assert_eq!(cpu.gpr(10), 0xFFFF_0000); // 0xFF000000 >> 8 arithmetic
    }

    // ---- Multiply / Divide ----

    #[test]
    fn test_mult_mfhi_mflo() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0x0001_0000); // 65536
        cpu.set_gpr(5, 0x0001_0000); // 65536
        let pc = 0x80A0_0000;
        write_insn(&mut mem, pc,     insn_mult(4, 5));
        write_insn(&mut mem, pc + 4, insn_mflo(8));
        write_insn(&mut mem, pc + 8, insn_mfhi(9));
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        // 65536 * 65536 = 0x1_0000_0000
        assert_eq!(cpu.gpr(8), 0); // lo
        assert_eq!(cpu.gpr(9), 1); // hi
    }

    #[test]
    fn test_mult_signed_negative() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, (-3i32) as u32);
        cpu.set_gpr(5, 7);
        let pc = 0x80A0_0000;
        write_insn(&mut mem, pc,     insn_mult(4, 5));
        write_insn(&mut mem, pc + 4, insn_mflo(8));
        write_insn(&mut mem, pc + 8, insn_mfhi(9));
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        // -3 * 7 = -21
        assert_eq!(cpu.gpr(8) as i32, -21);
        assert_eq!(cpu.gpr(9), 0xFFFF_FFFF); // sign extension
    }

    #[test]
    fn test_multu() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0xFFFF_FFFF);
        cpu.set_gpr(5, 2);
        let pc = 0x80A0_0000;
        write_insn(&mut mem, pc,     insn_multu(4, 5));
        write_insn(&mut mem, pc + 4, insn_mflo(8));
        write_insn(&mut mem, pc + 8, insn_mfhi(9));
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        // 0xFFFFFFFF * 2 = 0x1_FFFFFFFE
        assert_eq!(cpu.gpr(8), 0xFFFF_FFFE);
        assert_eq!(cpu.gpr(9), 1);
    }

    #[test]
    fn test_div() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, (-21i32) as u32);
        cpu.set_gpr(5, 4);
        let pc = 0x80A0_0000;
        write_insn(&mut mem, pc,     insn_div(4, 5));
        write_insn(&mut mem, pc + 4, insn_mflo(8));
        write_insn(&mut mem, pc + 8, insn_mfhi(9));
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8) as i32, -5);  // quotient
        assert_eq!(cpu.gpr(9) as i32, -1);  // remainder
    }

    #[test]
    fn test_divu() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 100);
        cpu.set_gpr(5, 7);
        let pc = 0x80A0_0000;
        write_insn(&mut mem, pc,     insn_divu(4, 5));
        write_insn(&mut mem, pc + 4, insn_mflo(8));
        write_insn(&mut mem, pc + 8, insn_mfhi(9));
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 14); // 100/7
        assert_eq!(cpu.gpr(9), 2);  // 100%7
    }

    #[test]
    fn test_div_by_zero() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 42);
        cpu.set_gpr(5, 0);
        cpu.lo = 0xAAAA;
        cpu.hi = 0xBBBB;
        write_insn(&mut mem, 0x80A0_0000, insn_div(4, 5));
        cpu.step(&mut mem);
        // Result undefined on real HW; our impl preserves hi/lo
        assert_eq!(cpu.lo, 0xAAAA);
        assert_eq!(cpu.hi, 0xBBBB);
    }

    // ---- SPECIAL2: MUL, CLZ, CLO, MADD ----

    #[test]
    fn test_mul_special2() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 7);
        cpu.set_gpr(5, 6);
        write_insn(&mut mem, 0x80A0_0000, insn_mul(8, 4, 5));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 42);
    }

    #[test]
    fn test_clz() {
        let (mut cpu, mut mem) = make_cpu_mem();
        let pc = 0x80A0_0000;
        cpu.set_gpr(4, 0x0000_0001);
        write_insn(&mut mem, pc, insn_clz(8, 4));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 31);

        cpu.pc = pc; cpu.next_pc = pc + 4;
        cpu.set_gpr(4, 0);
        write_insn(&mut mem, pc, insn_clz(8, 4));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 32);

        cpu.pc = pc; cpu.next_pc = pc + 4;
        cpu.set_gpr(4, 0x8000_0000);
        write_insn(&mut mem, pc, insn_clz(8, 4));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0);
    }

    #[test]
    fn test_clo() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0xFFFF_FFFF);
        write_insn(&mut mem, 0x80A0_0000, insn_clo(8, 4));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 32);
    }

    #[test]
    fn test_madd() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 3);
        cpu.set_gpr(5, 4);
        cpu.hi = 0;
        cpu.lo = 10;
        write_insn(&mut mem, 0x80A0_0000, insn_madd(4, 5));
        cpu.step(&mut mem);
        // 10 + 3*4 = 22
        assert_eq!(cpu.lo, 22);
        assert_eq!(cpu.hi, 0);
    }

    // ---- SPECIAL3: EXT, INS, SEB, SEH, WSBH ----

    #[test]
    fn test_ext() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0xDEAD_BEEF);
        // EXT $t0, $a0, pos=8, size=8 → extract bits [15:8] = 0xBE
        write_insn(&mut mem, 0x80A0_0000, insn_ext(8, 4, 8, 8));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0xBE);
    }

    #[test]
    fn test_ext_wide() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0xDEAD_BEEF);
        // EXT $t0, $a0, pos=0, size=16 → lower 16 bits = 0xBEEF
        write_insn(&mut mem, 0x80A0_0000, insn_ext(8, 4, 0, 16));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0xBEEF);
    }

    #[test]
    fn test_ins() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0xFF);         // source
        cpu.set_gpr(8, 0x1234_5678);  // dest
        // INS $t0, $a0, lsb=8, msb=15 → insert 8 bits at pos 8
        write_insn(&mut mem, 0x80A0_0000, insn_ins(8, 4, 8, 15));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0x1234_FF78);
    }

    #[test]
    fn test_seb() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0x0000_0080); // 128 → -128 as signed byte
        write_insn(&mut mem, 0x80A0_0000, insn_seb(8, 4));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0xFFFF_FF80);

        // Positive byte
        cpu.pc = 0x80A0_0000; cpu.next_pc = 0x80A0_0004;
        cpu.set_gpr(4, 0xFFFF_FF7F); // 0x7F = 127
        write_insn(&mut mem, 0x80A0_0000, insn_seb(8, 4));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0x0000_007F);
    }

    #[test]
    fn test_seh() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0x0000_8000); // -32768 as i16
        write_insn(&mut mem, 0x80A0_0000, insn_seh(8, 4));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0xFFFF_8000);
    }

    #[test]
    fn test_wsbh() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0x1234_5678);
        write_insn(&mut mem, 0x80A0_0000, insn_wsbh(8, 4));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0x3412_7856);
    }

    // ---- Load/Store byte/half ----

    #[test]
    fn test_lb_sign_extend() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(5, 0x80B0_0000);
        mem.write_u8(0x80B0_0000, 0x80); // -128
        write_insn(&mut mem, 0x80A0_0000, insn_lb(8, 5, 0));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0xFFFF_FF80);
    }

    #[test]
    fn test_lbu_zero_extend() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(5, 0x80B0_0000);
        mem.write_u8(0x80B0_0000, 0x80);
        write_insn(&mut mem, 0x80A0_0000, insn_lbu(8, 5, 0));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0x0000_0080);
    }

    #[test]
    fn test_lh_sign_extend() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(5, 0x80B0_0000);
        mem.write_u16(0x80B0_0000, 0x8000);
        write_insn(&mut mem, 0x80A0_0000, insn_lh(8, 5, 0));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0xFFFF_8000);
    }

    #[test]
    fn test_lhu_zero_extend() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(5, 0x80B0_0000);
        mem.write_u16(0x80B0_0000, 0x8000);
        write_insn(&mut mem, 0x80A0_0000, insn_lhu(8, 5, 0));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0x0000_8000);
    }

    #[test]
    fn test_sb_sh() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0xDEAD_BEEF);
        cpu.set_gpr(5, 0x80B0_0000);
        let pc = 0x80A0_0000;
        write_insn(&mut mem, pc,     insn_sb(4, 5, 0)); // store byte 0xEF
        write_insn(&mut mem, pc + 4, insn_sh(4, 5, 4)); // store half 0xBEEF at +4
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(mem.read_u8(0x80B0_0000), 0xEF);
        assert_eq!(mem.read_u16(0x80B0_0004), 0xBEEF);
    }

    #[test]
    fn test_load_store_negative_offset() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0xAB);
        cpu.set_gpr(5, 0x80B0_0010); // base
        // SB $a0, -4($a1) → store at 0x80B0_000C
        write_insn(&mut mem, 0x80A0_0000, insn_sb(4, 5, -4i16));
        cpu.step(&mut mem);
        assert_eq!(mem.read_u8(0x80B0_000C), 0xAB);
    }

    // ---- LWL/LWR (unaligned load) ----

    #[test]
    fn test_lwl_lwr_aligned() {
        let (mut cpu, mut mem) = make_cpu_mem();
        mem.write_u32(0x80B0_0000, 0xDEAD_BEEF);
        cpu.set_gpr(5, 0x80B0_0000);
        let pc = 0x80A0_0000;
        // LWL $t0, 3($a1)  — load full word (byte 3 = MSB for LE)
        write_insn(&mut mem, pc,     insn_lwl(8, 5, 3));
        // LWR $t0, 0($a1)  — load full word (byte 0 = LSB for LE)
        write_insn(&mut mem, pc + 4, insn_lwr(8, 5, 0));
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0xDEAD_BEEF);
    }

    #[test]
    fn test_lwl_lwr_unaligned() {
        let (mut cpu, mut mem) = make_cpu_mem();
        // Store bytes: 0x80B0_0001 = 0x11, 0x80B0_0002 = 0x22, 0x80B0_0003 = 0x33, 0x80B0_0004 = 0x44
        mem.write_u8(0x80B0_0001, 0x11);
        mem.write_u8(0x80B0_0002, 0x22);
        mem.write_u8(0x80B0_0003, 0x33);
        mem.write_u8(0x80B0_0004, 0x44);
        cpu.set_gpr(5, 0x80B0_0000);
        cpu.set_gpr(8, 0); // clear target
        let pc = 0x80A0_0000;
        // Load unaligned word starting at 0x80B0_0001 (4 bytes: 0x11, 0x22, 0x33, 0x44)
        // LWL $t0, 4($a1)  — addr = 0x80B0_0004, aligned = 0x80B0_0004, byte=0 → (old & 0x00FFFFFF) | (word << 24)
        write_insn(&mut mem, pc,     insn_lwl(8, 5, 4));
        // LWR $t0, 1($a1)  — addr = 0x80B0_0001, aligned = 0x80B0_0000, byte=1 → (old & 0xFF000000) | (word >> 8)
        write_insn(&mut mem, pc + 4, insn_lwr(8, 5, 1));
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0x4433_2211);
    }

    // ---- Branches ----

    #[test]
    fn test_bne_taken() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 1);
        cpu.set_gpr(5, 2);
        let pc = 0x80A0_0000;
        write_insn(&mut mem, pc,     insn_bne(4, 5, 2)); // taken
        write_insn(&mut mem, pc + 4, 0); // delay slot NOP
        write_insn(&mut mem, pc + 8, insn_addiu(8, 0, 99)); // skipped
        write_insn(&mut mem, pc + 12, 0); // target
        cpu.step(&mut mem); // BNE
        cpu.step(&mut mem); // delay slot
        assert_eq!(cpu.pc, pc + 12);
    }

    #[test]
    fn test_bne_not_taken() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 5);
        cpu.set_gpr(5, 5);
        write_insn(&mut mem, 0x80A0_0000, insn_bne(4, 5, 2));
        write_insn(&mut mem, 0x80A0_0004, 0);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x80A0_0008); // fall through
    }

    #[test]
    fn test_blez_taken() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0); // == 0
        write_insn(&mut mem, 0x80A0_0000, insn_blez(4, 2));
        write_insn(&mut mem, 0x80A0_0004, 0);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x80A0_000C);

        // Also test negative
        cpu.pc = 0x80A0_0000; cpu.next_pc = 0x80A0_0004;
        cpu.set_gpr(4, (-5i32) as u32);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x80A0_000C);
    }

    #[test]
    fn test_blez_not_taken() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 1); // positive
        write_insn(&mut mem, 0x80A0_0000, insn_blez(4, 2));
        write_insn(&mut mem, 0x80A0_0004, 0);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x80A0_0008);
    }

    #[test]
    fn test_bgtz() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 1);
        write_insn(&mut mem, 0x80A0_0000, insn_bgtz(4, 2));
        write_insn(&mut mem, 0x80A0_0004, 0);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x80A0_000C); // taken
    }

    #[test]
    fn test_bltz() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, (-1i32) as u32);
        write_insn(&mut mem, 0x80A0_0000, insn_bltz(4, 2));
        write_insn(&mut mem, 0x80A0_0004, 0);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x80A0_000C); // taken

        // Not taken: positive
        cpu.pc = 0x80A0_0000; cpu.next_pc = 0x80A0_0004;
        cpu.set_gpr(4, 0);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x80A0_0008); // 0 is not < 0
    }

    #[test]
    fn test_bgez() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0); // 0 >= 0
        write_insn(&mut mem, 0x80A0_0000, insn_bgez(4, 2));
        write_insn(&mut mem, 0x80A0_0004, 0);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x80A0_000C); // taken
    }

    #[test]
    fn test_branch_backward() {
        let (mut cpu, mut mem) = make_cpu_mem();
        // BEQ $0, $0, -1 (offset = -1 → target = pc+4 + (-1)*4 = pc)
        // This creates a tight loop — just verify the target
        write_insn(&mut mem, 0x80A0_0000, insn_beq(0, 0, -1i16));
        write_insn(&mut mem, 0x80A0_0004, 0); // delay slot
        cpu.step(&mut mem); // BEQ
        cpu.step(&mut mem); // delay slot
        assert_eq!(cpu.pc, 0x80A0_0000); // looped back
    }

    // ---- Jumps ----

    #[test]
    fn test_j() {
        let (mut cpu, mut mem) = make_cpu_mem();
        write_insn(&mut mem, 0x80A0_0000, insn_j(0x80A0_0100));
        write_insn(&mut mem, 0x80A0_0004, 0); // delay slot
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x80A0_0100);
    }

    #[test]
    fn test_jr() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(31, 0x80A0_0200); // $ra
        write_insn(&mut mem, 0x80A0_0000, insn_jr(31));
        write_insn(&mut mem, 0x80A0_0004, 0);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x80A0_0200);
    }

    #[test]
    fn test_jalr() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0x80A0_0300);
        // JALR $t0, $a0  (rd=8, rs=4)
        write_insn(&mut mem, 0x80A0_0000, (4 << 21) | (8 << 11) | 0x09);
        write_insn(&mut mem, 0x80A0_0004, 0);
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0x80A0_0008); // link = pc + 8
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x80A0_0300);
    }

    // ---- Immediate ALU ----

    #[test]
    fn test_addiu_sign_extend() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0x80A0_0000);
        // ADDIU $a0, $a0, -4 (0xFFFC)
        write_insn(&mut mem, 0x80A0_0000, insn_addiu(4, 4, 0xFFFC));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(4), 0x809F_FFFC);
    }

    #[test]
    fn test_andi_zero_extend() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0xFFFF_FFFF);
        // ANDI $t0, $a0, 0xFF00  — zero-extended, NOT sign-extended
        write_insn(&mut mem, 0x80A0_0000, insn_andi(8, 4, 0xFF00));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0x0000_FF00);
    }

    #[test]
    fn test_xori() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0x0000_00FF);
        write_insn(&mut mem, 0x80A0_0000, insn_xori(8, 4, 0x00FF));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0); // 0xFF ^ 0xFF = 0
    }

    #[test]
    fn test_slti() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, (-5i32) as u32);
        // SLTI $t0, $a0, 0 → (-5 < 0) = 1
        write_insn(&mut mem, 0x80A0_0000, insn_slti(8, 4, 0));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 1);
    }

    #[test]
    fn test_sltiu() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 5);
        // SLTIU $t0, $a0, 10 → (5 < 10) = 1
        write_insn(&mut mem, 0x80A0_0000, insn_sltiu(8, 4, 10));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 1);
    }

    #[test]
    fn test_sltu() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 0xFFFF_FFFF); // huge unsigned
        cpu.set_gpr(5, 1);
        // SLTU $t0, $a1, $a0 → (1 < 0xFFFFFFFF) = 1
        write_insn(&mut mem, 0x80A0_0000, insn_sltu(8, 5, 4));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 1);
    }

    // ---- MOVZ / MOVN ----

    #[test]
    fn test_movz() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 42);
        cpu.set_gpr(5, 0); // zero → move
        cpu.set_gpr(8, 99);
        write_insn(&mut mem, 0x80A0_0000, insn_movz(8, 4, 5));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 42); // moved

        cpu.pc = 0x80A0_0000; cpu.next_pc = 0x80A0_0004;
        cpu.set_gpr(5, 1); // non-zero → don't move
        cpu.set_gpr(8, 99);
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 99); // unchanged
    }

    #[test]
    fn test_movn() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(4, 42);
        cpu.set_gpr(5, 1); // non-zero → move
        cpu.set_gpr(8, 99);
        write_insn(&mut mem, 0x80A0_0000, insn_movn(8, 4, 5));
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 42);

        cpu.pc = 0x80A0_0000; cpu.next_pc = 0x80A0_0004;
        cpu.set_gpr(5, 0); // zero → don't move
        cpu.set_gpr(8, 99);
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 99);
    }

    // ---- LL/SC ----

    #[test]
    fn test_ll_sc() {
        let (mut cpu, mut mem) = make_cpu_mem();
        cpu.set_gpr(5, 0x80B0_0000);
        mem.write_u32(0x80B0_0000, 0x1234_5678);
        let pc = 0x80A0_0000;
        write_insn(&mut mem, pc,     insn_ll(8, 5, 0));      // LL $t0, 0($a1)
        write_insn(&mut mem, pc + 4, insn_addiu(8, 8, 1));    // $t0 += 1
        write_insn(&mut mem, pc + 8, insn_sc(8, 5, 0));       // SC $t0, 0($a1)
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0x1234_5678);
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 0x1234_5679);
        cpu.step(&mut mem);
        assert_eq!(cpu.gpr(8), 1); // SC success
        assert_eq!(mem.read_u32(0x80B0_0000), 0x1234_5679);
    }

    // ---- Insn count ----

    #[test]
    fn test_insn_count() {
        let (mut cpu, mut mem) = make_cpu_mem();
        assert_eq!(cpu.insn_count, 0);
        write_insn(&mut mem, 0x80A0_0000, 0); // NOP
        write_insn(&mut mem, 0x80A0_0004, 0);
        write_insn(&mut mem, 0x80A0_0008, 0);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        assert_eq!(cpu.insn_count, 3);
    }

    // ---- Multi-instruction sequence ----

    #[test]
    fn test_loop_sum() {
        // Sum 1+2+3+4+5 using a loop
        let (mut cpu, mut mem) = make_cpu_mem();
        let pc = 0x80A0_0000;
        // $a0 = counter (5), $v0 = sum (0), $a1 = 1 (decrement)
        cpu.set_gpr(4, 5); // counter
        cpu.set_gpr(2, 0); // sum
        // loop:
        //   addu $v0, $v0, $a0     ; sum += counter
        //   addiu $a0, $a0, -1     ; counter--
        //   bne $a0, $zero, loop   ; if counter != 0, loop
        //   nop                    ; delay slot
        write_insn(&mut mem, pc,      insn_special(0x21, 2, 2, 4)); // ADDU $v0, $v0, $a0
        write_insn(&mut mem, pc + 4,  insn_addiu(4, 4, 0xFFFF));    // ADDIU $a0, $a0, -1
        write_insn(&mut mem, pc + 8,  insn_bne(4, 0, -3i16));       // BNE $a0, $0, -3 (back to pc)
        write_insn(&mut mem, pc + 12, 0);                            // delay slot NOP

        // Run loop: 5 iterations * 4 insns = 20 insns
        for _ in 0..20 {
            cpu.step(&mut mem);
        }
        assert_eq!(cpu.gpr(2), 15); // 5+4+3+2+1
        assert_eq!(cpu.gpr(4), 0);  // counter = 0
    }

    #[test]
    fn test_jal_jr_call_return() {
        // Simulate a function call and return
        let (mut cpu, mut mem) = make_cpu_mem();
        let pc = 0x80A0_0000;
        // caller:
        //   addiu $a0, $zero, 10
        //   jal 0x80A0_0100
        //   nop (delay slot)
        //   addiu $a1, $v0, 0     ; $a1 = return value
        write_insn(&mut mem, pc,      insn_addiu(4, 0, 10));
        write_insn(&mut mem, pc + 4,  insn_jal(0x80A0_0100));
        write_insn(&mut mem, pc + 8,  0); // delay slot
        write_insn(&mut mem, pc + 12, insn_addiu(5, 2, 0)); // $a1 = $v0

        // callee at 0x80A0_0100: double the argument
        //   addu $v0, $a0, $a0
        //   jr $ra
        //   nop
        write_insn(&mut mem, 0x80A0_0100, insn_special(0x21, 2, 4, 4)); // ADDU $v0, $a0, $a0
        write_insn(&mut mem, 0x80A0_0104, insn_jr(31));
        write_insn(&mut mem, 0x80A0_0108, 0);

        // Execute: addiu, jal, delay, addu, jr, delay, addiu
        for _ in 0..7 {
            cpu.step(&mut mem);
        }
        assert_eq!(cpu.gpr(2), 20); // $v0 = 10 * 2
        assert_eq!(cpu.gpr(5), 20); // $a1 = $v0
    }
}
