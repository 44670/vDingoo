/*
 * HLE stubs for Dingoo A320 uC/OS-II SDK, running natively on PSP.
 *
 * Since both PSP and Dingoo are MIPS32 with o32 ABI, the guest binary calls
 * these functions directly via patched import stubs (J instruction).
 * Arguments arrive in $a0-$a3 / stack exactly as the original SDK expects.
 * Guest pointers are real PSP addresses (after relocation to 0x08A00000).
 */

#include "hle.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>
#include <ctype.h>

#include <pspkernel.h>
#include <pspdisplay.h>
#include <pspctrl.h>
#include <pspaudio.h>

/* ════════════════════════════════════════════════════════════════════════════
 * Global state
 * ════════════════════════════════════════════════════════════════════════════ */

static volatile int g_quit = 0;
static uint32_t     g_buttons = 0;        /* current Dingoo-format button state */
static uint64_t     g_start_time = 0;     /* microseconds at init */
static uint32_t     g_frame_count = 0;

/* Audio */
static int          g_audio_ch = -1;      /* PSP audio channel */
static int16_t      g_audio_buf[448] __attribute__((aligned(64))); /* 448 = ALIGN64(400) */

/* Filesystem */
#define MAX_OPEN_FILES 32
static SceUID g_files[MAX_OPEN_FILES];
static int    g_file_eof[MAX_OPEN_FILES];
static int    g_next_fd = 0;

/* String conversion scratch buffers */
static uint16_t g_unicode_buf[2048];
static char     g_ansi_buf[2048];

/* Semaphore mapping: guest opaque addr -> PSP SceUID */
#define MAX_SEMS 64
static struct { uint32_t guest_addr; SceUID psp_sem; } g_sems[MAX_SEMS];
static int g_sem_count = 0;
static uint32_t g_sem_next_addr = 0x08E00004;

/* Base path for file I/O */
static const char *g_base_path = "ms0:/PSP/GAME/VDINGOO/nand/";

/* ════════════════════════════════════════════════════════════════════════════
 * Helpers
 * ════════════════════════════════════════════════════════════════════════════ */

static uint64_t get_time_us(void) {
    return sceKernelGetSystemTimeWide();
}

/* Translate a guest path to a PSP ms0: path.
 * Strips drive prefix, leading slashes, "./" and blocks ".." traversal. */
static char g_path_scratch[512];

static const char *translate_path(const char *guest) {
    const char *p = guest;
    /* Strip drive prefix like "A:/" */
    if (p[0] && p[1] == ':') {
        p += 2;
        if (*p == '/' || *p == '\\') p++;
    }
    /* Strip leading "./" or ".\" */
    if (p[0] == '.' && (p[1] == '/' || p[1] == '\\')) p += 2;
    /* Strip leading "/" or "\" */
    while (*p == '/' || *p == '\\') p++;
    /* Block path traversal */
    if (strstr(p, "..")) {
        printf("[FS] BLOCKED: %s\n", guest);
        return NULL;
    }
    snprintf(g_path_scratch, sizeof(g_path_scratch), "%s%s", g_base_path, p);
    /* Normalize backslashes */
    for (char *c = g_path_scratch; *c; c++) {
        if (*c == '\\') *c = '/';
    }
    return g_path_scratch;
}

/* Read a UCS-2 LE wide string from memory, return static ANSI buffer */
static const char *read_wstring(const uint16_t *wstr) {
    int i = 0;
    while (wstr[i] && i < (int)sizeof(g_ansi_buf) - 1) {
        g_ansi_buf[i] = (char)(wstr[i] & 0xFF);
        i++;
    }
    g_ansi_buf[i] = 0;
    return g_ansi_buf;
}

/* Map SceIo flags from C fopen mode string */
static int mode_to_flags(const char *mode) {
    if (!mode) return PSP_O_RDONLY;
    if (strcmp(mode, "r") == 0 || strcmp(mode, "rb") == 0)
        return PSP_O_RDONLY;
    if (strcmp(mode, "w") == 0 || strcmp(mode, "wb") == 0)
        return PSP_O_WRONLY | PSP_O_CREAT | PSP_O_TRUNC;
    if (strcmp(mode, "a") == 0 || strcmp(mode, "ab") == 0)
        return PSP_O_WRONLY | PSP_O_CREAT | PSP_O_APPEND;
    if (strcmp(mode, "r+") == 0 || strcmp(mode, "rb+") == 0 || strcmp(mode, "r+b") == 0)
        return PSP_O_RDWR;
    if (strcmp(mode, "w+") == 0 || strcmp(mode, "wb+") == 0 || strcmp(mode, "w+b") == 0)
        return PSP_O_RDWR | PSP_O_CREAT | PSP_O_TRUNC;
    return PSP_O_RDONLY;
}

