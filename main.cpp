#include "vdingoo.hpp"
#include "hle.hpp"
#include "hleinit.hpp"



u32 appRAWDEntryPoint = 0x80a000a0;
u32 appEXPTAppMain = 0x80a001a4;
int mainTask = 0;

void vmWrite32(u32 addr, u32 v) {
    dram[addr - 0x80000000] = v;
}

void handleImptExptTable(u8* table, size_t tableSize, bool isImpt) {
    u32* ptr32 = (u32*)(table);
    int entryCount = table[0];
    char* strTable = (char*)table + 16 * (entryCount + 1);
    for (int i = 0; i < entryCount; i++) {
        ptr32 += 4;
        char* name = strTable + ptr32[0];
        u32 addr = ptr32[3];
        printf("%s, %x\n", name, addr);
        if (isImpt) {
            uc_hook* hk = new uc_hook();
            hleFuncDef funcPtr = hleFuncMap[name];
            uc_hook_add(uc, hk, UC_HOOK_CODE, hleCodeHookCallback, funcPtr, addr, addr + 4);
        }
    }
}

int loadApp() {
    FILE* f = fopen("qiye.app", "rb");
    u32 appRAWDOffset = 0x970;
    u32 appRAWDSize = 0x13a2e0;
    u32 appIMPTOffset = 0xA0;
    u32 appIMPTSize = 0x888;

    fseek(f, appRAWDOffset, SEEK_SET);
    fread(dram + 0xA00000, appRAWDSize, 1, f);
   
    u8* imptTable = (u8*)malloc(appIMPTSize);
    fseek(f, appIMPTOffset, SEEK_SET);
    fread(imptTable, appIMPTSize, 1, f);
    handleImptExptTable(imptTable, appIMPTSize, true);
    free((void*)imptTable);


    fclose(f);
    return 0;
}

u32 vmCallFunction(u32 addr) {
    u32 ra = 0x80000000;
    u32 pc = addr;
    u32 sp = 0;
    uc_reg_write(uc, UC_MIPS_REG_RA, &ra);
    uc_err err;
    while (1) {
        err = uc_emu_start(uc, pc, 0x80000000, 0, 80000000);
        uc_reg_read(uc, UC_MIPS_REG_PC, &pc);
        uc_reg_read(uc, UC_MIPS_REG_RA, &ra);
        uc_reg_read(uc, UC_MIPS_REG_SP, &sp);
        printf("pc: %x ra: %x sp: %x\n", pc, ra, sp);
        if (pc == 0x80000000) {
            break;
        }
        if (err != 0) {
            printf("%d %s\n", err, uc_strerror(err));
            break;
        }
    }
    return 0;
}

int main() {
    hleInit();

    uc_err err = uc_open(UC_ARCH_MIPS, (uc_mode)(UC_MODE_MIPS32 + UC_MODE_LITTLE_ENDIAN), &uc);
    if (err) {
        printf("Failed on uc_open() with error returned: %u (%s)\n",
            err, uc_strerror(err));
        return 1;
    }
    uc_mem_map_ptr(uc, 0x80001000UL, sizeof(dram), UC_PROT_ALL, dram + 0x1000);
    loadApp();
    u32 sp = 0x80100000;
    uc_reg_write(uc, UC_MIPS_REG_SP, &sp);
    vmCallFunction(appRAWDEntryPoint);
    u32 argc = 1;
    u32 argv = hle_malloc(4, 0, 0, 0);
    u32 argv0 = hle_malloc(16, 0, 0, 0);
    vmWrite32(argv, argv0);
    memcpy(vmConvertAddr(argv0), "qiye.app", 9);
    mainTask = hle_OSTaskCreate(appEXPTAppMain, 0, 0, 0);
    hleTasks[mainTask].regs[UC_MIPS_REG_A0] = argc;
    hleTasks[mainTask].regs[UC_MIPS_REG_A0] = argv;
    hleRescheduleTask();
    return 0;
}