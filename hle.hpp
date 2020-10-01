#pragma once
#include "vdingoo.hpp"


class hleTask {
public:
	u32 regs[256];
	u32 stack;
	int waitingForSem;
};

int hleSemaphores[1024];
hleTask hleTasks[1024];

std::map<std::string, hleFuncDef> hleFuncMap;

u32 hle_abort(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: abort %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_printf(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: printf %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_sprintf(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: sprintf %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fprintf(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fprintf %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_strncasecmp(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: strncasecmp %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_malloc(u32 a0, u32 a1, u32 a2, u32 a3) {
	static u32 heapCurrent = HEAP_BASE;
	a0 = (a0 / 16 + 1) * 16;
	u32 ret = heapCurrent;
	heapCurrent += a0;
	return ret;
}
u32 hle_realloc(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: realloc %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_free(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: free %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fread(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fread %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fwrite(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fwrite %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fseek(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fseek %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_LcdGetDisMode(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: LcdGetDisMode %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_vxGoHome(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: vxGoHome %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_StartSwTimer(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: StartSwTimer %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_free_irq(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: free_irq %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fsys_RefreshCache(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fsys_RefreshCache %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_strlen(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: strlen %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle__lcd_set_frame(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: _lcd_set_frame %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle__lcd_get_frame(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: _lcd_get_frame %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_lcd_get_cframe(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: lcd_get_cframe %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_ap_lcd_set_frame(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: ap_lcd_set_frame %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_lcd_flip(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: lcd_flip %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle___icache_invalidate_all(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: __icache_invalidate_all %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle___dcache_writeback_all(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: __dcache_writeback_all %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_TaskMediaFunStop(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: TaskMediaFunStop %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_OSCPUSaveSR(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: OSCPUSaveSR %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_OSCPURestoreSR(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: OSCPURestoreSR %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_serial_getc(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: serial_getc %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_serial_putc(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: serial_putc %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle__kbd_get_status(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: _kbd_get_status %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_get_game_vol(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: get_game_vol %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle__kbd_get_key(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: _kbd_get_key %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fsys_fopen(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fsys_fopen %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fsys_fread(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fsys_fread %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fsys_fclose(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fsys_fclose %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fsys_fseek(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fsys_fseek %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fsys_ftell(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fsys_ftell %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fsys_remove(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fsys_remove %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fsys_rename(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fsys_rename %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fsys_ferror(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fsys_ferror %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fsys_feof(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fsys_feof %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fsys_fwrite(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fsys_fwrite %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fsys_findfirst(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fsys_findfirst %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fsys_findnext(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fsys_findnext %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fsys_findclose(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fsys_findclose %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fsys_flush_cache(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: fsys_flush_cache %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_USB_Connect(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: USB_Connect %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_udc_attached(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: udc_attached %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_USB_No_Connect(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: USB_No_Connect %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_waveout_open(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: waveout_open %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_waveout_close(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: waveout_close %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_waveout_close_at_once(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: waveout_close_at_once %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_waveout_set_volume(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: waveout_set_volume %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_HP_Mute_sw(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: HP_Mute_sw %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_waveout_can_write(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: waveout_can_write %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_waveout_write(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: waveout_write %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_pcm_can_write(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: pcm_can_write %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_pcm_ioctl(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: pcm_ioctl %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_OSTimeGet(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: OSTimeGet %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_OSTimeDly(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: OSTimeDly %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_OSSemPend(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: OSSemPend %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_OSSemPost(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: OSSemPost %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_OSSemCreate(u32 a0, u32 a1, u32 a2, u32 a3) {
	static u32 semCnt = 0;
	semCnt++;
	return semCnt;
}
u32 hle_OSTaskCreate(u32 a0, u32 a1, u32 a2, u32 a3) {
	printf("OSTaskCreate: name: %x %x %x %x\n", a0, a1, a2, a3);
	static u32 taskCnt = 0;
	taskCnt++;
	hleTask& tsk = hleTasks[taskCnt];
	tsk.stack = hle_malloc(4096, 0, 0 ,0);
	tsk.regs[UC_MIPS_REG_SP] = tsk.stack + 4096 - 4;
	tsk.regs[UC_MIPS_REG_PC] = a0;
	tsk.regs[UC_MIPS_REG_A0] = a1;
	tsk.regs[UC_MIPS_REG_RA] = 0x80000000;
	tsk.waitingForSem = 0;
	return taskCnt;
}
u32 hle_OSSemDel(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: OSSemDel %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_OSTaskDel(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: OSTaskDel %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_GetTickCount(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: GetTickCount %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle__sys_judge_event(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: _sys_judge_event %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_fsys_fopenW(u32 a0, u32 a1, u32 a2, u32 a3) {
	u32 ra = 0;
	uc_reg_read(uc, UC_MIPS_REG_RA, &ra);
	char tmp[256] = { 0 };
	int i = 0;
	u16* p16 = (u16*)vmConvertAddr(a0);
	while (*p16) {
		tmp[i] = *p16;
		p16++;
		i++;
	}
	printf("fopenW: %s\n", tmp);
	logWarn("unimplemented: fsys_fopenW %x %x %x %x %x\n", a0, a1, a2, a3, ra);
	return 0;
}
u32 hle___to_unicode_le(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: __to_unicode_le %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle___to_locale_ansi(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: __to_locale_ansi %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}
u32 hle_get_current_language(u32 a0, u32 a1, u32 a2, u32 a3) {
	logWarn("unimplemented: get_current_language %x %x %x %x\n", a0, a1, a2, a3);
	return 0;
}



void hleCodeHookCallback(uc_engine* uc, uint64_t address, uint32_t size, void* user_data) {
	u32 pc = 0;
	uc_reg_read(uc, UC_MIPS_REG_PC, &pc);
	u32 ra = 0;
	uc_reg_read(uc, UC_MIPS_REG_RA, &ra);
	hleFuncDef fp = (hleFuncDef)(user_data);
	u32 a0 = 0;
	u32 a1 = 0;
	u32 a2 = 0;
	u32 a3 = 0;
	uc_reg_read(uc, UC_MIPS_REG_A0, &a0);
	uc_reg_read(uc, UC_MIPS_REG_A1, &a1);
	uc_reg_read(uc, UC_MIPS_REG_A2, &a2);
	uc_reg_read(uc, UC_MIPS_REG_A3, &a3);
	u32 v0 = fp(a0, a1, a2, a3);
	uc_reg_write(uc, UC_MIPS_REG_V0, &v0);
	uc_reg_write(uc, UC_MIPS_REG_PC, &ra);
	return;
}


void hleRescheduleTask() {

}