/* Find an open file slot, return internal index or -1 */
static int find_fd(uint32_t guest_fd) {
    int idx = (int)(guest_fd - 0x100);
    if (idx < 0 || idx >= MAX_OPEN_FILES) return -1;
    if (g_files[idx] < 0) return -1;
    return idx;
}

/* Find semaphore by guest address */
static int find_sem(uint32_t guest_addr) {
    for (int i = 0; i < g_sem_count; i++) {
        if (g_sems[i].guest_addr == guest_addr) return i;
    }
    return -1;
}

/* ════════════════════════════════════════════════════════════════════════════
 * HLE stub functions
 *
 * Each is a normal C function with MIPS o32 ABI calling convention.
 * The guest binary calls them via JAL → J (patched stub).
 * Args come from $a0-$a3, return value goes in $v0.
 * Guest pointers are real PSP addresses after relocation.
 * ════════════════════════════════════════════════════════════════════════════ */

/* ── C stdlib ─────────────────────────────────────────────────────────────── */

/* malloc/free/realloc — use PSP's libc directly.
 * Guest pointers returned are real PSP heap addresses. */

/* void *hle_malloc(size_t size) — already libc-compatible */
/* void  hle_free(void *ptr)     — already libc-compatible */
/* void *hle_realloc(void *p, size_t s) — already libc-compatible */

/* printf: guest passes format string + varargs, all in guest (= real) memory */
/* Since o32 ABI matches, we just delegate to libc printf directly. */

/* strlen: guest pointer is a real C string */
/* strncasecmp: same */

static void hle_abort_impl(void) {
    printf("[HLE] abort() called\n");
    sceKernelExitGame();
}

/* ── Filesystem ───────────────────────────────────────────────────────────── */

static uint32_t hle_fsys_fopen(const char *path, const char *mode) {
    const char *host_path = translate_path(path);
    if (!host_path) return 0;

    int flags = mode_to_flags(mode);
    SceUID fd = sceIoOpen(host_path, flags, 0777);
    if (fd < 0) return 0;

    /* Find free slot */
    if (g_next_fd >= MAX_OPEN_FILES) {
        sceIoClose(fd);
        return 0;
    }
    int idx = g_next_fd++;
    g_files[idx] = fd;
    g_file_eof[idx] = 0;
    return (uint32_t)(idx + 0x100);
}

static uint32_t hle_fsys_fopenW(const uint16_t *wpath, const uint16_t *wmode) {
    const char *path = read_wstring(wpath);
    char mode_buf[16];
    /* Read mode wstring separately */
    int i = 0;
    while (wmode[i] && i < 15) { mode_buf[i] = (char)(wmode[i] & 0xFF); i++; }
    mode_buf[i] = 0;
    return hle_fsys_fopen(path, mode_buf);
}

static uint32_t hle_fsys_fread(void *buf, uint32_t size, uint32_t count, uint32_t fd) {
    int idx = find_fd(fd);
    if (idx < 0) return 0;
    uint32_t total = size * count;
    int n = sceIoRead(g_files[idx], buf, total);
    if (n < 0) return 0;
    if ((uint32_t)n < total) g_file_eof[idx] = 1;
    return (size > 0) ? ((uint32_t)n / size) : 0;
}

static uint32_t hle_fsys_fwrite(const void *buf, uint32_t size, uint32_t count, uint32_t fd) {
    int idx = find_fd(fd);
    if (idx < 0) return 0;
    uint32_t total = size * count;
    int n = sceIoWrite(g_files[idx], buf, total);
    if (n < 0) return 0;
    return (size > 0) ? ((uint32_t)n / size) : 0;
}

static uint32_t hle_fsys_fclose(uint32_t fd) {
    int idx = find_fd(fd);
    if (idx < 0) return (uint32_t)-1;
    sceIoClose(g_files[idx]);
    g_files[idx] = -1;
    return 0;
}

