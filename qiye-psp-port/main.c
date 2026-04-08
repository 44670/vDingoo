/*
 * vDingoo PSP — Run Dingoo A320 CCDL binaries natively on PSP.
 *
 * The PSP Allegrex CPU is MIPS32, same as the Dingoo's SoC.
 * After relocating the binary from 0x80A00000 (Dingoo KSEG0) to 0x08A00000
 * (PSP user-space), the code executes natively — no interpreter needed.
 * OS/SDK imports are patched to jump to our C HLE stub functions.
 */

#include <pspkernel.h>
#include <pspdisplay.h>
#include <pspdebug.h>
#include <pspctrl.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>

#include "ccdl.h"
#include "hle.h"

/* #define printf pspDebugScreenPrintf */

PSP_MODULE_INFO("vDingoo", 0, 1, 0);
PSP_MAIN_THREAD_ATTR(THREAD_ATTR_USER);
PSP_HEAP_SIZE_KB(20*1024);

/* ── Exit callbacks ───────────────────────────────────────────────────────── */

static int exit_callback(int arg1, int arg2, void *common) {
    (void)arg1; (void)arg2; (void)common;
    sceKernelExitGame();
    return 0;
}

static int callback_thread(SceSize args, void *argp) {
    (void)args; (void)argp;
    int cbid = sceKernelCreateCallback("exit_cb", exit_callback, NULL);
    sceKernelRegisterExitCallback(cbid);
    sceKernelSleepThreadCB();
    return 0;
}

static void setup_callbacks(void) {
    int thid = sceKernelCreateThread("cb_thread", callback_thread,
                                     0x11, 0x1000, 0, NULL);
    if (thid >= 0) sceKernelStartThread(thid, 0, NULL);
}

/* ── File loading helper ──────────────────────────────────────────────────── */

static uint8_t *load_file(const char *path, uint32_t *out_size) {
    SceUID fd = sceIoOpen(path, PSP_O_RDONLY, 0);
    if (fd < 0) {
        printf("Failed to open: %s (0x%08x)\n", path, fd);
        return NULL;
    }
    SceOff size = sceIoLseek(fd, 0, PSP_SEEK_END);
    sceIoLseek(fd, 0, PSP_SEEK_SET);

    uint8_t *buf = (uint8_t *)malloc((size_t)size);
    if (!buf) {
        printf("Failed to alloc %lld bytes for %s\n", size, path);
        sceIoClose(fd);
        return NULL;
    }
    sceIoRead(fd, buf, (unsigned int)size);
    sceIoClose(fd);
    *out_size = (uint32_t)size;
    return buf;
}

/* ── Display setup ────────────────────────────────────────────────────────── */

static void setup_display(void) {
    /* Use VRAM directly with RGB565 format */
    sceDisplaySetMode(0, PSP_SCR_W, PSP_SCR_H);
    sceDisplaySetFrameBuf((void *)0x04000000, PSP_BUF_W,
                          PSP_DISPLAY_PIXEL_FORMAT_565,
                          PSP_DISPLAY_SETBUF_NEXTVSYNC);

    /* Clear VRAM to black */
    hle_set_vram(NULL);
}

/* ── Write UCS-2 LE wide string to memory ─────────────────────────────────── */

static void write_wstring(void *addr, const char *s) {
    uint16_t *dst = (uint16_t *)addr;
    while (*s) {
        *dst++ = (uint16_t)(uint8_t)*s++;
    }
    *dst = 0;
}

/* ── Main ─────────────────────────────────────────────────────────────────── */

