#ifndef HLE_H
#define HLE_H

#include "ccdl.h"
#include <stdint.h>

/* ── LCD constants ────────────────────────────────────────────────────────── */

#define LCD_W             320
#define LCD_H             240
#define LCD_FRAMEBUF      0x08F00000u  /* game writes RGB565 here */

/* PSP screen */
#define PSP_SCR_W         480
#define PSP_SCR_H         272
#define PSP_BUF_W         512         /* power-of-2 stride */
#define PSP_VRAM_BASE     0x44000000u /* uncached VRAM alias */
#define FB_X_OFF          ((PSP_SCR_W - LCD_W) / 2)   /* 80 */
#define FB_Y_OFF          ((PSP_SCR_H - LCD_H) / 2)   /* 16 */

/* ── Dingoo A320 key bitmasks (GPIO) ──────────────────────────────────────── */

#define DINGOO_UP         0x00100000u
#define DINGOO_DOWN       0x08000000u
#define DINGOO_LEFT       0x10000000u
#define DINGOO_RIGHT      0x00040000u
#define DINGOO_A          0x80000000u  /* Circle on PSP */
#define DINGOO_B          0x00200000u  /* Cross on PSP */
#define DINGOO_X          0x00010000u  /* Triangle on PSP */
#define DINGOO_Y          0x00000040u  /* Square on PSP */
#define DINGOO_LS         0x00000100u  /* L trigger */
#define DINGOO_RS         0x20000000u  /* R trigger */
#define DINGOO_SELECT     0x00000400u
#define DINGOO_START      0x00000800u

/* ── Audio ────────────────────────────────────────────────────────────────── */

#define WAVEOUT_SAMPLES   400   /* samples per waveout_write call */

/* ── HLE state (global) ──────────────────────────────────────────────────── */

void hle_init(void);

/* Returns the HLE dispatch table and its entry count. */
int hle_get_table(const HleEntry **out_table);

/* Called from main after display init to set up VRAM pointers. */
void hle_set_vram(void *vram_base);

#endif /* HLE_H */