static uint32_t hle_fsys_fseek(uint32_t fd, int32_t offset, uint32_t whence) {
    int idx = find_fd(fd);
    if (idx < 0) return (uint32_t)-1;
    int psp_whence;
    switch (whence) {
        case 0: psp_whence = PSP_SEEK_SET; break;
        case 1: psp_whence = PSP_SEEK_CUR; break;
        case 2: psp_whence = PSP_SEEK_END; break;
        default: return (uint32_t)-1;
    }
    SceOff r = sceIoLseek(g_files[idx], offset, psp_whence);
    if (r < 0) return (uint32_t)-1;
    g_file_eof[idx] = 0;
    return 0;
}

static uint32_t hle_fsys_ftell(uint32_t fd) {
    int idx = find_fd(fd);
    if (idx < 0) return 0;
    SceOff pos = sceIoLseek(g_files[idx], 0, PSP_SEEK_CUR);
    return (uint32_t)pos;
}

static uint32_t hle_fsys_feof(uint32_t fd) {
    int idx = find_fd(fd);
    if (idx < 0) return 1;
    return g_file_eof[idx];
}

static uint32_t hle_fsys_ferror(uint32_t fd) {
    (void)fd;
    return 0;
}

/* ── LCD / Video ──────────────────────────────────────────────────────────── */

static uint32_t hle_lcd_get_frame(void) {
    return LCD_FRAMEBUF;
}

static void hle_lcd_set_frame(uint32_t addr) {
    (void)addr;
    /* Copy 320x240 RGB565 from LCD_FRAMEBUF to PSP VRAM, centered */
    const uint16_t *src = (const uint16_t *)LCD_FRAMEBUF;
    uint16_t *dst = (uint16_t *)PSP_VRAM_BASE;

    /* Offset to center: y*stride + x */
    dst += FB_Y_OFF * PSP_BUF_W + FB_X_OFF;

    for (int y = 0; y < LCD_H; y++) {
        memcpy(dst, src, LCD_W * 2);
        src += LCD_W;
        dst += PSP_BUF_W;
    }

    g_frame_count++;
    if (g_frame_count <= 3 || (g_frame_count % 60) == 0) {
        uint64_t elapsed = get_time_us() - g_start_time;
        double secs = elapsed / 1000000.0;
        printf("[LCD] frame #%lu (%.1f fps)\n",
               (unsigned long)g_frame_count, g_frame_count / secs);
    }
}

static void hle_lcd_flip(void) {
    hle_lcd_set_frame(0);
}

/* ── Input ────────────────────────────────────────────────────────────────── */

static void poll_input(void) {
    SceCtrlData pad;
    sceCtrlPeekBufferPositive(&pad, 1);

    uint32_t btns = 0;
    if (pad.Buttons & PSP_CTRL_UP)       btns |= DINGOO_UP;
    if (pad.Buttons & PSP_CTRL_DOWN)     btns |= DINGOO_DOWN;
    if (pad.Buttons & PSP_CTRL_LEFT)     btns |= DINGOO_LEFT;
    if (pad.Buttons & PSP_CTRL_RIGHT)    btns |= DINGOO_RIGHT;
    if (pad.Buttons & PSP_CTRL_CIRCLE)   btns |= DINGOO_A;    /* A = confirm */
    if (pad.Buttons & PSP_CTRL_CROSS)    btns |= DINGOO_B;    /* B = cancel */
    if (pad.Buttons & PSP_CTRL_TRIANGLE) btns |= DINGOO_X;
    if (pad.Buttons & PSP_CTRL_SQUARE)   btns |= DINGOO_Y;
    if (pad.Buttons & PSP_CTRL_LTRIGGER) btns |= DINGOO_LS;
    if (pad.Buttons & PSP_CTRL_RTRIGGER) btns |= DINGOO_RS;
    if (pad.Buttons & PSP_CTRL_SELECT)   btns |= DINGOO_SELECT;
    if (pad.Buttons & PSP_CTRL_START)    btns |= DINGOO_START;

    g_buttons = btns;
}

static void hle_kbd_get_status(uint32_t *out_ptr) {
    poll_input();
    out_ptr[0] = 0;         /* field0 (unused) */
    out_ptr[1] = 0;         /* field1 (unused) */
    out_ptr[2] = g_buttons; /* button bitmask */
}

static uint32_t hle_kbd_get_key(void) {
    return g_buttons;
}

static uint32_t hle_sys_judge_event(void) {
    poll_input();
    return g_quit ? (uint32_t)-1 : 0;
}

static uint32_t hle_get_game_vol(void) {
    return 80;
}

/* ── OS / RTOS ────────────────────────────────────────────────────────────── */

