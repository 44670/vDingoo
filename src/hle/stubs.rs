use crate::hle::HleState;
use crate::mem::Memory;
use crate::mips::Cpu;

const LCD_FRAMEBUF: u32 = 0x80F0_0000;

pub fn hle_default(cpu: &mut Cpu, _mem: &mut Memory, _state: &mut HleState) {
    cpu.set_gpr(2, 0); // return 0
}

pub fn hle_lcd_get_frame(cpu: &mut Cpu, _mem: &mut Memory, _state: &mut HleState) {
    cpu.set_gpr(2, LCD_FRAMEBUF);
}

pub fn hle_ossem_create(cpu: &mut Cpu, _mem: &mut Memory, state: &mut HleState) {
    // Return a unique fake semaphore handle
    state.sem_counter += 1;
    cpu.set_gpr(2, 0x80E0_0000 + state.sem_counter * 4);
}

pub fn hle_get_tick(cpu: &mut Cpu, _mem: &mut Memory, _state: &mut HleState) {
    cpu.set_gpr(2, (cpu.insn_count / 336) as u32);
}

pub fn hle_exit(_cpu: &mut Cpu, _mem: &mut Memory, _state: &mut HleState) {
    eprintln!("[HLE] vxGoHome() — exiting");
    std::process::exit(0);
}
