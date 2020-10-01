#pragma once
#include <stdio.h>
#include <windows.h>
#include <stdint.h>
#include <unicorn/unicorn.h>
#include <unicorn/mips.h>
#include <iostream>
#include <map>

typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef int8_t s8;
typedef int16_t s16;
typedef int32_t s32;
typedef uintptr_t uptr;

typedef u32(*hleFuncDef) (u32, u32, u32, u32);

#define logWarn printf
#define DRAM_BASE 0x80000000
#define HEAP_BASE (DRAM_BASE + 32 * 1024 * 1024)


u8 dram[64 * 1024 * 1024];
uc_engine* uc;

static inline u8* vmConvertAddr(u32 userAddr) {
	return dram + (userAddr - DRAM_BASE);
}