static uint32_t hle_OSTimeGet(void) {
    /* 100Hz tick rate: 10ms per tick */
    uint64_t elapsed = get_time_us() - g_start_time;
    return (uint32_t)(elapsed / 10000);
}

static uint32_t hle_GetTickCount(void) {
    uint64_t elapsed = get_time_us() - g_start_time;
    return (uint32_t)(elapsed / 1000);
}

static void hle_OSTimeDly(uint32_t ticks) {
    uint32_t us = ticks * 10000;
    if (us > 0) sceKernelDelayThread(us);
}

static uint32_t hle_OSTaskCreate(void *fn, void *arg, void *stack, uint32_t prio) {
    /* The game creates a single audio mixer task which we stub as no-op.
     * The audio is handled by PSP's sceAudio in the main thread context. */
    printf("[HLE] OSTaskCreate(fn=%p, prio=%lu) — stubbed\n", fn, (unsigned long)prio);
    (void)arg; (void)stack;
    return 0; /* OS_ERR_NONE */
}

static uint32_t hle_OSTaskDel(uint32_t prio) {
    printf("[HLE] OSTaskDel(prio=%lu) — stubbed\n", (unsigned long)prio);
    return 0;
}

static uint32_t hle_OSSemCreate(uint32_t count) {
    if (g_sem_count >= MAX_SEMS) return 0;
    SceUID sem = sceKernelCreateSema("qsem", 0, count, 255, NULL);
    if (sem < 0) return 0;
    uint32_t addr = g_sem_next_addr;
    g_sem_next_addr += 4;
    g_sems[g_sem_count].guest_addr = addr;
    g_sems[g_sem_count].psp_sem = sem;
    g_sem_count++;
    return addr;
}

static void hle_OSSemPend(uint32_t sem_addr, uint32_t timeout, uint32_t *err_ptr) {
    (void)timeout;
    int i = find_sem(sem_addr);
    if (i >= 0) {
        sceKernelWaitSema(g_sems[i].psp_sem, 1, NULL);
    }
    if (err_ptr) *err_ptr = 0;
}

static uint32_t hle_OSSemPost(uint32_t sem_addr) {
    int i = find_sem(sem_addr);
    if (i >= 0) {
        sceKernelSignalSema(g_sems[i].psp_sem, 1);
    }
    return 0;
}

static uint32_t hle_OSSemDel(uint32_t sem_addr) {
    int i = find_sem(sem_addr);
    if (i >= 0) {
        sceKernelDeleteSema(g_sems[i].psp_sem);
        /* Remove by swapping with last */
        g_sems[i] = g_sems[g_sem_count - 1];
        g_sem_count--;
    }
    return 0;
}

static uint32_t hle_OSCPUSaveSR(void) { return 0; }
static void     hle_OSCPURestoreSR(uint32_t sr) { (void)sr; }

/* ── String conversion ────────────────────────────────────────────────────── */

static uint16_t *hle_to_unicode_le(const char *src) {
    int i = 0;
    while (src[i] && i < 2046) {
        g_unicode_buf[i] = (uint16_t)(uint8_t)src[i];
        i++;
    }
    g_unicode_buf[i] = 0;
    return g_unicode_buf;
}

static char *hle_to_locale_ansi(const uint16_t *src) {
    int i = 0;
    while (src[i] && i < (int)sizeof(g_ansi_buf) - 1) {
        g_ansi_buf[i] = (char)(src[i] & 0xFF);
        i++;
    }
    g_ansi_buf[i] = 0;
    return g_ansi_buf;
}

static uint32_t hle_get_current_language(void) { return 0; }

/* ── Audio ────────────────────────────────────────────────────────────────── */

static uint32_t hle_waveout_open(uint32_t *params_ptr) {
    uint32_t sample_rate = params_ptr[0];
    uint16_t bits = *(uint16_t *)(&params_ptr[1]);
    printf("[AUDIO] waveout_open(rate=%lu, bits=%u)\n", (unsigned long)sample_rate, bits);

    /* Use SRC channel for arbitrary sample rate support */
    int ret = sceAudioSRCChReserve(PSP_AUDIO_SAMPLE_ALIGN(WAVEOUT_SAMPLES),
                                   sample_rate, 2 /* stereo required by SRC */);
    if (ret < 0) {
        printf("[AUDIO] sceAudioSRCChReserve failed: 0x%08x\n", ret);
        /* Fallback: try regular channel at closest rate */
        g_audio_ch = sceAudioChReserve(PSP_AUDIO_NEXT_CHANNEL,
                                       PSP_AUDIO_SAMPLE_ALIGN(WAVEOUT_SAMPLES),
                                       PSP_AUDIO_FORMAT_MONO);
        if (g_audio_ch < 0) {
            printf("[AUDIO] fallback also failed\n");
            return 0;
        }
    } else {
        g_audio_ch = 0x1000; /* sentinel: using SRC channel */
    }
    return 1;
}