int main(int argc, char *argv[]) {
    (void)argc; (void)argv;

    pspDebugScreenInit();
    setup_callbacks();

    printf("vDingoo PSP — CCDL native loader\n");
    printf("================================\n\n");

    /* Print heap info */
    void *heap_lo = malloc(1);
    void *heap_hi = malloc(1);
    printf("Heap: 0x%08lx .. (30MB)\n", (unsigned long)heap_lo);
    free(heap_hi);
    free(heap_lo);

    /* ── Load qiye.app header ──────────────────────────────────────────── */

    /* Strategy: read CCDL header from original .app for import/export tables,
     * then load the patched RAWD binary (code+BSS+trampolines from rewrite.py)
     * and the patched reloc table. */

    const char *app_path   = "ms0:/PSP/GAME/VDINGOO/nand/qiye.app";
    const char *rawd_path  = "ms0:/PSP/GAME/VDINGOO/nand/qiye.patched.rawd.bin";
    const char *reloc_path = "ms0:/PSP/GAME/VDINGOO/nand/qiye.reloc.patched.bin";

    /* Read CCDL header from original .app — first 4KB has IMPT+EXPT tables */
    #define HEADER_SIZE 4096
    uint8_t header_buf[HEADER_SIZE];

    SceUID app_fd = sceIoOpen(app_path, PSP_O_RDONLY, 0);
    if (app_fd < 0) {
        printf("ERROR: Cannot open %s (0x%08lx)\n", app_path, (unsigned long)app_fd);
        sceKernelSleepThread();
        return 1;
    }
    sceIoRead(app_fd, header_buf, HEADER_SIZE);
    sceIoClose(app_fd);
    printf("Parsed header from %s\n", app_path);

    /* Load patched RAWD (code+BSS+trampolines, ~1.4MB) */
    uint32_t rawd_size = 0;
    uint8_t *rawd_data = load_file(rawd_path, &rawd_size);
    if (!rawd_data) {
        printf("ERROR: Cannot load %s\n", rawd_path);
        sceKernelSleepThread();
        return 1;
    }
    printf("Loaded %s (%lu bytes)\n", rawd_path, (unsigned long)rawd_size);

    /* Load patched reloc table */
    uint32_t reloc_size = 0;
    uint8_t *reloc_data = load_file(reloc_path, &reloc_size);
    if (!reloc_data) {
        printf("ERROR: Cannot load %s\n", reloc_path);
        free(rawd_data);
        sceKernelSleepThread();
        return 1;
    }
    printf("Loaded %s (%lu bytes)\n", reloc_path, (unsigned long)reloc_size);

    /* ── Parse CCDL ─────────────────────────────────────────────────────── */

    CcdlBinary ccdl;
    if (ccdl_parse(header_buf, HEADER_SIZE, &ccdl) < 0) {
        printf("ERROR: CCDL parse failed\n");
        goto fail;
    }

    /* ── Allocate memory for game code+data+trampolines ─────────────────── */

    /* Fixed load address — 300KB RWX buffer placed at 0x08A00000 via
     * --section-start. 64KB aligned for LO16 relocs. Deterministic across
     * PPSSPP / real PSP / Vita Adrenaline. */
    static uint8_t game_buf[2 * 1024 * 1024] __attribute__((section(".game_buf")));
    uint32_t alloc_size = rawd_size > ccdl.memory_size ? rawd_size : ccdl.memory_size;
    uint32_t new_base = (uint32_t)game_buf;
    printf("Game buffer at 0x%08lx (%lu bytes, need %lu)\n",
           (unsigned long)new_base, (unsigned long)sizeof(game_buf), (unsigned long)alloc_size);
    if (new_base != 0x08A00000) {
        printf("ERROR: game_buf not at 0x08A00000! Got 0x%08lx\n", (unsigned long)new_base);
        goto fail;
    }
    if (alloc_size > sizeof(game_buf)) {
        printf("ERROR: game needs %lu but buffer is %lu!\n",
               (unsigned long)alloc_size, (unsigned long)sizeof(game_buf));
        goto fail;
    }

    /* ── Load patched RAWD and relocate ────────────────────────────────── */

    if (ccdl_load_relocated_rawd(&ccdl, rawd_data, rawd_size,
                                  reloc_data, reloc_size, new_base) < 0) {
        printf("ERROR: relocation failed\n");
        goto fail;
    }

    free(rawd_data); rawd_data = NULL;
    free(reloc_data); reloc_data = NULL;

    /* ── Patch imports ──────────────────────────────────────────────────── */

    hle_init();

    const HleEntry *hle_table;
    int hle_count = hle_get_table(&hle_table);
    ccdl_patch_imports(&ccdl, hle_table, hle_count);

    /* ── Phase 1: _start(0, 0) ──────────────────────────────────────────── */

    printf("\n=== Phase 1: _start(0, 0) @ 0x%08lx ===\n", (unsigned long)ccdl.entry_point);

    typedef void (*start_fn)(int, int);
    start_fn entry = (start_fn)ccdl.entry_point;
    entry(0, 0);

    printf("_start returned OK\n");

    /* ── Phase 2: AppMain(wpath) ────────────────────────────────────────── */

    uint32_t appmain_addr = 0;
    for (int i = 0; i < ccdl.export_count; i++) {
        if (strcmp(ccdl.exports[i].name, "AppMain") == 0) {
            appmain_addr = ccdl.exports[i].vaddr;
            break;
        }
    }

    if (!appmain_addr) {
        printf("ERROR: AppMain export not found\n");
        goto fail;
    }

    printf("=== Phase 2: AppMain @ 0x%08lx ===\n", (unsigned long)appmain_addr);

    /* Write wide-string path to a scratch area */
    static uint16_t wpath_buf[256] __attribute__((aligned(4)));
    write_wstring(wpath_buf, "\\qiye.app");

    typedef void (*appmain_fn)(void *);
    appmain_fn appmain = (appmain_fn)appmain_addr;
    appmain(wpath_buf);

    printf("AppMain returned\n");

    /* ── Cleanup ────────────────────────────────────────────────────────── */

    free(ccdl.imports);
    free(ccdl.exports);
    sceKernelExitGame();
    return 0;

fail:
    if (rawd_data) free(rawd_data);
    if (reloc_data) free(reloc_data);
    printf("\nPress HOME to exit.\n");
    sceKernelSleepThread();
    return 1;
}