static uint32_t hle_waveout_write(uint32_t handle, const int16_t *buf_ptr) {
    (void)handle;
    if (g_audio_ch < 0) return 0;

    /* Copy 400 mono samples, pad to 448 (64-aligned) */
    int i;
    if (g_audio_ch == 0x1000) {
        /* SRC channel requires stereo: duplicate mono to L+R */
        int16_t stereo_buf[448 * 2] __attribute__((aligned(64)));
        for (i = 0; i < WAVEOUT_SAMPLES; i++) {
            stereo_buf[i * 2]     = buf_ptr[i];
            stereo_buf[i * 2 + 1] = buf_ptr[i];
        }
        /* Pad remaining with silence */
        for (; i < 448; i++) {
            stereo_buf[i * 2]     = 0;
            stereo_buf[i * 2 + 1] = 0;
        }
        sceAudioSRCOutputBlocking(PSP_AUDIO_VOLUME_MAX, stereo_buf);
    } else {
        /* Regular mono channel */
        memcpy(g_audio_buf, buf_ptr, WAVEOUT_SAMPLES * 2);
        memset(g_audio_buf + WAVEOUT_SAMPLES, 0, (448 - WAVEOUT_SAMPLES) * 2);
        sceAudioOutputBlocking(g_audio_ch, PSP_AUDIO_VOLUME_MAX, g_audio_buf);
    }
    return 0;
}

static uint32_t hle_waveout_can_write(uint32_t handle) {
    (void)handle;
    /* PSP audio is blocking, so always report ready.
     * The blocking output call handles flow control. */
    return 1;
}

static uint32_t hle_waveout_close(uint32_t handle) {
    (void)handle;
    if (g_audio_ch == 0x1000) {
        sceAudioSRCChRelease();
    } else if (g_audio_ch >= 0) {
        sceAudioChRelease(g_audio_ch);
    }
    g_audio_ch = -1;
    return 0;
}

/* ── System / misc stubs ──────────────────────────────────────────────────── */

static void hle_vxGoHome(void) {
    printf("[HLE] vxGoHome() — exiting\n");
    g_quit = 1;
    sceKernelExitGame();
}

/* No-ops */
static void     hle_nop(void) {}
static uint32_t hle_stub_zero(void) { return 0; }
static uint32_t hle_stub_one(void) { return 1; }
static uint32_t hle_stub_neg1(void) { return (uint32_t)-1; }

/* ════════════════════════════════════════════════════════════════════════════
 * Dispatch table
 * ════════════════════════════════════════════════════════════════════════════ */

#define HLE(name, func)  { name, (hle_func_t)(func) }

static const HleEntry hle_table[] = {
    /* C stdlib */
    HLE("malloc",           malloc),
    HLE("free",             free),
    HLE("realloc",          realloc),
    HLE("printf",           printf),
    HLE("sprintf",          sprintf),
    HLE("fprintf",          fprintf),
    HLE("strlen",           strlen),
    HLE("strncasecmp",      strncasecmp),
    HLE("abort",            hle_abort_impl),

    /* C stdio (wrappers to fsys_*) */
    HLE("fread",            hle_fsys_fread),
    HLE("fwrite",           hle_fsys_fwrite),
    HLE("fseek",            hle_fsys_fseek),

    /* Filesystem */
    HLE("fsys_fopen",       hle_fsys_fopen),
    HLE("fsys_fopenW",      hle_fsys_fopenW),
    HLE("fsys_fread",       hle_fsys_fread),
    HLE("fsys_fwrite",      hle_fsys_fwrite),
    HLE("fsys_fclose",      hle_fsys_fclose),
    HLE("fsys_fseek",       hle_fsys_fseek),
    HLE("fsys_ftell",       hle_fsys_ftell),
    HLE("fsys_feof",        hle_fsys_feof),
    HLE("fsys_ferror",      hle_fsys_ferror),
    HLE("fsys_remove",      hle_stub_zero),
    HLE("fsys_rename",      hle_stub_zero),
    HLE("fsys_findfirst",   hle_stub_neg1),
    HLE("fsys_findnext",    hle_stub_neg1),
    HLE("fsys_findclose",   hle_stub_zero),
    HLE("fsys_RefreshCache", hle_stub_zero),
    HLE("fsys_flush_cache", hle_stub_zero),

    /* LCD / Video */
    HLE("_lcd_get_frame",   hle_lcd_get_frame),
    HLE("lcd_get_cframe",   hle_lcd_get_frame),
    HLE("_lcd_set_frame",   hle_lcd_set_frame),
    HLE("ap_lcd_set_frame", hle_lcd_set_frame),
    HLE("lcd_flip",         hle_lcd_flip),
    HLE("LcdGetDisMode",    hle_stub_zero),

    /* Input */
    HLE("_kbd_get_status",  hle_kbd_get_status),
    HLE("_kbd_get_key",     hle_kbd_get_key),
    HLE("get_game_vol",     hle_get_game_vol),

    /* Event */
    HLE("_sys_judge_event", hle_sys_judge_event),

    /* RTOS */
    HLE("OSSemCreate",      hle_OSSemCreate),
    HLE("OSSemPend",        hle_OSSemPend),
    HLE("OSSemPost",        hle_OSSemPost),
    HLE("OSSemDel",         hle_OSSemDel),
    HLE("OSTimeGet",        hle_OSTimeGet),
    HLE("GetTickCount",     hle_GetTickCount),
    HLE("OSTimeDly",        hle_OSTimeDly),
    HLE("OSTaskCreate",     hle_OSTaskCreate),
    HLE("OSTaskDel",        hle_OSTaskDel),
    HLE("OSCPUSaveSR",      hle_OSCPUSaveSR),
    HLE("OSCPURestoreSR",   hle_OSCPURestoreSR),

    /* String conversion */
    HLE("__to_unicode_le",  hle_to_unicode_le),
    HLE("__to_locale_ansi", hle_to_locale_ansi),
    HLE("get_current_language", hle_get_current_language),

    /* Audio */
    HLE("waveout_open",     hle_waveout_open),
    HLE("waveout_write",    hle_waveout_write),
    HLE("waveout_can_write", hle_waveout_can_write),
    HLE("waveout_close",    hle_waveout_close),
    HLE("waveout_close_at_once", hle_waveout_close),
    HLE("waveout_set_volume", hle_stub_zero),
    HLE("pcm_can_write",    hle_stub_one),
    HLE("pcm_ioctl",        hle_stub_zero),
    HLE("HP_Mute_sw",       hle_nop),

    /* System / misc */
    HLE("vxGoHome",         hle_vxGoHome),
    HLE("__icache_invalidate_all", hle_nop),
    HLE("__dcache_writeback_all",  hle_nop),
    HLE("StartSwTimer",     hle_nop),
    HLE("free_irq",         hle_nop),
    HLE("TaskMediaFunStop", hle_nop),
    HLE("USB_Connect",      hle_stub_zero),
    HLE("USB_No_Connect",   hle_stub_zero),
    HLE("udc_attached",     hle_stub_zero),
    HLE("serial_putc",      hle_stub_zero),
    HLE("serial_getc",      hle_stub_zero),
};

#define HLE_TABLE_COUNT (sizeof(hle_table) / sizeof(hle_table[0]))

/* ════════════════════════════════════════════════════════════════════════════
 * Public API
 * ════════════════════════════════════════════════════════════════════════════ */

void hle_init(void) {
    g_start_time = get_time_us();
    g_quit = 0;
    g_buttons = 0;
    g_frame_count = 0;
    g_audio_ch = -1;
    g_next_fd = 0;
    g_sem_count = 0;
    g_sem_next_addr = 0x08E00004;

    for (int i = 0; i < MAX_OPEN_FILES; i++)
        g_files[i] = -1;

    /* Set up controller sampling */
    sceCtrlSetSamplingCycle(0);
    sceCtrlSetSamplingMode(PSP_CTRL_MODE_DIGITAL);
}

int hle_get_table(const HleEntry **out_table) {
    *out_table = hle_table;
    return (int)HLE_TABLE_COUNT;
}

void hle_set_vram(void *vram_base) {
    (void)vram_base;
    /* Clear entire VRAM to black (480x272 RGB565) */
    uint16_t *vram = (uint16_t *)PSP_VRAM_BASE;
    memset(vram, 0, PSP_BUF_W * PSP_SCR_H * 2);
}
